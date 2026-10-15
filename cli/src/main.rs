use clap::Parser;
use miette::Context;
use tokio::net::TcpStream;

use crate::{
    app::App,
    args::Args,
    terminal::{GuardedTerminal, try_connect_tcp, try_connect_unix_socket},
};

mod app;
mod args;
mod terminal;
mod types;

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = Args::parse();

    match args.command {
        Some(..) => todo!(),
        None => {
            if let Some(ip_addr) = args.tcp {
                let mut stream = try_connect_tcp(ip_addr)
                    .await
                    .wrap_err("Failed to connect to tcp stream")?;

                App::new()
                    .run(&mut GuardedTerminal::new(), stream.split())
                    .await
            } else {
                let mut stream = try_connect_unix_socket()
                    .await
                    .wrap_err("Failed to connect to unix stream")?;

                App::new()
                    .run(&mut GuardedTerminal::new(), stream.split())
                    .await
            }?
        }
    }

    Ok(())
}
