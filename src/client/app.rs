use std::{
    fmt::{Debug, Display},
    future::pending,
    io::IsTerminal,
    mem,
    path::PathBuf,
};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{StreamExt, future::Either};
use kdl::KdlDocument;
use miette::{Context, IntoDiagnostic, bail};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    macros::span,
    style::{Color, Style, Stylize},
    text::{Line, Span, ToSpan},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    pin,
    time::sleep,
};

use crate::{
    config::{self, Config},
    line_buffer::LineBuffer,
    messages::{ClientMessage, ServerMessage, display_bytes},
    read_message,
    servers::{self, ServerEntry, ServerGroup},
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
    Console {
        attached_server: usize,
        name: String,
        input: String,
        output: LineBuffer,
        state: ServerState,
        blink: bool,
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
            blink: false,
        }
    }
}

#[allow(dead_code)]
pub struct App {
    config: Config,
    mode: Mode,
    document: Option<KdlDocument>,
    servers: Vec<ServerEntry>,
    selected_server: usize,
    exiting: bool,
}

#[macro_export]
macro_rules! stderr_log {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        if !std::io::stderr().is_terminal() {
            eprintln!($($arg)*);
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

enum NestEntry<'a> {
    GroupEnd,
    Group(&'a mut ServerGroup),
}

impl App {
    pub async fn try_new() -> miette::Result<Self> {
        let (config, document) = config::read()
            .await
            .into_diagnostic()
            .wrap_err("Failed to create/read config")?;

        stderr_log!("Pretty config: {config:#?}");

        let servers = servers::read(config.servers_directory.as_ref()).await?;

        Ok(Self {
            config,
            document,
            servers,
            selected_server: 0usize,
            mode: Mode::Selecting,
            exiting: false,
        })
    }

    // returns the fully qualified server name if found
    pub async fn try_select(
        &mut self,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> io::Result<Option<String>> {
        let mut counter = 0;
        let mut name: Option<String> = None;

        let mut iterators = vec![self.servers.iter_mut()];
        let mut qualifier: Vec<&str> = vec![];

        while let Some(back_iter) = iterators.last_mut() {
            let mut exhausted = true;

            for mut entry in back_iter {
                if self.selected_server == counter {
                    match entry {
                        ServerEntry::Group(server_group) => {
                            server_group.expanded = !server_group.expanded;
                            return Ok(None);
                        }
                        ServerEntry::Object(server_object) => {
                            qualifier.push(&server_object.name);
                            return Ok(Some(qualifier.join("/")));
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

            if exhausted {
                iterators.pop();
                qualifier.pop();
            }
        }

        let Some(name) = name else {
            stderr_log!("{counter}");
            return Ok(None);
        };

        // let name = server_name.into();
        write_message!(writer, ClientMessage::Attach { name: name.clone() }).await?;

        Ok(Some(name))
    }

    pub async fn handle_keycode(
        &mut self,
        key: KeyEvent,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> miette::Result<()> {
        match &mut self.mode {
            Mode::Selecting => match key.code {
                KeyCode::Enter => {
                    let Some(name) = self
                        .try_select(&mut writer)
                        .await
                        .into_diagnostic()
                        .wrap_err("Failed to select item")?
                    else {
                        return Ok(());
                    };

                    write_message!(writer, ClientMessage::Attach { name: name.clone() })
                        .await
                        .into_diagnostic()
                        .wrap_err("Failed to send attach message to server")?;

                    stderr_log!("{name:?}");
                    self.mode = Mode::output(self.selected_server, name);
                }
                // KeyCode::Left | KeyCode::Char('h') => self.selection_left(),
                // KeyCode::Right | KeyCode::Char('l') => self.selection_right(),
                KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

                // TODO: these should actually by different from left and right
                KeyCode::Tab => todo!("Implement tab"),
                KeyCode::BackTab => todo!("Implement back tab"),

                // KeyCode::Esc => self.exit(),
                KeyCode::Char(ch) => match ch {
                    'q' => self.exit(),
                    'd' => todo!("Implement deletion"),
                    's' => todo!("Implement manual server list synchronization"),

                    'u' => {
                        stderr_log!("{}", readable_pretty(&self.servers));
                        stderr_log!("Position: {}", self.selected_server);
                    }

                    _ => {}
                },
                _ => {}
            },

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
                    KeyCode::Enter => {
                        stderr_log!("Trying to select rn");
                        if let Some(name) = self
                            .try_select(writer)
                            .await
                            .into_diagnostic()
                            .wrap_err("Failed to attach to server")?
                        {
                            self.mode = Mode::output(self.selected_server, name);
                        } else {
                            stderr_log!("None found!");
                        }
                    }
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

    pub async fn run(
        &mut self,
        terminal: &mut DefaultTerminal,
        (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
    ) -> miette::Result<()> {
        let mut event_reader = EventStream::new();

        let blink_event = if let Some(blink_interval) = self.config.blink_interval {
            Either::Left(sleep(blink_interval))
        } else {
            Either::Right(pending())
        };

        pin!(blink_event);

        loop {
            if self.exiting {
                writer
                    .shutdown()
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to send shutdown packet")?;
                break Ok(());
            }

            terminal
                .draw(|frame| self.render(frame))
                .into_diagnostic()
                .wrap_err("Failed to render tui")?;

            tokio::select! {
                    event = &mut event_reader.next() => {
                        let Some(event) = event else {
                            bail!("Failed to read from terminal");
                        };

                        let Event::Key(key) = event.into_diagnostic().wrap_err("Failed to read key input")? else {
                            // only care about those keys!!
                            continue;
                        };

                        self.handle_keycode(key, &mut writer).await?;
                    }

                    _ = &mut blink_event => {
                        blink_event.set(if let Some(blink_interval) = self.config.blink_interval {
                            Either::Left(sleep(blink_interval))
                        } else {
                            Either::Right(pending())
                        });

                        if let Mode::Console { blink, .. } = &mut self.mode {
                            *blink = !*blink;
                        }
                    }

                    length = reader.read_u32() => {
                        let length = match length {
                            Ok(length) => length,
                            Err(err) => match err.kind() {
                                io::ErrorKind::UnexpectedEof => {
                                    stderr_log!("Server closed");
                                    return Err(err).into_diagnostic().wrap_err("Server connection severed");
                                },
                                _ => return Err(err).into_diagnostic(),
                            },
                        };

                        match read_message!(reader, length).await.into_diagnostic().wrap_err("Failed to receive message from server")? {
                            ServerMessage::KillFailure(err) => stderr_log!("Failed to kill child: {err}"),
                            ServerMessage::KillSuccess => stderr_log!("Successfully killed the child process"),
                            ServerMessage::Closed => if let Mode::Console { input, state, .. } = &mut self.mode {
                                *input = "Press enter to return...".to_string();
                                *state = ServerState::Dead;
                            },
                            ServerMessage::Message(message) => {
                                stderr_log!("Received a message from the server: {message}");
                            },
                            ServerMessage::AlreadyStarted => {
                                stderr_log!("The server has already started apparently");
                            },
                            ServerMessage::NotAttached => {
                                stderr_log!("The server has not yet started apparently");
                            },
                            ServerMessage::Output(bytes) => if let Mode::Console { output, .. } = &mut self.mode {
                                output.push(display_bytes(&bytes).to_string());
                            },
                            ServerMessage::History(history) => if let Mode::Console { output, .. } = &mut self.mode {
                                history.into_iter().for_each(|line|{
                                    output.push(display_bytes(&line).to_string())
                                });
                            }
                        }
                    }
            }
        }
    }

    pub async fn run_attached(
        &mut self,
        terminal: &mut DefaultTerminal,
        (reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
        name: String,
    ) -> miette::Result<()> {
        // self.selected_server = self
        //     .servers
        //     .iter()
        //     .position(|server| *server == name)
        //     .wrap_err("Server not found")?;

        self.selected_server = todo!();

        // TODO: attach here
        // try_attach!(self, &mut writer);

        self.run(terminal, (reader, writer)).await
    }

    fn iterate_entries<'a>(
        &self,
        entries: &'a [ServerEntry],
        depth: usize,
        counter: &mut usize,
        attached: usize,
    ) -> Vec<Line<'a>> {
        let mut lines = vec![];
        let indentation = " ".repeat(depth * 2);
        let incremented_depth = depth + 1;

        for entry in entries.iter() {
            match entry {
                ServerEntry::Group(server_group) => {
                    let line = match (*counter == self.selected_server, server_group.expanded) {
                        (true, true) => {
                            Line::from(format!("{indentation}v {}/", server_group.name)).bold()
                        }
                        (true, false) => {
                            Line::from(format!("{indentation}> {}/", server_group.name)).bold()
                        }
                        (false, _) => Line::from(format!("{indentation}  {}/", server_group.name)),
                    };

                    lines.push(line);

                    *counter += 1;

                    if server_group.expanded {
                        lines.extend(self.iterate_entries(
                            &server_group.children,
                            incremented_depth,
                            counter,
                            attached,
                        ));
                    }
                }
                ServerEntry::Object(server_object) => {
                    let line = match (*counter == self.selected_server, *counter == attached) {
                        (true, true) => Line::from(vec![
                            span!(indentation),
                            '>'.to_span(),
                            '●'.green(),
                            server_object.name.to_span(),
                        ])
                        .fg(Color::White)
                        .bg(Color::Black)
                        .bold(),
                        (true, false) => {
                            Line::from(format!("{indentation}> {}", server_object.name)).bold()
                        }
                        (false, true) => {
                            Line::from(vec![span!(indentation), ' '.to_span(), '●'.green()])
                                .fg(Color::White)
                                .bg(Color::Black)
                        }
                        (false, false) => {
                            Line::from(format!("{indentation}  {}", server_object.name))
                        }
                    };

                    lines.push(line);

                    *counter += 1;
                }
            }
        }

        lines
    }

    fn draw_servers_list_widget(&self, height_available: u16) -> List<'_> {
        let mut counter = 0;

        let lines = self.iterate_entries(
            &self.servers,
            0,
            &mut counter,
            match self.mode {
                Mode::Console {
                    attached_server, ..
                } => attached_server,
                _ => usize::MAX,
            },
        );

        // stderr_log!("Servers: {:?}", self.servers);
        // stderr_log!("Lines: {lines:?}");

        let length = lines.len();

        // TODO: use this
        List::new(lines).block(Block::new().title("Servers").borders(Borders::ALL))
    }

    fn render(&self, frame: &mut Frame) {
        // Hella dark:
        // 26, 27, 38
        //
        // 30, 31, 42
        //
        // Very light, neutral, middle ground, valid color
        // 46, 52, 64

        if let Some(background_color) = self.config.terminal_interface_config.background_color {
            let background_block = Block::new().style(Style::new().bg(background_color));

            frame.render_widget(background_block, frame.area());
        }

        let Mode::Console {
            input,
            output,
            state,
            blink,
            ..
        } = &self.mode
        else {
            let area = frame.area();
            frame.render_widget(self.draw_servers_list_widget(area.height), area);
            return;
        };

        let output_area = if matches!(state, ServerState::Escaping) {
            let main_sections = Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                .split(frame.area());

            stderr_log!("Hello");

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

        // mono
        // negro
        // ------------
        // yes
        // sir
        // man
        // yesman

        let mut output_text = output
            .iter()
            // use skip here to get the maximum number of lines that might need to be used
            // Then, the lines that aren't used will simply not be rendered
            .skip(output.len().saturating_sub(output_lines_available))
            .fold(String::new(), |mut acc, item| {
                acc.push_str(item);
                // acc.push('\n');
                acc
            });

        output_text.push_str(input);

        if *blink {
            output_text.push('█');
        }

        let lines = output_text
            .split('\n')
            .flat_map(|line| textwrap::wrap(line, output_text_width))
            .count();

        let output_widget = Paragraph::new(output_text)
            .block(Block::new().title("Console").borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((lines.saturating_sub(output_lines_available) as u16, 0));

        frame.render_widget(output_widget, output_area);
    }

    fn selection_mode(&mut self) {
        self.mode = Mode::Selecting;
    }

    fn selection_up(&mut self) {
        let old = self.selected_server;
        self.selected_server = self.selected_server.saturating_sub(1);

        if old != self.selected_server {
            stderr_log!("New: {}", self.selected_server);
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
                ServerEntry::Object(server_object) => len += 1,
            }
        }

        len
    }

    fn selection_down(&mut self) {
        let old = self.selected_server;
        let mut length = Self::count_len(&self.servers);
        self.selected_server = usize::min(self.selected_server + 1, length - 1);

        if old != self.selected_server {
            stderr_log!("New: {}", self.selected_server);
        }
    }

    pub fn exit(&mut self) {
        self.exiting = true;
    }
}
