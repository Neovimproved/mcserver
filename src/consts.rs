use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use const_format::formatcp;

// Networking
pub const CONNECTION_PORT: u16 = 56602;

pub const LOCAL_TCP_ADDR: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), CONNECTION_PORT);

pub const TEMP_DIR: &str = "/tmp/mcserver";
pub const PID_FILE_PATH: &str = formatcp!("{TEMP_DIR}/mcserver.pid");

pub const SOCKET_FILE_NAME: &str = "mcserver.sock";
pub const SOCKET_PATH: &str = formatcp!("{TEMP_DIR}/{SOCKET_FILE_NAME}");

// 1MiB
pub const MAX_MESSAGE_LENGTH: usize = 1024 * 1024;

// Config

pub const DEFAULT_CONFIG_DIRECTORY: &str = "mcserver";
pub const DEFAULT_CONFIG_FILE_NAME: &str = "config.kdl";
pub const CONFIG_FILE_VARIABLE: &str = "MCSERVER_CONFIG_FILE";

// Metadata

pub const METADATA_DIRECTORY: &str = ".mcserver";
pub const JAR_FILE_NAME: &str = "jar_file.txt";
pub const LAST_USED_FILE_NAME: &str = "last_used.bin";

//
pub const CURSOR_CHAR: char = '█';
