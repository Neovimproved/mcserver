use std::{
    collections::HashSet,
    fmt::Debug,
    future::{Pending, pending},
    mem,
    pin::Pin,
};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{StreamExt, future::Either};
use kdl::KdlDocument;
use miette::{Context, IntoDiagnostic, bail};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, ToSpan},
    widgets::{Block, Borders, List, Paragraph, Wrap},
};
use tokio::{
    fs,
    io::{self, AsyncReadExt, AsyncWriteExt},
    pin,
    time::{Sleep, sleep},
};

use crate::{
    client::state::{
        AbsoluteArea, AddingField, ConnectionState, Lines, Mode, SelectionState, ServerList,
        TempMessage,
    },
    config::{self, Config, TerminalInterfaceConfig},
    consts::CURSOR_CHAR,
    line_buffer::LineBuffer,
    messages::{ClientMessage, ServerMessage, display_bytes},
    read_message,
    servers::{self, ServerEntry, ServerEntryType, ServerObject},
    write_message,
};

pub struct App {
    config: Config,
    mode: Mode,
    #[allow(dead_code)]
    document: Option<KdlDocument>,
    server_list: ServerList,
    last_area: AbsoluteArea,
    blinking: bool,
    exiting: bool,
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        use std::io::IsTerminal;

        #[cfg(debug_assertions)]
        if !std::io::stderr().is_terminal() {
            eprintln!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! log_bytes {
    ($bytes:expr) => {{
        #[cfg(debug_assertions)]
        if !std::io::stderr().is_terminal() {
            io::stderr()
                .write_all(&$bytes)
                .await
                .into_diagnostic()
                .wrap_err("Failed to write to stderr")?;
        }
    }};
}

fn log_less_whitespace(object: impl Debug, f: impl Fn(usize) -> usize) {
    let s = format!("{object:#?}");

    let split = s.split('\n');

    let mut trimmed = vec![];

    split.for_each(|line| {
        let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
        trimmed.push(&line[f(leading_spaces)..]);
    });

    let last = trimmed.len() - 1;

    let transformed =
        trimmed
            .into_iter()
            .enumerate()
            .fold(String::new(), |mut acc, (idx, item)| {
                acc.push_str(item);

                if idx != last {
                    acc.push('\n');
                }

                acc
            });

    log!("{transformed}");
}

impl App {
    pub async fn try_new() -> miette::Result<Self> {
        let (config, document) = config::read()
            .await
            .into_diagnostic()
            .wrap_err("Failed to create/read config")?;

        log!("Pretty config: {config:#?}");

        let servers = servers::read(config.servers_directory.as_ref()).await?;

        Ok(Self {
            config,
            document,
            server_list: ServerList {
                servers,
                selected: 0usize,
                scroll: 0usize,
            },
            // standard terminal area or something like that
            last_area: AbsoluteArea {
                width: 80,
                height: 24,
            },
            mode: Mode::Selecting(SelectionState::Normal),
            blinking: true,
            exiting: false,
        })
    }

    fn ti_cfg(&self) -> &TerminalInterfaceConfig {
        &self.config.terminal_interface
    }

    fn get_selected_mut(
        entries: &mut [ServerEntry],
        selected: usize,
    ) -> Option<(&mut ServerEntry, String)> {
        let mut counter = 0;

        let mut iterators = vec![entries.iter_mut()];
        let mut qualifier: Vec<&str> = vec![];

        while let Some(back_iter) = iterators.last_mut() {
            let mut exhausted = true;

            for entry in back_iter {
                if selected == counter {
                    let mut path = qualifier.iter().fold(String::new(), |mut acc, item| {
                        acc.push_str(item);
                        acc.push('/');
                        acc
                    });

                    path.push_str(entry.name());

                    return Some((entry, path));
                }

                counter += 1;

                if let ServerEntry::Group(server_group) = entry
                    && server_group.expanded
                {
                    qualifier.push(&server_group.name);
                    iterators.push(server_group.children.iter_mut());
                    exhausted = false;
                    break;
                }
            }

            if exhausted {
                iterators.pop();
                qualifier.pop();
            }
        }

        None
    }

    fn get_selected(entries: &[ServerEntry], selected: usize) -> Option<&ServerEntry> {
        let mut counter = 0;

        let mut iterators = vec![entries.iter()];
        let mut qualifier: Vec<&str> = vec![];

        while let Some(back_iter) = iterators.last_mut() {
            let mut exhausted = true;

            for entry in back_iter {
                if selected == counter {
                    return Some(entry);
                }

                counter += 1;

                if let ServerEntry::Group(server_group) = entry
                    && server_group.expanded
                {
                    qualifier.push(&server_group.name);
                    iterators.push(server_group.children.iter());
                    exhausted = false;
                    break;
                }
            }

            if exhausted {
                iterators.pop();
                qualifier.pop();
            }
        }

        None
    }

    async fn try_select(&mut self, mut writer: impl AsyncWriteExt + Unpin) -> miette::Result<()> {
        let Some((entry, path)) =
            Self::get_selected_mut(&mut self.server_list.servers, self.server_list.selected)
        else {
            bail!("Failed to find entry {}", self.server_list.selected);
        };

        match entry {
            ServerEntry::Group(server_group) => server_group.expanded = !server_group.expanded,
            ServerEntry::Object(server_object) => {
                write_message!(writer, ClientMessage::Attach { server: path })
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to send message")?;

                self.mode = Mode::output(self.server_list.selected, server_object.name.clone());
            }
        }

        Ok(())
    }

    pub async fn handle_keycode(
        &mut self,
        key: KeyEvent,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> miette::Result<()> {
        match &mut self.mode {
            Mode::Selecting(selection_state) => match selection_state {
                SelectionState::Normal => match key.code {
                    KeyCode::Enter => self
                        .try_select(&mut writer)
                        .await
                        .wrap_err("Failed to select item")?,
                    KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                    KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

                    KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.scroll_down();
                    }

                    KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.scroll_up();
                    }

                    KeyCode::Char('g') => self.server_list.selected = 0,
                    KeyCode::Char('G') => self.selection_bottom(),

                    KeyCode::Esc => self.exit(),
                    KeyCode::Char(ch) => match ch {
                        'a' => *selection_state = SelectionState::adding(),

                        'D' => {
                            if let Some(entry) = Self::get_selected(
                                &self.server_list.servers,
                                self.server_list.selected,
                            ) {
                                *selection_state = SelectionState::deleting(entry);
                            };
                        }

                        // for debugging things
                        #[cfg(debug_assertions)]
                        'd' => {
                            log_less_whitespace(&self.server_list.servers, |n| n / 2);
                            log!("Position: {}", self.server_list.selected);
                        }

                        'q' => self.exit(),

                        's' => write_message!(writer, ClientMessage::SynchronizationRequest)
                            .await
                            .into_diagnostic()
                            .wrap_err("Failed to make synchronization request")?,

                        _ => {}
                    },
                    _ => {}
                },
                SelectionState::Adding {
                    name,
                    version,
                    server_software,
                    field,
                } => match key.code {
                    KeyCode::Esc => selection_state.reset(),
                    KeyCode::Char(ch) => match field {
                        AddingField::Name => name,
                        AddingField::Version => version,
                        AddingField::Software => server_software,
                    }
                    .push(ch),
                    // TODO: actually create the server here
                    KeyCode::Enter => {
                        if field.next() {
                            todo!();
                            #[allow(unreachable_code)]
                            selection_state.reset();
                        }
                    }
                    KeyCode::Tab => {
                        field.next();
                    }
                    KeyCode::BackTab => {
                        field.prev();
                    }
                    KeyCode::Backspace => {
                        match field {
                            AddingField::Name => name,
                            AddingField::Version => version,
                            AddingField::Software => server_software,
                        }
                        .pop();
                    }
                    _ => {}
                },
                SelectionState::Deleting {
                    name,
                    confirmation,
                    error_message,
                    entry_type,
                } => match key.code {
                    KeyCode::Backspace => {
                        confirmation.pop();
                    }
                    KeyCode::Enter => {
                        if name == confirmation {
                            fs::remove_dir_all(name)
                                .await
                                .into_diagnostic()
                                .wrap_err("Failed to remove directory {}")?;
                        } else {
                            error_message.insert(format!("Failed to rename {entry_type}"));
                        }
                    }
                    KeyCode::Delete => todo!(
                        "Delete char is not yet implemented in deletion mode yet unfortunately"
                    ),
                    KeyCode::Char(ch) => confirmation.push(ch),
                    KeyCode::Esc => selection_state.reset(),
                    _ => {}
                },
            },

            Mode::Console {
                state: ConnectionState::Dead,
                ..
            } => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                    self.selection_mode()
                }
                _ => {}
            },

            // Escaping output
            Mode::Console {
                path: name, state, ..
            } if let ConnectionState::Escaping = state => match key.code {
                // KeyCode::Backspace => todo!(),
                KeyCode::Enter => self
                    .try_select(writer)
                    .await
                    .wrap_err("Failed to attach to server")?,
                KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

                KeyCode::Tab => state.reset(),

                KeyCode::BackTab => self.selection_mode(),

                // KeyCode::Delete => todo!(),
                // KeyCode::Insert => todo!(),
                // KeyCode::F(_) => todo!(),
                KeyCode::Char('q') => self.exit(),

                KeyCode::Char(':') => {
                    // TODO: implement commands
                }

                KeyCode::Esc => self.selection_mode(),
                _ => {}
            },

            Mode::Console { input, state, .. } => match key.code {
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => {
                    input.push('\n');

                    write_message!(
                        writer,
                        ClientMessage::ProgramInput(mem::take(input).into_bytes())
                    )
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to send input to the server")?
                }
                KeyCode::Tab => write_message!(writer, ClientMessage::ProgramInput(vec![b'\t']))
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to send tab to server")?,
                KeyCode::BackTab => {
                    *state = ConnectionState::Escaping;
                }
                KeyCode::Char(ch) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        write_message!(
                            writer,
                            ClientMessage::ProgramInput(vec![(ch as u8) - b'a' + 1])
                        )
                        .await
                        .into_diagnostic()?;
                    } else {
                        input.push(ch);
                    }
                }
                KeyCode::Esc => {
                    *state = ConnectionState::Escaping;
                }
                _ => {}
            },
        };

        Ok(())
    }

    pub async fn handle_blink_event(
        &mut self,
        blink_event: &mut Pin<&mut Either<Sleep, Pending<()>>>,
    ) {
        blink_event.set(if let Some(interval) = self.ti_cfg().cursor_blink {
            Either::Left(sleep(interval))
        } else {
            Either::Right(pending())
        });

        self.blinking = !self.blinking;
    }

    pub fn synchronize_servers(
        children: &mut [ServerEntry],
        online_servers: &HashSet<String>,
        path: Vec<String>,
    ) {
        let joint = path.iter().fold(String::new(), |mut acc, item| {
            acc.push_str(item);
            acc.push('/');
            acc
        });

        for entry in children {
            match entry {
                ServerEntry::Group(server_group) => {
                    let mut new_path = path.clone();
                    new_path.push(server_group.name.clone());
                    Self::synchronize_servers(&mut server_group.children, online_servers, new_path);
                }
                ServerEntry::Object(server_object) => {
                    let name = format!("{joint}{}", server_object.name);
                    if online_servers.contains(&name) {
                        server_object.online = true;
                        log!("{}!!!", server_object.name);
                    } else {
                        server_object.online = false;
                    }
                }
            }
        }
    }

    pub async fn read_message(
        &mut self,
        mut reader: impl AsyncReadExt + Unpin,
        length: io::Result<u32>,
    ) -> miette::Result<()> {
        let length = match length {
            Ok(length) => length,
            Err(err) => match err.kind() {
                io::ErrorKind::UnexpectedEof => {
                    log!("Server closed");
                    return Err(err)
                        .into_diagnostic()
                        .wrap_err("Server connection severed");
                }
                _ => return Err(err).into_diagnostic(),
            },
        };

        match read_message!(reader, length)
            .await
            .into_diagnostic()
            .wrap_err("Failed to receive message from server")?
        {
            ServerMessage::KillFailure(err) => log!("Failed to kill child: {err}"),
            ServerMessage::Closed => {
                if let Mode::Console { input, state, .. } = &mut self.mode {
                    *input = "Press enter to return...".to_string();
                    *state = ConnectionState::Dead;
                }
            }
            ServerMessage::Message(message) => {
                log!("Received a message from the server: {message}");
            }
            ServerMessage::AlreadyStarted => {
                log!("The server has already started apparently");
            }
            ServerMessage::NotAttached => {
                log!("The server has not yet started apparently");
            }
            ServerMessage::Output(bytes) => {
                if let Mode::Console { output, .. } = &mut self.mode {
                    output.push(display_bytes(&bytes).to_string());
                }
            }
            ServerMessage::History(history) => {
                if let Mode::Console { output, .. } = &mut self.mode {
                    history
                        .into_iter()
                        .for_each(|line| output.push(display_bytes(&line).to_string()));
                }
            }

            ServerMessage::SynchronizationPayload { online_servers } => {
                log!("Received payload: {online_servers:?}!");
                Self::synchronize_servers(&mut self.server_list.servers, &online_servers, vec![]);
            }
        }

        Ok(())
    }

    pub async fn run(
        &mut self,
        terminal: &mut DefaultTerminal,
        (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
    ) -> miette::Result<()> {
        let mut event_reader = EventStream::new();

        let blink_event = if let Some(blink_interval) = self.ti_cfg().cursor_blink {
            Either::Left(sleep(blink_interval))
        } else {
            Either::Right(pending())
        };

        pin!(blink_event);

        let mut redraw = true;

        loop {
            if self.exiting {
                writer
                    .shutdown()
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to send shutdown packet")?;
                break Ok(());
            }

            if redraw {
                terminal
                    .draw(|frame| self.last_area = self.render(frame))
                    .into_diagnostic()
                    .wrap_err("Failed to render tui")?;
            }

            redraw = tokio::select! {
                event = &mut event_reader.next() => {
                    let Some(event) = event else {
                        bail!("Failed to read from terminal");
                    };

                    match event.into_diagnostic().wrap_err("Failed to read key input")? {
                        Event::Key(key) => self.handle_keycode(key, &mut writer).await?,
                        Event::Resize(_columns, rows) => {
                            let last_visible = self.server_list.scroll + rows.saturating_sub(3) as usize;
                            if self.server_list.selected > last_visible {
                                self.server_list.selected = last_visible;
                            } else {
                                continue;
                            }
                        },
                        _ => continue,
                    };
                    true
                }

                _ = &mut blink_event => {
                    self.handle_blink_event(&mut blink_event).await;
                    true
                },

                length = reader.read_u32() => {
                    self.read_message(&mut reader, length).await?;
                    true
                },
            };
        }
    }

    pub fn find_server_and_expand(
        name: &str,
        entries: &mut [ServerEntry],
        base_path: &str,
        mut base_counter: usize,
    ) -> Option<usize> {
        for entry in entries {
            match entry {
                ServerEntry::Group(server_group) => {
                    let group_entries = &mut server_group.children;

                    if let Some(pos) = Self::find_server_and_expand(
                        name,
                        group_entries,
                        &format!("{base_path}{}/", server_group.name),
                        base_counter + 1,
                    ) {
                        server_group.expanded = true;
                        return Some(pos);
                    }
                }
                ServerEntry::Object(server_object) => {
                    if name == format!("{base_path}{}", server_object.name) {
                        return Some(base_counter);
                    }
                }
            }

            base_counter += 1;
        }

        None
    }

    pub async fn run_attached(
        &mut self,
        terminal: &mut DefaultTerminal,
        (reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
        name: Option<String>,
    ) -> miette::Result<()> {
        if let Some(name) = name {
            let pos = Self::find_server_and_expand(&name, &mut self.server_list.servers, "", 0)
                .wrap_err_with(|| format!("{name} could not be found"))?;

            self.server_list.selected = pos;

            self.try_select(&mut writer)
                .await
                .wrap_err_with(|| format!("Failed to attach to {name}"))?;

            self.run(terminal, (reader, writer)).await
        } else {
            // TODO: implement optional server arg..
            todo!("Implement optional server arg")
        }
    }

    fn find_first_online(children: &[ServerEntry]) -> Option<&ServerObject> {
        for child in children {
            match child {
                ServerEntry::Group(server_group) => {
                    if let Some(active) = Self::find_first_online(&server_group.children) {
                        return Some(active);
                    }
                }
                ServerEntry::Object(server_object) => {
                    if server_object.online {
                        return Some(server_object);
                    }
                }
            }
        }

        None
    }

    fn draw_entries<'a>(
        &'a self,
        entries: &'a [ServerEntry],
        accumulator: &mut Vec<Line<'a>>,
        base_indentation: &str,
        counter: &mut usize,
        attached: usize,
    ) {
        for (idx, entry) in entries.iter().enumerate() {
            const CONTINUATION: &str = "├╴";
            const END: &str = "└╴";

            let mut spans = match (
                *counter == self.server_list.selected,
                idx == entries.len() - 1,
            ) {
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
                    spans.push(if server_group.expanded {
                        self.ti_cfg()
                            .group_expanded_symbol
                            .as_str()
                            .fg(self.ti_cfg().group_symbol_color)
                    } else {
                        let group_folded_sym = self.ti_cfg().group_folded_symbol.as_str();

                        if Self::find_first_online(&server_group.children).is_some() {
                            group_folded_sym.fg(self.ti_cfg().server_online_color)
                        } else {
                            group_folded_sym.fg(self.ti_cfg().group_symbol_color)
                        }
                    });

                    spans.push(Span::from(&server_group.name));

                    let line = if *counter == self.server_list.selected {
                        Line::from(spans).bg(self.ti_cfg().selected_color)
                    } else {
                        Line::from(spans)
                    };

                    accumulator.push(line);

                    *counter += 1;

                    if server_group.expanded {
                        self.draw_entries(
                            &server_group.children,
                            accumulator,
                            &format!("{base_indentation}│   "),
                            counter,
                            attached,
                        );
                    }
                }

                ServerEntry::Object(server_object) => {
                    spans.extend(if server_object.online || *counter == attached {
                        [
                            self.ti_cfg()
                                .server_online_symbol
                                .as_str()
                                .fg(self.ti_cfg().server_online_color),
                            server_object.name.to_span(),
                        ]
                    } else {
                        [
                            self.ti_cfg()
                                .server_offline_symbol
                                .as_str()
                                .fg(self.ti_cfg().server_offline_color),
                            server_object.name.to_span(),
                        ]
                    });

                    let line = if *counter == self.server_list.selected {
                        Line::from(spans).bg(self.ti_cfg().selected_color)
                    } else {
                        Line::from(spans)
                    };

                    accumulator.push(line);

                    *counter += 1;
                }
            }
        }
    }

    fn draw_servers_list_widget(&self) -> List<'_> {
        let mut counter = 0;

        let mut lines = vec![];

        self.draw_entries(
            &self.server_list.servers,
            &mut lines,
            "",
            &mut counter,
            match self.mode {
                Mode::Console {
                    attached_server, ..
                } => attached_server,
                _ => usize::MAX,
            },
        );

        // 2 because of the top and bottom border
        // Extra 1 to make it representative of an index

        List::new(lines.into_iter().skip(self.server_list.scroll))
            .block(Block::new().title("Servers").borders(Borders::ALL))
    }

    fn draw_output_widget<'a>(
        &self,
        output_area: AbsoluteArea,
        input: &str,
        output: &LineBuffer,
    ) -> Paragraph<'a> {
        // TODO: use a f terminal thing
        let output_text_width = output_area.width.saturating_sub(2);
        let output_lines_available = output_area.height.saturating_sub(2);

        let mut output_lines = Lines::new(output_text_width);

        // let mut state_machine = Parser::new();
        // let mut logger = Log::new();

        output
            .iter()
            // use skip here to get the maximum number of lines that might need to be used
            // Then, the lines that aren't used will simply not be rendered
            .skip(output.len().saturating_sub(output_lines_available))
            .map(String::as_str)
            .for_each(|line| line.chars().for_each(|ch| output_lines.push(ch)));

        input.chars().for_each(|ch| output_lines.push(ch));

        if self.blinking {
            output_lines.push(CURSOR_CHAR);
        }

        let vscroll = output_lines.len().saturating_sub(output_lines_available);

        Paragraph::new(output_lines.into_buf())
            .block(Block::new().title("Console").borders(Borders::ALL))
            .scroll((vscroll as u16, 0))
    }

    fn center_rect(area: Rect, x_percent: u16, y_percent: u16) -> Rect {
        let horizontal_fill = 50u16.saturating_sub(x_percent / 2);
        let vertical_fill = 50u16.saturating_sub(y_percent / 2);

        let vertical_layout =
            Layout::vertical([vertical_fill, y_percent, vertical_fill].map(Constraint::Percentage))
                .split(area);

        let horizontal_layout = Layout::horizontal(
            [horizontal_fill, x_percent, horizontal_fill].map(Constraint::Percentage),
        )
        .split(vertical_layout[1]);

        horizontal_layout[1]
    }

    fn draw_addition_widget(
        &self,
        name: &str,
        version: &str,
        server_software: &str,
        field: AddingField,
    ) -> List<'_> {
        // let mut lines = vec![];
        let mut addition_text = format!("Name: {name}");
        let mut version_text = format!("Version: {version}");
        let mut server_software_text = format!("Server: {server_software}");

        if self.blinking {
            match field {
                AddingField::Name => &mut addition_text,
                AddingField::Version => &mut version_text,
                AddingField::Software => &mut server_software_text,
            }
            .push(CURSOR_CHAR)
        }

        List::new([addition_text, version_text, server_software_text])
            .block(Block::bordered().title("Adding server"))
    }

    fn draw_deletion_widget<'a>(
        &self,
        name: &str,
        confirmation: &str,
        error_message: &'a TempMessage<String>,
        entry_type: ServerEntryType,
    ) -> Paragraph<'a> {
        let mut deletion_text = format!(
            "Confirm you would like to delete the {entry_type} `{name}` by typing it out: {confirmation}"
        );

        if self.blinking {
            deletion_text.push(CURSOR_CHAR);
        }

        Paragraph::new(deletion_text)
            .block(
                Block::bordered().title(if let Some(message) = error_message.get() {
                    Line::from(vec![
                        Span::from(format!("Deleting {entry_type}   ")),
                        message.to_span().red(),
                    ])
                } else {
                    Line::from(format!("Deleting {entry_type}"))
                }),
            )
            .wrap(Wrap { trim: false })
            .bg(self.ti_cfg().background_color.unwrap_or(Color::Reset))
    }

    fn render(&self, frame: &mut Frame) -> AbsoluteArea {
        if let Some(background_color) = self.ti_cfg().background_color {
            let background_block = Block::new().style(Style::new().bg(background_color));

            frame.render_widget(background_block, frame.area());
        }

        let frame_area = frame.area();

        match &self.mode {
            Mode::Selecting(selection_state) => {
                frame.render_widget(self.draw_servers_list_widget(), frame_area);

                match selection_state {
                    SelectionState::Normal => {}
                    SelectionState::Adding {
                        name,
                        version,
                        server_software,
                        field,
                    } => frame.render_widget(
                        self.draw_addition_widget(name, version, server_software, *field),
                        Self::center_rect(frame.area(), 50, 40),
                    ),
                    SelectionState::Deleting {
                        name,
                        confirmation,
                        error_message,
                        entry_type,
                    } => frame.render_widget(
                        self.draw_deletion_widget(name, confirmation, error_message, *entry_type),
                        Self::center_rect(frame.area(), 50, 40),
                    ),
                }
            }

            Mode::Console {
                input,
                output,
                state,
                ..
            } => {
                let output_area = if matches!(state, ConnectionState::Escaping) {
                    let main_sections =
                        Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                            .split(frame.area());

                    frame.render_widget(self.draw_servers_list_widget(), main_sections[0]);

                    main_sections[1]
                } else {
                    frame.area()
                };

                let widget = self.draw_output_widget(output_area.into(), input, output);

                frame.render_widget(widget, output_area);
            }
        }

        frame_area.into()
    }

    fn selection_mode(&mut self) {
        self.mode = Mode::Selecting(SelectionState::Normal);
    }

    fn selection_up(&mut self) {
        if self.server_list.selected == 0 {
            self.server_list.selected =
                Self::count_len(&self.server_list.servers).saturating_sub(1);
            return;
        }

        self.server_list.selected -= 1;

        if self.server_list.scroll > self.server_list.selected {
            self.server_list.scroll = self.server_list.selected;
        }
    }

    fn count_len(entries: &[ServerEntry]) -> usize {
        let mut len = 0;

        for entry in entries {
            match entry {
                ServerEntry::Group(server_group) => {
                    if server_group.expanded {
                        len += Self::count_len(&server_group.children) + 1;
                    } else {
                        len += 1;
                    }
                }
                ServerEntry::Object(_) => len += 1,
            }
        }

        len
    }

    fn selection_down(&mut self) {
        let length = Self::count_len(&self.server_list.servers);

        if self.server_list.selected >= length.saturating_sub(1) {
            self.server_list.selected = 0;
            return;
        }

        self.server_list.selected += 1;

        let height_index = self.last_area.height.saturating_sub(3);
        let last_visible = self.server_list.scroll + height_index;

        if self.server_list.selected > last_visible {
            self.server_list.scroll = self.server_list.selected - height_index;
        }
    }

    fn selection_bottom(&mut self) {
        self.server_list.selected = Self::count_len(&self.server_list.servers).saturating_sub(1);
    }

    fn scroll_up(&mut self) {
        if self.server_list.scroll == 0 {
            return;
        }

        self.server_list.scroll -= 1;

        let last_visible = self.server_list.scroll + self.last_area.height.saturating_sub(3);

        if self.server_list.selected >= last_visible {
            self.server_list.selected = last_visible;
        }
    }

    fn scroll_down(&mut self) {
        let new_scroll = self.server_list.scroll + 1;

        let list_length = Self::count_len(&self.server_list.servers);

        if new_scroll >= list_length {
            return;
        }

        if new_scroll > self.server_list.selected {
            self.server_list.selected = (list_length - 1).min(self.server_list.selected + 1);
        }

        self.server_list.scroll = new_scroll;
    }

    pub fn exit(&mut self) {
        self.exiting = true;
    }
}
