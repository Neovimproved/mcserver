use std::{
    ops::{self, Index, Range, RangeBounds},
    time::{Duration, SystemTime},
};

use ratatui::layout::Rect;

use crate::{
    line_buffer::LineBuffer,
    log,
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
pub enum ConnectionState {
    Normal,
    Escaping,
    Dead,
}

impl ConnectionState {
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
        state: ConnectionState,
    },
}

impl Mode {
    pub fn output(index: usize, path: String) -> Self {
        Self::Console {
            attached_server: index,
            path,
            input: String::new(),
            output: LineBuffer::new(),
            state: ConnectionState::Normal,
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

#[derive(Debug)]
pub struct Lines {
    buf: String,
    markers: Vec<usize>,
    max_width: usize,
    current_width: usize,
}

impl Lines {
    pub fn new(max_width: usize) -> Self {
        Self {
            buf: String::new(),
            markers: vec![0],
            max_width,
            current_width: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.markers.len()
    }

    pub fn markers(&self) -> &[usize] {
        &self.markers
    }

    pub fn push(&mut self, ch: char) {
        if ch == '\n' {
            self.buf.push('\n');
            self.markers.push(self.buf.len());
            self.current_width = 0;
            return;
        }

        let added_width = self.current_width + ch.len_utf8();

        if added_width > self.max_width {
            self.buf.push('\n');
            self.markers.push(self.buf.len());
            self.current_width = 0;
        } else {
            self.current_width = added_width;
        }

        self.buf.push(ch);
    }
}

impl From<Lines> for String {
    fn from(value: Lines) -> Self {
        value.buf
    }
}

impl Index<Range<usize>> for Lines {
    type Output = str;

    fn index(&self, index: Range<usize>) -> &Self::Output {
        let start_line = match index.start_bound() {
            ops::Bound::Included(n) => *n,
            ops::Bound::Excluded(n) => n + 1,
            ops::Bound::Unbounded => 0,
        };

        log!(
            "{:?} compared to length: {}",
            index.end_bound(),
            self.markers.len()
        );

        let end_line = match index.end_bound() {
            ops::Bound::Included(n) => n + 1,
            ops::Bound::Excluded(n) => *n,
            ops::Bound::Unbounded => self.markers.len(),
        };

        log!("{start_line}, {end_line} (delta {})", end_line - start_line);

        let start = self.markers[start_line];
        let end = if end_line == self.markers.len() {
            self.buf.len()
        } else {
            self.markers[end_line]
        };

        &self.buf[start..end]
    }
}

pub struct LineCache {
    buf: Option<Lines>,
}

impl LineCache {
    pub fn new() -> Self {
        Self { buf: None }
    }

    pub fn set(&mut self, value: Lines) -> &mut Lines {
        self.buf.insert(value)
    }
}
