#![allow(unused)]
use std::fs::{self, File};

use clap::Parser;
use color_eyre::eyre::Context as EyreContext;
use const_format::formatcp;
use daemonize2::Daemonize;
use tokio::runtime::Runtime;

use crate::{
    args::{Args, Command},
    consts::{PID_FILE_PATH, TEMP_DIR},
    server::runtime,
};

mod args;
mod client;
mod config;
mod consts;
mod error;
mod line_buffer;
mod messages;
mod metadata;
mod parser;
mod server;
mod servers;

fn server(daemonize: bool) -> color_eyre::Result<()> {
    color_eyre::install()?;

    fs::create_dir_all(TEMP_DIR).wrap_err("Failed to create daemon directory")?;

    if !daemonize {
        // no daemonize
        Runtime::new()?.block_on(runtime::main())?;
        return Ok(());
    }

    let stdout = File::create(formatcp!("{TEMP_DIR}/daemon.out"))
        .wrap_err("Failed to create stdout file")?;

    let stderr = File::create(formatcp!("{TEMP_DIR}/daemon.err"))
        .wrap_err("Failed to create stderr file")?;

    let daemonize = Daemonize::new()
        .pid_file(PID_FILE_PATH) // Every method except `new` and `start`
        .working_directory(TEMP_DIR) // for default behaviour.
        .stdout(stdout)
        .stderr(stderr)
        .privileged_action(|| "Executed before drop privileges");

    unsafe { daemonize.start() }.wrap_err("Failed to daemonize process")?;

    println!("Successfully daemonized!");

    Runtime::new()
        .wrap_err("Failed to")?
        .block_on(server::runtime::main())?;

    println!("Finished!");

    Ok(())
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    if args.server {
        return server(true).map_err(miette::Report::msg);
    }

    if args.server_no_daemonize {
        return server(false).map_err(miette::Report::msg);
    }

    let Some(command) = args.command else {
        return client::start(args.tcp, None);
    };

    match command {
        Command::Attach { server } => client::start(args.tcp, Some(server.into())),
    }
}
