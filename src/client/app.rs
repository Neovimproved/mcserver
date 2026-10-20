use std::{
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
    layout::{Constraint, Layout},
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
    servers::{self, ServerEntry},
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

#[allow(dead_code)]
pub struct App {
    config: Config,
    mode: Mode,
    document: Option<KdlDocument>,
    servers: Vec<ServerEntry>,
    server_scroll: usize,
    selected_server: usize,
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
            servers,
            selected_server: 0usize,
            server_scroll: 0usize,
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

        let mut iterators = vec![self.servers.iter_mut()];
        let mut qualifier: Vec<&str> = vec![];

        while let Some(back_iter) = iterators.last_mut() {
            let mut exhausted = true;

            for entry in back_iter {
                if self.selected_server == counter {
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
            log!("{counter}");
            return Ok(());
        };

        write_message!(writer, ClientMessage::Attach { name: name.clone() }).await?;

        self.mode = Mode::output(self.selected_server, name);

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

                KeyCode::Char('g') => self.selected_server = 0,
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
                        log!("{}", readable_pretty(&self.servers));
                        log!("Position: {}", self.selected_server);
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
            ServerMessage::KillSuccess => log!("Successfully killed the child process"),
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
                    let mut log_message = Vec::from(b"Message: ");

                    for b in bytes.iter() {
                        match *b {
                            b'\n' => log_message.extend_from_slice(b"\\n"),
                            b'\r' => log_message.extend_from_slice(b"\\r"),
                            _ => log_message.push(*b),
                        }
                    }

                    log_message.push(10);

                    log_bytes!(log_message);

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

            ServerMessage::SynchronizationPayload(_payload) => todo!(),
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
                    .draw(|frame| self.render(frame))
                    .into_diagnostic()
                    .wrap_err("Failed to render tui")?;
            }

            redraw = tokio::select! {
                event = &mut event_reader.next() => {
                    let Some(event) = event else {
                        bail!("Failed to read from terminal");
                    };

                    let Event::Key(key) = event.into_diagnostic().wrap_err("Failed to read key input")? else {
                        // only care about those keys!!
                        continue;
                    };

                    self.handle_keycode(key, &mut writer).await?;
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

    fn display_entries<'a>(
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
                    } else {
                        spans.extend_from_slice(&[
                            self.ti_cfg()
                                .group_folded_symbol
                                .as_str()
                                .fg(self.ti_cfg().group_symbol_color),
                            Span::from(server_group.name.clone()),
                        ]);
                    };

                    let line = if *counter == self.selected_server {
                        Line::from(spans).bg(self.ti_cfg().selected_color)
                    } else {
                        Line::from(spans)
                    };

                    accumulator.push(line);

                    *counter += 1;

                    if server_group.expanded {
                        self.display_entries(
                            &server_group.children,
                            accumulator,
                            &format!("{base_indentation}│   "),
                            counter,
                            attached,
                        );
                    }
                }
                ServerEntry::Object(server_object) => {
                    spans.extend(if *counter == attached {
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

                    let line = if *counter == self.selected_server {
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

    fn draw_servers_list_widget(&self, _height_available: u16) -> List<'_> {
        let mut counter = 0;

        let mut lines = vec![];

        self.display_entries(
            &self.servers,
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

        let _length = lines.len();

        // TODO: use this
        List::new(lines).block(Block::new().title("Servers").borders(Borders::ALL))
    }

    fn render(&self, frame: &mut Frame) {
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
            frame.render_widget(self.draw_servers_list_widget(area.height), area);
            return;
        };

        // TODO: use a fucking terminal thing

        let output_area = if matches!(state, ServerState::Escaping) {
            let main_sections = Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                .split(frame.area());

            log!("Hello");

            frame.render_widget(
                self.draw_servers_list_widget(main_sections[0].height),
                main_sections[0],
            );

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
    }

    fn selection_mode(&mut self) {
        self.mode = Mode::Selecting;
    }

    fn selection_up(&mut self) {
        let old = self.selected_server;
        self.selected_server = self.selected_server.saturating_sub(1);

        if old != self.selected_server {
            // log!("New: {}", self.selected_server);
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
        let old = self.selected_server;
        let length = Self::count_len(&self.servers);
        self.selected_server = usize::min(self.selected_server + 1, length.saturating_sub(1));

        if old != self.selected_server {
            log!("New: {}", self.selected_server);
        }
    }

    fn selection_bottom(&mut self) {
        self.selected_server = Self::count_len(&self.servers).saturating_sub(1);
    }

    pub fn exit(&mut self) {
        self.exiting = true;
    }
}
