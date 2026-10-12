use std::{
    env,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    process::exit,
    str::FromStr,
};

use color_eyre::eyre::{Context, eyre};

use mcserver_core::*;
use ratatui::DefaultTerminal;
use tokio::{
    io,
    net::{TcpStream, UnixStream},
};

mod app;
mod error;

use app::App;

const CONNECTION_REFUSED_ERROR_MESSAGE: &str =
    "Server refused to connect. It is likely not running";

async fn try_connect_unix_socket() -> color_eyre::Result<UnixStream> {
    let stream = match UnixStream::connect(SOCKET_PATH).await {
        Ok(stream) => stream,
        Err(err) => match err.kind() {
            io::ErrorKind::ConnectionRefused => {
                return Err(eyre!(CONNECTION_REFUSED_ERROR_MESSAGE));
            }
            _ => return Err(err).wrap_err("Failed to connect to unix socket"),
        },
    };

    Ok(stream)
}

async fn try_connect_tcp(addr: Option<&str>) -> color_eyre::Result<TcpStream> {
    let stream = TcpStream::connect(
        addr.map(SocketAddr::from_str)
            .transpose()?
            .unwrap_or(LOCAL_TCP_ADDR),
    )
    .await
    .map_err(|err| match err.kind() {
        io::ErrorKind::ConnectionRefused => eyre!(CONNECTION_REFUSED_ERROR_MESSAGE),
        _ => err.into(),
    })?;

    Ok(stream)
}

struct GuardedTerminal {
    terminal: DefaultTerminal,
}

impl GuardedTerminal {
    pub fn new() -> Self {
        Self {
            terminal: ratatui::init(),
        }
    }
}

impl Drop for GuardedTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

impl Deref for GuardedTerminal {
    type Target = DefaultTerminal;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for GuardedTerminal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut args = env::args().skip(1);

    let mut app = App::new();

    match args.next().as_deref() {
        Some("tcp") => {
            let mut stream = try_connect_tcp(args.next().as_deref()).await?;
            app.run(&mut GuardedTerminal::new(), stream.split()).await?;
        }
        Some("sock") | Some("socket") | None => {
            let mut stream = try_connect_unix_socket().await?;
            app.run(&mut GuardedTerminal::new(), stream.split()).await?
        }
        Some(other) => return Err(eyre!("Connection type {other} is not supported")),
    }

    println!("Ending...");

    // To prevent it from waiting for stdin to finish reading a line
    exit(0);
}
