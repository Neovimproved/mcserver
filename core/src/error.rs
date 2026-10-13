use std::{io, net::AddrParseError, result};

use derive_more::{Display, Error, From};

#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display("{_0}")]
    Io(io::Error),

    #[display("{_0}")]
    Bitcode(bitcode::Error),

    #[display("Connection type {_0} is not supported")]
    UnsupportedConnectionType(#[error(not(source))] String),

    #[display("{_0}")]
    AddressParse(AddrParseError),
}

pub type Result<T> = result::Result<T, Error>;
