use std::fmt::Display;

use color_eyre::eyre::{Context, Result, bail};
use derive_more::From;
use serde::Deserialize;

use crate::crud::{
    Version,
    create::{DownloadableJarSoftware, ServerSoftware},
};

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
pub struct Fabric<B> {
    base_url: B,
}

impl<B: Display> ServerSoftware<DownloadableJarSoftware> for Fabric<B> {
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
