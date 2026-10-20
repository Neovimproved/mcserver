use tokio::sync::{broadcast, mpsc::UnboundedSender, oneshot};

use crate::{line_buffer::LineBuffer, messages::ServerInstruction};

pub struct ServerCommunicator {
    pub instruction_sender: UnboundedSender<ServerInstruction>,
    pub client_request_sender: UnboundedSender<Request>,
    pub server_message_sender: broadcast::Sender<Vec<u8>>,
}

pub enum Request {
    GetHistory(oneshot::Sender<LineBuffer<Vec<u8>>>),
}
