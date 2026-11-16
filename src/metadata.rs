#![allow(dead_code)]
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, SystemTimeError},
};

use derive_more::{Display, Error, From};
use tokio::{fs, io};

use crate::error::display_path;

pub const DIRECTORY: &str = ".mcserver";
pub const JAR_FILE_NAME: &str = "jar_file.txt";
pub const LAST_USED_FILE_NAME: &str = "last_used.bin";

#[derive(Debug, Display, Error, From)]
pub enum MetadataError {
    #[display(
        "Metadata directory was not found (expected it to exist at: {})",
        display_path(_0)
    )]
    DirectoryNotFound(#[error(not(source))] PathBuf),

    Io(io::Error),

    #[display("Failed to read jar file ({}) with error: {_1}", display_path(_0))]
    JarFileRead(PathBuf, io::Error),

    SystemTime(SystemTimeError),
}

pub type MetadataResult<T> = std::result::Result<T, MetadataError>;

#[derive(Clone, Debug)]
pub enum LastUsed {
    Never,
    Unknown,
    Time(String),
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub jar_file: PathBuf,
    pub last_used: LastUsed,
}

pub fn dir(server_dir: impl Into<PathBuf>) -> PathBuf {
    let mut server_dir = server_dir.into();
    server_dir.push(DIRECTORY);
    server_dir
}

pub async fn set_jar_file_with_metadata_dir(
    metadata_dir: impl Into<PathBuf>,
    jar_file_name: &str,
) -> MetadataResult<PathBuf> {
    let mut metadata_dir = metadata_dir.into();
    metadata_dir.push(JAR_FILE_NAME);
    let file_path = metadata_dir;

    fs::write(&file_path, jar_file_name).await?;

    Ok(file_path)
}

pub async fn set_jar_file(
    server_dir: impl Into<PathBuf>,
    jar_file_name: &str,
) -> MetadataResult<PathBuf> {
    set_jar_file_with_metadata_dir(dir(server_dir), jar_file_name).await
}

pub async fn get_jar_file(server_dir: impl Into<PathBuf>) -> MetadataResult<PathBuf> {
    let mut server_dir = server_dir.into();
    let mut file_path = dir(&server_dir);
    file_path.push(JAR_FILE_NAME);

    let mut contents = fs::read_to_string(&file_path)
        .await
        .map_err(|err| MetadataError::JarFileRead(file_path, err))?;

    // If a user has written to the file, it is possible that they inadvertently added newlines
    while let Some(ch) = contents.chars().next_back()
        && ch == '\n'
    {
        contents.pop();
    }

    server_dir.push(contents);

    Ok(server_dir)
}

fn get_time_since_unix_epoch_secs() -> MetadataResult<u64> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs())
}

pub async fn get_last_used(metadata_dir: &Path) -> MetadataResult<LastUsed> {
    let bytes = match fs::read(metadata_dir.join(LAST_USED_FILE_NAME)).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(LastUsed::Never),
        Err(err) => return Err(err.into()),
    };

    let timestamp = u64::from_le_bytes(match bytes.try_into() {
        Ok(bytes) => bytes,
        Err(_) => return Ok(LastUsed::Unknown),
    });

    let total_seconds = timestamp.saturating_sub(get_time_since_unix_epoch_secs()?);

    const SECS_MINUTE: u64 = 60;
    const SECS_HOUR: u64 = SECS_MINUTE * 60;
    const SECS_DAY: u64 = SECS_HOUR * 24;

    let days = total_seconds / SECS_DAY;
    let days_remainder = timestamp % SECS_DAY;

    let hours = days_remainder / SECS_HOUR;
    let hours_remainder = days_remainder % SECS_HOUR;

    let minutes = hours_remainder / SECS_MINUTE;
    let seconds = hours_remainder % SECS_MINUTE;

    Ok(LastUsed::Time(if days > 0 {
        format!("{days}d {hours}m {minutes}m {seconds}s")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }))
}

pub async fn update_last_used(metadata_dir: PathBuf) -> MetadataResult<()> {
    fs::create_dir_all(&metadata_dir).await?;

    let mut path = metadata_dir;
    path.push(LAST_USED_FILE_NAME);

    let current_time = get_time_since_unix_epoch_secs()?;
    fs::write(path, current_time.to_be_bytes()).await?;

    Ok(())
}

pub async fn get(server_dir: &Path) -> MetadataResult<Metadata> {
    let dir = dir(server_dir);

    if !dir.try_exists()? {
        return Err(dir.into());
    }

    let jar_file = get_jar_file(server_dir).await?;
    let last_used = get_last_used(&dir).await?;

    Ok(Metadata {
        jar_file,
        last_used,
    })
}
