#![allow(unused)]

use std::{
    convert::Infallible,
    ops::Deref,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    str::FromStr,
};

use miette::{Context, IntoDiagnostic};

// Order:
// inherent impls
// owning/consuming
// borrowing/referencing

#[derive(Clone, Debug)]
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
