#![allow(dead_code)]
use std::io::Cursor;

use color_eyre::eyre::{Context, ContextCompat, Result};
use reqwest::{
    Url,
    header::{self, HeaderValue},
};
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::creator::{ServerSoftware, Version};

static BASE_PAPER_URL: &str = "https://fill.papermc.io/v3/projects/paper";

static BASE_FABRIC_URL: &str = "https://meta.fabricmc.net/v2/versions";

static BASE_VANILLA_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Deserialize)]
struct FabricVersions {
    version: String,
    stable: bool,
}

#[derive(Debug)]
struct Fabric {}

#[derive(Debug, Deserialize)]
struct PaperVersions {
    versions: Vec<PaperVersion>,
}

#[derive(Debug, Deserialize)]
struct PaperVersion {
    id: String,
    builds: Vec<usize>,
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

async fn paper(version: Option<Version>) -> Result<Url> {
    let version: PaperVersion = match version {
        Some(version) => {
            let version_url = format!("{BASE_PAPER_URL}/versions/{version}");
            reqwest::get(&version_url)
                .await
                .wrap_err_with(|| {
                    format!(
                        "Failed to get information for version {version} using url {version_url}"
                    )
                })?
                .json()
                .await
                .wrap_err("Failed to convert response to json")?
        }
        None => {
            let versions_url = format!("{BASE_PAPER_URL}/versions");

            let versions: PaperVersions = reqwest::get(&versions_url)
                .await
                .wrap_err_with(|| format!("Failed to get versions from {versions_url}"))?
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

    let version_id = version.id;

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

    Url::parse(&build.downloads.server_default.url)
        .wrap_err_with(|| format!("Failed to parse url {}", build.downloads.server_default.url))
}

async fn fabric(_version: Option<Version>) -> Result<Url> {
    todo!()
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

pub async fn get(
    server_software: ServerSoftware,
    version: Option<Version>,
) -> Result<(impl AsyncReadExt, String)> {
    let download_url: Url = match server_software {
        ServerSoftware::Fabric => fabric(version).await?,
        ServerSoftware::Forge => todo!(),
        ServerSoftware::NeoForge => todo!(),
        ServerSoftware::Paper => paper(version).await?,
        ServerSoftware::Purpur => todo!(),
        ServerSoftware::Vanilla => todo!("Implement vanilla"),
    };

    let response = reqwest::get(download_url)
        .await
        .wrap_err_with(|| "Failed to get from {url}")?;

    let file_name = extract_response_file_name(&response)
        .await?
        .map(String::from)
        .unwrap_or_else(|| format!("{server_software}-"));

    let bytes = response.bytes().await?;

    let reader = Cursor::new(bytes);

    Ok((reader, file_name))
}
