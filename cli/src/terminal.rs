use std::{
    env::current_exe,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use const_format::formatcp;
use mcserver_core::{LOCAL_TCP_ADDR, SOCKET_PATH};
use miette::{Context, IntoDiagnostic};
use ratatui::DefaultTerminal;
use tokio::{
    fs, io,
    net::{TcpStream, UnixStream},
    process::Command,
    time::sleep,
};

const DAEMON_EXECUTABLE_NAME: &str = "mcserver-daemon";

async fn find_possible_path() -> io::Result<Option<PathBuf>> {
    if let Some(exe_parent) = current_exe()?.parent() {
        let path = exe_parent.join(DAEMON_EXECUTABLE_NAME);

        if fs::try_exists(&path).await? {
            return Ok(Some(path));
        }
    }

    let path = formatcp!("./{DAEMON_EXECUTABLE_NAME}");

    if fs::try_exists(path).await? {
        Ok(Some(PathBuf::from(path)))
    } else {
        Ok(None)
    }
}

pub async fn try_connect_unix_socket() -> miette::Result<UnixStream> {
    let stream = match UnixStream::connect(SOCKET_PATH).await {
        Ok(stream) => stream,
        Err(err) => match err.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                // spin it up!! 🕸️
                let mut cmd = Command::new(
                    find_possible_path()
                        .await
                        .into_diagnostic()?
                        .context("Failed to find daemon executable path")?,
                );

                cmd.status()
                    .await
                    .into_diagnostic()
                    .context("Failed to spin up server instance")?;

                sleep(Duration::from_millis(100)).await;
                UnixStream::connect(SOCKET_PATH)
                    .await
                    .into_diagnostic()
                    .context("Failed to connect to unix stream")?
            }
            _ => {
                return Err(err)
                    .into_diagnostic()
                    .context("Failed to connect to unix stream");
            }
        },
    };

    Ok(stream)
}

pub async fn try_connect_tcp(addr: Option<&str>) -> miette::Result<TcpStream> {
    let stream = TcpStream::connect(
        addr.map(SocketAddr::from_str)
            .transpose()
            .into_diagnostic()
            .context("Failed to convert address")?
            .unwrap_or(LOCAL_TCP_ADDR),
    )
    .await
    .into_diagnostic()
    .context("Failed to connect to TCP stream")?;

    Ok(stream)
}

pub struct GuardedTerminal {
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
