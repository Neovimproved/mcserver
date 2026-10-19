use std::{borrow::Cow, collections::HashSet};

use bitcode::{Decode, Encode};
use derive_more::Display;

use crate::line_buffer::LineBuffer;

fn terminal_like_bytes(bytes: &str, normal_end: usize) -> String {
    let mut acc = String::from(&bytes[..normal_end]);

    bytes[normal_end..].chars().for_each(|ch| {
        let num = ch as u32;
        match num {
            0..31 => unsafe { acc.push_str(str::from_utf8_unchecked(&[b'^', ch as u8 + 64])) },
            _ => acc.push(ch),
        }
    });

    acc
}

pub fn display_bytes<'a>(bytes: &'a [u8]) -> Cow<'a, str> {
    let string = String::from_utf8_lossy(bytes);

    for (idx, ch) in string.char_indices() {
        match ch as u32 {
            // tab, newline, carriage return (legacy costs)
            9 | 10 | 13 => {}
            0..=31 => return Cow::Owned(terminal_like_bytes(&string, idx)),
            _ => {}
        }
    }

    string
}

#[derive(Debug, Decode, Clone, Encode, Display)]
#[display("Synchronization payload")]
pub struct SynchronizationPayload {
    pub online_servers: HashSet<String>,
}

// TODO: use this instead of forwarding server messages
#[derive(Debug)]
#[allow(dead_code)]
pub enum ServerInstruction {}

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

    #[display("Line history sent:\n{}", _0.iter().fold(String::new(), |mut acc, vec| {
        acc.push_str(&display_bytes(vec));
        acc.push('\n');
        acc
    }))]
    History(LineBuffer<Vec<u8>>),

    SynchronizationPayload(SynchronizationPayload),
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

    #[display("Request for synchronization of servers")]
    SynchronizationRequest,
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
            $crate::write_bytes!($writer, encoded_message).await
        }
    };
}

#[macro_export]
macro_rules! read_message {
    ($reader:ident) => {
        async {
            let length = $reader.read_u32().await?;
            $crate::read_message!($reader, length).await
        }
    };

    ($reader:ident, $length:ident) => {
        async {
            let mut buf = vec![0u8; $length as usize];
            $reader.read_exact(&mut buf).await?;

            $crate::error::Result::<_>::Ok(bitcode::decode(&buf)?)
        }
    };
}
