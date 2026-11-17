use std::fmt::Display;

use color_eyre::eyre::{Context, ContextCompat, Result};
use derive_more::From;
use serde::Deserialize;

use crate::crud::{
    Version,
    create::{DownloadableJarSoftware, ServerSoftware},
};

#[derive(Debug, From)]
pub struct Paper<B> {
    base_url: B,
}

impl<B: Display> ServerSoftware<DownloadableJarSoftware> for Paper<B> {
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
                let versions_url = format!("{}/versions", self.base_url);

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

        let build_url = format!("{}/versions/{version_id}/builds/{build}", self.base_url);

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
