use mcserver_core::{ClientMessage, LineBuffer, ServerMessage};
use tokio::sync::{broadcast, mpsc::UnboundedSender, oneshot};

pub struct ServerCommunicator {
    pub client_message_sender: UnboundedSender<ClientMessage>,
    pub client_request_sender: UnboundedSender<Request>,
    pub server_message_sender: broadcast::Sender<ServerMessage>,
}

pub enum Request {
    GetHistory(oneshot::Sender<LineBuffer<Vec<u8>>>),
}
