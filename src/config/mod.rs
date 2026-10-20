use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    fmt::Debug,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use dirs::home_dir;
use kdl::KdlDocument;
use ratatui::style::Color;
use tokio::{fs, io};

use crate::{
    consts::{CONFIG_DIRECTORY_NAME, CONFIG_FILE_NAME, CONFIG_FILE_VARIABLE},
    error::{Error, Result},
};

mod parser;

pub use parser::ParseConfigError;
use parser::parse_config;

#[derive(Clone, Debug)]
pub struct ServersDirectory {
    pub value: String,
    pub path: PathBuf,
}

impl TryFrom<String> for ServersDirectory {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        let expanded = shellexpand::full(&value)?.to_string();

        Ok(if Path::new(&expanded).is_relative() {
            let mut path = home_dir().ok_or(Error::HomeDirectoryNotFound)?;
            path.push(expanded);

            Self {
                value: path.clone().into_string().map_err(Error::NonUtf8Path)?,
                path,
            }
        } else {
            Self {
                value,
                path: expanded.into(),
            }
        })
    }
}

impl FromStr for ServersDirectory {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::try_from(s.to_owned())
    }
}

impl AsRef<OsStr> for ServersDirectory {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(&self.path)
    }
}

impl AsRef<Path> for ServersDirectory {
    fn as_ref(&self) -> &Path {
        Path::new(&self.path)
    }
}

#[derive(Debug)]
pub struct TerminalInterfaceConfig {
    pub cursor_blink: Option<Duration>,
    pub background_color: Option<Color>,
    pub selected_color: Color,

    pub group_symbol_color: Color,
    pub group_folded_symbol: String,
    pub group_expanded_symbol: String,

    pub server_offline_color: Color,
    pub server_online_color: Color,
    pub server_offline_symbol: String,
    pub server_online_symbol: String,
}

impl Default for TerminalInterfaceConfig {
    fn default() -> Self {
        Self {
            cursor_blink: TerminalInterfaceDefaults::CURSOR_BLINK,
            background_color: Default::default(),
            selected_color: TerminalInterfaceDefaults::SELECTED_COLOR,

            group_symbol_color: TerminalInterfaceDefaults::GROUP_SYMBOL_COLOR,
            group_folded_symbol: TerminalInterfaceDefaults::GROUP_FOLDED_SYMBOL.to_owned(),
            group_expanded_symbol: TerminalInterfaceDefaults::GROUP_EXPANDED_SYMBOL.to_owned(),

            server_offline_color: TerminalInterfaceDefaults::SERVER_OFFLINE_COLOR,
            server_online_color: TerminalInterfaceDefaults::SERVER_ONLINE_COLOR,
            server_offline_symbol: TerminalInterfaceDefaults::SERVER_OFFLINE_SYMBOL.to_owned(),
            server_online_symbol: TerminalInterfaceDefaults::SERVER_ONLINE_SYMBOL.to_owned(),
        }
    }
}

pub struct TerminalInterfaceKeys;

impl TerminalInterfaceKeys {
    pub const CURSOR_BLINK: &str = "cursor-blink";

    pub const BACKGROUND_COLOR: &str = "background-color";
    pub const SELECTED_COLOR: &str = "selected-color";

    pub const GROUP_SYMBOL_COLOR: &str = "group-symbol-color";
    pub const GROUP_FOLDED_SYMBOL: &str = "group-folded-symbol";
    pub const GROUP_EXPANDED_SYMBOL: &str = "group-expanded-symbol";

    pub const SERVER_OFFLINE_COLOR: &str = "server-offline-color";
    pub const SERVER_ONLINE_COLOR: &str = "server-online-color";
    pub const SERVER_OFFLINE_SYMBOL: &str = "server-offline-symbol";
    pub const SERVER_ONLINE_SYMBOL: &str = "server-online-symbol";
}

pub struct TerminalInterfaceDefaults;

impl TerminalInterfaceDefaults {
    const CURSOR_BLINK: Option<Duration> = Some(Duration::from_millis(500));

    // Background color
    // Hella dark:
    // 26, 27, 38
    //
    // 30, 31, 42
    //
    // Very light, neutral, middle ground, valid color
    // 46, 52, 64

    // Selected color
    // Light blue looks kinda mid, blue is a reasonable enough default
    const SELECTED_COLOR: Color = Color::Blue;

    const GROUP_SYMBOL_COLOR: Color = Color::Rgb(122, 162, 247);
    const GROUP_FOLDED_SYMBOL: &str = "󰉋 ";
    const GROUP_EXPANDED_SYMBOL: &str = "󰝰 ";

    const SERVER_OFFLINE_COLOR: Color = Color::Rgb(222, 156, 50);
    const SERVER_ONLINE_COLOR: Color = Color::Green;
    const SERVER_OFFLINE_SYMBOL: &str = " ";
    const SERVER_ONLINE_SYMBOL: &str = "● ";
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Config {
    pub default_java_args: Vec<String>,
    pub servers_directory: ServersDirectory,
    pub aliases: HashMap<String, String>,
    pub auto_restart: Option<usize>,
    pub terminal_interface: TerminalInterfaceConfig,
}

pub struct Keys;

impl Keys {
    pub const DEFAULT_JAVA_ARGS: &str = "default-java-args";
    pub const SERVERS_DIRECTORY: &str = "servers-directory";
    pub const ALIASES: &str = "aliases";
    pub const AUTO_RESTART: &str = "auto-restart";
    pub const TERIMNAL_INTERFACE: &str = "terminal-interface";
}

pub struct Defaults;

impl Defaults {
    pub const SERVERS_DIRECTORY: &str = "~/Servers";
}

impl Config {
    pub fn try_default() -> Result<Self> {
        Ok(Self {
            default_java_args: Default::default(),
            servers_directory: ServersDirectory::from_str(Defaults::SERVERS_DIRECTORY)?,
            aliases: Default::default(),
            auto_restart: Default::default(),
            terminal_interface: Default::default(),
        })
    }
}

fn get_file_path() -> Result<PathBuf> {
    let mut config_path = dirs::config_dir().ok_or(Error::ConfigDirectoryNotFound)?;
    config_path.extend([CONFIG_DIRECTORY_NAME, CONFIG_FILE_NAME]);
    Ok(config_path)
}

async fn read_file(file_path: &Path) -> Result<(Config, Option<KdlDocument>)> {
    match fs::read_to_string(file_path).await {
        Ok(contents) => {
            let document = KdlDocument::parse(&contents)?;
            let config = parse_config(&document)?;

            Ok((config, Some(document)))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok((Config::try_default()?, None)),
        Err(err) => Err(err.into()),
    }
}

pub async fn read() -> Result<(Config, Option<KdlDocument>)> {
    read_file(&match env::var_os(CONFIG_FILE_VARIABLE) {
        Some(var) => PathBuf::from(var),
        None => get_file_path()?,
    })
    .await
}

#[cfg(test)]
mod test {
    use miette::{Context, IntoDiagnostic};

    use crate::test_utils::tempdir_init;

    use super::*;

    #[tokio::test]
    async fn test_read_file() -> miette::Result<()> {
        let dir = tempdir_init()?;

        let config_path = dir.path().join(CONFIG_FILE_NAME);

        let java_args = [
            "-Xms1G",
            "-Xmx2G",
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+DisableExplicitGC",
        ];

        let java_args_string = java_args.iter().fold(String::from('\n'), |mut acc, item| {
            acc.push_str("    \"");
            acc.push_str(item);
            acc.push_str("\"\n");
            acc
        });

        // to test tilde and env expansion
        let servers_directory = "~/$HOME/bum_cheeks";

        // TODO: convert the test cases by removing the backslashes, leading and trailing quotations etc. in comparison rhs (removed for now)

        let aliases = HashMap::from([
            ("default", "hi"),
            ("~^file.nix", "file.nix"),
            ("-%po1a+!:-o.bum", "poo.bum"),
            ("poop%", "hi"),
            ("poop", "ksib"),
        ]);

        let aliases_string = aliases.iter().fold(String::from('\n'), |mut acc, item| {
            acc.push_str("    ");
            acc.push_str(item.0);
            acc.push(' ');
            acc.push_str(item.1);
            acc.push('\n');
            acc
        });

        // let terminal_interface = TerminalInterfaceConfig {
        //     cursor_blink: todo!(),
        //     background_color: todo!(),
        //     selected_color: todo!(),
        //     group_symbol_color: todo!(),
        //     group_folded_symbol: todo!(),
        //     group_expanded_symbol: todo!(),
        //     server_offline_color: todo!(),
        //     server_online_color: todo!(),
        //     server_offline_symbol: todo!(),
        //     server_online_symbol: todo!(),
        // };

        let config_string = format!(
            r#"
default-java-args {{{}}}

// The directory where servers are stored and expected to be stored (local to home)
servers-directory "{servers_directory}"

// Aliases for your servers!
aliases {{{}}}

terminal-interface {{
    cursor-blink #false

    // background-color 22 22 30
    background-color 26 27 38

    selected-color 40 52 87
}}
            "#,
            java_args_string, aliases_string
        );

        fs::write(&config_path, config_string)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write to config file `{config_path:?}`"))?;

        let (config, _) = read_file(&config_path)
            .await
            .into_diagnostic()
            .wrap_err("Failed to read config")?;

        assert_eq!(&config.default_java_args, &java_args);

        assert_eq!(&config.servers_directory.value, servers_directory);

        assert_eq!(
            config
                .aliases
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect::<HashMap<&str, &str>>(),
            aliases
        );

        Ok(())
    }
}
