use std::{cell::UnsafeCell, io::IsTerminal, iter, mem};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt;
use mcserver_core::{ClientMessage, LineBuffer, ServerMessage, read_message, write_message};
use miette::{Context, IntoDiagnostic, bail};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
struct Cache<T> {
    value: UnsafeCell<Option<T>>,
}

impl<T> Cache<T> {
    pub fn new() -> Self {
        Self {
            value: UnsafeCell::new(None),
        }
    }

    pub fn invalidate(&mut self) -> Option<T> {
        self.value.get_mut().take()
    }

    #[allow(unused)]
    pub fn get_or_set(&self, set: impl FnOnce() -> T) -> &T {
        unsafe { (*self.value.get()).get_or_insert_with(set) }
    }

    #[allow(dead_code)]
    pub fn get_or_set_mut(&mut self, set: impl FnOnce() -> T) -> &mut T {
        self.value.get_mut().get_or_insert_with(set)
    }
}

enum Mode {
    Selecting,
    Output {
        name: String,
        input: String,
        output: LineBuffer,
        escaping: bool,
        output_text: Cache<String>,
    },
}

impl Mode {
    fn output(name: String) -> Self {
        Self::Output {
            name,
            input: String::new(),
            output: LineBuffer::new(),
            escaping: false,
            output_text: Cache::new(),
        }
    }
}

pub struct App {
    servers: Vec<String>,
    selected_server: usize,
    mode: Mode,
    exiting: bool,
}

macro_rules! stderr_log {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        if !std::io::stderr().is_terminal() {
            eprintln!($($arg)*);
        }
    }};
}

impl App {
    pub fn new() -> Self {
        Self {
            servers: ["ChaosCraft", "DonutSMP", "Hypixel"]
                .into_iter()
                .map(String::from)
                .collect(),
            selected_server: 0usize,
            mode: Mode::Selecting,
            exiting: false,
        }
    }

    pub async fn attach(
        &self,
        server_name: impl Into<String>,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> io::Result<()> {
        let name = server_name.into();
        write_message!(writer, ClientMessage::Attach { name: name.clone() }).await?;

        stderr_log!("Hello, world!");

        Ok(())
    }

    pub async fn handle_keycode(
        &mut self,
        key: KeyEvent,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> miette::Result<()> {
        match &mut self.mode {
            Mode::Selecting => match key.code {
                KeyCode::Enter => {
                    let Some(name) = self.servers.get(self.selected_server) else {
                        stderr_log!("This should not happen! There is no server selected");
                        return Ok(());
                    };

                    self.attach(name, writer)
                        .await
                        .into_diagnostic()
                        .context("Failed to attach to server")?;
                    self.mode = Mode::output(name.to_owned());
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
                    'u' => todo!("Implement manual server list updation"),
                    _ => {}
                },
                _ => {}
            },

            Mode::Output {
                input,
                output,
                escaping,
                output_text,
                ..
            } if !*escaping => match key.code {
                KeyCode::Backspace => {
                    input.pop();
                    output_text.invalidate();
                }
                KeyCode::Enter => {
                    output_text.invalidate();

                    write_message!(
                        writer,
                        ClientMessage::ProgramInput(mem::take(input).into_bytes())
                    )
                    .await
                    .into_diagnostic()
                    .context("Failed to send input to the server")?
                }
                KeyCode::Left => todo!("Implement cursor left"),
                KeyCode::Right => todo!("Implement cursor right"),
                KeyCode::Tab => todo!("Implement autocompletions"),
                KeyCode::BackTab => {
                    *escaping = true;
                }
                KeyCode::Delete => todo!("Implement deletion"),
                // KeyCode::F(_) => todo!(),
                KeyCode::Char(ch) => {
                    input.push(ch);
                    output_text.invalidate();
                }
                KeyCode::Esc => {
                    *escaping = true;
                }
                _ => {}
            },

            // Escaping output
            Mode::Output { name, escaping, .. } => match key.code {
                KeyCode::Backspace => todo!(),
                KeyCode::Enter => {
                    let Some(selected_server) = self.servers.get(self.selected_server) else {
                        // TODO: ig this is an error
                        return Ok(());
                    };

                    *escaping = false;

                    if name != selected_server {
                        self.attach(selected_server, writer)
                            .await
                            .into_diagnostic()
                            .context("Failed to attach to server")?;
                        self.mode = Mode::output(selected_server.to_owned());
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

                KeyCode::Tab => {
                    *escaping = false;
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

        loop {
            if self.exiting {
                break Ok(());
            }

            terminal
                .draw(|frame| self.render(frame))
                .into_diagnostic()
                .context("Failed to render tui")?;

            tokio::select! {
                event = &mut event_reader.next() => {
                    let Some(event) = event else {
                        bail!("Failed to read from terminal");
                    };

                    let Event::Key(key) = event.into_diagnostic().context("Failed to read key input")? else {
                        // only care about those keys!!
                        continue;
                    };

                    self.handle_keycode(key, &mut writer).await?;
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

                    match read_message!(reader, length).await.into_diagnostic().context("Failed to receive message from server")? {
                        ServerMessage::KillFailure(err) => stderr_log!("Failed to kill child: {err}"),
                        ServerMessage::KillSuccess => stderr_log!("Successfully killed the child process"),

                        // automatically cleans up
                        ServerMessage::Closed => self.selection_mode(),
                        ServerMessage::Message(message) => stderr_log!("Received a message from the server: {message}"),
                        ServerMessage::AlreadyStarted => stderr_log!("The server has already started apparently"),
                        ServerMessage::NotAttached => stderr_log!("The server has not yet started apparently"),
                        ServerMessage::Output(bytes) => if let Mode::Output { output, output_text, .. } = &mut self.mode {
                            stderr_log!("Btw, got {}", str::from_utf8(&bytes).unwrap_or("couldn't nonvert"));
                            let line = String::from_utf8_lossy(&bytes).to_string();
                            output.push(line);
                            output_text.invalidate();
                        },
                        ServerMessage::Error(bytes) => if let Mode::Output { output, output_text, .. } = &mut self.mode {
                            output.push(String::from_utf8_lossy(&bytes).to_string());
                            output_text.invalidate();
                        }
                        ServerMessage::History(history) => if let Mode::Output { output, output_text, .. } = &mut self.mode {
                            output.extend(history);
                            output_text.invalidate();
                        }
                    };
                }
            }
        }
    }

    fn draw_servers_list_widget(&self) -> List<'_> {
        List::new(self.servers.iter().enumerate().map(|(idx, name)| {
            ListItem::new(if idx == self.selected_server {
                Text::styled(format!("> {name}"), Style::new().bold())
            } else {
                Text::raw(format!("  {name}"))
            })
        }))
        .block(Block::new().title("Servers").borders(Borders::ALL))
    }

    fn render(&self, frame: &mut Frame) {
        // Hella dark:
        // 26, 27, 38
        //
        // 30, 31, 42
        //
        // Very light, neutral, middle ground, valid color
        // 46, 52, 64

        let background_block = Block::new().style(Style::new().bg(Color::Rgb(46, 52, 64)));

        frame.render_widget(background_block, frame.area());

        let Mode::Output {
            input,
            output,
            escaping,
            ..
        } = &self.mode
        else {
            frame.render_widget(self.draw_servers_list_widget(), frame.area());
            return;
        };

        let output_area = if *escaping {
            let main_sections = Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                .split(frame.area());

            frame.render_widget(self.draw_servers_list_widget(), main_sections[0]);

            main_sections[1]
        } else {
            frame.area()
        };

        let output_text_width = output_area.width.saturating_sub(2) as usize;
        let lines_available = output_area.height.saturating_sub(2) as usize;

        let mut output_text = output
            .iter()
            // use skip here to get the maximum number of lines that might need to be used
            // Then, the lines that aren't used will simply not be rendered
            .skip(output.len().saturating_sub(lines_available))
            .fold(String::new(), |mut acc, item| {
                acc.push_str(item);
                // acc.push('\n');
                acc
            });

        output_text.push_str(input);

        let lines = output_text
            .split('\n')
            .flat_map(|line| textwrap::wrap(line, output_text_width))
            .count();

        // stderr_log!(
        //     "Last 10 characters: '{}'",
        //     &output_text[output_text.len().saturating_sub(11)..output_text.len()]
        // );

        let output_widget = Paragraph::new(output_text)
            .block(Block::new().title("Output").borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((lines.saturating_sub(lines_available) as u16, 0));

        frame.render_widget(output_widget, output_area);
    }

    fn selection_mode(&mut self) {
        self.mode = Mode::Selecting;
    }

    fn selection_up(&mut self) {
        self.selected_server = self.selected_server.saturating_sub(1);
    }

    fn selection_down(&mut self) {
        self.selected_server = usize::min(self.selected_server + 1, self.servers.len() - 1);
    }

    pub fn exit(&mut self) {
        self.exiting = true;
    }
}
