use tokio::sync::{broadcast, mpsc::UnboundedSender, oneshot};

use crate::{line_buffer::LineBuffer, messages::ClientMessage};

pub struct ServerCommunicator {
    pub client_message_sender: UnboundedSender<ClientMessage>,
    pub client_request_sender: UnboundedSender<Request>,
    pub server_message_sender: broadcast::Sender<Vec<u8>>,
}

pub enum Request {
    GetHistory(oneshot::Sender<LineBuffer<Vec<u8>>>),
}
