use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use const_format::formatcp;

pub const TCP_PORT: u16 = 56608;

pub const LOCAL_TCP_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), TCP_PORT);

pub const TEMP_DIR: &str = "/tmp/mcserver";
pub const PID_FILE_PATH: &str = formatcp!("{TEMP_DIR}/mcserver.pid");

pub const SOCKET_PATH: &str = formatcp!("{TEMP_DIR}/mcserver.sock");

// 1MiB
pub const MAX_MESSAGE_LENGTH: usize = 1024 * 1024;

pub const CONFIG_DIRECTORY_NAME: &str = "mcserver";
pub const CONFIG_FILE_NAME: &str = "config.kdl";

pub const DEFAULT_SERVERS_DIRECTORY: &str = "~/Servers";
