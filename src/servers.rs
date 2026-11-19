use std::{
    collections::HashSet,
    fmt::{self, Display},
    path::{Path, PathBuf},
};

use color_eyre::eyre::{Context, ContextCompat, bail};
use derive_more::Display;
use indexmap::IndexMap;
use ratatui::{
    style::Stylize,
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};
use tokio::fs::{self, DirEntry, ReadDir};

use crate::{
    config::TerminalInterfaceConfig,
    error::{Error, display_path},
    log, metadata,
};

const SERVER_GROUP_ANSI_COLOR: u8 = 34;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ServerEntry {
    Group(ServerGroup),
    Object(ServerObject),
}

impl ServerEntry {
    pub fn ty(&self) -> ServerEntryType {
        match self {
            ServerEntry::Group(_) => ServerEntryType::Group,
            ServerEntry::Object(_) => ServerEntryType::Object,
        }
    }
}

impl ServerEntry {
    fn object(online: bool) -> Self {
        Self::Object(ServerObject { online })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerGroup {
    pub children: IndexMap<String, ServerEntry>,
    #[serde(skip)]
    pub expanded: bool,
}

impl ServerGroup {
    pub fn new(children: IndexMap<String, ServerEntry>, expanded: bool) -> Self {
        Self { children, expanded }
    }

    fn find_first_online(&self) -> Option<&ServerObject> {
        for child in self.children.values() {
            match child {
                ServerEntry::Group(server_group) => {
                    if let Some(active) = server_group.find_first_online() {
                        return Some(active);
                    }
                }
                ServerEntry::Object(server_object) => {
                    if server_object.online {
                        return Some(server_object);
                    }
                }
            }
        }

        None
    }

    pub fn fold_to_line<'a>(
        &'a self,
        name: &'a str,
        mut spans: Vec<Span<'a>>,
        is_selected: bool,
        ti_cfg: &'a TerminalInterfaceConfig,
    ) -> Line<'a> {
        spans.push(if self.expanded {
            ti_cfg
                .group_expanded_symbol
                .as_str()
                .fg(ti_cfg.group_symbol_color)
        } else {
            let group_folded_sym = ti_cfg.group_folded_symbol.as_str();

            if self.find_first_online().is_some() {
                group_folded_sym.fg(ti_cfg.server_online_color)
            } else {
                group_folded_sym.fg(ti_cfg.group_symbol_color)
            }
        });

        spans.push(Span::from(name));

        if is_selected {
            Line::from(spans).bg(ti_cfg.selected_color)
        } else {
            Line::from(spans)
        }
    }
}

impl Display for ServerGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        display_entries(&self.children, f, "", f.width().unwrap_or(3), f.alternate())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerObject {
    pub online: bool,
}

impl ServerObject {
    pub fn fold_to_line<'a>(
        &'a self,
        name: &'a str,
        mut spans: Vec<Span<'a>>,
        is_attached: bool,
        is_selected: bool,
        ti_cfg: &'a TerminalInterfaceConfig,
    ) -> Line<'a> {
        spans.extend(if self.online || is_attached {
            [
                ti_cfg
                    .server_online_symbol
                    .as_str()
                    .fg(ti_cfg.server_online_color),
                Span::from(name),
            ]
        } else {
            [
                ti_cfg
                    .server_offline_symbol
                    .as_str()
                    .fg(ti_cfg.server_offline_color),
                Span::from(name),
            ]
        });

        if is_selected {
            Line::from(spans).bg(ti_cfg.selected_color)
        } else {
            Line::from(spans)
        }
    }
}

#[derive(Clone, Copy, Debug, Display)]
pub enum ServerEntryType {
    #[display("directory of servers")]
    Group,
    #[display("server")]
    Object,
}

pub trait ReplicateExpansion {
    fn replicate_expansion_to(&self, other: &mut Self);
}

impl ReplicateExpansion for IndexMap<String, ServerEntry> {
    fn replicate_expansion_to(&self, other: &mut Self) {
        for (name, entry) in self {
            if let ServerEntry::Group(server_group) = entry
                && let Some(ServerEntry::Group(other_group)) = other.get_mut(name)
            {
                other_group.expanded = server_group.expanded;
                server_group
                    .children
                    .replicate_expansion_to(&mut other_group.children);
            }
        }
    }
}

pub fn display_entries(
    entries: &IndexMap<String, ServerEntry>,
    f: &mut fmt::Formatter,
    base_indentation: &str,
    indentation_width: usize,
    ansi_colors: bool,
) -> fmt::Result {
    for (idx, (name, entry)) in entries.iter().enumerate() {
        match entry {
            ServerEntry::Group(server_group) => {
                if idx == entries.len() - 1 {
                    write!(f, "\n{base_indentation}└──")?;

                    if ansi_colors {
                        write!(f, "\x1b[1;{SERVER_GROUP_ANSI_COLOR}m{}\x1b[0m", name)?;
                    } else {
                        write!(f, "{}", name)?;
                    }

                    display_entries(
                        &server_group.children,
                        f,
                        &format!("{base_indentation}{}", " ".repeat(indentation_width)),
                        indentation_width,
                        ansi_colors,
                    )
                } else {
                    write!(f, "\n{base_indentation}├──")?;

                    if ansi_colors {
                        write!(f, "\x1b[1;34m{}\x1b[0m", name)?;
                    } else {
                        write!(f, "{}", name)?;
                    }

                    display_entries(
                        &server_group.children,
                        f,
                        &format!(
                            "{base_indentation}│{}",
                            " ".repeat(indentation_width.saturating_sub(1))
                        ),
                        indentation_width,
                        ansi_colors,
                    )
                }
            }
            // TODO: show if online
            ServerEntry::Object(_server_object) => {
                if idx == entries.len() - 1 {
                    write!(f, "\n{base_indentation}└──")?;
                } else {
                    write!(f, "\n{base_indentation}├──")?;
                }

                // if ansi_colors {
                //     writeln!(f, "\x1b[1;33m{}\x1b[0m", server_object.name)
                // } else {
                //     writeln!(f, "{}", server_object.name)
                // }

                write!(f, "{}", name)
            }
        }?;
    }

    Ok(())
}

struct ReadFrame {
    path: PathBuf,
    entries: IndexMap<String, ServerEntry>,
    read_dir: ReadDir,
}

impl ReadFrame {
    async fn try_new(path: PathBuf) -> color_eyre::Result<Self> {
        let read_dir = fs::read_dir(&path)
            .await
            .wrap_err_with(|| format!("Failed to read directory `{}`", display_path(&path)))?;

        Ok(Self {
            path,
            entries: IndexMap::new(),
            read_dir,
        })
    }

    async fn next(&mut self) -> color_eyre::Result<Option<DirEntry>> {
        self.read_dir.next_entry().await.wrap_err_with(|| {
            format!(
                "Failed to read from directory `{}`",
                display_path(&self.path)
            )
        })
    }
}

impl From<ReadFrame> for ServerGroup {
    fn from(val: ReadFrame) -> Self {
        Self {
            children: val.entries,
            expanded: false,
        }
    }
}

pub async fn read(
    root_dir: &Path,
    online: HashSet<String>,
) -> color_eyre::Result<IndexMap<String, ServerEntry>> {
    let mut frames = vec![ReadFrame::try_new(root_dir.to_path_buf()).await?];

    println!("Online: {online:?}");

    loop {
        let frame = frames.last_mut().unwrap();
        let mut exhausted = true;

        while let Some(entry) = frame.next().await? {
            let entry_path = entry.path();

            let base_name = Path::new(entry_path.file_name().wrap_err_with(|| {
                format!(
                    "Failed to strip `{}` from `{}`",
                    display_path(root_dir),
                    display_path(&entry_path)
                )
            })?)
            .to_path_buf()
            .into_string()
            .map_err(Error::NonUtf8Path)?;

            if fs::try_exists(entry_path.join(metadata::DIRECTORY))
                .await
                .wrap_err("Failed to check if directory exists")?
            {
                let entry_path = entry.path();
                let relative_path = entry_path
                    .strip_prefix(root_dir)
                    .wrap_err("Failed to strip root dir from path")?
                    .to_str()
                    .wrap_err("Failed to convert path to string")?;

                frame.entries.insert(
                    base_name,
                    ServerEntry::object(online.contains(relative_path)),
                );
            } else {
                exhausted = false;
                frames.push(ReadFrame::try_new(entry_path).await?);
                break;
            }
        }

        if !exhausted {
            continue;
        }

        let Some(mut frame) = frames.pop() else {
            bail!("Something went wrong")
        };

        frame.entries.sort_unstable_keys();

        if let Some(parent) = frames.last_mut() {
            let name = Path::new(
                frame
                    .path
                    .file_name()
                    .wrap_err("Failed to get basename of path")?,
            )
            .to_path_buf()
            .into_string()
            .map_err(Error::NonUtf8Path)?;

            parent
                .entries
                .insert(name, ServerEntry::Group(frame.into()));
        } else {
            break Ok(frame.entries);
        }
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use crate::test_utils::{new_dir, tempdir_init_eyre};

    use super::*;

    async fn new_server(
        parent: &Path,
        name: impl AsRef<Path>,
    ) -> color_eyre::Result<(PathBuf, PathBuf)> {
        let server_dir = new_dir(parent, name).await?;
        let metadata_dir = new_dir(&server_dir, metadata::DIRECTORY).await?;

        Ok((server_dir, metadata_dir))
    }

    #[tokio::test]
    async fn entries() -> color_eyre::Result<()> {
        let temp_dir = tempdir_init_eyre()?;
        let servers_directory = temp_dir.path();

        for i in 0..2 {
            let group = new_dir(servers_directory, format!("group-{i}")).await?;

            for j in 0..2 {
                new_server(&group, format!("server-{j}-group{i}")).await?;

                let subgroup =
                    new_dir(servers_directory, format!("subgroup-{j}-group-{i}")).await?;

                for k in 0..2 {
                    new_server(&subgroup, format!("server-{k}-subgroup-{j}-group-{i}")).await?;
                }
            }
        }

        let output = format!(
            "{}",
            ServerGroup::new(read(servers_directory, HashSet::new()).await?, false)
        );

        let expected = "
├──group-0
│  ├──server-0-group0
│  └──server-1-group0
├──group-1
│  ├──server-0-group1
│  └──server-1-group1
├──subgroup-0-group-0
│  ├──server-0-subgroup-0-group-0
│  └──server-1-subgroup-0-group-0
├──subgroup-0-group-1
│  ├──server-0-subgroup-0-group-1
│  └──server-1-subgroup-0-group-1
├──subgroup-1-group-0
│  ├──server-0-subgroup-1-group-0
│  └──server-1-subgroup-1-group-0
└──subgroup-1-group-1
   ├──server-0-subgroup-1-group-1
   └──server-1-subgroup-1-group-1";

        assert_eq!(output, expected);

        Ok(())
    }
}
