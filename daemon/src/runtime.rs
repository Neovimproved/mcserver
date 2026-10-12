use std::process::Stdio;

use color_eyre::eyre::{Context, eyre};
use mcserver_core::*;
use tokio::{
    fs,
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener},
    process::{Child, Command},
    select,
};

async fn handle_server_child(
    child: &mut Child,
    mut client_reader: impl AsyncReadExt + Unpin,
    mut client_writer: impl AsyncWriteExt + Unpin,
) -> color_eyre::Result<()> {
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| eyre!("Unable to take stdin from child"))?;

    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("Unable to take stdout from child"))?;

    let mut child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre!("Unable to take stderr from child"))?;

    loop {
        let mut child_stdout_buf = vec![0u8; 1024];
        let mut child_stderr_buf = vec![0u8; 1024];

        let client_message_length = select! {
            res = client_reader.read_u32() => {
                match res {
                    Ok(len) => len as usize,
                    Err(err) => match err.kind() {
                        io::ErrorKind::UnexpectedEof => break,
                        _ => return Err(err).wrap_err("Failed to read from client"),
                    }
                }
            },

            res = child.wait() => {
                res?;
                break;
            },

            res = child_stdout.read(&mut child_stdout_buf) => {
                res?;
                write_message!(client_writer, ServerMessage::Output(child_stdout_buf)).await?;
                continue;
            }

            res = child_stderr.read(&mut child_stderr_buf) => {
                res?;
                write_message!(client_writer, ServerMessage::Error(child_stderr_buf)).await?;
                continue;
            }
        };

        if client_message_length == 0 {
            println!("EOL");
            break;
        }

        if client_message_length > MAX_MESSAGE_LENGTH {
            println!("Too long");
            return Err(eyre!("The user sent a message that was too long"));
        }

        let message = read_message!(ClientMessage, client_reader, client_message_length).await?;

        println!("Received {message}");

        let initial_response =
            ServerMessage::Message("That's nice buddy. I'll handle your message rq".to_string());

        write_message!(client_writer, initial_response).await?;

        match message {
            ClientMessage::Start { name: _, flags: _ } => {
                write_message!(client_writer, ServerMessage::AlreadyStarted).await?;
            }
            ClientMessage::Kill => match child.kill().await {
                Ok(()) => {
                    write_message!(client_writer, ServerMessage::KillSuccess).await?;
                    break;
                }
                Err(err) => {
                    write_message!(client_writer, ServerMessage::KillFailure(err.to_string()))
                        .await?
                }
            },
            ClientMessage::ProgramInput(bytes) => {
                child_stdin.write_all(&bytes).await?;
                child_stdin.write_all(b"\n").await?;
            }
        }
    }

    Ok(())
}

async fn init_connection(
    (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
) -> color_eyre::Result<()> {
    let (name, flags) = loop {
        let message = read_message!(ClientMessage, reader).await?;

        match message {
            ClientMessage::Start { name, flags } => break (name, flags),
            _ => match write_message!(writer, ServerMessage::NotStarted).await {
                Ok(()) => println!("Successfully wrote"),
                Err(err) => eprintln!("Failed to write: {err:?}"),
            },
        };
    };

    let mut child = Command::new(name)
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    handle_server_child(&mut child, reader, writer).await?;

    Ok(())
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
                    let (mut stream, addr) = res?;

                    println!("Connected to client by TCP: {addr}");

                    if let Err(err) = init_connection(stream.split()).await {
                        eprintln!("{err:?}");
                    }

                    println!("Connection with {addr} ended");

                    io::Result::Ok(())
                });
            },
            res = unix_sock_listener.accept() => {
                tokio::spawn(async move {
                    let (mut stream, addr) = res?;

                    println!("Connected to client by Unix socket: {addr:?}");

                    if let Err(err) = init_connection(stream.split()).await {
                        eprintln!("{err:?}");
                    }

                    println!("Connection with {addr:?} ended");

                    io::Result::Ok(())
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
