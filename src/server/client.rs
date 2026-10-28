use color_eyre::eyre::{Context, Result, eyre};
use dashmap::DashMap;
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    select,
    sync::{RwLock, broadcast, mpsc::UnboundedSender, oneshot},
};

use crate::{
    config::Config,
    consts::MAX_MESSAGE_LENGTH,
    line_buffer::LineBuffer,
    messages::{ClientMessage, ServerInstruction, ServerMessage},
    read_message,
    server::{
        runtime::{initialize_server, make_synchronization_payload},
        types::{Request, ServerCommunicator},
    },
    write_bytes, write_message,
};

enum ServeAction {
    Prompt,
    Attach(String),
    Disconnect,
}

pub struct Client<R: AsyncReadExt + Unpin, W: AsyncWriteExt + Unpin> {
    reader: R,
    writer: W,
    config: &'static RwLock<Config>,
    server_communicators: &'static DashMap<String, ServerCommunicator>,

    action: ServeAction,
}

impl<R: AsyncReadExt + Unpin, W: AsyncWriteExt + Unpin> Client<R, W> {
    pub fn new(
        (reader, writer): (R, W),
        config: &'static RwLock<Config>,
        server_communicators: &'static DashMap<String, ServerCommunicator>,
    ) -> Self {
        Self {
            reader,
            writer,
            config,
            server_communicators,
            action: ServeAction::Prompt,
        }
    }

    async fn send_synchronization_payload(&mut self) -> Result<()> {
        let message = make_synchronization_payload(self.server_communicators);

        let writer = &mut self.writer;
        write_message!(writer, message)
            .await
            .wrap_err("Failed to send synchronization payload to client")
    }

    async fn prompt(&mut self) -> Result<ServeAction> {
        loop {
            let reader = &mut self.reader;
            let message: ClientMessage = match read_message!(reader).await {
                Ok(message) => message,
                Err(crate::error::Error::Io(err)) if err.kind() == io::ErrorKind::UnexpectedEof => {
                    println!("Most graceful client disconnect");
                    break Ok(ServeAction::Disconnect);
                }
                Err(err) => return Err(err).wrap_err("Erroneous client disconnect"),
            };

            match message {
                ClientMessage::Attach { server } => break Ok(ServeAction::Attach(server)),

                ClientMessage::SynchronizationRequest => {
                    self.send_synchronization_payload().await?
                }
                _ => {
                    let writer = &mut self.writer;
                    write_message!(writer, ServerMessage::NotAttached).await?;
                    println!("Not attached but received {message}!");
                }
            };
        }
    }

    async fn handle_attached_message(
        &mut self,
        size: usize,
        message_sender: &UnboundedSender<ServerInstruction>,
    ) -> Result<Option<ServeAction>> {
        if size > MAX_MESSAGE_LENGTH {
            eprintln!(
                "The client is being very disingenuous right now!! I'm going to have to break up with them unfortunately"
            );
            return Err(eyre!("Disingenuous client"));
        }

        let reader = &mut self.reader;
        let message: ClientMessage = read_message!(reader, size).await?;

        let action = match message {
            ClientMessage::Attach { server } => Some(ServeAction::Attach(server)),
            ClientMessage::Detach => Some(ServeAction::Prompt),
            ClientMessage::Kill => {
                message_sender
                    .send(ServerInstruction::Kill)
                    .wrap_err("Failed to send kill instruction")?;

                None
            }
            ClientMessage::ProgramInput(bytes) => {
                message_sender
                    .send(ServerInstruction::SendInput(bytes))
                    .wrap_err("Failed to send input instruction")?;

                None
            }
            ClientMessage::SynchronizationRequest => {
                self.send_synchronization_payload().await?;
                None
            }
        };

        Ok(action)
    }

    async fn attach(&mut self, name: String) -> Result<ServeAction> {
        let (message_sender, requets_sender, mut receiver) =
            match self.server_communicators.get(&name) {
                Some(com) => (
                    com.instruction_sender.clone(),
                    com.client_request_sender.clone(),
                    com.server_message_sender.subscribe(),
                ),
                None => initialize_server(self.config, name, self.server_communicators).await?,
            };

        let (tx, rx) = oneshot::channel();

        requets_sender.send(Request::GetHistory(tx))?;

        let history = rx.await.unwrap_or_else(|err| {
            LineBuffer::from(vec![
                format!("Failed to produce history: {err:?}\n").into_bytes(),
            ])
        });

        {
            let writer = &mut self.writer;
            write_message!(writer, ServerMessage::History(history)).await?;
        }

        let action = loop {
            select! {
                res = receiver.recv() => {
                    let message = match res {
                        Ok(message) => message,
                        Err(broadcast::error::RecvError::Closed) => {
                            let writer = &mut self.writer;
                            write_message!(writer, ServerMessage::Closed).await?;
                            break ServeAction::Prompt;
                        },
                        Err(broadcast::error::RecvError::Lagged(messages)) => {
                            eprintln!("Missed {messages} messages");
                            continue;
                        },
                    };

                    let writer = &mut self.writer;
                    write_bytes!(writer, message).await?;
                }

                res = self.reader.read_u32() => {
                    let size = match res {
                        Ok(n) => n as usize,
                        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                            println!("Graceful disconnect!!");
                            break ServeAction::Disconnect;
                        }
                        Err(err) => return Err(err).wrap_err("Failed to read message size"),
                    };

                    if let Some(action) = self.handle_attached_message(size, &message_sender).await? {
                        break action;
                    }
                }
            }
        };

        Ok(action)
    }

    pub async fn serve(&mut self) -> Result<()> {
        self.send_synchronization_payload().await?;

        loop {
            match &self.action {
                ServeAction::Prompt => {
                    self.action = self.prompt().await?;
                }

                ServeAction::Attach(name) => {
                    self.action = self.attach(name.clone()).await?;
                }

                ServeAction::Disconnect => break Ok(()),
            }
        }
    }
}
