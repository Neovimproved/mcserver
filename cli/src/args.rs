use std::net::IpAddr;

use clap::{Parser, Subcommand, ValueHint};

use crate::types::ServerArgument;

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(short, long, num_args=0..=1, conflicts_with = "unix_socket", default_missing_value = "127.0.0.1")]
    pub tcp: Option<IpAddr>,

    #[arg(short, long, conflicts_with = "tcp")]
    pub unix_socket: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(visible_alias = "a", about = "Directly attach to a server")]
    Attach {
        #[arg(value_hint = ValueHint::DirPath)]
        // TODO: maybe make optional (attach to default if none)
        server: ServerArgument,
    },
}
