#![cfg(test)]

use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use tempfile::TempDir;
use tokio::fs;

use crate::error::display_path;

// Not to be confused with the TEMP_DIR
pub fn tempdir_init() -> miette::Result<TempDir> {
    tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Failed to create temporary directory")
}

pub async fn new_dir(parent: &Path, name: impl AsRef<Path>) -> miette::Result<PathBuf> {
    let dir = parent.join(name);

    fs::create_dir(&dir)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to create directory `{}`", display_path(&dir)))?;

    Ok(dir)
}
