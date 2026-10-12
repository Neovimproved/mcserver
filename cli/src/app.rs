#![allow(unused)]

use std::env;

use color_eyre::eyre::{Context, eyre};
use crossterm::event::{Event, EventStream, KeyCode};
use futures::StreamExt;
use mcserver_core::{ClientMessage, Result, ServerMessage, read_message, write_message};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem},
};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};

pub enum Selection {
    Server(usize),
}

impl Default for Selection {
    fn default() -> Self {
        Self::Server(0)
    }
}

enum Mode {
    Exiting,
    Selecting,
    Output,
    EscapingOutput,
}

pub struct App {
    servers: Vec<String>,
    selected_server: usize,
    mode: Mode,
}

impl App {
    pub fn new() -> Self {
        Self {
            servers: ["ChaosCraft", "DonutSMP"]
                .into_iter()
                .map(String::from)
                .collect(),
            selected_server: 0usize,
            mode: Mode::Selecting,
        }
    }

    // async fn request_server_start(
    //     &mut self,
    //     terminal: &mut DefaultTerminal,
    //     mut writer: impl AsyncWriteExt + Unpin,
    // ) -> color_eyre::Result<()> {
    //     let mut path = env::home_dir().ok_or(io::Error::new(
    //         io::ErrorKind::NotFound,
    //         "could not find home directory",
    //     ))?;
    //
    //     path.extend(&[
    //         "Dev",
    //         "example-terminal-app",
    //         "target",
    //         "debug",
    //         "example-terminal-app",
    //     ]);
    //
    //     write_message!(
    //         writer,
    //         ClientMessage::Start {
    //             name: path.into_string().map_err(|path_buf| {
    //                 eyre!("Failed to convert {} into string", path_buf.display())
    //             })?,
    //             flags: env::args().skip(1).collect(),
    //         }
    //     )
    //     .await?;
    //
    //     Ok(())
    // }

    pub async fn handle_keycode(&mut self) -> Result<()> {
        todo!()
    }

    pub async fn run(
        &mut self,
        terminal: &mut DefaultTerminal,
        (mut reader, mut writer): (impl AsyncReadExt + Unpin, impl AsyncWriteExt + Unpin),
    ) -> Result<()> {
        let mut event_reader = EventStream::new();

        loop {
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                event = &mut event_reader.next() => {
                    let Some(event) = event else {
                        // TODO: handle this error
                        // (it is an error, as the event reader should not yield None)
                        break Ok(());
                    };

                    let Event::Key(key) = event? else {
                        // only care about those keys!!
                        continue;
                    };

                    match key.code {
                        KeyCode::Backspace => todo!(),
                        KeyCode::Enter => todo!(),
                        // KeyCode::Left | KeyCode::Char('h') => self.selection_left(),
                        // KeyCode::Right | KeyCode::Char('l') => self.selection_right(),
                        KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.selection_down(),
                        // TODO: these should actually by different from left and right
                        KeyCode::Tab => todo!("Implement tab"),
                        KeyCode::BackTab => todo!("Implement back tab"),
                        KeyCode::Char('q') => self.exit(),
                        KeyCode::Delete => todo!("Implement delete"),
                        KeyCode::Esc => self.exit(),
                        _ => {}
                    }
                }

                length = reader.read_u32() => {
                    let length = match length {
                        Ok(length) => length,
                        Err(err) => match err.kind() {
                            io::ErrorKind::UnexpectedEof => {
                                eprintln!("Server closed");
                                break Ok(());
                            },
                            _ => return Err(err.into()),
                        },
                    };

                    match read_message!(ServerMessage, reader, length).await? {
                        ServerMessage::KillFailure(err) => eprintln!("Failed to kill child: {err}"),
                        ServerMessage::KillSuccess => println!("Successfully killed the child process"),
                        ServerMessage::Message(message) => println!("Received a message from the server: {message}"),
                        ServerMessage::AlreadyStarted => println!("The server has already started apparently"),
                        ServerMessage::NotStarted => eprintln!("The server has not yet started apparently"),
                        ServerMessage::Output(output) => {
                            // io::stdout().write_all(&output).await?
                        },
                        ServerMessage::Error(error) => {
                            // io::stderr().write_all(&error).await?;
                        }
                    };
                }
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let main_sections =
            Layout::horizontal([Constraint::Length(20), Constraint::Fill(1)]).split(frame.area());

        let servers_list = List::new(self.servers.iter().enumerate().map(|(idx, name)| {
            ListItem::new(if idx == self.selected_server {
                Text::styled(format!("> {name}"), Style::new().bold())
            } else {
                Text::raw(format!("  {name}"))
            })
        }))
        .block(Block::new().title("Servers").borders(Borders::ALL));

        frame.render_widget(servers_list, main_sections[0]);
    }

    pub fn selection_up(&mut self) {
        self.selected_server = self.selected_server.saturating_sub(1);
    }

    pub fn selection_down(&mut self) {
        self.selected_server = usize::min(self.selected_server + 1, self.servers.len() - 1);
    }

    pub fn exit(&mut self) {
        self.mode = Mode::Exiting;
    }
}
