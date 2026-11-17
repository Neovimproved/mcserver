use std::{
    fmt::{self, Display},
    num::ParseIntError,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use bitcode::{Decode, Encode};
use color_eyre::eyre::{Context, ContextCompat, Result, bail};
use derive_more::{Display, Error, From};
use reqwest::header::{self, HeaderValue};
use serde::Deserialize;
use tokio::{fs, sync::RwLock};

use crate::{
    config::Config,
    error::{self, display_path},
    messages::CreateMessage,
    metadata,
};

#[derive(Clone, Copy, Debug, Decode, Default, Display, Encode)]
pub enum ServerSoftware {
    #[display("fabric")]
    Fabric,
    #[display("forge")]
    Forge,
    #[display("neoforge")]
    NeoForge,
    #[display("paper")]
    Paper,
    #[display("purpur")]
    Purpur,
    #[default]
    #[display("vanilla")]
    Vanilla,
}

impl FromStr for ServerSoftware {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "fabric" => Self::Fabric,
            "forge" => Self::Forge,
            "neoforge" => Self::NeoForge,
            "paper" => Self::Paper,
            "purpur" => Self::Purpur,
            "vanilla" => Self::Vanilla,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Decode, Encode)]
pub struct Version {
    pub major: usize,
    pub minor: usize,
    pub patch: Option<usize>,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;

        if let Some(patch) = self.patch {
            write!(f, ".{patch}")
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Display, Error, From)]
pub enum ParseVersionError {
    ParseIntError(ParseIntError),

    #[display("A minor version was not given")]
    NoMinor,

    #[display("Malformed version (received trailing period after minor version)")]
    MalformedTrailingPeriod,
}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let period = s.find('.').ok_or(ParseVersionError::NoMinor)?;

        let major = s[..period].parse()?;

        let remaining = s.get(period + 1..).ok_or(ParseVersionError::NoMinor)?;

        let (minor, patch) = if let Some(period) = remaining.find('.') {
            let minor = remaining[..period].parse()?;
            let patch = remaining
                .get(period + 1..)
                .ok_or(ParseVersionError::MalformedTrailingPeriod)?
                .parse()?;

            (minor, Some(patch))
        } else {
            (remaining.parse()?, None)
        };

        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

static BASE_PAPER_URL: &str = "https://fill.papermc.io/v3/projects/paper";

static BASE_FABRIC_URL: &str = "https://meta.fabricmc.net/v2/versions";

static _BASE_VANILLA_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

trait Software<T> {
    const NAME: &str;

    async fn get(&self, version: Option<Version>) -> Result<T>;
}

trait CreateSoftware<T>: Software<T> {
    async fn create_jar(&self, version: Option<Version>, server_dir: &Path) -> Result<String>;
}

struct DownloadableJarSoftware {
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

impl<T: Software<DownloadableJarSoftware>> CreateSoftware<DownloadableJarSoftware> for T {
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

#[derive(Debug, Deserialize)]
struct FabricEntry {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct FabricVersions {
    game: Vec<FabricEntry>,
    loader: Vec<FabricEntry>,
    installer: Vec<FabricEntry>,
}

#[derive(Debug, From)]
struct FabricSoftware<B> {
    base_url: B,
}

impl<B: Display> Software<DownloadableJarSoftware> for FabricSoftware<B> {
    const NAME: &str = "fabric";

    async fn get(&self, version: Option<Version>) -> Result<DownloadableJarSoftware> {
        let response = reqwest::get(self.base_url.to_string())
            .await
            .wrap_err("Get request failed")?;

        let versions: FabricVersions = response
            .json()
            .await
            .wrap_err("Failed to convert response to json")?;

        let game_version = match version {
            Some(version) => version.to_string(),
            None => match versions.game.into_iter().find(|e| e.stable) {
                Some(game) => game.version,
                None => bail!("No stable version was found"),
            },
        };

        let loader_version = match versions.loader.into_iter().find(|e| e.stable) {
            Some(loader) => loader.version,
            None => bail!("No stable loader was found"),
        };

        let installer_version = match versions.installer.into_iter().find(|e| e.stable) {
            Some(installer) => installer.version,
            None => bail!("No stable installer was found"),
        };

        Ok(DownloadableJarSoftware::new(
            format!(
                "{}/loader/{}/{}/{}/server/jar",
                self.base_url, game_version, loader_version, installer_version
            ),
            game_version,
        ))
    }
}

#[derive(Debug, From)]
struct PaperSoftware<B> {
    base_url: B,
}

impl<B: Display> Software<DownloadableJarSoftware> for PaperSoftware<B> {
    const NAME: &str = "paper";

    async fn get(&self, version: Option<Version>) -> Result<DownloadableJarSoftware> {
        let version: PaperVersion = match version {
            Some(version) => {
                let version_url = format!("{}/versions/{version}", self.base_url);

                let response = reqwest::get(&version_url)
                    .await
                    .wrap_err("Version get request failed")?;

                response
                    .json()
                    .await
                    .wrap_err("Failed to convert response to json")?
            }
            None => {
                let versions_url = format!("{BASE_PAPER_URL}/versions");

                let response = reqwest::get(&versions_url)
                    .await
                    .wrap_err("Versions get request failed")?;

                let versions: PaperVersions = response
                    .json()
                    .await
                    .wrap_err("Failed to convert response to json")?;

                versions
                    .versions
                    .into_iter()
                    .next()
                    .wrap_err("No versions found for paper")?
            }
        };

        let version_id = version.version.id;

        let build = version
            .builds
            .first()
            .wrap_err_with(|| format!("No builds found for version {version_id}"))?;

        let build_url = format!("{BASE_PAPER_URL}/versions/{version_id}/builds/{build}");

        let build: PaperBuild = reqwest::get(&build_url)
            .await
            .wrap_err_with(|| {
                format!("Failed to get build info for version {version_id} using url {build_url}")
            })?
            .json()
            .await
            .wrap_err("Failed to convert response into json")?;

        Ok(DownloadableJarSoftware::new(
            build.downloads.server_default.url,
            version_id,
        ))
    }
}

#[derive(Debug, Deserialize)]
struct PaperVersionData {
    id: String,
}

#[derive(Debug, Deserialize)]
struct PaperVersion {
    version: PaperVersionData,
    builds: Vec<usize>,
}

#[derive(Debug, Deserialize)]
struct PaperVersions {
    versions: Vec<PaperVersion>,
}

#[derive(Debug, Deserialize)]
struct PaperBuildServer {
    url: String,
}

#[derive(Debug, Deserialize)]
struct PaperBuildDownloads {
    #[serde(rename = "server:default")]
    server_default: PaperBuildServer,
}

#[derive(Debug, Deserialize)]
struct PaperBuild {
    downloads: PaperBuildDownloads,
}

pub trait SoftwareResolver {
    async fn resolve(
        self,
        server_software: ServerSoftware,
        version: Option<Version>,
        server_dir: &Path,
    ) -> Result<String>;
}

pub struct DefaultSoftwareResolver;

impl SoftwareResolver for DefaultSoftwareResolver {
    async fn resolve(
        self,
        server_software: ServerSoftware,
        version: Option<Version>,
        server_dir: &Path,
    ) -> Result<String> {
        match server_software {
            ServerSoftware::Fabric => {
                FabricSoftware::from(BASE_FABRIC_URL)
                    .create_jar(version, server_dir)
                    .await
            }
            ServerSoftware::Forge => todo!(),
            ServerSoftware::NeoForge => todo!(),
            ServerSoftware::Paper => {
                PaperSoftware::from(BASE_PAPER_URL)
                    .create_jar(version, server_dir)
                    .await
            }
            ServerSoftware::Purpur => todo!(),
            ServerSoftware::Vanilla => todo!(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Builder<R: SoftwareResolver = DefaultSoftwareResolver> {
    resolver: R,
}

impl Builder<DefaultSoftwareResolver> {
    pub fn new() -> Self {
        Self {
            resolver: DefaultSoftwareResolver,
        }
    }
}

impl Default for Builder<DefaultSoftwareResolver> {
    fn default() -> Self {
        Self {
            resolver: DefaultSoftwareResolver,
        }
    }
}

impl<R: SoftwareResolver> Builder<R> {
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
            server_software: ServerSoftware,
            version: Option<Version>,
            server_dir: &Path,
        ) -> Result<String> {
            match server_software {
                ServerSoftware::Fabric => {
                    FabricSoftware::from(BASE_FABRIC_URL)
                        .create_jar(version, server_dir)
                        .await
                }
                ServerSoftware::Forge => todo!(),
                ServerSoftware::NeoForge => todo!(),
                ServerSoftware::Paper => {
                    PaperSoftware::from(BASE_PAPER_URL)
                        .create_jar(version, server_dir)
                        .await
                }
                ServerSoftware::Purpur => todo!(),
                ServerSoftware::Vanilla => todo!(),
            }
        }
    }

    async fn create_test(server_software: ServerSoftware) -> Result<()> {
        let temp_dir = tempdir_init_eyre()?;
        let servers_directory = temp_dir.path();
        let config = server_configured_directory(servers_directory)?;

        let path = "foo/bar".into();

        let (server_dir, _metadata_dir) = Builder {
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
        create_test(ServerSoftware::Paper).await
    }

    #[tokio::test]
    async fn create_fabric_test() -> Result<()> {
        create_test(ServerSoftware::Fabric).await
    }
}
