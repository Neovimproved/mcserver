use std::path::{Component, Path, PathBuf};

use color_eyre::eyre::{Context, Result};
use reqwest::header::{self, HeaderValue};
use tokio::{fs, sync::RwLock};

use crate::{
    config::Config,
    crud::{ServerSoftwareType, Version, base_url, fabric::Fabric, paper::Paper, vanilla::Vanilla},
    error::{self, display_path},
    messages::CreateMessage,
    metadata,
};

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
            ServerSoftwareType::Vanilla => {
                Vanilla::from(base_url::VANILLA)
                    .create_jar(version, server_dir)
                    .await
            }
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

struct CountedName {
    buffer: String,
    count_start: usize,
}

impl CountedName {
    fn new(mut buffer: String) -> Self {
        let mut rev_char_indices = buffer.char_indices().rev();

        let count_start: usize = match rev_char_indices.next() {
            Some((_, ch)) if ch.is_ascii_digit() => rev_char_indices
                .find(|(_, ch)| !ch.is_ascii_digit())
                .map(|(i, _)| i + 1)
                .unwrap_or(0),
            Some(_) => {
                buffer.push_str("-0");
                buffer.len() - 1
            }
            None => {
                buffer.push('0');
                0
            }
        };

        Self {
            buffer,
            count_start,
        }
    }

    fn increment(&mut self) {
        let ch = match self.buffer.pop() {
            Some(ch) => ch,
            None => {
                self.buffer.push('0');
                return;
            }
        };

        if ch == '9' {
            if self.buffer.len() == self.count_start {
                // => there are no more digits
                self.buffer.push_str("10");
            } else {
                self.increment();
                self.buffer.push('0');
            }
        } else {
            self.buffer.push((ch as u8 + 1) as char);
        }
    }
}

impl From<&str> for CountedName {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

impl AsRef<str> for CountedName {
    fn as_ref(&self) -> &str {
        &self.buffer
    }
}

async fn default_path(
    servers_directory: impl Into<PathBuf>,
    server_software: ServerSoftwareType,
) -> Result<PathBuf> {
    let mut file_name = format!("{server_software}-server");
    let mut server_path = servers_directory.into();

    server_path.push(&file_name);

    if !fs::try_exists(&server_path)
        .await
        .wrap_err("Failed to check if it exists")?
    {
        return Ok(server_path);
    }

    file_name.push_str("-1");
    let mut counted_file_name = CountedName::new(file_name);

    loop {
        server_path.pop();
        server_path.push(counted_file_name.as_ref());

        if fs::try_exists(&server_path).await? {
            counted_file_name.increment();
        } else {
            break Ok(server_path);
        }
    }
}

fn resolve_path(servers_directory: &Path, path: String) -> Result<PathBuf> {
    let path = PathBuf::from(path);

    for component in path.components() {
        let Component::Normal(_) = component else {
            return Err(error::Error::EscapedPath(path).into());
        };
    }

    Ok(servers_directory.join(path))
}

impl<R: SoftwareResolver> Creator<R> {
    pub async fn build(
        self,
        config: &RwLock<Config>,
        create_message: CreateMessage,
    ) -> Result<(PathBuf, PathBuf)> {
        let servers_directory = &config.read().await.servers_directory.path;

        let path = if create_message.path.is_empty() {
            default_path(servers_directory, create_message.server_software).await?
        } else {
            resolve_path(servers_directory, create_message.path)?
        };

        let server_dir = servers_directory.join(path);

        if fs::try_exists(&server_dir).await.wrap_err_with(|| {
            format!(
                "Failed to check if server directory ({}) exists",
                display_path(&server_dir)
            )
        })? {
            return Err(error::Error::ServerAlreadyExists(server_dir).into());
        }

        let metadata_dir = metadata::dir(&server_dir);
        let mut last_nonexistent_ancestor: &Path = &server_dir;

        for ancestor in metadata_dir.ancestors().skip(1) {
            if fs::try_exists(ancestor).await.wrap_err_with(|| {
                format!(
                    "Failed to check if ancestor {} exists",
                    display_path(ancestor)
                )
            })? {
                break;
            }

            last_nonexistent_ancestor = ancestor;
        }

        fs::create_dir_all(&server_dir)
            .await
            .wrap_err("Failed to create metadata directory")?;

        let jar_file_name = match self
            .resolver
            .resolve(
                create_message.server_software,
                create_message.version,
                &server_dir,
            )
            .await
        {
            Ok(n) => n,
            Err(err) => {
                fs::remove_dir_all(last_nonexistent_ancestor)
                    .await
                    .wrap_err_with(|| {
                        format!(
                            "Failed to remove path: {}",
                            display_path(last_nonexistent_ancestor)
                        )
                    })?;

                return Err(err);
            }
        };

        fs::create_dir(&metadata_dir).await?;

        metadata::set_jar_file_with_metadata_dir(&metadata_dir, &jar_file_name).await?;

        Ok((server_dir, metadata_dir))
    }
}

#[cfg(test)]
mod test {
    // use wiremock::{
    //     Mock, MockServer, ResponseTemplate,
    //     matchers::{method, path},
    // };

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
        if true {
            return Ok(());
        }

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

    #[test]
    fn counted_name_test() {
        // starts at 0
        let mut name = CountedName::from("hello world");

        for i in 0..=100_000 {
            assert_eq!(name.as_ref(), format!("hello world-{i}"));
            name.increment();
        }

        let mut name = CountedName::from("hello world-69");
        for i in 69..=100_000 {
            assert_eq!(name.as_ref(), format!("hello world-{i}"));
            name.increment();
        }
    }

    async fn test_default_path(server_software: ServerSoftwareType) -> Result<()> {
        let tempdir = tempdir_init_eyre()?;
        let servers_dir = tempdir.path();

        let name = format!("{server_software}-server");

        fs::create_dir(servers_dir.join(&name)).await?;

        for i in 0..100 {
            fs::create_dir(servers_dir.join(format!("{name}-{i}"))).await?;
        }

        assert_eq!(
            default_path(servers_dir, server_software).await?,
            servers_dir.join(format!("{server_software}-server-100"))
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_default_path_softwares() -> Result<()> {
        test_default_path(ServerSoftwareType::Fabric).await?;
        test_default_path(ServerSoftwareType::Forge).await?;
        test_default_path(ServerSoftwareType::NeoForge).await?;
        test_default_path(ServerSoftwareType::Paper).await?;
        test_default_path(ServerSoftwareType::Purpur).await?;
        test_default_path(ServerSoftwareType::Vanilla).await?;

        Ok(())
    }
}
