use std::{
    env::current_exe,
    net::{IpAddr, SocketAddr},
    ops::{Deref, DerefMut},
    path::PathBuf,
    time::Duration,
};

use futures::StreamExt;
use inotify::{Inotify, WatchMask};
use mcserver_core::{SOCKET_FILE_NAME, SOCKET_PATH, TCP_PORT, TEMP_DIR};
use miette::{Context, IntoDiagnostic, bail};
use ratatui::DefaultTerminal;
use tokio::{
    fs, io,
    net::{TcpStream, UnixStream},
    process::Command,
    time::{sleep, timeout},
};

const DAEMON_EXECUTABLE_NAME: &str = "mcserver-daemon";

async fn find_possible_path() -> miette::Result<PathBuf> {
    let daemon_path = fs::canonicalize(
        current_exe()
            .into_diagnostic()
            .wrap_err("Failed to get path of current executable")?,
    )
    .await
    .into_diagnostic()
    .wrap_err("Failed to canonicalize executable path")?
    .parent()
    .wrap_err("Failed to get parent of executable path")?
    .join(DAEMON_EXECUTABLE_NAME);

    if fs::try_exists(&daemon_path)
        .await
        .into_diagnostic()
        .wrap_err("Failed to check if daemon exists")?
    {
        return Ok(daemon_path);
    }

    if !fs::try_exists(&daemon_path)
        .await
        .into_diagnostic()
        .wrap_err("Failed to check if daemon executable exists")?
    {
        bail!(
            "Failed to find a path for the daemon. Have you installed it? (try cargo install mcserver-daemon)"
        );
    }

    Ok(daemon_path)
}

async fn spin_up_server_instance() -> miette::Result<()> {
    let mut cmd = Command::new(
        find_possible_path()
            .await
            .wrap_err("Daemon binary not found")?,
    );

    cmd.status()
        .await
        .into_diagnostic()
        .wrap_err("Failed to spin up server instance")?;

    Ok(())
}

pub async fn try_connect_unix_socket() -> miette::Result<UnixStream> {
    let stream = match UnixStream::connect(SOCKET_PATH).await {
        Ok(stream) => stream,
        Err(err) => match err.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                // spin it up!! 🕸️
                spin_up_server_instance().await?;

                let inotify = Inotify::init()
                    .into_diagnostic()
                    .wrap_err("Failed to initialize inotify instance")?;

                inotify
                    .watches()
                    .add(TEMP_DIR, WatchMask::MODIFY | WatchMask::CREATE)
                    .into_diagnostic()
                    .wrap_err("Failed to add inotify watch")?;

                let event_buffer = [0u8; 4096];

                let mut event_stream = inotify
                    .into_event_stream(event_buffer)
                    .into_diagnostic()
                    .wrap_err("Failed to create event stream")?;

                while let Some(event) = timeout(Duration::from_secs(1), event_stream.next())
                    .await
                    .into_diagnostic()
                    .wrap_err("Server timed out after 1 second")?
                {
                    if let Some(name) = event
                        .into_diagnostic()
                        .wrap_err("Failed to read event from inotify stream")?
                        .name
                        && name == SOCKET_FILE_NAME
                    {
                        break;
                    }
                }

                UnixStream::connect(SOCKET_PATH)
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to connect to unix stream")?
            }
            _ => {
                return Err(err)
                    .into_diagnostic()
                    .wrap_err("Failed to connect to unix stream");
            }
        },
    };

    Ok(stream)
}

pub async fn try_connect_tcp(ip_addr: IpAddr) -> miette::Result<TcpStream> {
    let socket_addr = SocketAddr::new(ip_addr, TCP_PORT);

    if ip_addr.is_loopback() {
        match TcpStream::connect(socket_addr).await {
            Ok(stream) => Ok(stream),
            Err(err) => match err.kind() {
                io::ErrorKind::ConnectionRefused => {
                    spin_up_server_instance().await?;
                    sleep(Duration::from_millis(100)).await;
                    TcpStream::connect(socket_addr).await
                }
                _ => Err(err),
            },
        }
    } else {
        TcpStream::connect(socket_addr).await
    }
    .into_diagnostic()
    .wrap_err("Failed to connect to TCP stream")
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
