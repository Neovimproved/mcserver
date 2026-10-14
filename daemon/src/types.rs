use mcserver_core::{ClientMessage, LineBuffer, ServerMessage};
use tokio::{
    net::{tcp, unix},
    sync::{broadcast, mpsc::UnboundedSender, oneshot},
};

pub struct ServerCommunicator {
    pub client_message_sender: UnboundedSender<ClientMessage>,
    pub client_request_sender: UnboundedSender<Request>,
    pub server_message_sender: broadcast::Sender<ServerMessage>,
}

pub enum OwnedPair {
    UnixSocket(unix::OwnedReadHalf, unix::OwnedWriteHalf),

    #[allow(unused)]
    TcpConnection(tcp::OwnedReadHalf, tcp::OwnedWriteHalf),
}

impl OwnedPair {
    pub fn unix((reader, writer): (unix::OwnedReadHalf, unix::OwnedWriteHalf)) -> Self {
        Self::UnixSocket(reader, writer)
    }

    pub fn tcp((reader, writer): (tcp::OwnedReadHalf, tcp::OwnedWriteHalf)) -> Self {
        Self::TcpConnection(reader, writer)
    }
}

pub enum Request {
    GetHistory(oneshot::Sender<LineBuffer<Vec<u8>>>),
}
