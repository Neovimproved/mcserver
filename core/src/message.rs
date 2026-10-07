use std::borrow::Cow;

use bitcode::{Decode, Encode};
use derive_more::Display;

fn display_bytes<'a>(bytes: &'a [u8]) -> Cow<'a, str> {
    str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .unwrap_or_else(|_| {
            Cow::Owned(format!(
                "(some non-utf-8 data has been lost) {}",
                String::from_utf8_lossy(bytes)
            ))
        })
}

#[derive(Debug, Decode, Display, Encode)]
pub enum ServerMessage {
    #[display("Child has not been successfully killed ({_0})")]
    KillFailure(String),

    #[display("Child has been killed")]
    KillSuccess,

    #[display("Message from server: {_0}")]
    Message(String),

    #[display("A server has not yet been created, so you cannot interact with one yet")]
    ServerNotStarted,

    #[display("A server has already started")]
    ServerAlreadyStarted,
}

#[derive(Debug, Decode, Display, Encode)]
pub enum ClientMessage {
    #[display("Start variant with name: {name} and flags: {}", flags.join(", "))]
    Start { name: String, flags: Vec<String> },

    #[display("Kill active process")]
    Kill,

    #[display("Program input: {}", display_bytes(_0))]
    ProgramInput(Vec<u8>),
}
