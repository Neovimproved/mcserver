use std::{
    env::current_exe,
    net::{IpAddr, SocketAddr},
    ops::{Deref, DerefMut},
    time::Duration,
};

use futures::StreamExt;
use inotify::{Inotify, WatchMask};
use miette::{Context, IntoDiagnostic, bail};
use ratatui::DefaultTerminal;
use tokio::{
    io,
    net::{TcpStream, UnixStream},
    pin,
    process::Command,
    runtime::Runtime,
    select,
    time::sleep,
};

mod app;
mod types;

use app::App;

pub use types::ServerArgument;

use crate::consts::{CONNECTION_PORT, SOCKET_FILE_NAME, SOCKET_PATH, TEMP_DIR};

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

async fn spin_up_server_instance() -> miette::Result<()> {
    Command::new(
        current_exe()
            .into_diagnostic()
            .wrap_err("Failed to get path of current executable")?,
    )
    .arg("--server")
    .status()
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

                let timeout = sleep(Duration::from_secs(1));
                pin!(timeout);

                loop {
                    select! {
                        _ = &mut timeout => {
                            bail!("Server timed out after 1 second");
                        }

                        event = event_stream.next() => {
                            let Some(event) = event else {
                                bail!("Inotify event stream closed early");
                            };

                            let event = event.into_diagnostic().wrap_err("Failed to read event from inotify stream")?;

                            if let Some(name) = event.name && name == SOCKET_FILE_NAME {
                                break;
                            }
                        }
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
    let socket_addr = SocketAddr::new(ip_addr, CONNECTION_PORT);

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

pub fn start(ip_addr: Option<IpAddr>, server_arg: Option<String>) -> miette::Result<()> {
    let runtime = Runtime::new()
        .into_diagnostic()
        .wrap_err("Failed to create client runtime")?;

    runtime.block_on(async {
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
    })
}
