use std::{
    borrow::Cow,
    fmt,
    num::ParseIntError,
    path::{Path, PathBuf},
    str::FromStr,
};

use bitcode::{Decode, Encode};
use color_eyre::eyre::{Context, Result};
use derive_more::{Display, Error, From};
use tokio::fs;

use crate::{messages::CreateMessage, metadata};

#[derive(Clone, Copy, Debug, Decode, Default, Display, Encode)]
pub enum ServerSoftware {
    #[display("fabric")]
    Fabric,
    #[display("forge")]
    Forge,
    #[display("neoforge")]
    NeoForge,
    #[display("paper")]
    Paper,
    #[display("purpur")]
    Purpur,
    #[default]
    #[display("vanilla")]
    Vanilla,
}

impl FromStr for ServerSoftware {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "fabric" => Self::Fabric,
            "forge" => Self::Forge,
            "neoforge" => Self::NeoForge,
            "paper" => Self::Paper,
            "purpur" => Self::Purpur,
            "vanilla" => Self::Vanilla,
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, Debug, Decode, Encode)]
pub struct Version {
    pub major: usize,
    pub minor: usize,
    pub patch: Option<usize>,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;

        if let Some(patch) = self.patch {
            write!(f, ".{patch}")
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Display, Error, From)]
pub enum ParseVersionError {
    ParseIntError(ParseIntError),

    #[display("A minor version was not given")]
    NoMinor,

    #[display("Malformed version (received trailing period after minor version)")]
    MalformedTrailingPeriod,
}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let period = s.find('.').ok_or(ParseVersionError::NoMinor)?;

        let major = s[..period].parse()?;

        let remaining = s.get(period + 1..).ok_or(ParseVersionError::NoMinor)?;

        let (minor, patch) = if let Some(period) = remaining.find('.') {
            let minor = remaining[..period].parse()?;
            let patch = remaining
                .get(period + 1..)
                .ok_or(ParseVersionError::MalformedTrailingPeriod)?
                .parse()?;

            (minor, Some(patch))
        } else {
            (remaining.parse()?, None)
        };

        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

pub async fn create(create_message: CreateMessage) -> Result<()> {
    let rel_server_dir = shellexpand::full(&create_message.path).wrap_err("Failed to expand")?;
    let metadata_dir = metadata::dir(match rel_server_dir {
        Cow::Borrowed(s) => Cow::Borrowed(Path::new(s)),
        Cow::Owned(s) => Cow::Owned(PathBuf::from(s)),
    });

    fs::create_dir_all(metadata_dir)
        .await
        .wrap_err("Failed to create metadata directory")?;

    Ok(())
}
