use std::{env, process::Stdio, sync::Arc};

use color_eyre::eyre::{Context, ContextCompat, eyre};
use dashmap::DashMap;
use mcserver_core::*;
use tokio::{
    fs,
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener},
    process::{Child, Command},
    select,
    sync::{
        broadcast,
        mpsc::{UnboundedReceiver, unbounded_channel},
        oneshot,
    },
};

use crate::types::{OwnedPair, Request, ServerCommunicator};

async fn start_server(
    mut child: Child,
    mut message_receiver: UnboundedReceiver<ClientMessage>,
    mut request_receiver: UnboundedReceiver<Request>,
    sender: broadcast::Sender<ServerMessage>,
) -> color_eyre::Result<()> {
    let mut history: LineBuffer = LineBuffer::new();

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

    let mut stdout_finished = false;

    loop {
        let mut child_stdout_buf = vec![0u8; 1024];
        let mut child_stderr_buf = vec![0u8; 1024];

        select! {
            res = child.wait() => {
                // Server closed
                res?;
                break;
            },

            res = child_stdout.read(&mut child_stdout_buf), if !stdout_finished => {
                if res? == 0 {
                    stdout_finished = true;
                    continue;
                }
                // TODO: (potentially) encode before sending so that it only is encoded once
                history.push(String::from_utf8_lossy(&child_stdout_buf).to_string());
                let _ = sender.send(ServerMessage::Output(child_stdout_buf));
                continue;
            }

            res = child_stderr.read(&mut child_stderr_buf) => {
                res?;
                history.push(String::from_utf8_lossy(&child_stderr_buf).to_string());
                let _ = sender.send(ServerMessage::Error(child_stderr_buf));
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
                    ClientMessage::ProgramInput(mut bytes) => {
                        history.push(String::from_utf8_lossy(&bytes).to_string());
                        child_stdin.write_all(&bytes).await?;
                        child_stdin.write_all(b"\n").await?;

                        bytes.push(b'\n');
                        let _ = sender.send(ServerMessage::Output(bytes));
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
    name: String,
    message_receiver: UnboundedReceiver<ClientMessage>,
    request_receiver: UnboundedReceiver<Request>,
    sender: broadcast::Sender<ServerMessage>,
    child: Child,
    server_communicators: Arc<DashMap<String, ServerCommunicator>>,
) -> color_eyre::Result<()> {
    println!("Starting {name}");
    let res = start_server(child, message_receiver, request_receiver, sender).await;

    if let Err(err) = &res {
        eprintln!("Server: {name} btwwwwww\n{err:?}");
    }

    server_communicators.remove(&name);

    println!("Server {name} closed");

    res
}

// async fn unattached_client_loop(
//     rw_pair: OwnedPair,
//     server_communicators: &Arc<DashMap<String, ServerCommunicator>>,
// ) {
// }

enum ServeAction {
    Prompt,
    Attach(String),
}

fn create_command(name: &str) -> color_eyre::Result<Command> {
    let mut process_path = env::home_dir().wrap_err("Failed to get home directory")?;

    process_path.extend([
        "Dev",
        "example-terminal-app",
        "target",
        "debug",
        "example-terminal-app",
    ]);

    let mut command = Command::new(process_path);

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg(format!("{name} echoes "));

    Ok(command)
}

async fn serve_client(
    rw_pair: OwnedPair,
    server_communicators: &Arc<DashMap<String, ServerCommunicator>>,
) -> color_eyre::Result<()> {
    #[allow(unreachable_code)]
    let (mut reader, mut writer) = match rw_pair {
        OwnedPair::UnixSocket(owned_read_half, owned_write_half) => {
            (owned_read_half, owned_write_half)
        }
        OwnedPair::TcpConnection(..) => todo!(),
    };

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
                let (message_sender, requets_sender, mut receiver) = match server_communicators
                    .get(&name)
                {
                    Some(com) => (
                        com.client_message_sender.clone(),
                        com.client_request_sender.clone(),
                        com.server_message_sender.subscribe(),
                    ),
                    None => {
                        let child = create_command(&name)?.spawn()?;

                        let (client_message_sender, client_message_receiver) = unbounded_channel();

                        let (client_request_sender, client_request_receiver) = unbounded_channel();

                        let (server_message_sender, server_message_receiver) =
                            broadcast::channel(4096);

                        server_communicators.insert(
                            name.to_owned(),
                            ServerCommunicator {
                                client_message_sender: client_message_sender.clone(),
                                client_request_sender: client_request_sender.clone(),
                                server_message_sender: server_message_sender.clone(),
                            },
                        );

                        tokio::spawn(server_lifecycle(
                            name.to_owned(),
                            client_message_receiver,
                            client_request_receiver,
                            server_message_sender.clone(),
                            child,
                            server_communicators.clone(),
                        ));

                        (
                            client_message_sender,
                            client_request_sender,
                            server_message_receiver,
                        )
                    }
                };

                let (tx, rx) = oneshot::channel();

                requets_sender.send(Request::GetHistory(tx))?;

                let history = rx.await.unwrap_or_else(|err| {
                    LineBuffer::from(vec![format!("Falied to produce history: {err:?}")])
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

pub async fn main() -> color_eyre::Result<()> {
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
                    let (stream, addr) = res?;

                    println!("Connected to client by tcp: {addr}");

                    if let Err(err) = serve_client(OwnedPair::tcp(stream.into_split()), &server_communicators).await {
                        eprintln!("{err:?}");
                    }

                    io::Result::Ok(())
                });
            },

            res = unix_sock_listener.accept() => {
                let server_communicators = server_communicators.clone();

                tokio::spawn(async move {
                    let (stream, addr) = res?;

                    println!("Connected to client by unix socket: {addr:?}");

                    if let Err(err) = serve_client(OwnedPair::unix(stream.into_split()), &server_communicators).await {
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
