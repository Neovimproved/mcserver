use std::net::IpAddr;

use clap::Parser;
use miette::Context;

use crate::{
    app::App,
    args::{Args, Command},
    terminal::{GuardedTerminal, try_connect_tcp, try_connect_unix_socket},
};

mod app;
mod args;
mod servers;
mod terminal;
mod types;

pub async fn start(ip_addr: Option<IpAddr>, server_arg: Option<String>) -> miette::Result<()> {
    let mut app = App::try_new()
        .await
        .wrap_err("Failed to create application object")?;
    let mut terminal = GuardedTerminal::new();

    if let Some(ip_addr) = ip_addr {
        let mut stream = try_connect_tcp(ip_addr).await?;

        if let Some(server_arg) = server_arg {
            app.run_attached(&mut terminal, stream.split(), server_arg)
                .await
        } else {
            app.run(&mut terminal, stream.split()).await
        }
    } else {
        let mut stream = try_connect_unix_socket().await?;

        if let Some(server_arg) = server_arg {
            app.run_attached(&mut terminal, stream.split(), server_arg)
                .await
        } else {
            app.run(&mut terminal, stream.split()).await
        }
    }
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = Args::parse();

    let Some(command) = args.command else {
        return start(args.tcp, None).await;
    };

    match command {
        Command::Attach { server } => start(args.tcp, Some(server.into())).await,
    }
}
