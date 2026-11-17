use std::{fmt, num::ParseIntError, str::FromStr};

use bitcode::{Decode, Encode};
use derive_more::{Display, Error, From};

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
