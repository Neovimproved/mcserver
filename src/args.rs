use std::net::IpAddr;

use clap::{Parser, Subcommand, ValueHint};

use crate::client::ServerArgument;

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(short, long, num_args=0..=1, conflicts_with_all = ["unix_socket", "server", "server_no_daemonize"], default_missing_value = "127.0.0.1")]
    pub tcp: Option<IpAddr>,

    #[arg(short, long, conflicts_with_all = ["tcp", "server", "server_no_daemonize"])]
    pub unix_socket: bool,

    #[arg(long, conflicts_with_all = ["tcp", "unix_socket", "server_no_daemonize"], help = "Spin up a server instance to handle connections")]
    pub server: bool,

    #[arg(long, conflicts_with_all = ["tcp", "unix_socket", "server"])]
    pub server_no_daemonize: bool,

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
