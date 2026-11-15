#![cfg(test)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use color_eyre::eyre::Context as EyreContext;
use miette::{Context, IntoDiagnostic};
use tempfile::TempDir;
use tokio::{fs, sync::RwLock};

use crate::{
    config::{Config, ServersDirectory, TerminalInterfaceConfig},
    error::display_path,
};

// Not to be confused with the TEMP_DIR
pub fn tempdir_init_miette() -> miette::Result<TempDir> {
    tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Failed to create temporary directory")
}

pub fn tempdir_init_eyre() -> color_eyre::Result<TempDir> {
    tempfile::tempdir().wrap_err("Failed to create temporary directory")
}

pub async fn new_dir(parent: &Path, name: impl AsRef<Path>) -> miette::Result<PathBuf> {
    let dir = parent.join(name);

    fs::create_dir(&dir)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to create directory `{}`", display_path(&dir)))?;

    Ok(dir)
}

pub fn configured_directory<T>(
    servers_directory: T,
) -> Result<Config, <T as TryInto<ServersDirectory>>::Error>
where
    ServersDirectory: TryFrom<T>,
{
    Ok(Config {
        default_java_args: vec![],
        servers_directory: ServersDirectory::try_from(servers_directory)?,
        aliases: HashMap::new(),
        auto_restart: false,
        terminal_interface: TerminalInterfaceConfig::default(),
    })
}

pub fn server_configured_directory<T>(
    servers_directory: T,
) -> Result<&'static RwLock<Config>, <T as TryInto<ServersDirectory>>::Error>
where
    ServersDirectory: TryFrom<T>,
{
    Ok(Box::leak(Box::new(RwLock::new(configured_directory(
        servers_directory,
    )?))))
}
