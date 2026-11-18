use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    fmt,
};

use bitcode::{Decode, Encode};
use derive_more::Display;

use crate::{
    crud::{ServerSoftwareType, Version},
    line_buffer::LineBuffer,
    servers::ServerEntry,
};

pub const MAX_MESSAGE_LENGTH: usize = 1024 * 1024;

fn terminal_like_bytes(bytes: &str, normal_end: usize) -> String {
    let mut acc = String::from(&bytes[..normal_end]);

    bytes[normal_end..].chars().for_each(|ch| {
        let num = ch as u32;
        match num {
            // 9: horizontal tab
            // 10: newline
            // 11: vertical tab
            // 12: form feed
            // 13: carriage return
            // (legacy costs)
            9..=13 => acc.push(ch),
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
            9..=13 => {}
            0..=31 => return Cow::Owned(terminal_like_bytes(&string, idx)),
            _ => {}
        }
    }

    string
}

#[derive(Debug)]
pub enum ServerInstruction {
    Kill,

    SendInput(Vec<u8>),
}

#[derive(Clone, Debug, Decode, Display, Encode)]
#[display("Encodable server entries (cannot be displayed as it has been serialized into binary)")]
pub struct EncodableServerEntries(Vec<u8>);

impl EncodableServerEntries {
    pub fn serialize(value: &BTreeMap<String, ServerEntry>) -> Result<Self, bitcode::Error> {
        bitcode::serialize(value).map(Self)
    }

    pub fn deserialize(&self) -> Result<BTreeMap<String, ServerEntry>, bitcode::Error> {
        bitcode::deserialize(&self.0)
    }
}

#[derive(Clone, Debug, Decode, Display, Encode)]
pub enum ServerMessage {
    #[display("A server has already started")]
    AlreadyStarted,

    #[display("Child has naturally closed")]
    Closed,

    #[display("Line history sent:\n{}", _0.iter().fold(String::new(), |mut acc, vec| {
        acc.push_str(&display_bytes(vec));
        acc.push('\n');
        acc
    }))]
    History(LineBuffer<Vec<u8>>),

    #[display("Child has not been successfully killed ({_0})")]
    KillFailure(String),

    #[display("Message from server: {_0}")]
    Message(String),

    #[display("A server has not yet been created, so you cannot interact with one yet")]
    NotAttached,

    #[display("Output from the server: {}", display_bytes(_0))]
    Output(Vec<u8>),

    #[display("{}", OnlineServersView(_0))]
    OnlineSynchronization(HashSet<String>),

    EntriesSynchronization(EncodableServerEntries),
}

struct OnlineServersView<'a>(&'a HashSet<String>);

impl<'a> fmt::Display for OnlineServersView<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Online servers: ()")?;

        for (idx, server) in self.0.iter().enumerate() {
            write!(f, "{server}")?;

            if idx != self.0.len() - 1 {
                write!(f, ", ")?;
            }
        }

        write!(f, ")")?;

        Ok(())
    }
}

#[derive(Debug, Decode, Encode)]
pub struct CreateMessage {
    pub path: String,
    pub server_software: ServerSoftwareType,
    pub version: Option<Version>,
}

impl fmt::Display for CreateMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Create server with path `{}`, software `{}`, version `{}`",
            self.path,
            self.server_software,
            self.version
                .map(|v| v.to_string())
                .as_deref()
                .unwrap_or("latest")
        )
    }
}

#[derive(Debug, Decode, Display, Encode)]
pub enum ClientMessage {
    #[display("Attach to server {server}")]
    Attach {
        server: String,
    },

    Create(CreateMessage),

    #[display("Detach from the current server")]
    Detach,

    #[display("Delete server with path {server}")]
    Delete {
        server: String,
    },

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
