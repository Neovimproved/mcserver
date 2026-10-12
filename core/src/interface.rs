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

    #[display("A server has already started")]
    AlreadyStarted,

    #[display("A server has not yet been created, so you cannot interact with one yet")]
    NotStarted,

    #[display("Output from the server: {}", display_bytes(_0))]
    Output(Vec<u8>),
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

#[macro_export]
macro_rules! write_message {
    ($writer:ident, $message:expr) => {
        async {
            let encoded_message = bitcode::encode(&$message);
            $writer.write_u32(encoded_message.len() as u32).await?;
            $writer.write_all(&encoded_message).await?;

            mcserver_core::Result::<()>::Ok(())
        }
    };
}

#[macro_export]
macro_rules! read_message {
    ($t:ty, $reader:ident) => {
        async {
            let length = $reader.read_u32().await?;
            read_message!($t, $reader, length).await
        }
    };

    ($t:ty, $reader:ident, $length:ident) => {
        async {
            let mut buf = vec![0u8; $length as usize];
            $reader.read_exact(&mut buf).await?;

            mcserver_core::Result::<$t>::Ok(bitcode::decode(&buf)?)
        }
    };
}
