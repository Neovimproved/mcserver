use std::{convert::Infallible, str::FromStr};

use clap::{Parser, Subcommand, ValueHint, command};

#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Attach {
        #[arg(value_hint = ValueHint::DirPath)]
        server: Option<ServerArgument>,
    },
}

#[derive(Clone, Debug)]
pub struct ServerArgument {
    name: String,
}

impl FromStr for ServerArgument {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            name: s.to_string(),
        })
    }
}
