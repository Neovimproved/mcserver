use std::{collections::HashMap, env::home_dir, fmt::Debug, path::PathBuf, str::FromStr};

use kdl::KdlDocument;
use ratatui::style::Color;
use tokio::{fs, io};

use crate::{
    CONFIG_DIRECTORY_NAME, CONFIG_FILE_NAME, DEFAULT_SERVERS_DIRECTORY, Error, Result,
    parser::parse_config,
};

#[derive(Debug, Default)]
pub struct TerminalInterfaceConfig {
    pub background_color: Option<Color>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct ServersDirectory(PathBuf);

impl TryFrom<String> for ServersDirectory {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        let value = PathBuf::from(value);

        Ok(Self(if value.is_relative() {
            let mut home = home_dir().ok_or(Error::HomeDirectoryNotFound)?;
            home.extend(&value);
            home
        } else {
            value
        }))
    }
}

impl FromStr for ServersDirectory {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::try_from(s.to_owned())
    }
}

#[derive(Debug)]
pub struct Config {
    pub default_java_args: Vec<String>,
    pub servers_directory: ServersDirectory,
    pub aliases: HashMap<String, String>,
    pub auto_restart: Option<usize>,
    pub terminal_interface_config: TerminalInterfaceConfig,
}

impl Config {
    pub fn try_default() -> Result<Self> {
        Ok(Self {
            default_java_args: Default::default(),
            servers_directory: ServersDirectory::from_str(DEFAULT_SERVERS_DIRECTORY)?,
            aliases: Default::default(),
            auto_restart: Default::default(),
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
