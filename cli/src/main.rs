use std::{
    env,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    process::Command,
    str::FromStr,
    thread::sleep,
    time::Duration,
};

use color_eyre::eyre::Context;
use mcserver_core::{LOCAL_TCP_ADDR, SOCKET_PATH};
use ratatui::DefaultTerminal;
use tokio::{
    io,
    net::{TcpStream, UnixStream},
};

use crate::app::{App, ExitReason};

mod app;

async fn try_connect_unix_socket() -> color_eyre::Result<UnixStream> {
    let stream = match UnixStream::connect(SOCKET_PATH).await {
        Ok(stream) => stream,
        Err(err) => match err.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                // spin it up!! 🕸️
                let mut cmd = Command::new("./mcserver-daemon");
                cmd.status()?;

                sleep(Duration::from_millis(100));
                UnixStream::connect(SOCKET_PATH).await?
            }
            _ => return Err(err).wrap_err("Failed to connect to unix stream"),
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
    .await?;

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

    let exit_reason = match args.next().as_deref() {
        Some("tcp") => {
            let mut stream = try_connect_tcp(args.next().as_deref()).await?;
            app.run(&mut GuardedTerminal::new(), stream.split()).await?
        }
        Some("sock") | Some("socket") | None => {
            let mut stream = try_connect_unix_socket().await?;
            app.run(&mut GuardedTerminal::new(), stream.split()).await?
        }
        Some(other) => {
            return Err(mcserver_core::Error::UnsupportedConnectionType(other.to_string()).into());
        }
    };

    match exit_reason {
        ExitReason::ServerClosed => println!("Server closed"),
        ExitReason::ClientExit => {}
    };

    Ok(())
}
