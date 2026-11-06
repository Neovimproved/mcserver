use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget, Wrap},
};

use crate::{
    client::state::{AddingField, TempMessage},
    log,
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

fn slice_to_fit(s: &str, width: u16) -> &str {
    &s[(s.len() + 1).saturating_sub(width as usize)..]
}

fn blinking_input_or<'a>(input: &'a str, default: &'a str, width: u16) -> Line<'a> {
    if input.is_empty() {
        blinking_default(slice_to_fit(default, width))
    } else {
        blinking_input(slice_to_fit(input, width))
    }
}

fn blinking_text_or_else<'a>(
    input: &'a str,
    default: impl FnOnce() -> String,
    width: u16,
) -> Line<'a> {
    log!("Blinking text or else");
    if input.is_empty() {
        let string = default();
        let visible_string = slice_to_fit(&string, width);

        let (next_char_idx, first_char) = visible_string
            .char_indices()
            .next()
            .map(|(idx, ch)| (idx + 1, ch))
            .unwrap_or((0, ' '));

        Line::from_iter([
            first_char.on_white().black(),
            visible_string[next_char_idx..].to_string().gray(),
        ])
    } else {
        blinking_input(slice_to_fit(input, width))
    }
}

fn non_blinking_text_or<'a>(input: &'a str, default: &'a str, width: u16) -> Line<'a> {
    if input.is_empty() {
        slice_to_fit(default, width).gray().into()
    } else {
        Line::from(slice_to_fit(input, width))
    }
}

fn non_blinking_text_or_else<'a>(
    input: &'a str,
    default: impl FnOnce() -> String,
    width: u16,
) -> Line<'a> {
    if input.is_empty() {
        let string = default();
        let visible_string = slice_to_fit(&string, width);
        visible_string.to_string().gray().into()
    } else {
        Line::from(slice_to_fit(input, width))
    }
}

impl<'a> Addition<'a> {
    pub fn new(
        name: &'a str,
        version: &'a str,
        software: &'a str,
        adding_field: AddingField,
        blinking: bool,
        text_width: u16,
    ) -> Self {
        const DEFAULT_NAME: &str = "Unnamed";
        const DEFAULT_VERSION: &str = "Latest";
        const DEFAULT_SOFTWARE: &str = "Vanilla";

        let [addition_text, version_text, server_software_text] = if blinking {
            let name_text = if let AddingField::Name = adding_field {
                blinking_input_or(name, DEFAULT_NAME, text_width)
            } else {
                non_blinking_text_or(name, DEFAULT_NAME, text_width)
            };

            let version_text = if let AddingField::Version = adding_field {
                blinking_input_or(version, DEFAULT_VERSION, text_width)
            } else {
                non_blinking_text_or(version, DEFAULT_VERSION, text_width)
            };

            let software_text = if let AddingField::Software = adding_field {
                blinking_input_or(software, DEFAULT_SOFTWARE, text_width)
            } else {
                non_blinking_text_or(software, DEFAULT_SOFTWARE, text_width)
            };

            [name_text, version_text, software_text]
        } else {
            [
                non_blinking_text_or(name, DEFAULT_NAME, text_width),
                non_blinking_text_or(version, DEFAULT_VERSION, text_width),
                non_blinking_text_or(software, DEFAULT_SOFTWARE, text_width),
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
        width: u16,
    ) -> Self {
        let default = || format!("Type {name} here to confirm");

        let deletion_line = if blinking {
            blinking_text_or_else(confirmation, default, width)
        } else {
            non_blinking_text_or_else(confirmation, default, width)
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
