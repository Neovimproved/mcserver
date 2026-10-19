use std::io;

use derive_more::{Display, Error, From};

#[derive(Debug, Display, Error, From)]
enum Error {
    #[display("{_0}")]
    Io(io::Error),
    // #[display("{_0}")]
    //
}
