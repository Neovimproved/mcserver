use std::{env, net::SocketAddr, process::exit, str::FromStr};

use color_eyre::eyre::{Context, eyre};

use mcserver_core::*;
use tokio::{
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpStream, UnixStream},
};

const CONNECTION_REFUSED_ERROR_MESSAGE: &str =
    "Server refused to connect. It is likely not running";

async fn request_server_start(mut writer: impl AsyncWriteExt + Unpin) -> color_eyre::Result<()> {
    let mut path = env::home_dir().ok_or(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find home directory",
    ))?;

    path.extend(&[
        "Dev",
        "example-terminal-app",
        "target",
        "debug",
        "example-terminal-app",
    ]);

    write_message!(
        writer,
        ClientMessage::Start {
            name: path.into_string().map_err(|path_buf| {
                eyre!("Failed to convert {} into string", path_buf.display())
            })?,
            flags: env::args().skip(1).collect(),
        }
    )
    .await?;

    Ok(())
}

async fn client(
    (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
) -> color_eyre::Result<()> {
    request_server_start(&mut writer).await?;

    let mut stdin_reader = BufReader::new(io::stdin());
    loop {
        let mut stdin_buf = String::new();

        tokio::select! {
            res = stdin_reader.read_line(&mut stdin_buf) => {
                res?;

                // Remove newline from the end
                if stdin_buf.pop().is_none() {
                    // EOF
                    break;
                }

                write_message!(writer, ClientMessage::ProgramInput(stdin_buf.into_bytes())).await?;
            }
            length = reader.read_u32() => {
                let length = match length {
                    Ok(length) => length,
                    Err(err) => match err.kind() {
                        io::ErrorKind::UnexpectedEof => {
                            eprintln!("Server closed");
                            break;
                        },
                        _ => return Err(err).wrap_err("Failed to read message length"),
                    },
                };

                match read_message!(ServerMessage, reader, length).await? {
                    ServerMessage::KillFailure(err) => eprintln!("Failed to kill child: {err}"),
                    ServerMessage::KillSuccess => println!("Successfully killed the child process"),
                    ServerMessage::Message(message) => println!("Received a message from the server: {message}"),
                    ServerMessage::AlreadyStarted => println!("The server has already started apparently"),
                    ServerMessage::NotStarted => eprintln!("The server has not yet started apparently"),
                    ServerMessage::Output(output) => {
                        io::stdout().write_all(&output).await?
                    },
                };
            }
        }
    }

    Ok(())
}

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
    let stream = match TcpStream::connect(
        addr.map(SocketAddr::from_str)
            .transpose()?
            .unwrap_or(LOCAL_TCP_ADDR),
    )
    .await
    {
        Ok(stream) => stream,
        Err(err) => match err.kind() {
            io::ErrorKind::ConnectionRefused => {
                return Err(eyre!(CONNECTION_REFUSED_ERROR_MESSAGE));
            }
            _ => return Err(err).wrap_err("Failed to connect to TCP stream"),
        },
    };

    Ok(stream)
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("tcp") => {
            let mut stream = try_connect_tcp(args.next().as_deref()).await?;
            client(stream.split()).await?;
        }
        Some("sock") | Some("socket") | None => {
            let mut stream = try_connect_unix_socket().await?;
            client(stream.split()).await?
        }
        Some(other) => return Err(eyre!("Connection type {other} is supported")),
    }

    println!("Ending...");

    // To prevent it from waiting for stdin to finish reading a line
    exit(0);
}
