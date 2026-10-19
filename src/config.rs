use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt::Debug,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use dirs::home_dir;
use kdl::KdlDocument;
use ratatui::style::Color;
use tokio::{fs, io};

use crate::{
    consts::{
        CONFIG_DIRECTORY_NAME, CONFIG_FILE_NAME, DEFAULT_BLINK_INTERVAL, DEFAULT_SERVERS_DIRECTORY,
    },
    error::{Error, Result},
    parser::parse_config,
};

#[derive(Debug, Default)]
pub struct TerminalInterfaceConfig {
    pub background_color: Option<Color>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ServersDirectory(String);

impl TryFrom<String> for ServersDirectory {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        let expanded = shellexpand::full(&value)?.to_string();

        Ok(Self(if Path::new(&expanded).is_relative() {
            let mut path = home_dir().ok_or(Error::HomeDirectoryNotFound)?;
            path.push(expanded);
            path.into_string().map_err(Error::NonUtf8Path)?
        } else {
            expanded
        }))
    }
}

impl FromStr for ServersDirectory {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::try_from(s.to_owned())
    }
}

impl Deref for ServersDirectory {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ServersDirectory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRef<OsStr> for ServersDirectory {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(&self.0)
    }
}

impl AsRef<Path> for ServersDirectory {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Config {
    pub default_java_args: Vec<String>,
    pub servers_directory: ServersDirectory,
    pub aliases: HashMap<String, String>,
    pub auto_restart: Option<usize>,
    pub blink_interval: Option<Duration>,
    pub terminal_interface_config: TerminalInterfaceConfig,
}

impl Config {
    pub fn try_default() -> Result<Self> {
        Ok(Self {
            default_java_args: Default::default(),
            servers_directory: ServersDirectory::from_str(DEFAULT_SERVERS_DIRECTORY)?,
            aliases: Default::default(),
            auto_restart: Default::default(),
            blink_interval: DEFAULT_BLINK_INTERVAL,
            terminal_interface_config: Default::default(),
        })
    }
}

fn get_directory() -> Result<PathBuf> {
    let mut config_dir = dirs::config_dir().ok_or(Error::ConfigDirectoryNotFound)?;
    config_dir.push(CONFIG_DIRECTORY_NAME);
    Ok(config_dir)
}

fn get_file_path() -> Result<PathBuf> {
    let mut path = get_directory()?;
    path.push(CONFIG_FILE_NAME);
    Ok(path)
}

pub async fn read() -> Result<(Config, Option<KdlDocument>)> {
    match fs::read_to_string(get_file_path()?).await {
        Ok(contents) => {
            let document = KdlDocument::parse(&contents)?;
            let config = parse_config(&document)?;

            Ok((config, Some(document)))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok((Config::try_default()?, None)),
        Err(err) => Err(err.into()),
    }
}
