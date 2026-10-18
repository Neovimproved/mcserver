use std::path::Path;

use tokio::fs::{self, ReadDir};

use miette::{Context, IntoDiagnostic};

use mcserver::error::{Error, display_path};

const METADATA_DIRECTORY: &str = ".mcserver";

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerGroup {
    pub name: String,
    pub children: Vec<ServerEntry>,
    pub expanded: bool,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerObject {
    pub name: String,
    pub online: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ServerEntry {
    Group(ServerGroup),
    Object(ServerObject),
}

impl ServerEntry {
    fn group(name: String, children: Vec<ServerEntry>) -> Self {
        Self::Group(ServerGroup {
            name,
            children,
            expanded: false,
        })
    }

    fn object(name: String, online: bool) -> Self {
        Self::Object(ServerObject { name, online })
    }
}

impl ServerEntry {
    fn name(&self) -> &str {
        match self {
            Self::Group(server_group) => &server_group.name,
            Self::Object(server_object) => &server_object.name,
        }
    }
}

impl Ord for ServerEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name().cmp(other.name())
    }
}

impl PartialOrd for ServerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub async fn make_read_dir(dir: &Path) -> miette::Result<ReadDir> {
    fs::read_dir(dir)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read directory `{}`", display_path(dir)))
}

#[async_recursion::async_recursion]
pub async fn read(dir: &Path) -> miette::Result<Vec<ServerEntry>> {
    let mut children: Vec<ServerEntry> = vec![];
    let mut read_dir = make_read_dir(dir).await?;

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "Failed to read entry from directory `{}`",
                display_path(dir)
            )
        })?
    {
        let entry_path = entry.path();

        let relative_path = entry_path
            .strip_prefix(dir)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to strip `{}` from `{}`",
                    display_path(dir),
                    display_path(&entry_path)
                )
            })?
            .to_path_buf()
            .into_string()
            .map_err(Error::NonUtf8Path)
            .into_diagnostic()?;

        if fs::try_exists(entry_path.join(METADATA_DIRECTORY))
            .await
            .into_diagnostic()
            .wrap_err("Failed to check if directory exists")?
        {
            children.push(ServerEntry::object(relative_path, false));
        } else {
            children.push(ServerEntry::group(relative_path, read(&entry_path).await?));
        }
    }

    children.sort_unstable();

    Ok(children)
}

fn print_entries(entries: &[ServerEntry], depth: usize) {
    let increased_depth = depth + 1;

    let base_indentation = " ".repeat(depth.saturating_sub(1) * 2);

    for entry in entries {
        match entry {
            ServerEntry::Group(server_group) => {
                println!("{base_indentation}v {}", server_group.name);
                print_entries(&server_group.children, increased_depth)
            }
            ServerEntry::Object(server_object) => {
                println!("{base_indentation}->{}", server_object.name);
            }
        }
    }
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let dirs = read(Path::new("/home/finnian/old-servers/")).await?;

    println!("Servers:");
    print_entries(&dirs, 1);

    Ok(())
}
