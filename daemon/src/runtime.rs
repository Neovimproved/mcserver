use std::process::Stdio;

use color_eyre::eyre::Context;
use mcserver_core::*;
use tokio::{
    fs,
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener},
    process::{Child, Command},
    select,
};

async fn handle_server_child(
    mut child: Child,
    mut reader: impl AsyncReadExt + Unpin,
    mut writer: impl AsyncWriteExt + Unpin,
) -> color_eyre::Result<()> {
    loop {
        let mut buf = [0u8; 1024];

        let count = select! {
            count = reader.read(&mut buf) => count?,
            res = child.wait() => {
                res?;
                break;
            },
        };

        if count == 0 {
            break;
        }

        let message: ClientMessage = bitcode::decode(&buf[..count])?;

        println!("Received: {message}");

        let initial_response =
            ServerMessage::Message("That's nice buddy. I'll handle your message rq".to_string());

        writer
            .write_all(&bitcode::encode(&initial_response))
            .await?;

        if let Some(response) = match message {
            ClientMessage::Start { name: _, flags: _ } => Some(ServerMessage::ServerNotStarted),
            ClientMessage::Kill => match child.kill().await {
                Ok(()) => {
                    writer
                        .write_all(&bitcode::encode(&ServerMessage::KillSuccess))
                        .await?;
                    break;
                }
                Err(err) => Some(ServerMessage::KillFailure(err.to_string())),
            },
            ClientMessage::ProgramInput(bytes) => {
                if bytes.is_empty() {
                    println!("Schemed");
                    continue;
                }
                println!("Program input type but is there a child?");

                if let Some(stdin) = child.stdin.as_mut() {
                    println!("Feeding...");
                    stdin.write_all(&bytes).await?;
                } else {
                    println!(
                        "stdin: {}; stdout: {}",
                        if child.stdin.is_some() { "yep" } else { "nope" },
                        if child.stdout.is_some() {
                            "yep"
                        } else {
                            "nope"
                        }
                    );
                }

                None
            }
        } {
            writer.write_all(&bitcode::encode(&response)).await?;
        }
    }
    Ok(())
}

async fn init_connection(
    (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
) -> color_eyre::Result<()> {
    let (name, flags) = loop {
        let mut buf = [0u8; 1024];
        let count = reader.read(&mut buf).await?;

        let message: ClientMessage = bitcode::decode(&buf[..count])?;

        match message {
            ClientMessage::Start { name, flags } => break (name, flags),
            _ => {
                writer
                    .write(&bitcode::encode(&ServerMessage::ServerNotStarted))
                    .await?
            }
        };
    };

    handle_server_child(
        Command::new(name)
            .args(flags)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?,
        reader,
        writer,
    )
    .await
}

pub async fn main() -> color_eyre::Result<()> {
    println!("Doing things...");

    if let Err(err) = fs::remove_file(SOCKET_PATH).await
        && err.kind() != io::ErrorKind::NotFound
    {
        return Err(err).wrap_err("Failed to remove socket file");
    }

    let tcp_listener = TcpListener::bind(LOCAL_TCP_ADDR).await?;
    println!("Bound to TCP address!");

    let unix_sock_listener = UnixListener::bind(SOCKET_PATH)?;
    println!("Bound to unix socket!");

    let mut stdin_reader = BufReader::new(io::stdin());

    loop {
        let mut buf = String::new();

        select! {
            res = tcp_listener.accept() => {
                tokio::spawn(async move {
                    let (mut stream, _addr) = res?;

                    if let Err(err) = init_connection(stream.split()).await {
                        eprintln!("{err:?}");
                    }

                    Ok::<(), io::Error>(())
                });
            },
            res = unix_sock_listener.accept() => {
                tokio::spawn(async move {
                    let (mut stream, _addr) = res?;
                    let (reader, writer) = stream.split();

                    if let Err(err) = init_connection(stream.split()).await {
                        eprintln!("{err:?}");
                    }

                    Ok::<(), io::Error>(())
                });
            }
            res = stdin_reader.read_line(&mut buf) => {
                res?;

                let response = buf.trim();

                match response {
                    "stop" => {
                        println!("OK, stopping");
                        break;
                    }
                    _ => eprintln!("I don't understand"),
                }
            }
        };
    }

    println!("Closing now...");

    Ok(())
}
