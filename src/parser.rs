use std::{result, str::FromStr, time::Duration};

use const_format::formatcp;
use derive_more::{Display, Error};
use kdl::{KdlDocument, KdlNode, KdlValue};
use ratatui::style::{Color, ParseColorError};

use crate::{
    config::{Config, ServersDirectory, TerminalInterfaceConfig},
    consts::{DEFAULT_BLINK_INTERVAL, DEFAULT_SERVERS_DIRECTORY},
    error::Result,
};

const TUI_BACKGROUND_COLOR_FIELD: &str = "background_color";

#[derive(Debug, Display)]
pub enum ParseConfigErrorKind {
    #[display("{_0}")]
    Color(ParseColorError),

    #[display("Structure of value is wrong")]
    InvalidStructure,

    #[display("Invalid type")]
    InvalidType,

    #[display("Value not found")]
    ValueNotFound,
}

#[derive(Debug, Display, Error)]
#[display("{kind}: {context}")]
pub struct ParseConfigError {
    kind: ParseConfigErrorKind,
    context: &'static str,
}

impl ParseConfigError {
    fn new(kind: ParseConfigErrorKind, context: &'static str) -> Self {
        Self { kind, context }
    }
}

impl ParseConfigError {
    fn invalid_type(context: &'static str) -> Self {
        Self {
            kind: ParseConfigErrorKind::InvalidType,
            context,
        }
    }

    fn invalid_structure(context: &'static str) -> Self {
        Self {
            kind: ParseConfigErrorKind::InvalidStructure,
            context,
        }
    }
}

pub type ParseConfigResult<T> = std::result::Result<T, ParseConfigError>;

fn parse_value<T>(
    node: &KdlNode,
    value_name: &'static str,
    map_as: impl FnOnce(&KdlValue) -> Option<T>,
) -> ParseConfigResult<T> {
    map_as(node.get(0).ok_or(ParseConfigError::new(
        ParseConfigErrorKind::ValueNotFound,
        value_name,
    ))?)
    .ok_or(ParseConfigError::new(
        ParseConfigErrorKind::InvalidType,
        value_name,
    ))
}

fn kdl_value_to_string(value: &KdlValue) -> Option<String> {
    value.as_string().map(str::to_owned)
}

fn parse_alias(node: &KdlNode) -> ParseConfigResult<(String, String)> {
    let reference = parse_value(node, "alias", kdl_value_to_string)?;

    Ok((node.name().value().to_string(), reference.to_string()))
}

fn parse_blink_interval(val: &KdlValue) -> Option<Option<Duration>> {
    // Explanation:
    // Allow the user to enter booleans, strings or integers here
    // A boolean means either use the default interval or don't blink at all
    // A string can be used to give a blink interval in seconds or milliseconds (e.g. 1s, 500ms)
    // An integer can be used to give a millisecond blink interval

    if let Some(n) = val.as_integer() {
        return Some(Some(Duration::from_millis(n as u64)));
    }

    if let Some(b) = val.as_bool() {
        return Some(if b { DEFAULT_BLINK_INTERVAL } else { None });
    }

    let s = val.as_string()?;

    if let Some(stripped) = s.strip_prefix('s') {
        Some(Some(Duration::from_secs(stripped.parse().ok()?)))
    } else if let Some(stripped) = s.strip_prefix("ms") {
        Some(Some(Duration::from_millis(stripped.parse().ok()?)))
    } else {
        None
    }
}

fn triple_ok_transpose<T, U, V, E>(
    a: Option<result::Result<T, E>>,
    b: Option<result::Result<U, E>>,
    c: Option<result::Result<V, E>>,
) -> Option<result::Result<(T, U, V), E>> {
    Some(Ok((
        match a? {
            Ok(val) => val,
            Err(err) => return Some(Err(err)),
        },
        match b? {
            Ok(val) => val,
            Err(err) => return Some(Err(err)),
        },
        match c? {
            Ok(val) => val,
            Err(err) => return Some(Err(err)),
        },
    )))
}

fn parse_tui_background_color(node: &KdlNode) -> ParseConfigResult<Option<Color>> {
    match node.entries() {
        [first] => {
            if let Some(color) = first.value().as_string() {
                Ok(Some(Color::from_str(color).map_err(|err| {
                    ParseConfigError::new(
                        ParseConfigErrorKind::Color(err),
                        TUI_BACKGROUND_COLOR_FIELD,
                    )
                })?))
            } else {
                Ok(None)
            }
        }
        [first, second, third] => {
            let rgb = triple_ok_transpose(
                first.value().as_integer().map(u8::try_from),
                second.value().as_integer().map(u8::try_from),
                third.value().as_integer().map(u8::try_from),
            )
            .ok_or(ParseConfigError::invalid_type(TUI_BACKGROUND_COLOR_FIELD))?
            .map_err(|_| {
                ParseConfigError::new(
                    ParseConfigErrorKind::InvalidType,
                    formatcp!("{TUI_BACKGROUND_COLOR_FIELD} received a value outside of the bounds permitted (0..256)"),
                )
            })?;

            Ok(Some(Color::Rgb(rgb.0, rgb.1, rgb.2)))
        }
        _ => Err(ParseConfigError::invalid_structure(formatcp!(
            "{TUI_BACKGROUND_COLOR_FIELD}: a structure like `{TUI_BACKGROUND_COLOR_FIELD} 46, 52, 64`"
        ))),
    }
}

fn parse_tui_config(node: Option<&KdlNode>) -> ParseConfigResult<TerminalInterfaceConfig> {
    let Some(children) = node.and_then(KdlNode::children) else {
        return Ok(TerminalInterfaceConfig::default());
    };

    let background_color = children
        .get(TUI_BACKGROUND_COLOR_FIELD)
        .map(parse_tui_background_color)
        .transpose()?
        .unwrap_or_default();

    Ok(TerminalInterfaceConfig { background_color })
}

pub fn parse_config(document: &KdlDocument) -> Result<Config> {
    let default_java_args = document
        .get("default_java_args")
        .map(|node| {
            node.iter_children()
                .map(|child| format!("\"{}\"", child.name().value()))
                .collect()
        })
        .unwrap_or_default();

    let servers_directory = match document.get("servers_directory") {
        Some(node) => {
            ServersDirectory::try_from(parse_value(node, "servers_directory", kdl_value_to_string)?)
        }
        None => ServersDirectory::from_str(DEFAULT_SERVERS_DIRECTORY),
    }?;

    let aliases = document
        .get("aliases")
        .map(|node| node.iter_children().map(parse_alias).collect())
        .transpose()?
        .unwrap_or_default();

    let auto_restart = document
        .get("auto_restart")
        .map(|node| {
            parse_value(node, "auto_restart", |val| {
                val.as_integer().map(|x| x as usize)
            })
        })
        .transpose()?;

    let blink_interval = document
        .get("blink_interval")
        .map(|node| parse_value(node, "blink_interval", parse_blink_interval))
        .transpose()?
        .unwrap_or(DEFAULT_BLINK_INTERVAL);

    let terminal_interface_config = parse_tui_config(document.get("terminal_user_interface"))?;

    Ok(Config {
        default_java_args,
        servers_directory,
        aliases,
        auto_restart,
        blink_interval,
        terminal_interface_config,
    })
}
