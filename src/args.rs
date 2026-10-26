use std::{
    convert::Infallible,
    net::IpAddr,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use clap::{Parser, Subcommand, ValueHint};
use derive_more::Display;
use miette::{Context, IntoDiagnostic};

pub const DAEMONIZE_OPTION_NAME: &str = "daemonize";
pub const NO_DAEMONIZE_OPTION_NAME: &str = "no-daemonize";

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(short, long, num_args=0..=1, conflicts_with_all = ["unix_socket", "server"], default_missing_value = "127.0.0.1")]
    pub tcp: Option<IpAddr>,

    #[arg(short, long, conflicts_with_all = ["tcp", "server"])]
    pub unix_socket: bool,

    #[arg(
        long,
        conflicts_with_all = ["tcp", "unix_socket"],
        help = "Spin up a server instance to handle connections",
        num_args = 0..=1,
        default_missing_value = DAEMONIZE_OPTION_NAME,
        value_parser=[DAEMONIZE_OPTION_NAME, NO_DAEMONIZE_OPTION_NAME],
        require_equals = true,
    )]
    pub server: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(visible_alias = "a", about = "Directly attach to a server")]
    Attach {
        #[arg(value_hint = ValueHint::DirPath)]
        server: Option<ServerArgument>,
    },

    #[command(about = "Output a tree-like output of all of the servers")]
    Tree,
}

#[derive(Clone, Debug, Display)]
#[display("{_0}")]
pub struct ServerArgument(String);

impl ServerArgument {
    fn into_name(self) -> String {
        self.0
    }

    fn into_server_path(self) -> ServerPath {
        ServerPath::from(PathBuf::from(self.0))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<ServerArgument> for String {
    fn from(value: ServerArgument) -> Self {
        value.0
    }
}

impl FromStr for ServerArgument {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl AsRef<str> for ServerArgument {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug)]
pub struct ServerPath(PathBuf);

impl ServerPath {
    fn relative_path(&self, server_path: &Path) -> miette::Result<&Path> {
        self.strip_prefix(server_path)
            .into_diagnostic()
            .wrap_err("Failed to strip prefix")
    }

    fn try_as_server_argument(&self, server_path: &Path) -> miette::Result<ServerArgument> {
        let relative_path = self.relative_path(server_path)?;

        ServerArgument::from_str(
            relative_path
                .to_str()
                .wrap_err("Path contains non utf-8 bytes")?,
        )
        .into_diagnostic()
        .wrap_err("Failed to construct server argument")
    }

    fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for ServerPath {
    fn from(value: PathBuf) -> Self {
        Self(value)
    }
}

impl AsRef<Path> for ServerPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl Deref for ServerPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}
