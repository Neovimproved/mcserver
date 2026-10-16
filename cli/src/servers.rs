use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
};

use mcserver_core::{METADATA_DIRECTORY, display_path};
use tokio::fs;

use miette::{Context, IntoDiagnostic};

use crate::stderr_log;

pub async fn read(root_dir: &Path) -> miette::Result<Vec<PathBuf>> {
    let mut servers = vec![];
    let mut paths = vec![root_dir.to_owned()];

    while let Some(path) = paths.iter().next_back() {
        let mut entries = fs::read_dir(path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("Failed to read entries from directory {}", path.display())
            })?;

        let mut path_additions: Vec<PathBuf> = vec![];

        while let Some(entry) = entries
            .next_entry()
            .await
            .into_diagnostic()
            .wrap_err("Bad dir entry")?
        {
            let entry_path = entry.path();

            if !entry_path.is_dir() {
                continue;
            }

            stderr_log!("Entry path: {entry_path:?}");

            if fs::try_exists(entry_path.join(METADATA_DIRECTORY))
                .await
                .into_diagnostic()
                .wrap_err("Failed to check if directory exists")?
            {
                servers.push(
                    entry_path
                        .strip_prefix(root_dir)
                        .into_diagnostic()
                        .wrap_err_with(|| {
                            format!(
                                "Failed to strip `{}` from `{}`",
                                display_path(root_dir),
                                display_path(&entry_path)
                            )
                        })?
                        .to_path_buf(),
                );
            } else {
                path_additions.push(path.join(entry_path));
            }
        }

        paths.pop();
        paths.extend(path_additions);
    }

    Ok(servers)
}
