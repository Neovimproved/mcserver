use clap::Parser;
use miette::Context;

use crate::{
    app::App,
    args::Args,
    terminal::{GuardedTerminal, try_connect_unix_socket},
};

mod app;
mod args;
mod terminal;

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = Args::parse();

    match args.command {
        Some(..) => todo!(),
        None => {
            let stream = try_connect_unix_socket()
                .await
                .context("Failed to connect to unix stream")?;

            App::new()
                .run(&mut GuardedTerminal::new(), stream.into_split())
                .await?
        }
    }

    Ok(())
}
