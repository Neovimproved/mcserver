mod base_url;
mod create;
mod delete;
mod version;

mod fabric;
mod paper;

use std::str::FromStr;

use bitcode::{Decode, Encode};
use derive_more::Display;

pub use create::Creator;
pub use delete::delete;
pub use version::Version;

#[derive(Clone, Copy, Debug, Decode, Default, Display, Encode)]
pub enum ServerSoftwareType {
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

impl FromStr for ServerSoftwareType {
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
