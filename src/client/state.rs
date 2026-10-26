use std::{
    num::ParseIntError,
    ops::{self, Index, RangeBounds},
    str::FromStr,
    time::{Duration, SystemTime},
};

use derive_more::{Display, Error, From};
use ratatui::layout::Rect;

use crate::{
    line_buffer::LineBuffer,
    log,
    servers::{ServerEntry, ServerEntryType},
};

// Order:
// inherent impls
// owning/consuming
// borrowing/referencing

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

#[derive(Clone, Copy, Debug)]
pub enum AddingField {
    Name,
    Version,
    Software,
}

impl AddingField {
    pub fn prev(&mut self) -> bool {
        *self = match self {
            AddingField::Name => {
                *self = AddingField::Software;
                return true;
            }
            AddingField::Version => AddingField::Name,
            AddingField::Software => AddingField::Version,
        };

        false
    }

    pub fn next(&mut self) -> bool {
        *self = match self {
            AddingField::Name => AddingField::Version,
            AddingField::Version => AddingField::Software,
            AddingField::Software => {
                *self = AddingField::Name;
                return true;
            }
        };

        false
    }
}

#[derive(Clone, Copy, Debug, Display)]
pub enum ServerSoftware {
    #[display("Fabric")]
    Fabric,
    #[display("Forge")]
    Forge,
    #[display("NeoForge")]
    NeoForge,
    #[display("Paper")]
    Paper,
    Purpur,
    Vanilla,
}

#[derive(Clone, Copy, Debug)]
pub struct Version {
    pub major: usize,
    pub minor: usize,
    pub patch: Option<usize>,
}

#[derive(Debug, Display, Error, From)]
pub enum ParseVersionError {
    #[display("{_0}")]
    ParseIntError(ParseIntError),

    #[display("No major version was found")]
    NoMajor,

    #[display("No minor version was found")]
    NoMinor,

    #[display("Malformed version (received trailing period)")]
    Malformed,
}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let period = s.find('.').ok_or(ParseVersionError::NoMajor)?;

        let major = s[..period].parse()?;

        let remaining = s.get(period + 1..).ok_or(ParseVersionError::NoMinor)?;

        let (minor, patch) = if let Some(period) = remaining.find('.') {
            let minor = remaining[..period].parse()?;
            let patch = remaining
                .get(period + 1..)
                .ok_or(ParseVersionError::Malformed)?
                .parse()?;

            (minor, Some(patch))
        } else {
            (remaining.parse()?, None)
        };

        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

#[derive(Debug)]
pub enum SelectionState {
    Normal,
    Adding {
        name: String,
        version: String,
        server_software: String,
        field: AddingField,
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
            name: String::new(),
            version: String::new(),
            server_software: String::new(),
            field: AddingField::Name,
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

#[allow(dead_code)]
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
            self.current_width = ch.len_utf8();
        } else {
            self.current_width = added_width;
        }

        self.buf.push(ch);
    }

    pub fn into_buf(self) -> String {
        self.buf
    }
}

impl<R: RangeBounds<usize>> Index<R> for Lines {
    type Output = str;

    fn index(&self, index: R) -> &Self::Output {
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

#[cfg(test)]
mod test {
    use std::iter::repeat_n;

    use super::*;

    #[test]
    fn test_lines() -> miette::Result<()> {
        let width_values = [1, 5, 10, 100];

        for width in width_values {
            let mut lines = Lines::new(width);

            let mut simulated_lines = vec![];

            for i in 0..10 {
                let line = repeat_n((b'a' + i) as char, width);

                if i == 9 {
                    simulated_lines.push(line.clone().collect::<String>());
                } else {
                    simulated_lines.push(format!("{}\n", line.clone().collect::<String>()));
                }
                line.for_each(|ch| lines.push(ch));
            }

            for i in 0..10 {
                for j in i..10 {
                    let simulated_str: String = simulated_lines
                        .iter()
                        .enumerate()
                        .skip(i)
                        .map_while(|(idx, val)| if idx > j { None } else { Some(val) })
                        .map(String::from)
                        .collect();

                    assert_eq!(simulated_str, lines[i..j + 1]);
                }
            }
        }

        Ok(())
    }
}
