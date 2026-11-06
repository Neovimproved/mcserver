use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget, Wrap},
};

use crate::{
    client::state::{AddingField, TempMessage},
    servers::ServerEntryType,
};

fn blinking_default(default: &str) -> Line<'_> {
    let (next_char_idx, first_char) = default
        .char_indices()
        .next()
        .map(|(idx, ch)| (idx + 1, ch))
        .unwrap_or((0, ' '));

    Line::from_iter([
        first_char.on_white().black(),
        default[next_char_idx..].gray(),
    ])
}

fn blinking_input(input: &str) -> Line<'_> {
    Line::from_iter([Span::from(input), ' '.on_white()])
}

pub struct Addition<'a> {
    name_paragraph: Paragraph<'a>,
    version_paragraph: Paragraph<'a>,
    software_paragraph: Paragraph<'a>,
}

fn blinking_text<'a>(input: &'a str, default: &'a str) -> Line<'a> {
    if input.is_empty() {
        blinking_default(default)
    } else {
        blinking_input(input)
    }
}

fn non_blinking_text<'a>(input: &'a str, default: &'a str) -> Line<'a> {
    if input.is_empty() {
        Line::from(input)
    } else {
        default.gray().into()
    }
}

impl<'a> Addition<'a> {
    pub fn new(
        name: &'a str,
        version: &'a str,
        software: &'a str,
        adding_field: AddingField,
        blinking: bool,
    ) -> Self {
        const DEFAULT_NAME: &str = "Enter name:";
        const DEFAULT_VERSION: &str = "Latest";
        const DEFAULT_SOFTWARE: &str = "Vanilla";

        let [addition_text, version_text, server_software_text] = if blinking {
            let name_text = if let AddingField::Name = adding_field {
                blinking_text(name, DEFAULT_NAME)
            } else {
                non_blinking_text(name, DEFAULT_NAME)
            };

            let version_text = if let AddingField::Version = adding_field {
                blinking_text(version, DEFAULT_VERSION)
            } else {
                non_blinking_text(version, DEFAULT_VERSION)
            };

            let software_text = if let AddingField::Software = adding_field {
                blinking_text(software, DEFAULT_SOFTWARE)
            } else {
                non_blinking_text(software, DEFAULT_SOFTWARE)
            };

            [name_text, version_text, software_text]
        } else {
            [
                non_blinking_text(name, DEFAULT_NAME),
                non_blinking_text(version, DEFAULT_VERSION),
                non_blinking_text(software, DEFAULT_SOFTWARE),
            ]
        }
        .map(Paragraph::new);

        Self {
            name_paragraph: addition_text
                .block(Block::bordered().border_style(Color::Reset).title("Name")),
            version_paragraph: version_text.block(
                Block::bordered()
                    .border_style(Color::Reset)
                    .title("Version"),
            ),
            software_paragraph: server_software_text
                .block(Block::bordered().border_style(Color::Reset).title("Server")),
        }
    }
}

impl<'a> Widget for Addition<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let x = area.x;
        let width = area.width;
        let height = area.height / 3;

        self.name_paragraph.render(
            Rect {
                x,
                y: area.y,
                width,
                height,
            },
            buf,
        );

        let version_y = area.y + height;

        self.version_paragraph.render(
            Rect {
                x,
                y: version_y,
                width,
                height,
            },
            buf,
        );

        self.software_paragraph.render(
            Rect {
                x,
                y: version_y + height,
                width,
                height,
            },
            buf,
        );
    }
}

pub struct Deletion<'a>(Paragraph<'a>);

impl<'a> Deletion<'a> {
    pub fn new(
        name: &'a str,
        confirmation: &'a str,
        error_message: &'a TempMessage,
        entry_type: ServerEntryType,
        blinking: bool,
        background_color: Color,
    ) -> Self {
        let deletion_line = if confirmation.is_empty() {
            let string = format!("Type {name} here to confirm");

            if blinking {
                let mut chars = string.chars();
                let first_char = chars.next().unwrap_or(' ');
                Line::from_iter([
                    first_char.on_white().black(),
                    chars.collect::<String>().gray(),
                ])
            } else {
                Line::from(string)
            }
        } else if blinking {
            Line::from_iter([Span::from(confirmation), ' '.on_white()])
        } else {
            Line::from(confirmation)
        };

        let mut block = Block::bordered().title(format!("Deleting {entry_type}"));

        if let Some(message) = error_message.get() {
            block = block.title(Line::from(message.as_str()).red().right_aligned());
        }

        Self(
            Paragraph::new(deletion_line)
                .block(block)
                .wrap(Wrap { trim: false })
                .bg(background_color),
        )
    }
}

impl Widget for Deletion<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        self.0.render(area, buf);
    }
}
