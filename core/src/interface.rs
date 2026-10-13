use std::borrow::Cow;

use bitcode::{Decode, Encode};
use derive_more::Display;

use crate::LineBuffer;

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

#[derive(Clone, Debug, Decode, Display, Encode)]
pub enum ServerMessage {
    #[display("Child has not been successfully killed ({_0})")]
    KillFailure(String),

    #[display("Child has been killed")]
    KillSuccess,

    #[display("Child has naturally closed")]
    Closed,

    #[display("Message from server: {_0}")]
    Message(String),

    #[display("A server has already started")]
    AlreadyStarted,

    #[display("A server has not yet been created, so you cannot interact with one yet")]
    NotAttached,

    #[display("Output from the server: {}", display_bytes(_0))]
    Output(Vec<u8>),

    #[display("Stderr from the application: {}", display_bytes(_0))]
    Error(Vec<u8>),

    #[display("Line history sent:{}", _0.iter().fold(String::new(), |mut acc, item| {
        acc.push('\n');
        acc.push_str(item);
        acc
    }))]
    History(LineBuffer),
}

#[derive(Debug, Decode, Display, Encode)]
pub enum ClientMessage {
    #[display("Attach to server {name}")]
    Attach { name: String },

    #[display("Detach from the current server")]
    Detach,

    #[display("Kill active process")]
    Kill,

    #[display("Program input: {}", display_bytes(_0))]
    ProgramInput(Vec<u8>),
}

#[macro_export]
macro_rules! write_bytes {
    ($writer:ident, $bytes:expr) => {
        async {
            $writer.write_u32($bytes.len() as u32).await?;
            $writer.write_all(&$bytes).await?;

            io::Result::<()>::Ok(())
        }
    };
}

#[macro_export]
macro_rules! write_message {
    ($writer:ident, $message:expr) => {
        async {
            let encoded_message = bitcode::encode(&$message);
            mcserver_core::write_bytes!($writer, encoded_message).await
        }
    };
}

#[macro_export]
macro_rules! read_message {
    ($reader:ident) => {
        async {
            let length = $reader.read_u32().await?;
            read_message!($reader, length).await
        }
    };

    ($reader:ident, $length:ident) => {
        async {
            let mut buf = vec![0u8; $length as usize];
            $reader.read_exact(&mut buf).await?;

            mcserver_core::Result::<_>::Ok(bitcode::decode(&buf)?)
        }
    };
}
