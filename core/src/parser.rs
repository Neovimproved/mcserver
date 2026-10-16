use derive_more::{Display, Error};
use kdl::{KdlDocument, KdlNode, KdlValue};

use crate::{
    Result,
    config::{Config, ServersDirectory, TerminalInterfaceConfig},
};

#[derive(Debug, Display)]
pub enum ParseConfigErrorKind {
    #[display("Value not found")]
    ValueNotFound,

    #[display("Invalid type")]
    InvalidType,
}

impl ParseConfigErrorKind {
    fn error(self, context: &'static str) -> ParseConfigError {
        ParseConfigError::new(self, context)
    }
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

pub fn parse_config(document: &KdlDocument) -> Result<Config> {
    let default_java_args = document
        .get("default_java_args")
        .map(|node| {
            node.iter_children()
                .map(|child| format!("\"{}\"", child.name().value()))
                .collect()
        })
        .unwrap_or_default();

    let servers_directory = ServersDirectory::try_from(parse_value(
        document
            .get("servers_directory")
            .ok_or(ParseConfigError::new(
                ParseConfigErrorKind::ValueNotFound,
                "servers_directory",
            ))?,
        "servers_directory",
        kdl_value_to_string,
    )?)?;

    let auto_restart = document
        .get("auto_restart")
        .map(|node| {
            parse_value(node, "auto_restart", |val| {
                val.as_integer().map(|x| x as usize)
            })
        })
        .transpose()?;

    let aliases = document
        .get("aliases")
        .map(|node| node.iter_children().map(parse_alias).collect())
        .transpose()?
        .unwrap_or_default();

    let terminal_interface_config = TerminalInterfaceConfig {
        background_color: None,
    };

    Ok(Config {
        default_java_args,
        servers_directory,
        aliases,
        auto_restart,
        terminal_interface_config,
    })
}
