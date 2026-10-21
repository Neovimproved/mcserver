use std::net::IpAddr;

use clap::{Parser, Subcommand, ValueHint};

use crate::client::ServerArgument;

pub const DAEMONIZE_OPTION_NAME: &str = "daemonize";
pub const NO_DAEMONIZE_OPTION_NAME: &str = "no-daemonize";

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(short, long, num_args=0..=1, conflicts_with_all = ["unix_socket", "server"], default_missing_value = "127.0.0.1")]
    pub tcp: Option<IpAddr>,

    #[arg(short, long, conflicts_with_all = ["tcp", "server"])]
    pub unix_socket: bool,

    #[arg(
        long,
        conflicts_with_all = ["tcp", "unix_socket"],
        help = "Spin up a server instance to handle connections",
        num_args = 0..=1,
        default_missing_value = DAEMONIZE_OPTION_NAME,
        value_parser=[DAEMONIZE_OPTION_NAME, NO_DAEMONIZE_OPTION_NAME],
        require_equals = true,
    )]
    pub server: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(visible_alias = "a", about = "Directly attach to a server")]
    Attach {
        #[arg(value_hint = ValueHint::DirPath)]
        server: Option<ServerArgument>,
    },

    #[command(about = "Output a tree-like output of all of the servers")]
    Tree,
}
