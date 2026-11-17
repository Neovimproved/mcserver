use color_eyre::eyre::{Context, Result};
use tokio::{fs, sync::RwLock};

use crate::{config::Config, error::Error};

pub async fn delete(config: &RwLock<Config>, name: String) -> Result<()> {
    let servers_directory = &config.read().await.servers_directory.path;

    let Ok(target_directory) = fs::canonicalize(servers_directory.join(&name)).await else {
        eprintln!("The client is being malicious");
        return Ok(());
    };

    if !target_directory.starts_with(servers_directory) {
        return Err(Error::EscapedPath(target_directory).into());
    }

    if target_directory.as_os_str().len() == servers_directory.as_os_str().len() {
        return Err(Error::EmptyPath.into());
    }

    fs::remove_dir_all(config.read().await.servers_directory.path.join(name))
        .await
        .wrap_err("Failed to delete server")
}
