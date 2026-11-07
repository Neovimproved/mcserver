#![allow(dead_code)]
use std::{borrow::Cow, num::ParseIntError, str::FromStr};

use derive_more::{Display, Error, From};

#[derive(Clone, Copy, Debug, Display)]
pub enum ServerSoftware {
    Fabric,
    Forge,
    NeoForge,
    Paper,
    Purpur,
    Vanilla,
}

#[derive(Debug, Display)]
#[display("`{_0}` is not a valid server software")]
pub struct ParseSoftwareError(String);

impl ServerSoftware {
    fn try_from_str<'a>(value: impl Into<Cow<'a, str>>) -> Result<Self, ParseSoftwareError> {
        let value = value.into();

        Ok(match value.as_ref() {
            "fabric" => Self::Fabric,
            "forge" => Self::Forge,
            "neoforge" => Self::NeoForge,
            "paper" => Self::Paper,
            "purpur" => Self::Purpur,
            "vanilla" => Self::Vanilla,
            _ => return Err(ParseSoftwareError(value.into_owned())),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Version {
    pub major: usize,
    pub minor: usize,
    pub patch: Option<usize>,
}

#[derive(Debug, Display, Error, From)]
pub enum ParseVersionError {
    ParseIntError(ParseIntError),

    #[display("No major version was found")]
    NoMajor,

    #[display("No minor version was found")]
    NoMinor,

    #[display("Malformed version (received trailing period after minor version)")]
    MalformedTrailingPeriod,
}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let period = s.find('.').ok_or(ParseVersionError::NoMajor)?;

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
