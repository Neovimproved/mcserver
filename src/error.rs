use std::{
    env::VarError,
    io,
    net::AddrParseError,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    result,
    time::SystemTimeError,
};

use derive_more::{Display, Error, From};
use kdl::KdlError;
use shellexpand::LookupError;

use crate::{metadata::MetadataError, parser::ParseConfigError};

pub fn display_path(path: &Path) -> String {
    let mut buf = String::new();

    for chunk in path.as_os_str().as_bytes().utf8_chunks() {
        buf.push_str(chunk.valid());

        if !chunk.invalid().is_empty() {
            buf.push('\u{FFFD}');
        }
    }

    buf
}

#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display("{_0}")]
    AddressParse(AddrParseError),

    #[display("{_0}")]
    Bitcode(bitcode::Error),

    #[display("Home directory was not found")]
    HomeDirectoryNotFound,

    #[display("{_0}")]
    Io(io::Error),

    #[display("{_0}")]
    KdlParse(KdlError),

    #[display("{_0}")]
    Lookup(LookupError<VarError>),

    #[display("{_0}")]
    Metadata(MetadataError),

    // TODO: replace non utf-8 bytes with question mark symbol
    #[display("`{}` could not be converted to string", display_path(_0))]
    NonUtf8Path(#[error(not(source))] PathBuf),

    #[display("{_0}")]
    SystemTime(SystemTimeError),

    #[display("Connection type {_0} is not supported")]
    UnsupportedConnectionType(#[error(not(source))] String),

    #[display("Config directory was not found")]
    ConfigDirectoryNotFound,

    #[display("{_0}")]
    ParseConfig(ParseConfigError),
}

pub type Result<T> = result::Result<T, Error>;
