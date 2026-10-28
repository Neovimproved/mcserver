use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use futures::StreamExt;
use inotify::{Event, EventStream, Inotify, WatchMask};
use tokio::io;

use crate::error::{Error, Result};

pub struct Watch<const SIZE: usize = 4096> {
    directory: PathBuf,
    file_name: OsString,
    event_stream: EventStream<[u8; SIZE]>,
}

fn make_event_stream<const SIZE: usize>(
    path: impl AsRef<Path>,
) -> io::Result<EventStream<[u8; SIZE]>> {
    let inotify = Inotify::init()?;

    inotify
        .watches()
        .add(path, WatchMask::CREATE | WatchMask::MODIFY)?;

    let event_stream = inotify.into_event_stream([0u8; SIZE])?;

    Ok(event_stream)
}

impl<const SIZE: usize> Watch<SIZE> {
    pub fn try_new(mut file_path: PathBuf) -> Result<Self> {
        let event_stream = make_event_stream(&file_path)?;

        let file_name = file_path
            .file_name()
            .ok_or_else(|| Error::FileNameNotFound(file_path.clone()))?
            .to_os_string();

        file_path.pop();
        let directory = file_path;

        Ok(Self {
            directory,
            file_name,
            event_stream,
        })
    }

    pub async fn next(&mut self) -> Option<Result<Event<OsString>>> {
        let event = match self.event_stream.next().await? {
            Ok(event) => event,
            Err(err) => return Some(Err(err.into())),
        };

        if event.name.as_ref()? == &self.file_name {
            Some(Ok(event))
        } else {
            None
        }
    }

    pub fn restore(&mut self) -> Result<()> {
        self.event_stream = make_event_stream(&self.directory)?;
        Ok(())
    }
}
