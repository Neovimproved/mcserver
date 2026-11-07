use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span, ToSpan},
    widgets::{Block, List, Paragraph, Widget, Wrap},
};

use crate::{
    client::state::{AddingField, AdditionState, DeletionState},
    config::TerminalInterfaceConfig,
    log,
    servers::ServerEntry,
};

pub struct ServersList<'a> {
    entries: &'a [ServerEntry],
    attached: Option<usize>,
    selected: usize,
    scroll: usize,
    terminal_interface_config: &'a TerminalInterfaceConfig,
}

impl<'a> ServersList<'a> {
    pub fn new(
        entries: &'a [ServerEntry],
        terminal_interface_config: &'a TerminalInterfaceConfig,
    ) -> Self {
        Self {
            entries,
            attached: None,
            selected: 0,
            scroll: 0,
            terminal_interface_config,
        }
    }

    pub fn attached(mut self, attached: usize) -> Self {
        self.attached = Some(attached);
        self
    }

    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    pub fn scroll(mut self, scroll: usize) -> Self {
        self.scroll = scroll;
        self
    }

    fn accumulate(
        entries: &'a [ServerEntry],
        accumulator: &mut Vec<Line<'a>>,
        base_indentation: &str,
        counter: &mut usize,
        attached: Option<usize>,
        selected: usize,
        ti_cfg: &'a TerminalInterfaceConfig,
    ) {
        for (idx, entry) in entries.iter().enumerate() {
            const CONTINUATION: &str = "├╴";
            const END: &str = "└╴";

            let is_selected = selected == *counter;

            let spans = match (is_selected, idx == entries.len() - 1) {
                (true, last) => {
                    let trimmed_indentation = if let Some(ch) = base_indentation.chars().next() {
                        format!(
                            "{}{}",
                            &base_indentation[ch.len_utf8()..],
                            if last { END } else { CONTINUATION }
                        )
                    } else {
                        if last { END } else { CONTINUATION }
                            .chars()
                            .skip(1)
                            .collect::<String>()
                    };

                    vec!["❯".to_span(), Span::from(trimmed_indentation)]
                }
                (false, true) => {
                    vec![Span::from(format!("{base_indentation}{END}"))]
                }
                (false, false) => {
                    vec![Span::from(format!("{base_indentation}{CONTINUATION}"))]
                }
            };

            match entry {
                ServerEntry::Group(server_group) => {
                    let line = server_group.fold_to_line(spans, is_selected, ti_cfg);

                    accumulator.push(line);
                    *counter += 1;

                    if server_group.expanded {
                        Self::accumulate(
                            &server_group.children,
                            accumulator,
                            &format!("{base_indentation}│   "),
                            counter,
                            attached,
                            selected,
                            ti_cfg,
                        );
                    }
                }

                ServerEntry::Object(server_object) => {
                    let line = server_object.fold_to_line(
                        spans,
                        attached == Some(*counter),
                        is_selected,
                        ti_cfg,
                    );

                    accumulator.push(line);

                    *counter += 1;
                }
            }
        }
    }
}

impl Widget for ServersList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let mut lines = vec![];

        let mut counter = 0;

        Self::accumulate(
            self.entries,
            &mut lines,
            "",
            &mut counter,
            self.attached,
            self.selected,
            self.terminal_interface_config,
        );

        let list = List::new(lines.into_iter().skip(self.scroll))
            .block(Block::bordered().title("Servers"));

        list.render(area, buf);
    }
}

fn slice_to_fit(s: &str, width: u16) -> &str {
    &s[(s.len() + 1).saturating_sub(width as usize)..]
}

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
pub struct Addition<'a> {
    addition_state: &'a AdditionState,
    blinking: bool,
}

impl<'a> Addition<'a> {
    pub fn new(addition_state: &'a AdditionState) -> Self {
        Self {
            addition_state,
            blinking: false,
        }
    }

    pub fn blinking(mut self, blinking: bool) -> Self {
        self.blinking = blinking;
        self
    }
}

impl<'a> Widget for Addition<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        const DEFAULT_NAME: &str = "Unnamed";
        const DEFAULT_VERSION: &str = "Latest";
        const DEFAULT_SOFTWARE: &str = "Vanilla";

        let Self {
            addition_state,
            blinking,
        } = self;

        let text_width = area.width.saturating_sub(2);

        let [addition_text, version_text, server_software_text] = if blinking {
            let name_text = if let AddingField::Name = self.addition_state.field {
                blinking_input_or(&self.addition_state.name, DEFAULT_NAME, text_width)
            } else {
                non_blinking_text_or(&self.addition_state.name, DEFAULT_NAME, text_width)
            };

            let version_text = if let AddingField::Version = addition_state.field {
                blinking_input_or(&addition_state.version, DEFAULT_VERSION, text_width)
            } else {
                non_blinking_text_or(&addition_state.version, DEFAULT_VERSION, text_width)
            };

            let software_text = if let AddingField::Software = addition_state.field {
                blinking_input_or(
                    &addition_state.server_software,
                    DEFAULT_SOFTWARE,
                    text_width,
                )
            } else {
                non_blinking_text_or(
                    &addition_state.server_software,
                    DEFAULT_SOFTWARE,
                    text_width,
                )
            };

            [name_text, version_text, software_text]
        } else {
            [
                non_blinking_text_or(&addition_state.name, DEFAULT_NAME, text_width),
                non_blinking_text_or(&addition_state.version, DEFAULT_VERSION, text_width),
                non_blinking_text_or(
                    &addition_state.server_software,
                    DEFAULT_SOFTWARE,
                    text_width,
                ),
            ]
        }
        .map(Paragraph::new);

        let name_paragraph =
            addition_text.block(Block::bordered().border_style(Color::Reset).title("Name"));
        let version_paragraph = version_text.block(
            Block::bordered()
                .border_style(Color::Reset)
                .title("Version"),
        );
        let software_paragraph = server_software_text
            .block(Block::bordered().border_style(Color::Reset).title("Server"));

        let x = area.x;
        let width = area.width;
        let height = area.height / 3;

        name_paragraph.render(
            Rect {
                x,
                y: area.y,
                width,
                height,
            },
            buf,
        );

        let version_y = area.y + height;

        version_paragraph.render(
            Rect {
                x,
                y: version_y,
                width,
                height,
            },
            buf,
        );

        software_paragraph.render(
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

pub struct Deletion<'a> {
    deletion_state: &'a DeletionState,
    blinking: bool,
    terminal_interface_config: &'a TerminalInterfaceConfig,
}

impl<'a> Deletion<'a> {
    pub fn new(
        deletion_state: &'a DeletionState,
        blinking: bool,
        terminal_interface_config: &'a TerminalInterfaceConfig,
    ) -> Self {
        Self {
            deletion_state,
            blinking,
            terminal_interface_config,
        }
    }
}

impl Widget for Deletion<'_> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let Self {
            deletion_state: deleting_state,
            blinking,
            terminal_interface_config,
        } = self;

        let default = || format!("Type {} here to confirm", deleting_state.name);

        let text_width = area.width.saturating_sub(2);

        let deletion_line = if blinking {
            blinking_text_or_else(&deleting_state.confirmation, default, text_width)
        } else {
            non_blinking_text_or_else(&deleting_state.confirmation, default, text_width)
        };

        let mut block = Block::bordered().title(format!("Deleting {}", deleting_state.entry_type));

        if let Some(message) = deleting_state.error_message.get() {
            block = block.title(Line::from(message.as_str()).red().right_aligned());
        }

        let paragraph = Paragraph::new(deletion_line)
            .block(block)
            .wrap(Wrap { trim: false })
            .bg(terminal_interface_config
                .background_color
                .unwrap_or(Color::Reset));

        paragraph.render(area, buf);
    }
}
