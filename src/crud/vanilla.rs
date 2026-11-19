use std::fmt::Display;

use color_eyre::eyre::{Context, ContextCompat};
use derive_more::From;
use serde::{Deserialize, Serialize};

use crate::crud::create::{DownloadableJarSoftware, ServerSoftware};

#[derive(Debug, From)]
pub struct Vanilla<B> {
    base_url: B,
}

const RELEASE_TYPE: &str = "release";

impl<B: Display> ServerSoftware<DownloadableJarSoftware> for Vanilla<B> {
    const NAME: &str = "paper";

    async fn get(
        &self,
        version: Option<super::Version>,
    ) -> color_eyre::Result<DownloadableJarSoftware> {
        let response: VanillaResponse = reqwest::get(self.base_url.to_string())
            .await
            .wrap_err("Failed to get response")?
            .json()
            .await
            .wrap_err("Failed to decode json")?;

        let version = match version.map(|v| v.to_string()) {
            Some(version_id) => response
                .versions
                .into_iter()
                .find(|version| version.id == version_id)
                .wrap_err_with(|| "Failed to find version {version_id}")?,
            None => response
                .versions
                .into_iter()
                .find(|version| version._type == RELEASE_TYPE)
                .wrap_err("Failed to find release version")?,
        };

        Ok(DownloadableJarSoftware::new(version.url, version.id))
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct VanillaVersion {
    id: String,
    #[serde(rename = "type")]
    _type: String,
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct VanillaResponse {
    versions: Vec<VanillaVersion>,
}
