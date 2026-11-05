use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use futures::StreamExt;
use inotify::{EventStream, Inotify, WatchMask};
use tokio::fs;

use crate::{
    error::{Error, Result},
    log,
};

#[derive(Debug)]
enum CurrentlyWatching {
    Other(PathBuf),
    Parent,
}

/// for example it is 0 if the path exists
async fn first_existent_ancestor(path: &Path) -> Result<&Path> {
    for ancestor in path.ancestors() {
        if fs::try_exists(ancestor).await? {
            return Ok(ancestor);
        }
    }

    Ok(Path::new(""))
}

impl CurrentlyWatching {
    async fn try_new(path: &Path) -> Result<Self> {
        if fs::try_exists(&path).await? {
            log!("{path:?} exists!");
            Ok(Self::Parent)
        } else {
            let path = first_existent_ancestor(path).await?;
            log!("{path:?} exists (other)");
            Ok(Self::Other(path.to_path_buf()))
        }
    }
}

trait PathExt {
    fn file_name_or_err(&self) -> Result<&OsStr>;
}

impl PathExt for Path {
    fn file_name_or_err(&self) -> Result<&OsStr> {
        self.file_name()
            .ok_or_else(|| Error::FileNameNotFound(self.to_path_buf()))
    }
}

impl PathExt for PathBuf {
    fn file_name_or_err(&self) -> Result<&OsStr> {
        self.as_path().file_name_or_err()
    }
}

static MASK: LazyLock<WatchMask> = LazyLock::new(|| {
    WatchMask::CREATE | WatchMask::MODIFY | WatchMask::MOVE | WatchMask::MOVE_SELF
});

pub struct Watch<const SIZE: usize = 4096> {
    path_parent: PathBuf,
    file_name: OsString,
    event_stream: EventStream<[u8; SIZE]>,
    currently_watching: CurrentlyWatching,
}

impl<const SIZE: usize> Watch<SIZE> {
    pub async fn try_new(path: impl Into<PathBuf>) -> Result<Self> {
        Self::with_path_buf(path.into()).await
    }

    pub async fn with_path_buf(target_path: PathBuf) -> Result<Self> {
        let file_name = target_path.file_name_or_err()?.to_os_string();

        let mut path_parent = target_path;
        path_parent.pop();

        let inotify = Inotify::init()?;
        let currently_watching = CurrentlyWatching::try_new(&path_parent).await?;

        inotify.watches().add(
            match &currently_watching {
                CurrentlyWatching::Other(path_buf) => path_buf,
                CurrentlyWatching::Parent => &path_parent,
            },
            *MASK,
        )?;

        let event_stream = inotify.into_event_stream([0u8; SIZE])?;

        Ok(Self {
            path_parent,
            file_name,
            event_stream,
            currently_watching,
        })
    }

    pub fn restore(&mut self) -> Result<()> {
        Ok(())
    }

    pub async fn recreate_watch(&mut self) -> Result<()> {
        self.currently_watching = CurrentlyWatching::try_new(&self.path_parent).await?;

        let inotify = Inotify::init()?;
        inotify.watches().add(
            match &self.currently_watching {
                CurrentlyWatching::Other(path) => path,
                CurrentlyWatching::Parent => &self.path_parent,
            },
            *MASK,
        )?;

        self.event_stream = inotify.into_event_stream([0u8; SIZE])?;

        Ok(())
    }

    async fn read_from_parent(&mut self) -> Result<bool> {
        loop {
            let event = match self.event_stream.next().await {
                Some(event) => event,
                None => return Ok(false),
            }?;

            if let Some(name) = &event.name
                && *name == self.file_name
            {
                break Ok(true);
            }
        }
    }

    pub async fn next(&mut self) -> Result<bool> {
        if matches!(self.currently_watching, CurrentlyWatching::Parent) {
            return self.read_from_parent().await;
        }

        // If the current watch's path is equal to the target path
        while let CurrentlyWatching::Other(path) = &self.currently_watching {
            let event = match self.event_stream.next().await {
                Some(event) => event,
                None => return Ok(false),
            }?;

            let Some(event_name) = event.name else {
                continue;
            };

            if !self.path_parent.starts_with(path.join(event_name)) {
                continue;
            }

            self.recreate_watch().await?;
        }

        Ok(true)
    }
}
