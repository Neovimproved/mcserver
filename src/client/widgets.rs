use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span, ToSpan},
    widgets::{Block, List, Paragraph, Widget},
};

use crate::{
    client::state::{AddingField, AdditionState, DeletionState},
    config::TerminalInterfaceConfig,
    consts::{DEFAULT_NAME_INPUT, DEFAULT_SOFTWARE_INPUT, DEFAULT_VERSION_INPUT},
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

trait LazyDefault<'a> {
    fn blinking(&'a self) -> Line<'a>;

    fn non_blinking(&'a self) -> Line<'a>;
}

impl<'a> LazyDefault<'a> for &'a str {
    fn blinking(&'a self) -> Line<'a> {
        let default = self;

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

    fn non_blinking(&'a self) -> Line<'a> {
        Line::from(self.gray())
    }
}

impl<'a, T: Into<Cow<'a, str>>, F: Fn() -> T> LazyDefault<'a> for F {
    fn blinking(&self) -> Line<'a> {
        let default = self().into();

        let (next_char_idx, first_char) = default
            .char_indices()
            .next()
            .map(|(idx, ch)| (idx + 1, ch))
            .unwrap_or((0, ' '));

        Line::from_iter([
            first_char.on_white().black(),
            default[next_char_idx..].to_string().gray(),
        ])
    }

    fn non_blinking(&'a self) -> Line<'a> {
        Line::from(self().into().on_white().black())
    }
}

struct Input<'a, D: LazyDefault<'a>> {
    title: Cow<'a, str>,
    default: D,
    input: &'a str,
    blinking: bool,
    background_color: Option<Color>,
    error: Option<&'a str>,
}

impl<'a, D: LazyDefault<'a>> Input<'a, D> {
    pub fn new(title: impl Into<Cow<'a, str>>, input: &'a str, default: D) -> Self {
        Self {
            title: title.into(),
            default,
            input,
            blinking: false,
            background_color: None,
            error: None,
        }
    }

    pub fn error(mut self, value: Option<&'a str>) -> Self {
        self.error = value;
        self
    }

    pub fn blinking(mut self, blinking: bool) -> Self {
        self.blinking = blinking;
        self
    }

    pub fn bg(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    fn slice_to_fit(s: impl Into<Cow<'a, str>>, width: u16) -> Cow<'a, str> {
        let s = s.into();
        let start = (s.len() + 1).saturating_sub(width as usize);

        if start == 0 {
            s
        } else {
            match s {
                Cow::Borrowed(s) => Cow::Borrowed(&s[start..]),
                Cow::Owned(s) => Cow::Owned(s[start..].to_string()),
            }
        }
    }

    fn non_blinking_line(&'a self, width: u16) -> Line<'a> {
        if self.input.is_empty() {
            self.default.non_blinking()
        } else {
            Line::from(Self::slice_to_fit(self.input, width))
        }
    }

    fn blinking_line(&'a self, width: u16) -> Line<'a> {
        if self.input.is_empty() {
            self.default.blinking()
        } else {
            Line::from_iter([
                Self::slice_to_fit(self.input, width).white(),
                ' '.on_white(),
            ])
        }
    }
}

impl<'a, D: LazyDefault<'a>> Widget for &'a Input<'a, D> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let text_width = area.width.saturating_sub(2);

        let line = if self.blinking {
            self.blinking_line(text_width)
        } else {
            self.non_blinking_line(text_width)
        };

        let mut block = Block::bordered()
            .border_style(Color::Reset)
            .title(self.title.as_ref());

        if let Some(error) = self.error {
            block = block.title(Line::from(error).red().right_aligned())
        }

        let mut para = Paragraph::new(line).block(block).style(Color::Reset);

        if let Some(color) = self.background_color {
            para = para.bg(color);
        }

        para.render(area, buf);
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
        let Self {
            addition_state,
            blinking,
        } = self;

        let name_input = Input::new("Name", &addition_state.name, DEFAULT_NAME_INPUT)
            .blinking(blinking && matches!(addition_state.field, AddingField::Name));

        let version_input = Input::new("Version", &addition_state.version, DEFAULT_VERSION_INPUT)
            .blinking(blinking && matches!(addition_state.field, AddingField::Version));

        let software_input = Input::new(
            "Server",
            &addition_state.server_software,
            DEFAULT_SOFTWARE_INPUT,
        )
        .blinking(blinking && matches!(addition_state.field, AddingField::Software));

        let x = area.x;
        let width = area.width;
        let height = area.height / 3;

        name_input.render(
            Rect {
                x,
                y: area.y,
                width,
                height,
            },
            buf,
        );

        let version_y = area.y + height;

        version_input.render(
            Rect {
                x,
                y: version_y,
                width,
                height,
            },
            buf,
        );

        software_input.render(
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

        let input = Input::new(
            format!("Deleting {}", deleting_state.entry_type),
            &deleting_state.confirmation,
            || format!("Type {} here to confirm", deleting_state.name),
        )
        .error(deleting_state.error_message.get_str())
        .blinking(blinking)
        .bg(terminal_interface_config
            .background_color
            .unwrap_or(Color::Reset));

        input.render(area, buf);
    }
}
