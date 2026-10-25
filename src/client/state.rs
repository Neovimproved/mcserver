use std::time::{Duration, SystemTime};

use ratatui::layout::Rect;

use crate::{
    line_buffer::LineBuffer,
    servers::{ServerEntry, ServerEntryType},
};

#[derive(Debug)]
pub struct TempMessage<T = String> {
    message: Option<(SystemTime, T)>,
    duration: Duration,
}

#[allow(unused)]
impl<T> TempMessage<T> {
    pub fn new(duration: Duration) -> Self {
        Self {
            message: None,
            duration,
        }
    }

    pub fn with_message(message: T, duration: Duration) -> Self {
        Self {
            message: Some((SystemTime::now(), message)),
            duration,
        }
    }

    pub fn get(&self) -> Option<&T> {
        let (earlier, message) = self.message.as_ref()?;

        if SystemTime::now().duration_since(*earlier).ok()? < self.duration {
            Some(message)
        } else {
            None
        }
    }

    pub fn insert(&mut self, message: T) {
        self.message.insert((SystemTime::now(), message));
    }

    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = duration;
    }

    pub fn insert_with_duration(&mut self, message: T, duration: Duration) {
        self.message.insert((SystemTime::now(), message));
        self.duration = duration;
    }
}

#[derive(Debug)]
pub enum SelectionState {
    Normal,
    Adding {
        name: String,
    },
    Deleting {
        name: String,
        confirmation: String,
        error_message: TempMessage,
        entry_type: ServerEntryType,
    },
}

impl SelectionState {
    pub fn reset(&mut self) {
        *self = SelectionState::Normal;
    }

    pub fn adding() -> Self {
        Self::Adding {
            name: "".to_string(),
        }
    }

    pub fn deleting(entry: &ServerEntry) -> Self {
        Self::Deleting {
            name: entry.name().to_string(),
            entry_type: entry.ty(),
            confirmation: "".to_string(),
            error_message: TempMessage::new(Duration::from_secs(2)),
        }
    }
}

#[derive(Debug)]
pub enum ServerState {
    Normal,
    Escaping,
    Dead,
}

impl ServerState {
    pub fn reset(&mut self) {
        *self = Self::Normal;
    }
}

#[derive(Debug)]
pub enum Mode {
    Selecting(SelectionState),
    Console {
        attached_server: usize,
        path: String,
        input: String,
        output: LineBuffer,
        state: ServerState,
    },
}

impl Mode {
    pub fn output(index: usize, path: String) -> Self {
        Self::Console {
            attached_server: index,
            path,
            input: String::new(),
            output: LineBuffer::new(),
            state: ServerState::Normal,
        }
    }
}

pub struct ServerList {
    pub servers: Vec<ServerEntry>,
    pub selected: usize,
    pub scroll: usize,
}

pub struct AbsoluteArea {
    #[allow(dead_code)]
    pub width: usize,
    pub height: usize,
}

impl From<Rect> for AbsoluteArea {
    fn from(Rect { width, height, .. }: Rect) -> Self {
        Self {
            width: width as usize,
            height: height as usize,
        }
    }
}
