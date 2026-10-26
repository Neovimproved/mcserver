use std::{borrow::Cow, ffi::OsStr, panic, sync::Arc};

use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use dashmap::DashMap;
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
    },
};

use crate::{
    config::{self, Config},
    consts::{LOCAL_TCP_ADDR, SOCKET_PATH},
    line_buffer::LineBuffer,
    messages::{ServerInstruction, ServerMessage},
    metadata,
    server::{
        client::Client,
        types::{Request, ServerCommunicator},
    },
};

async fn spawn_server_process(
    config: &'static Config,
    name: &str,
    flags: &[impl AsRef<OsStr>],
) -> Result<(Pty, Child)> {
    let (pty, pts) = pty_process::open()?;
    pty.resize(pty_process::Size::new(24, 80))?;

    let server_dir = config.servers_directory.path.join(name);

    let child = Command::new("java")
        .arg("-jar")
        .arg(metadata::get_jar_file(server_dir.clone()).await?)
        .args(flags)
        .current_dir(server_dir)
        .spawn(pts)?;

    // pts (slave thing) can be dropped
    Ok((pty, child))
}

async fn start_server(
    Server {
        config,
        name,
        message_receiver,
        request_receiver,
        sender,
        server_communicators,
    }: &mut Server,
) -> Result<()> {
    let mut history: LineBuffer<Vec<u8>> = LineBuffer::new();

    let (mut pty, mut child) = spawn_server_process(config, &name, &config.default_java_args)
        .await
        .wrap_err("Failed to spawn server process")?;

    let _ = sender.send(bitcode::encode(&make_synchronization_payload(
        &server_communicators,
    )));

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

                let _ = sender.send(bitcode::encode(&ServerMessage::Output(bytes_read.to_owned())));
                continue;
            }

            message = message_receiver.recv() => {
                let Some(message) = message else {
                    continue;
                };

                match message {
                    ServerInstruction::Kill => {
                        match child.kill().await {
                            Ok(()) => {
                                sender.send(bitcode::encode(&ServerMessage::Closed))?;
                                break;
                            }
                            Err(err) => {
                                sender.send(bitcode::encode(&ServerMessage::KillFailure(err.to_string())))?;
                                return Err(eyre!("Failed to kill server"));
                            }
                        };
                    }
                    ServerInstruction::SendInput(bytes) => {
                        pty.write_all(&bytes).await?;
                        pty.flush().await.wrap_err("Failed to flush")?;
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

pub fn make_synchronization_payload(
    server_communicators: &Arc<DashMap<String, ServerCommunicator>>,
) -> ServerMessage {
    ServerMessage::SynchronizationPayload {
        online_servers: server_communicators
            .iter()
            .map(|pair| pair.key().clone())
            .collect(),
    }
}

struct Server {
    config: &'static Config,
    name: String,
    message_receiver: UnboundedReceiver<ServerInstruction>,
    request_receiver: UnboundedReceiver<Request>,
    sender: broadcast::Sender<Vec<u8>>,
    server_communicators: Arc<DashMap<String, ServerCommunicator>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.server_communicators.remove(&self.name);

        let _ = self
            .sender
            .send(bitcode::encode(&make_synchronization_payload(
                &self.server_communicators,
            )));
    }
}

async fn server_lifecycle(
    config: &'static Config,
    name: String,
    message_receiver: UnboundedReceiver<ServerInstruction>,
    request_receiver: UnboundedReceiver<Request>,
    sender: broadcast::Sender<Vec<u8>>,
    server_communicators: Arc<DashMap<String, ServerCommunicator>>,
) -> Result<()> {
    println!("Starting {name}");

    let mut server = Server {
        config,
        name,
        message_receiver,
        request_receiver,
        sender,
        server_communicators,
    };

    let res = start_server(&mut server).await;

    if let Err(err) = &res {
        eprintln!("Server: {} btwwwwww\n{err:?}", server.name);
    }

    metadata::update_last_used(Cow::Owned(metadata::dir(&server.name)))
        .await
        .wrap_err("Failed to update metadata")?;

    if let Err(err) = &res {
        eprintln!("Server {} ended with error\n{err:?}", server.name);
    } else {
        println!("Server {} closed", server.name);
    }

    Ok(())
}

pub async fn initialize_server(
    config: &'static Config,
    name: String,
    server_communicators: Arc<DashMap<String, ServerCommunicator>>,
) -> Result<(
    UnboundedSender<ServerInstruction>,
    UnboundedSender<Request>,
    broadcast::Receiver<Vec<u8>>,
)> {
    let (instruction_sender, instruction_receiver) = unbounded_channel();

    let (client_request_sender, client_request_receiver) = unbounded_channel();

    let (server_message_sender, server_message_receiver) = broadcast::channel(4096);

    server_communicators.insert(
        name.clone(),
        ServerCommunicator {
            instruction_sender: instruction_sender.clone(),
            client_request_sender: client_request_sender.clone(),
            server_message_sender: server_message_sender.clone(),
        },
    );

    tokio::spawn(server_lifecycle(
        config,
        name,
        instruction_receiver,
        client_request_receiver,
        server_message_sender.clone(),
        server_communicators,
    ));

    Ok((
        instruction_sender,
        client_request_sender,
        server_message_receiver,
    ))
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

                    if let Err(err) = Client::new(stream.split(), config, server_communicators).serve().await {
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

                    if let Err(err) = Client::new(stream.split(), config, server_communicators).serve().await {
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
