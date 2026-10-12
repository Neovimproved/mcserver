use std::{io, result};

use derive_more::{Display, Error, From};

#[derive(Debug, Display, Error, From)]
pub enum Error {
    Io(io::Error),

    Bitcode(bitcode::Error),
}

pub type Result<T> = result::Result<T, Error>;
