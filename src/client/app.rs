use std::{
    collections::HashSet,
    fmt::Debug,
    future::{Pending, pending},
    io::IsTerminal,
    iter, mem,
    pin::Pin,
};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{StreamExt, future::Either};
use kdl::KdlDocument;
use miette::{Context, IntoDiagnostic, bail};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, ToLine, ToSpan},
    widgets::{Block, Borders, List},
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    pin,
    time::{Sleep, sleep},
};

use crate::{
    config::{self, Config, TerminalInterfaceConfig},
    line_buffer::LineBuffer,
    messages::{ClientMessage, ServerMessage, display_bytes},
    read_message,
    servers::{self, ServerEntry, ServerObject},
    write_message,
};

#[derive(Debug)]
enum ServerState {
    Normal,
    Escaping,
    Dead,
}

#[derive(Debug)]
enum Mode {
    Selecting,
    Deletion,
    Console {
        attached_server: usize,
        name: String,
        input: String,
        output: LineBuffer,
        state: ServerState,
        blinking: bool,
    },
}

impl Mode {
    fn output(index: usize, name: String) -> Self {
        Self::Console {
            attached_server: index,
            name,
            input: String::new(),
            output: LineBuffer::new(),
            state: ServerState::Normal,
            blinking: true,
        }
    }
}

struct ServerList {
    servers: Vec<ServerEntry>,
    selected: usize,
    scroll: usize,
}

struct Area {
    #[allow(dead_code)]
    width: usize,
    height: usize,
}

impl From<Rect> for Area {
    fn from(Rect { width, height, .. }: Rect) -> Self {
        Self {
            width: width as usize,
            height: height as usize,
        }
    }
}

pub struct App {
    config: Config,
    mode: Mode,
    #[allow(dead_code)]
    document: Option<KdlDocument>,
    server_list: ServerList,
    last_area: Area,
    exiting: bool,
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
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

fn readable_pretty(object: impl Debug) -> String {
    let s = format!("{object:#?}");

    let mut transformed = s
        .split('\n')
        .map(|line| {
            let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
            &line[leading_spaces / 2..]
        })
        .fold(String::new(), |mut acc, item| {
            acc.push_str(item);
            acc.push('\n');
            acc
        });

    transformed.pop();

    transformed
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
            last_area: Area {
                width: 80,
                height: 24,
            },
            mode: Mode::Selecting,
            exiting: false,
        })
    }

    fn ti_cfg(&self) -> &TerminalInterfaceConfig {
        &self.config.terminal_interface
    }

    // returns the fully qualified server name if found
    pub async fn try_select(&mut self, mut writer: impl AsyncWriteExt + Unpin) -> io::Result<()> {
        let mut counter = 0;
        let mut name: Option<String> = None;

        let mut iterators = vec![self.server_list.servers.iter_mut()];
        let mut qualifier: Vec<&str> = vec![];

        while let Some(back_iter) = iterators.last_mut() {
            let mut exhausted = true;

            for entry in back_iter {
                if self.server_list.selected == counter {
                    match entry {
                        ServerEntry::Group(server_group) => {
                            server_group.expanded = !server_group.expanded;
                            return Ok(());
                        }
                        ServerEntry::Object(server_object) => {
                            qualifier.push(&server_object.name);
                            name = Some(qualifier.join("/"));
                            break;
                        }
                    }
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

            if name.is_some() {
                break;
            }

            if exhausted {
                iterators.pop();
                qualifier.pop();
            }
        }

        let Some(name) = name else {
            return Ok(());
        };

        write_message!(writer, ClientMessage::Attach { name: name.clone() }).await?;

        self.mode = Mode::output(self.server_list.selected, name);

        Ok(())
    }

    pub async fn handle_keycode(
        &mut self,
        key: KeyEvent,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> miette::Result<()> {
        match &mut self.mode {
            Mode::Selecting => match key.code {
                KeyCode::Enter => self
                    .try_select(&mut writer)
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to select item")?,
                // KeyCode::Left | KeyCode::Char('h') => self.selection_left(),
                // KeyCode::Right | KeyCode::Char('l') => self.selection_right(),
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

                // TODO: these should actually by different from left and right
                KeyCode::Tab => todo!("Implement tab"),
                KeyCode::BackTab => todo!("Implement back tab"),

                // KeyCode::Esc => self.exit(),
                KeyCode::Char(ch) => match ch {
                    'q' => self.exit(),
                    'D' => {
                        self.mode = Mode::Deletion;
                    }

                    's' => write_message!(writer, ClientMessage::SynchronizationRequest)
                        .await
                        .into_diagnostic()
                        .wrap_err("Failed to make synchronization request")?,

                    'u' => {
                        log!("{}", readable_pretty(&self.server_list.servers));
                        log!("Position: {}", self.server_list.selected);
                    }

                    _ => {}
                },
                _ => {}
            },

            Mode::Deletion => todo!(),

            Mode::Console {
                state: ServerState::Dead,
                ..
            } => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                    self.selection_mode()
                }
                _ => {}
            },

            // Escaping output
            Mode::Console { name, state, .. } if let ServerState::Escaping = state => {
                match key.code {
                    KeyCode::Backspace => todo!(),
                    KeyCode::Enter => self
                        .try_select(writer)
                        .await
                        .into_diagnostic()
                        .wrap_err("Failed to attach to server")?,
                    KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                    KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

                    KeyCode::Tab => {
                        *state = ServerState::Normal;
                    }

                    KeyCode::BackTab => self.selection_mode(),

                    KeyCode::Char('q') => self.exit(),

                    KeyCode::Delete => todo!(),
                    KeyCode::Insert => todo!(),
                    KeyCode::F(_) => todo!(),
                    KeyCode::Char(_) => todo!(),
                    KeyCode::Null => todo!(),
                    KeyCode::Esc => self.selection_mode(),
                    _ => {}
                }
            }

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
                KeyCode::Left => todo!("Implement cursor left"),
                KeyCode::Right => todo!("Implement cursor right"),
                KeyCode::Tab => write_message!(writer, ClientMessage::ProgramInput(vec![b'\t']))
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to send tab to server")?,
                KeyCode::BackTab => {
                    *state = ServerState::Escaping;
                }
                KeyCode::Delete => todo!("Implement deletion"),
                // KeyCode::F(_) => todo!(),
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
                    *state = ServerState::Escaping;
                }
                _ => {}
            },
        };

        Ok(())
    }

    pub async fn handle_blink_event(
        &mut self,
        blink_event: &mut Pin<&mut Either<Sleep, Pending<()>>>,
    ) -> miette::Result<bool> {
        blink_event.set(if let Some(interval) = self.ti_cfg().cursor_blink {
            Either::Left(sleep(interval))
        } else {
            Either::Right(pending())
        });

        Ok(
            if let Mode::Console {
                blinking: blink, ..
            } = &mut self.mode
            {
                *blink = !*blink;
                true
            } else {
                false
            },
        )
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

        // log!("{}", online_servers.len());
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
                    *state = ServerState::Dead;
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
                log!(
                    "Received history: {}",
                    history
                        .iter()
                        .map(|line| String::from_utf8_lossy(line))
                        .collect::<Vec::<_>>()
                        .join("\n")
                );
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

                _ = &mut blink_event => self.handle_blink_event(&mut blink_event).await?,

                length = reader.read_u32() => {
                    self.read_message(&mut reader, length).await?;
                    true
                },
            };
        }
    }

    pub async fn run_attached(
        &mut self,
        terminal: &mut DefaultTerminal,
        (reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
        name: String,
    ) -> miette::Result<()> {
        // TODO: use correct server
        self.try_select(&mut writer)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to attach to {name}"))?;

        self.run(terminal, (reader, writer)).await
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
        for (index, entry) in entries.iter().enumerate() {
            let mut spans = if index == entries.len().saturating_sub(1) {
                vec![Span::from(format!("{base_indentation}└╴"))]
            } else {
                vec![Span::from(format!("{base_indentation}├╴"))]
            };

            match entry {
                ServerEntry::Group(server_group) => {
                    if server_group.expanded {
                        spans.extend_from_slice(&[
                            self.ti_cfg()
                                .group_expanded_symbol
                                .as_str()
                                .fg(self.ti_cfg().group_symbol_color),
                            Span::from(server_group.name.clone()),
                        ]);
                    } else if Self::find_first_online(&server_group.children).is_some() {
                        spans.extend_from_slice(&[
                            self.ti_cfg()
                                .group_folded_symbol
                                .as_str()
                                .fg(self.ti_cfg().server_online_color),
                            // self.ti_cfg()
                            //     .server_online_symbol
                            //     .as_str()
                            //     .fg(self.ti_cfg().server_online_color),
                            Span::from(server_group.name.clone()),
                        ]);
                    } else {
                        spans.extend_from_slice(&[
                            self.ti_cfg()
                                .group_folded_symbol
                                .as_str()
                                .fg(self.ti_cfg().group_symbol_color),
                            Span::from(server_group.name.clone()),
                        ]);
                    };

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
                        // [" ".to_span(), server_object.name.to_span()]
                        [
                            // highly intrusive: 252, 186, 3
                            // less intrusive: 222, 186, 30
                            // quite intrusive stil but: 222, 156, 50
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

        // TODO: improve drawing efficiency
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

        // TODO: use this
        List::new(lines.into_iter().skip(self.server_list.scroll))
            .block(Block::new().title("Servers").borders(Borders::ALL))
    }

    fn render(&self, frame: &mut Frame) -> Area {
        if let Some(background_color) = self.ti_cfg().background_color {
            let background_block = Block::new().style(Style::new().bg(background_color));

            frame.render_widget(background_block, frame.area());
        }

        let Mode::Console {
            input,
            output,
            state,
            blinking,
            ..
        } = &self.mode
        else {
            let area = frame.area();
            frame.render_widget(self.draw_servers_list_widget(), area);
            return area.into();
        };

        // TODO: use a fucking terminal thing

        let output_area = if matches!(state, ServerState::Escaping) {
            let main_sections = Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                .split(frame.area());

            frame.render_widget(self.draw_servers_list_widget(), main_sections[0]);

            main_sections[1]
        } else {
            frame.area()
        };

        let output_text_width = output_area.width.saturating_sub(2) as usize;
        let output_lines_available = output_area.height.saturating_sub(2) as usize;

        let mut output_lines = vec![];
        let mut current_line_utf8_length = 0;

        let mut last_line = output
            .iter()
            // use skip here to get the maximum number of lines that might need to be used
            // Then, the lines that aren't used will simply not be rendered
            .skip(output.len().saturating_sub(output_lines_available))
            .chain(iter::once(input))
            .fold(String::new(), |mut acc, item| {
                for ch in item.chars() {
                    if ch == '\n' {
                        output_lines.push(Line::from(acc));
                        acc = String::new();
                        current_line_utf8_length = 0;
                        continue;
                    }

                    let new_length = current_line_utf8_length + ch.len_utf8();

                    if new_length > output_text_width {
                        output_lines.push(Line::from(acc));
                        acc = ch.to_string();
                        current_line_utf8_length = 0;
                    } else {
                        acc.push(ch);
                        current_line_utf8_length = new_length;
                    }
                }

                acc
            });

        if !*blinking {
            output_lines.push(Line::from(last_line));
        } else if current_line_utf8_length + 1 >= output_text_width {
            output_lines.push(Line::from(last_line));
            output_lines.push('█'.to_line());
        } else {
            last_line.push('█');
            output_lines.push(Line::from(last_line));
        }

        let line_count = output_lines.len();

        let output_widget = List::new(
            output_lines
                .into_iter()
                .skip(line_count.saturating_sub(output_lines_available)),
        )
        .block(Block::new().title("Console").borders(Borders::ALL));

        frame.render_widget(output_widget, output_area);
        frame.area().into()
    }

    fn selection_mode(&mut self) {
        self.mode = Mode::Selecting;
    }

    fn selection_up(&mut self) {
        self.server_list.selected = self.server_list.selected.saturating_sub(1);

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

        self.server_list.selected =
            usize::min(self.server_list.selected + 1, length.saturating_sub(1));

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
