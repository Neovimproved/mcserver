use std::{borrow::Cow, result, str::FromStr, time::Duration};

use derive_more::{Display, Error};
use kdl::{KdlDocument, KdlNode, KdlValue};
use ratatui::style::{Color, ParseColorError};

use crate::{config::Keys, error::Result};

use super::{
    Config, Defaults, ServersDirectory, TerminalInterfaceConfig, TerminalInterfaceDefaults,
    TerminalInterfaceKeys,
};

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
    context: Cow<'static, str>,
}

impl ParseConfigError {
    fn new(kind: ParseConfigErrorKind, context: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            context: context.into(),
        }
    }
}

impl ParseConfigError {
    fn invalid_type(context: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind: ParseConfigErrorKind::InvalidType,
            context: context.into(),
        }
    }

    fn invalid_structure(context: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind: ParseConfigErrorKind::InvalidStructure,
            context: context.into(),
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
        return Some(if b {
            TerminalInterfaceDefaults::CURSOR_BLINK
        } else {
            None
        });
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

fn transpose_opt<T, U, V>((a, b, c): (Option<T>, Option<U>, Option<V>)) -> Option<(T, U, V)> {
    Some((a?, b?, c?))
}

fn transpose_res<T, U, V, E>(
    (a, b, c): (
        result::Result<T, E>,
        result::Result<U, E>,
        result::Result<V, E>,
    ),
) -> result::Result<(T, U, V), E> {
    Ok((a?, b?, c?))
}

fn parse_color(
    document: &KdlDocument,
    field_name: &'static str,
) -> ParseConfigResult<Option<Color>> {
    let Some(node) = document.get(field_name) else {
        return Ok(None);
    };

    match node.entries() {
        [first] => {
            if let Some(color) = first.value().as_string() {
                Ok(Some(Color::from_str(color).map_err(|err| {
                    ParseConfigError::new(ParseConfigErrorKind::Color(err), field_name)
                })?))
            } else {
                Ok(None)
            }
        }
        [first, second, third] => {
            let rgb = transpose_res(
                transpose_opt((
                    first.value().as_integer().map(u8::try_from),
                    second.value().as_integer().map(u8::try_from),
                    third.value().as_integer().map(u8::try_from),
                ))
                .ok_or(ParseConfigError::invalid_type(field_name))?,
            )
            .map_err(|_| {
                ParseConfigError::new(
                    ParseConfigErrorKind::InvalidType,
                    format!(
                        "{field_name} received a value outside of the bounds permitted (0..256)"
                    ),
                )
            })?;

            Ok(Some(Color::Rgb(rgb.0, rgb.1, rgb.2)))
        }
        _ => Err(ParseConfigError::invalid_structure(format!(
            "{field_name}: a structure like `{field_name} 46, 52, 64`"
        ))),
    }
}

fn parse_symbol(
    document: &KdlDocument,
    field_name: &'static str,
) -> ParseConfigResult<Option<String>> {
    match document.get(field_name) {
        Some(node) => Ok(Some(parse_value(node, field_name, |x| {
            KdlValue::as_string(x).map(str::to_owned)
        })?)),
        None => Ok(None),
    }
}

fn parse_ti_config(node: Option<&KdlNode>) -> ParseConfigResult<TerminalInterfaceConfig> {
    let Some(document) = node.and_then(KdlNode::children) else {
        return Ok(TerminalInterfaceConfig::default());
    };

    let cursor_blink = document
        .get(TerminalInterfaceKeys::CURSOR_BLINK)
        .map(|node| parse_value(node, "blink", parse_blink_interval))
        .transpose()?
        .unwrap_or(TerminalInterfaceDefaults::CURSOR_BLINK);

    // General
    let background_color = parse_color(document, TerminalInterfaceKeys::BACKGROUND_COLOR)?;

    let selected_color = parse_color(document, TerminalInterfaceKeys::SELECTED_COLOR)?
        .unwrap_or(TerminalInterfaceDefaults::SELECTED_COLOR);

    // Groups
    let group_symbol_color = parse_color(document, TerminalInterfaceKeys::GROUP_SYMBOL_COLOR)?
        .unwrap_or(TerminalInterfaceDefaults::GROUP_SYMBOL_COLOR);

    let group_folded_symbol = parse_symbol(document, TerminalInterfaceKeys::GROUP_FOLDED_SYMBOL)?
        .unwrap_or_else(|| TerminalInterfaceDefaults::GROUP_FOLDED_SYMBOL.to_string());

    let group_expanded_symbol =
        parse_symbol(document, TerminalInterfaceKeys::GROUP_EXPANDED_SYMBOL)?
            .unwrap_or_else(|| TerminalInterfaceDefaults::GROUP_EXPANDED_SYMBOL.to_string());

    // Servers
    let server_offline_color = parse_color(document, TerminalInterfaceKeys::SERVER_OFFLINE_COLOR)?
        .unwrap_or(TerminalInterfaceDefaults::SERVER_OFFLINE_COLOR);

    let server_online_color = parse_color(document, TerminalInterfaceKeys::SERVER_ONLINE_COLOR)?
        .unwrap_or(TerminalInterfaceDefaults::SERVER_ONLINE_COLOR);

    let server_offline_symbol =
        parse_symbol(document, TerminalInterfaceKeys::SERVER_OFFLINE_SYMBOL)?
            .unwrap_or_else(|| TerminalInterfaceDefaults::SERVER_OFFLINE_SYMBOL.to_string());

    let server_online_symbol = parse_symbol(document, TerminalInterfaceKeys::SERVER_ONLINE_SYMBOL)?
        .unwrap_or_else(|| TerminalInterfaceDefaults::SERVER_ONLINE_SYMBOL.to_string());

    Ok(TerminalInterfaceConfig {
        cursor_blink,
        background_color,
        selected_color,

        group_symbol_color,
        group_folded_symbol,
        group_expanded_symbol,

        server_offline_symbol,
        server_online_symbol,
        server_offline_color,
        server_online_color,
    })
}

pub fn parse_config(document: &KdlDocument) -> Result<Config> {
    let default_java_args = document
        .get(Keys::DEFAULT_JAVA_ARGS)
        .map(|node| {
            node.iter_children()
                .map(|child| child.name().value().to_string())
                .collect()
        })
        .unwrap_or_default();

    let servers_directory = match document.get(Keys::SERVERS_DIRECTORY) {
        Some(node) => ServersDirectory::try_from(parse_value(
            node,
            Keys::SERVERS_DIRECTORY,
            kdl_value_to_string,
        )?),
        None => ServersDirectory::from_str(Defaults::SERVERS_DIRECTORY),
    }?;

    let aliases = document
        .get(Keys::ALIASES)
        .map(|node| node.iter_children().map(parse_alias).collect())
        .transpose()?
        .unwrap_or_default();

    let auto_restart = document
        .get(Keys::AUTO_RESTART)
        .map(|node| {
            parse_value(node, Keys::AUTO_RESTART, |val| {
                val.as_integer().map(|x| x as usize)
            })
        })
        .transpose()?;

    let terminal_interface = parse_ti_config(document.get(Keys::TERIMNAL_INTERFACE))?;

    Ok(Config {
        default_java_args,
        servers_directory,
        aliases,
        auto_restart,
        terminal_interface,
    })
}
