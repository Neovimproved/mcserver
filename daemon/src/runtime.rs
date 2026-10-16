use std::{ffi::OsStr, path::PathBuf, sync::Arc};

use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use dashmap::DashMap;
use mcserver_core::{
    ClientMessage, Config, LOCAL_TCP_ADDR, LineBuffer, MAX_MESSAGE_LENGTH, SOCKET_PATH,
    ServerMessage, config, metadata, read_message, write_message,
};
use pty_process::{Command, Pty};
use tokio::{
    fs,
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener},
    process::Child,
    select,
    sync::{
        broadcast,
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
};

use crate::types::{Request, ServerCommunicator};

async fn spawn_server_process(
    config: &'static Config,
    name: &str,
    flags: &[impl AsRef<OsStr>],
) -> Result<(Pty, Child)> {
    let (pty, pts) = pty_process::open()?;
    pty.resize(pty_process::Size::new(24, 80))?;

    let server_dir = PathBuf::from(config.servers_directory.to_string());

    let child = Command::new("java")
        .arg("-jar")
        .arg(dbg!(metadata::get_jar_file(server_dir.join(name)).await?))
        .args(flags)
        .current_dir(server_dir)
        .spawn(pts)?;

    // pts (slave thing) can be dropped
    Ok((pty, child))
}

async fn start_server(
    config: &'static Config,
    name: &str,
    mut message_receiver: UnboundedReceiver<ClientMessage>,
    mut request_receiver: UnboundedReceiver<Request>,
    sender: broadcast::Sender<ServerMessage>,
) -> Result<()> {
    let mut history: LineBuffer<Vec<u8>> = LineBuffer::new();

    let (mut pty, mut child) = spawn_server_process(config, name, &config.default_java_args)
        .await
        .wrap_err("Failed to spawn server process")?;

    let mut stdout_finished = false;

    loop {
        let mut stdout_buf = [0u8; 1024];

        select! {
            res = child.wait() => {
                // Server closed
                res?;
                break;
            },

            res = pty.read(&mut stdout_buf), if !stdout_finished => {
                let count = res?;

                if count == 0 {
                    stdout_finished = true;
                    continue;
                }

                let bytes_read = &stdout_buf[..count];

                history.push(bytes_read.to_owned());

                // TODO: (potentially) encode before sending so that it only is encoded once
                let _ = sender.send(ServerMessage::Output(bytes_read.to_owned()));
                continue;
            }

            message = message_receiver.recv() => {
                let Some(message) = message else {
                    continue;
                };

                match message {
                    ClientMessage::Attach { name } => {
                        eprintln!("Tried attaching to {name} (does not work here)");
                    },
                    ClientMessage::Detach => {
                        eprintln!("Tried detaching (does not work here)");
                    },
                    ClientMessage::Kill => {
                        match child.kill().await {
                            Ok(()) => {
                                sender.send(ServerMessage::KillSuccess)?;
                                break;
                            }
                            Err(err) => {
                                sender.send(ServerMessage::KillFailure(err.to_string()))?;
                                return Err(eyre!("Failed to kill server"));
                            }
                        };
                    }
                    ClientMessage::ProgramInput(bytes) => {
                        history.push(bytes.clone());
                        pty.write_all(&bytes).await?;
                    }
                }
            }

            request = request_receiver.recv() => {
                let Some(request) = request else {
                    continue;
                };

                let _ = match request {
                    Request::GetHistory(sender) => sender.send(history.clone()).inspect_err(|_| eprintln!("Failed to send history")),
                };
            }
        };
    }

    Ok(())
}

async fn server_lifecycle(
    config: &'static Config,
    name: String,
    message_receiver: UnboundedReceiver<ClientMessage>,
    request_receiver: UnboundedReceiver<Request>,
    sender: broadcast::Sender<ServerMessage>,
    server_communicators: Arc<DashMap<String, ServerCommunicator>>,
) -> Result<()> {
    println!("Starting {name}");
    let res = start_server(config, &name, message_receiver, request_receiver, sender).await;

    if let Err(err) = &res {
        eprintln!("Server: {name} btwwwwww\n{err:?}");
    }

    server_communicators.remove(&name);

    println!("Server {name} closed");

    res
}

enum ServeAction {
    Prompt,
    Attach(String),
}

async fn initialize_server(
    config: &'static Config,
    name: String,
    server_communicators: &Arc<DashMap<String, ServerCommunicator>>,
) -> Result<(
    UnboundedSender<ClientMessage>,
    UnboundedSender<Request>,
    broadcast::Receiver<ServerMessage>,
)> {
    let (client_message_sender, client_message_receiver) = unbounded_channel();

    let (client_request_sender, client_request_receiver) = unbounded_channel();

    let (server_message_sender, server_message_receiver) = broadcast::channel(4096);

    server_communicators.insert(
        name.clone(),
        ServerCommunicator {
            client_message_sender: client_message_sender.clone(),
            client_request_sender: client_request_sender.clone(),
            server_message_sender: server_message_sender.clone(),
        },
    );

    tokio::spawn(server_lifecycle(
        config,
        name,
        client_message_receiver,
        client_request_receiver,
        server_message_sender.clone(),
        server_communicators.clone(),
    ));

    Ok((
        client_message_sender,
        client_request_sender,
        server_message_receiver,
    ))
}

async fn serve_client(
    (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
    config: &'static Config,
    server_communicators: &Arc<DashMap<String, ServerCommunicator>>,
) -> Result<()> {
    #[allow(unreachable_code)]
    let mut action = ServeAction::Prompt;

    loop {
        match action {
            ServeAction::Prompt => {
                let name = &loop {
                    let message: ClientMessage = match read_message!(reader).await {
                        Ok(message) => message,
                        Err(mcserver_core::Error::Io(err))
                            if err.kind() == io::ErrorKind::UnexpectedEof =>
                        {
                            println!("Most graceful client disconnect");
                            return Ok(());
                        }
                        Err(err) => return Err(err).wrap_err("Erroneous client disconnect"),
                    };

                    match message {
                        ClientMessage::Attach { name } => break name,
                        _ => {
                            write_message!(writer, ServerMessage::NotAttached).await?;
                            println!("Not attached but received {message}!");
                        }
                    };
                };

                action = ServeAction::Attach(name.to_owned());
            }

            ServeAction::Attach(name) => {
                let (message_sender, requets_sender, mut receiver) =
                    match server_communicators.get(&name) {
                        Some(com) => (
                            com.client_message_sender.clone(),
                            com.client_request_sender.clone(),
                            com.server_message_sender.subscribe(),
                        ),
                        None => initialize_server(config, name, server_communicators).await?,
                    };

                let (tx, rx) = oneshot::channel();

                requets_sender.send(Request::GetHistory(tx))?;

                let history = rx.await.unwrap_or_else(|err| {
                    LineBuffer::from(vec![
                        format!("Falied to produce history: {err:?}\n").into_bytes(),
                    ])
                });

                write_message!(writer, ServerMessage::History(history)).await?;

                action = loop {
                    select! {
                        res = receiver.recv() => {
                            let message = match res {
                                Ok(message) => message,
                                Err(broadcast::error::RecvError::Closed) => {
                                    write_message!(writer, ServerMessage::Closed).await?;
                                    break ServeAction::Prompt;
                                },
                                Err(broadcast::error::RecvError::Lagged(messages)) => {
                                    eprintln!("Missed {messages} messages");
                                    continue;
                                },
                            };
                            write_message!(writer, message).await?;
                        }

                        res = reader.read_u32() => {
                            let size = match res {
                                Ok(n) => n as usize,
                                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                                    println!("Graceful disconnect!!");
                                    return Ok(());
                                }
                                Err(err) => return Err(err).wrap_err("Failed to read message size"),
                            };

                            if size > MAX_MESSAGE_LENGTH {
                                eprintln!("The client is being very disingenuous right now!! I'm going to have to break up with them unfortunately");
                                return Err(eyre!("Disingenuous client"));
                            }

                            let message: ClientMessage = read_message!(reader, size).await?;

                            match message {
                                ClientMessage::Attach { name } => break ServeAction::Attach(name),
                                ClientMessage::Detach => break ServeAction::Prompt,
                                ClientMessage::Kill | ClientMessage::ProgramInput(..) => message_sender.send(message)?,
                            }
                        }
                    }
                }
            }
        }
    }
}

pub async fn main() -> Result<()> {
    let config: &'static Config = Box::leak(Box::new(
        config::read().await.wrap_err("Failed to read config")?.0,
    ));

    if let Err(err) = fs::remove_file(SOCKET_PATH).await
        && err.kind() != io::ErrorKind::NotFound
    {
        return Err(err).wrap_err("Failed to remove socket file");
    }

    let tcp_listener = TcpListener::bind(LOCAL_TCP_ADDR).await?;
    println!("Bound to TCP address!");

    let unix_sock_listener = UnixListener::bind(SOCKET_PATH)?;
    println!("Bound to unix socket!");

    let server_communicators: Arc<DashMap<String, ServerCommunicator>> = Arc::new(DashMap::new());

    let mut buf = String::new();
    let mut stdin_finished = false;
    let mut stdin_reader = BufReader::new(io::stdin());

    loop {
        select! {
            res = tcp_listener.accept() => {
                let server_communicators = server_communicators.clone();
                tokio::spawn(async move {
                    let (mut stream, addr) = res?;

                    println!("Connected to client by tcp: {addr}");

                    if let Err(err) = serve_client(stream.split(), config, &server_communicators).await {
                        eprintln!("{err:?}");
                    }

                    io::Result::Ok(())
                });
            },

            res = unix_sock_listener.accept() => {
                let server_communicators = server_communicators.clone();

                tokio::spawn(async move {
                    let (mut stream, addr) = res?;

                    println!("Connected to client by unix socket: {addr:?}");

                    if let Err(err) = serve_client(stream.split(), config, &server_communicators).await {
                        eprintln!("{err:?}");
                    }

                    io::Result::Ok(())
                });
            }

            res = stdin_reader.read_line(&mut buf), if !stdin_finished => {
                res?;

                if buf.is_empty() {
                    stdin_finished = true;
                    continue;
                }

                match &buf[..buf.len()-1] {
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
