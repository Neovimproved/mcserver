use std::fs::{self, File};

use clap::Parser;
use color_eyre::eyre::Context as EyreContext;
use const_format::formatcp;
use daemonize2::Daemonize;
use miette::{Context, IntoDiagnostic, bail};
use tokio::runtime::Runtime;

use crate::{
    args::{Args, Command, DAEMONIZE_OPTION_NAME, NO_DAEMONIZE_OPTION_NAME},
    client::StartState,
    consts::{PID_FILE_PATH, TEMP_DIR},
    server::runtime,
    servers::ServerGroup,
};

mod args;
mod client;
mod config;
mod consts;
mod error;
mod line_buffer;
mod messages;
mod metadata;
mod server;
mod servers;

#[cfg(test)]
mod test_utils;

fn server(daemonize: bool) -> color_eyre::Result<()> {
    color_eyre::install()?;

    fs::create_dir_all(TEMP_DIR).wrap_err("Failed to create daemon directory")?;

    if !daemonize {
        // no daemonize
        Runtime::new()
            .wrap_err("Failed to initialize runtime")?
            .block_on(runtime::main())?;
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
        .wrap_err("Failed to initialize runtime")?
        .block_on(server::runtime::main())?;

    println!("Finished!");

    Ok(())
}

fn tree() -> miette::Result<()> {
    let runtime = Runtime::new()
        .into_diagnostic()
        .wrap_err("Failed to create runtime")?;

    runtime.block_on(async {
        let servers_directory = config::read()
            .await
            .into_diagnostic()
            .wrap_err("Failed to read config")?
            .0
            .servers_directory;

        let entries = servers::read(&servers_directory.path)
            .await
            .wrap_err("Failed to read server entries")?;

        println!(
            "{:#}",
            ServerGroup::new(servers_directory.value, entries, true)
        );

        Ok(())
    })
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    if let Some(option) = args.server {
        return server(match option.as_str() {
            DAEMONIZE_OPTION_NAME => true,
            NO_DAEMONIZE_OPTION_NAME => false,
            _ => {
                bail!("Unknown server option");
            }
        })
        .map_err(miette::Report::msg);
    }

    let Some(command) = args.command else {
        return client::start(args.tcp, StartState::Unattached);
    };

    match command {
        Command::Attach { server } => client::start(args.tcp, server.map(String::from).into()),
        Command::Tree => tree(),
    }
}
