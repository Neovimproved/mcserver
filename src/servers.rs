use std::{
    fmt::{self, Display},
    io::Write,
    path::Path,
};

use tokio::{
    fs::{self, ReadDir},
    io,
};

use miette::{Context, IntoDiagnostic};

use mcserver::error::{Error, display_path};

const METADATA_DIRECTORY: &str = ".mcserver";

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerGroup {
    pub name: String,
    pub children: Vec<ServerEntry>,
    pub expanded: bool,
}

impl ServerGroup {
    fn new(name: String, children: Vec<ServerEntry>, expanded: bool) -> Self {
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
        fmt_entries(&self.children, f, "")
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
    fn group(name: String, children: Vec<ServerEntry>) -> Self {
        Self::Group(ServerGroup::new(name, children, false))
    }

    fn object(name: String, online: bool) -> Self {
        Self::Object(ServerObject { name, online })
    }
}

impl Display for ServerEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerEntry::Group(server_group) => fmt_entries(&server_group.children, f, ""),
            ServerEntry::Object(server_object) => write!(f, "{}", server_object.name),
        }
    }
}

pub fn fmt_entries(
    entries: &[ServerEntry],
    f: &mut fmt::Formatter,
    base_indentation: &str,
) -> fmt::Result {
    for (idx, entry) in entries.iter().enumerate() {
        match entry {
            ServerEntry::Group(server_group) => {
                if idx == entries.len().saturating_sub(1) {
                    writeln!(f, "{base_indentation}└╴{}", server_group.name)?;
                    fmt_entries(&server_group.children, f, &format!("{base_indentation}  "))
                } else {
                    writeln!(f, "{base_indentation}├╴{}", server_group.name)?;
                    fmt_entries(&server_group.children, f, &format!("{base_indentation}│ "))
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

// TODO: replace these functions with tests

fn write_entries(
    writer: &mut std::io::Stdout,
    entries: &[ServerEntry],
    depth: usize,
) -> io::Result<()> {
    let increased_depth = depth + 1;

    let base_indentation = " ".repeat(depth.saturating_sub(1) * 2);

    for entry in entries {
        match entry {
            ServerEntry::Group(server_group) => {
                write!(writer, "{base_indentation}v {}", server_group.name)?;
                write_entries(writer, &server_group.children, increased_depth)?
            }
            ServerEntry::Object(server_object) => {
                println!("{base_indentation}->{}", server_object.name);
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
#[tokio::main]
async fn main() -> miette::Result<()> {
    let dirs = read(Path::new("/home/finnian/old-servers/")).await?;

    println!("Servers:");
    // write_entries(io::stdout(), &dirs, 1);

    Ok(())
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

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
│ ├╴server-0-group0
│ └╴server-1-group0
├╴group-1
│ ├╴server-0-group1
│ └╴server-1-group1
├╴subgroup-0-group-0
│ ├╴server-0-subgroup-0-group-0
│ └╴server-1-subgroup-0-group-0
├╴subgroup-0-group-1
│ ├╴server-0-subgroup-0-group-1
│ └╴server-1-subgroup-0-group-1
├╴subgroup-1-group-0
│ ├╴server-0-subgroup-1-group-0
│ └╴server-1-subgroup-1-group-0
└╴subgroup-1-group-1
  ├╴server-0-subgroup-1-group-1
  └╴server-1-subgroup-1-group-1
";

        assert_eq!(output, expected);

        Ok(())
    }
}
