use std::{io, net::AddrParseError, result};

use derive_more::{Display, Error, From};
use kdl::KdlError;

use crate::parser::ParseConfigError;

#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display("{_0}")]
    AddressParse(AddrParseError),

    #[display("{_0}")]
    Bitcode(bitcode::Error),

    #[display("{_0}")]
    Io(io::Error),

    #[display("{_0}")]
    KdlParse(KdlError),

    #[display("Connection type {_0} is not supported")]
    UnsupportedConnectionType(#[error(not(source))] String),

    #[display("Home directory was not found")]
    HomeDirectoryNotFound,

    #[display("Config directory was not found")]
    ConfigDirectoryNotFound,

    #[display("{_0}")]
    ParseConfig(ParseConfigError),
}

pub type Result<T> = result::Result<T, Error>;
