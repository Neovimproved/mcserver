use std::{
    fmt::{self, Display},
    path::{Path, PathBuf},
};

use miette::{Context, IntoDiagnostic};
use tokio::fs::{self, DirEntry, ReadDir};

use crate::{
    consts::METADATA_DIRECTORY,
    error::{Error, display_path},
};

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerGroup {
    pub name: String,
    pub children: Vec<ServerEntry>,
    pub expanded: bool,
}

impl ServerGroup {
    pub fn new(name: String, children: Vec<ServerEntry>, expanded: bool) -> Self {
        Self {
            name,
            children,
            expanded,
        }
    }
}

impl Display for ServerGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.name)?;
        fmt_entries(&self.children, f, "", 3)
    }
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerObject {
    pub name: String,
    pub online: bool,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ServerEntry {
    Group(ServerGroup),
    Object(ServerObject),
}

impl ServerEntry {
    fn object(name: String, online: bool) -> Self {
        Self::Object(ServerObject { name, online })
    }
}

impl Display for ServerEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerEntry::Group(server_group) => write!(f, "{server_group}"),
            ServerEntry::Object(server_object) => write!(f, "{}", server_object.name),
        }
    }
}

pub fn fmt_entries(
    entries: &[ServerEntry],
    f: &mut fmt::Formatter,
    base_indentation: &str,
    indentation_width: usize,
) -> fmt::Result {
    for (idx, entry) in entries.iter().enumerate() {
        match entry {
            ServerEntry::Group(server_group) => {
                if idx == entries.len().saturating_sub(1) {
                    writeln!(f, "{base_indentation}└╴{}", server_group.name)?;
                    fmt_entries(
                        &server_group.children,
                        f,
                        &format!("{base_indentation}{}", " ".repeat(indentation_width)),
                        indentation_width,
                    )
                } else {
                    writeln!(f, "{base_indentation}├╴{}", server_group.name)?;
                    fmt_entries(
                        &server_group.children,
                        f,
                        &format!(
                            "{base_indentation}│{}",
                            " ".repeat(indentation_width.saturating_sub(1))
                        ),
                        indentation_width,
                    )
                }
            }
            ServerEntry::Object(server_object) => {
                if idx == entries.len().saturating_sub(1) {
                    writeln!(f, "{base_indentation}└╴{}", server_object.name)
                } else {
                    writeln!(f, "{base_indentation}├╴{}", server_object.name)
                }
            }
        }?;
    }

    Ok(())
}

struct ReadFrame {
    path: PathBuf,
    entries: Vec<ServerEntry>,
    read_dir: ReadDir,
}

impl ReadFrame {
    async fn try_new(path: PathBuf) -> miette::Result<Self> {
        let read_dir = fs::read_dir(&path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to read directory `{}`", display_path(&path)))?;

        Ok(Self {
            path,
            entries: vec![],
            read_dir,
        })
    }

    async fn next(&mut self) -> miette::Result<Option<DirEntry>> {
        self.read_dir
            .next_entry()
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to read from directory `{}`",
                    display_path(&self.path)
                )
            })
    }

    async fn try_into_group(self) -> miette::Result<ServerGroup> {
        Ok(ServerGroup {
            name: Path::new(
                self.path
                    .file_name()
                    .wrap_err("Failed to get basename of path")?,
            )
            .to_path_buf()
            .into_string()
            .map_err(Error::NonUtf8Path)
            .into_diagnostic()?,

            children: self.entries,
            expanded: false,
        })
    }
}

pub async fn read(dir: &Path) -> miette::Result<Vec<ServerEntry>> {
    let mut frames = vec![ReadFrame::try_new(dir.to_path_buf()).await?];

    loop {
        let frame = frames.last_mut().unwrap();
        let mut exhausted = true;

        while let Some(entry) = frame.next().await? {
            let entry_path = entry.path();

            let base_name = Path::new(entry_path.file_name().wrap_err_with(|| {
                format!(
                    "Failed to strip `{}` from `{}`",
                    display_path(dir),
                    display_path(&entry_path)
                )
            })?)
            .to_path_buf()
            .into_string()
            .map_err(Error::NonUtf8Path)
            .into_diagnostic()?;

            if fs::try_exists(entry_path.join(METADATA_DIRECTORY))
                .await
                .into_diagnostic()
                .wrap_err("Failed to check if directory exists")?
            {
                frame.entries.push(ServerEntry::object(base_name, false));
            } else {
                exhausted = false;
                frames.push(ReadFrame::try_new(entry_path).await?);
                break;
            }
        }

        if !exhausted {
            continue;
        }

        let mut frame = frames.pop().unwrap();

        frame.entries.sort_unstable();

        if let Some(parent) = frames.last_mut() {
            parent
                .entries
                .push(ServerEntry::Group(frame.try_into_group().await?));
        } else {
            break Ok(frame.entries);
        }
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    // Not to be confused with the TEMP_DIR
    fn tempdir_init() -> miette::Result<TempDir> {
        tempfile::tempdir()
            .into_diagnostic()
            .wrap_err("Failed to create temporary directory")
    }

    async fn new_dir(parent: &Path, name: impl AsRef<Path>) -> miette::Result<PathBuf> {
        let dir = parent.join(name);

        fs::create_dir(&dir)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to create directory `{}`", display_path(&dir)))?;

        Ok(dir)
    }

    async fn new_server(
        parent: &Path,
        name: impl AsRef<Path>,
    ) -> miette::Result<(PathBuf, PathBuf)> {
        let server_dir = new_dir(parent, name).await?;
        let metadata_dir = new_dir(&server_dir, METADATA_DIRECTORY).await?;

        Ok((server_dir, metadata_dir))
    }

    #[tokio::test]
    async fn entries() -> miette::Result<()> {
        let temp_dir = tempdir_init()?;
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
            ServerGroup::new("root".to_string(), read(servers_directory).await?, false)
        );

        let expected = "\
root
├╴group-0
│  ├╴server-0-group0
│  └╴server-1-group0
├╴group-1
│  ├╴server-0-group1
│  └╴server-1-group1
├╴subgroup-0-group-0
│  ├╴server-0-subgroup-0-group-0
│  └╴server-1-subgroup-0-group-0
├╴subgroup-0-group-1
│  ├╴server-0-subgroup-0-group-1
│  └╴server-1-subgroup-0-group-1
├╴subgroup-1-group-0
│  ├╴server-0-subgroup-1-group-0
│  └╴server-1-subgroup-1-group-0
└╴subgroup-1-group-1
   ├╴server-0-subgroup-1-group-1
   └╴server-1-subgroup-1-group-1
";

        assert_eq!(output, expected);

        Ok(())
    }
}
