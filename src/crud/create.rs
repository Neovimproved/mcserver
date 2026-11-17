use std::path::{Component, Path, PathBuf};

use color_eyre::eyre::{Context, Result};
use reqwest::header::{self, HeaderValue};
use tokio::{fs, sync::RwLock};

use crate::{
    config::Config,
    crud::{ServerSoftwareType, Version, base_url, fabric::Fabric, paper::Paper},
    error::{self, display_path},
    messages::CreateMessage,
    metadata,
};

static _BASE_VANILLA_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub trait ServerSoftware<T> {
    const NAME: &str;

    async fn get(&self, version: Option<Version>) -> Result<T>;
}

trait CreateSoftware<T>: ServerSoftware<T> {
    async fn create_jar(&self, version: Option<Version>, server_dir: &Path) -> Result<String>;
}

pub struct DownloadableJarSoftware {
    url: String,
    version: String,
}

impl DownloadableJarSoftware {
    pub fn new(url: String, version: String) -> Self {
        Self { url, version }
    }
}

async fn extract_response_file_name(response: &reqwest::Response) -> Result<Option<&str>> {
    let file_name = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .map(HeaderValue::to_str)
        .transpose()?
        .and_then(|header| header.split("filename=\"").nth(1))
        .and_then(|header| header.split('"').next());

    Ok(file_name)
}

impl<T: ServerSoftware<DownloadableJarSoftware>> CreateSoftware<DownloadableJarSoftware> for T {
    async fn create_jar(&self, version: Option<Version>, server_dir: &Path) -> Result<String> {
        let DownloadableJarSoftware {
            url,
            version: version_used,
        } = self.get(version).await?;

        let response = reqwest::get(url)
            .await
            .wrap_err("Failed to get server jar")?;

        let file_name = extract_response_file_name(&response)
            .await?
            .map(String::from);

        let bytes = response.bytes().await?;

        let file_name = file_name.unwrap_or_else(|| format!("{}-{version_used}.jar", T::NAME));

        let file_path = server_dir.join(&file_name);

        fs::write(&file_path, bytes)
            .await
            .wrap_err_with(|| format!("Failed to write jar to {}", display_path(&file_path)))?;

        Ok(file_name)
    }
}

pub trait SoftwareResolver {
    async fn resolve(
        self,
        server_software: ServerSoftwareType,
        version: Option<Version>,
        server_dir: &Path,
    ) -> Result<String>;
}

pub struct DefaultSoftwareResolver;

impl SoftwareResolver for DefaultSoftwareResolver {
    async fn resolve(
        self,
        server_software: ServerSoftwareType,
        version: Option<Version>,
        server_dir: &Path,
    ) -> Result<String> {
        match server_software {
            ServerSoftwareType::Fabric => {
                Fabric::from(base_url::FABRIC)
                    .create_jar(version, server_dir)
                    .await
            }
            ServerSoftwareType::Forge => todo!(),
            ServerSoftwareType::NeoForge => todo!(),
            ServerSoftwareType::Paper => {
                Paper::from(base_url::PAPER)
                    .create_jar(version, server_dir)
                    .await
            }
            ServerSoftwareType::Purpur => todo!(),
            ServerSoftwareType::Vanilla => todo!(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Creator<R: SoftwareResolver = DefaultSoftwareResolver> {
    resolver: R,
}

impl Creator<DefaultSoftwareResolver> {
    pub fn new() -> Self {
        Self {
            resolver: DefaultSoftwareResolver,
        }
    }
}

impl Default for Creator<DefaultSoftwareResolver> {
    fn default() -> Self {
        Self {
            resolver: DefaultSoftwareResolver,
        }
    }
}

impl<R: SoftwareResolver> Creator<R> {
    pub async fn build(
        self,
        config: &RwLock<Config>,
        create_message: CreateMessage,
    ) -> Result<(PathBuf, PathBuf)> {
        if create_message.path.is_empty() {
            return Err(error::Error::EmptyPath.into());
        }

        let servers_directory = &config.read().await.servers_directory.path;

        let path = PathBuf::from(create_message.path);

        for component in path.components() {
            let Component::Normal(_) = component else {
                return Err(error::Error::EscapedPath(path).into());
            };
        }

        let server_dir = servers_directory.join(path);

        let metadata_dir = metadata::dir(&server_dir);

        fs::create_dir_all(&metadata_dir)
            .await
            .wrap_err("Failed to create metadata directory")?;

        let jar_file_name: String = self
            .resolver
            .resolve(
                create_message.server_software,
                create_message.version,
                &server_dir,
            )
            .await?;

        metadata::set_jar_file_with_metadata_dir(&metadata_dir, &jar_file_name).await?;

        Ok((server_dir, metadata_dir))
    }
}

#[cfg(test)]
mod test {
    use crate::{
        crud::fabric::Fabric,
        metadata::LastUsed,
        test_utils::{server_configured_directory, tempdir_init_eyre},
    };

    use super::*;

    // TODO: use wiremock for some tests

    #[derive(Debug)]
    struct MockingResolver;

    impl SoftwareResolver for MockingResolver {
        async fn resolve(
            self,
            server_software: ServerSoftwareType,
            version: Option<Version>,
            server_dir: &Path,
        ) -> Result<String> {
            match server_software {
                ServerSoftwareType::Fabric => {
                    Fabric::from(base_url::FABRIC)
                        .create_jar(version, server_dir)
                        .await
                }
                ServerSoftwareType::Forge => todo!(),
                ServerSoftwareType::NeoForge => todo!(),
                ServerSoftwareType::Paper => {
                    Paper::from(base_url::PAPER)
                        .create_jar(version, server_dir)
                        .await
                }
                ServerSoftwareType::Purpur => todo!(),
                ServerSoftwareType::Vanilla => todo!(),
            }
        }
    }

    async fn create_test(server_software: ServerSoftwareType) -> Result<()> {
        let temp_dir = tempdir_init_eyre()?;
        let servers_directory = temp_dir.path();
        let config = server_configured_directory(servers_directory)?;

        let path = "foo/bar".into();

        let (server_dir, _metadata_dir) = Creator {
            resolver: MockingResolver,
        }
        .build(
            config,
            CreateMessage {
                path,
                server_software,
                version: None,
            },
        )
        .await?;

        assert!(
            fs::try_exists(servers_directory.join(format!("foo/bar/{}", metadata::DIRECTORY)))
                .await
                .wrap_err("Failed to check if servers directory exists")?
        );

        let metadata = metadata::get(&server_dir).await?;

        assert!(matches!(metadata.last_used, LastUsed::Never));

        assert!(fs::try_exists(metadata.jar_file).await?);

        Ok(())
    }

    #[tokio::test]
    async fn create_paper_test() -> Result<()> {
        create_test(ServerSoftwareType::Paper).await
    }

    #[tokio::test]
    async fn create_fabric_test() -> Result<()> {
        create_test(ServerSoftwareType::Fabric).await
    }
}
