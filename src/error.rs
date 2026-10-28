use std::{
    env::VarError,
    io,
    net::AddrParseError,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    result,
};

use derive_more::{Display, Error, From};
use kdl::KdlError;
use shellexpand::LookupError;

use crate::{config::ParseConfigError, metadata::MetadataError};

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
    AddressParse(AddrParseError),

    Bitcode(bitcode::Error),

    #[display("File name of path `{}` could not be found", display_path(_0))]
    #[from(ignore)]
    FileNameNotFound(#[error(not(source))] PathBuf),

    #[display("Home directory was not found")]
    HomeDirectoryNotFound,

    Io(io::Error),

    KdlParse(KdlError),

    Lookup(LookupError<VarError>),

    Metadata(MetadataError),

    #[display("`{}` could not be converted to string", display_path(_0))]
    #[from(ignore)]
    NonUtf8Path(#[error(not(source))] PathBuf),

    #[display("Connection type {_0} is not supported")]
    UnsupportedConnectionType(#[error(not(source))] String),

    #[display("Config directory was not found")]
    ConfigDirectoryNotFound,

    ParseConfig(ParseConfigError),
}

pub type Result<T> = result::Result<T, Error>;
