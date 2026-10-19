use std::time::Duration;

use crossterm::event::{EventStream, KeyCode};
use futures::StreamExt;
use rand::{
    RngExt, SeedableRng,
    rngs::{StdRng, SysRng},
};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::Event,
    layout::{Constraint, Layout},
    style::Color,
    text::Text,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use tokio::{
    io, pin, select,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::{Instant, sleep},
};

#[derive(Debug, Default)]
struct Cache<T> {
    value: Option<T>,
}

#[allow(dead_code)]
impl<T> Cache<T> {
    pub fn new() -> Self {
        Self { value: None }
    }

    pub fn with_value(value: T) -> Self {
        Self { value: Some(value) }
    }

    pub fn invalidate(&mut self) -> Option<T> {
        self.value.take()
    }

    pub fn get_or_set(&mut self, set: impl FnOnce() -> T) -> &mut T {
        self.value.get_or_insert_with(set)
    }
}

#[derive(Default)]
pub struct App {
    text: String,
    servers: Vec<String>,
    selected_server: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            text: "uninitialized".to_string(),
            servers: ["DonutSMP", "ChaosCraft", "LifestealSMP", "Dream SMP"]
                .map(String::from)
                .into(),
            ..Default::default()
        }
    }

    fn render(&self, frame: &mut Frame) {
        let horizontal_sections =
            Layout::horizontal([Constraint::Length(20), Constraint::Fill(1)]).split(frame.area());

        frame.render_widget(
            List::new(self.servers.iter().enumerate().map(|(idx, server)| {
                ListItem::new(if idx == self.selected_server {
                    Text::styled(format!("> {server}"), Color::Green)
                } else {
                    Text::styled(format!("  {server}"), Color::White)
                })
            }))
            .block(Block::new().borders(Borders::ALL).title("Servers")),
            horizontal_sections[0],
        );

        let output = "Hello, world!";

        frame.render_widget(
            Paragraph::new(output).block(Block::new().borders(Borders::ALL).title("Output")),
            horizontal_sections[1],
        );
    }

    pub async fn connect(
        &mut self,
        terminal: &mut DefaultTerminal,
    ) -> io::Result<(UnboundedSender<String>, UnboundedReceiver<String>)> {
        let mut periods = 0;

        let mut server_handle = tokio::spawn(async {
            sleep(Duration::from_secs(1)).await;
            unbounded_channel::<String>()
        });

        const PERIOD_UPDATE_INTERVAL: Duration = Duration::from_millis(300);

        let sleep_task = sleep(PERIOD_UPDATE_INTERVAL);
        pin!(sleep_task);

        let (tx, rx) = loop {
            terminal.draw(|frame| {
                let text_layout = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                ])
                .split(frame.area());

                let text = Paragraph::new(
                    Text::raw(format!("Connecting to server{}", ".".repeat(periods))).centered(),
                );

                periods = (periods + 1) % 4;

                frame.render_widget(text, text_layout[1]);
            })?;

            select! {
                res = &mut server_handle => {
                    break res?;
                }
                _ = &mut sleep_task => {
                    sleep_task.as_mut().reset(Instant::now() + PERIOD_UPDATE_INTERVAL);
                }
            }
        };

        Ok((tx, rx))
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut event_reader = EventStream::new();

        let (tx, mut rx) = self.connect(terminal).await?;

        tokio::spawn(async move {
            let mut rng = StdRng::try_from_rng(&mut SysRng)?;

            rng.random_range(0..5);

            loop {
                sleep(Duration::from_secs(1)).await;

                let strings = ["Hello", "Goodbye", "Hughie"];

                if tx
                    .send(strings[rng.random_range(0..strings.len())].to_string())
                    .is_err()
                {
                    // receiver dropped
                    break io::Result::Ok(());
                };
            }
        });

        loop {
            terminal.draw(|f| self.render(f))?;

            select! {
                event = &mut event_reader.next() => {
                    let Some(event) = event else {
                        eprintln!("Something went wrong!!");
                        // TODO: add an error variant for this
                        return Ok(());
                    };

                    let Event::Key(key) = event? else {
                        continue;
                    };

                    match key.code {
                        KeyCode::Backspace => todo!(),
                        KeyCode::Enter => todo!(),
                        KeyCode::Left | KeyCode::Char('h') => {
                            todo!()
                        },
                        KeyCode::Right => todo!(),
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.selected_server = self.selected_server.saturating_sub(1);
                        },
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.selected_server = usize::min(self.selected_server + 1, self.servers.len() - 1);
                        },
                        KeyCode::Tab => todo!(),
                        KeyCode::BackTab => todo!(),
                        KeyCode::Delete => todo!(),
                        KeyCode::Insert => todo!(),
                        KeyCode::F(_) => todo!(),
                        KeyCode::Char(_) => todo!(),
                        KeyCode::Esc => break Ok(()),
                        _ => {}
                    };
                },
                message = rx.recv() => {
                    let Some(message) = message else {
                        eprintln!("Transmitter was dropped!");
                        // TODO: handle appropriately, likely with an error
                        break Ok(());
                    };

                    self.text = message;
                }
            }
        }
    }
}
