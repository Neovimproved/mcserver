use std::path::Path;

use tokio::fs;

use miette::{Context, IntoDiagnostic, miette};

#[cfg(not(feature = "servers-only"))]
use crate::{Error, display_path};

#[cfg(feature = "servers-only")]
use crate::Error;

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
    fn group(name: String) -> Self {
        Self::Group(ServerGroup {
            name,
            children: vec![],
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

pub async fn read(root_dir: &Path) -> miette::Result<Vec<ServerEntry>> {
    let mut iterators = vec![
        fs::read_dir(root_dir)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to read entries from directory `{}`",
                    display_path(root_dir)
                )
            })?,
    ];

    let root: Vec<ServerEntry> = vec![];
    let ancestors = vec![(&mut root, root_dir.to_path_buf())];

    while let Some(iter) = iterators.last_mut() {
        let Some(entry) = iter
            .next_entry()
            .await
            .into_diagnostic()
            .wrap_err("Failed to read entry")?
        else {
            iterators.pop();
            ancestors.pop();
            continue;
        };

        let entry_path = entry.path();
        let (parent_container, parent_path) = ancestors.last_mut().unwrap();

        let relative_path = entry_path
            .strip_prefix(parent_path)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to strip `{}` from `{}`",
                    display_path(parent_path),
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
            parent_container.push(ServerEntry::object(relative_path, false));
        } else {
            parent_container.push_mut(ServerEntry::Group());
                .last_mut()
                .unwrap()
                .push(ServerEntry::Group(ServerGroup {
                    children: vec![],
                    name: relative_name,
                    expanded: false,
                }));
        }
    }

    Ok(root)
}

fn print_entries(entries: &[ServerEntry], depth: usize) {
    let increased_depth = depth + 1;

    let group_indentation = " ".repeat(depth * 2);
    let object_indentation = format!("{}{}", " ".repeat(depth.saturating_sub(1) * 2), "->");

    for entry in entries {
        match entry {
            ServerEntry::Group(server_group) => {
                println!("{group_indentation}{}", server_group.name);
                print_entries(&server_group.children, increased_depth)
            }
            ServerEntry::Object(server_object) => {
                println!("{object_indentation}{}", server_object.name);
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
