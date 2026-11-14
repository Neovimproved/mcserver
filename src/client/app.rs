use std::{
    collections::HashSet,
    fmt::Debug,
    future::{Pending, pending},
    mem,
    path::PathBuf,
    pin::Pin,
    str::FromStr,
};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{StreamExt, future::Either};
use kdl::KdlDocument;
use miette::{Context, IntoDiagnostic, bail};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph},
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    pin, select,
    time::{Sleep, sleep},
};

use crate::{
    client::{
        state::{
            AbsoluteArea, AddingField, ConnectionState, Lines, Mode, SelectionState, ServerList,
        },
        widgets::{Addition, Deletion, ServersList},
    },
    config::{self, Config, TerminalInterfaceConfig},
    consts::CURSOR_CHAR,
    creator::{ServerSoftware, Version},
    line_buffer::LineBuffer,
    log,
    messages::{ClientMessage, CreateMessage, ServerMessage, display_bytes},
    read_message,
    servers::{self, ServerEntry},
    write_message,
};

pub struct App {
    config: Config,
    document: KdlDocument,
    config_file: PathBuf,
    mode: Mode,
    server_list: ServerList,
    last_area: AbsoluteArea,
    blinking: bool,
    exiting: bool,
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
        let (config, document, config_file) = config::read()
            .await
            .into_diagnostic()
            .wrap_err("Failed to create/read config")?;

        log!("Pretty config: {config:#?}");

        let servers = servers::read(config.servers_directory.as_ref()).await?;

        Ok(Self {
            config,
            document,
            config_file,
            mode: Mode::Selecting(SelectionState::Normal),
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
            blinking: true,
            exiting: false,
        })
    }

    fn ti_cfg(&self) -> &TerminalInterfaceConfig {
        self.config.ti()
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

    async fn list_motion(
        &mut self,
        key: KeyEvent,
        mut writer: impl AsyncWriteExt + Unpin,
    ) -> miette::Result<()> {
        match key.code {
            // TODO: implement an enter key with a control modifier to fold/unfold all
            KeyCode::Enter => self
                .try_select(writer)
                .await
                .wrap_err("Failed to attach to server")?,

            KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
            KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_down();
            }

            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_up();
            }

            KeyCode::Char('g') => self.selection_top(),
            KeyCode::Char('G') => self.selection_bottom(),

            // for debugging things
            #[cfg(debug_assertions)]
            KeyCode::Char('d') => {
                log_less_whitespace(&self.server_list.servers, |n| n / 2);
                log!("Position: {}", self.server_list.selected);
            }

            KeyCode::Char('q') => self.exit(),

            KeyCode::Char('s') => write_message!(writer, ClientMessage::SynchronizationRequest)
                .await
                .into_diagnostic()
                .wrap_err("Failed to make synchronization request")?,

            _ => {}
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
                    KeyCode::Char('a') => *selection_state = SelectionState::adding(),

                    KeyCode::Char('D') => {
                        if let Some(entry) =
                            Self::get_selected(&self.server_list.servers, self.server_list.selected)
                        {
                            *selection_state = SelectionState::deleting(entry);
                        };
                    }

                    // for debugging things
                    #[cfg(debug_assertions)]
                    KeyCode::Char('d') => {
                        log_less_whitespace(&self.server_list.servers, |n| n / 2);
                        log!("Position: {}", self.server_list.selected);
                    }

                    KeyCode::Char('s') => {
                        write_message!(writer, ClientMessage::SynchronizationRequest)
                            .await
                            .into_diagnostic()
                            .wrap_err("Failed to make synchronization request")?
                    }

                    _ => self.list_motion(key, writer).await?,
                },
                SelectionState::Adding(addition_state) => match key.code {
                    KeyCode::Esc => selection_state.reset(),
                    KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        match ch {
                            'h' => {
                                let input_string = match addition_state.field {
                                    AddingField::Name => &mut addition_state.name,
                                    AddingField::Version => &mut addition_state.version,
                                    AddingField::Software => &mut addition_state.server_software,
                                };

                                while let Some(last) = input_string.chars().next_back()
                                    && last == ' '
                                {
                                    input_string.pop();
                                }

                                while let Some(last) = input_string.chars().next_back()
                                    && last != ' '
                                {
                                    input_string.pop();
                                }
                            }
                            'q' => self.exit(),
                            _ => {}
                        }
                    }

                    KeyCode::Char(ch) => match addition_state.field {
                        AddingField::Name => &mut addition_state.name,
                        AddingField::Version => &mut addition_state.version,
                        AddingField::Software => &mut addition_state.server_software,
                    }
                    .push(ch),
                    // TODO: actually create the server here
                    KeyCode::Enter => {
                        if addition_state.field.next() {
                            let server_software = if addition_state.server_software.is_empty() {
                                // vanilla
                                ServerSoftware::default()
                            } else {
                                let Ok(server_software) =
                                    ServerSoftware::from_str(&addition_state.server_software)
                                else {
                                    addition_state.error_message.insert(format!(
                                        "`{}` is not a valid server software",
                                        addition_state.server_software
                                    ));
                                    return Ok(());
                                };

                                server_software
                            };

                            let version = if addition_state.version.is_empty() {
                                log!("Empty!");
                                None
                            } else {
                                log!("Parsing `{}`", addition_state.version);
                                match Version::from_str(&addition_state.version) {
                                    Ok(version) => Some(version),
                                    Err(err) => {
                                        addition_state.error_message.insert(err.to_string());
                                        return Ok(());
                                    }
                                }
                            };

                            let message = CreateMessage {
                                path: mem::take(&mut addition_state.name),
                                server_software,
                                version,
                            };

                            write_message!(writer, ClientMessage::Create(message))
                                .await
                                .into_diagnostic()
                                .wrap_err("Failed to send create server request")?;

                            selection_state.reset();
                        }
                    }
                    KeyCode::Tab => {
                        addition_state.field.next();
                    }
                    KeyCode::BackTab => {
                        addition_state.field.prev();
                    }
                    KeyCode::Backspace => {
                        match addition_state.field {
                            AddingField::Name => &mut addition_state.name,
                            AddingField::Version => &mut addition_state.version,
                            AddingField::Software => &mut addition_state.server_software,
                        }
                        .pop();
                    }
                    _ => {}
                },
                SelectionState::Deleting(deletion_state) => match key.code {
                    KeyCode::Backspace => {
                        deletion_state.confirmation.pop();
                    }
                    KeyCode::Enter => {
                        if deletion_state.name == deletion_state.confirmation {
                            write_message!(
                                writer,
                                ClientMessage::Delete {
                                    server: mem::take(&mut deletion_state.name)
                                }
                            )
                            .await
                            .into_diagnostic()
                            .wrap_err("Failed to send server deletion message")?;
                            selection_state.reset();
                        } else {
                            deletion_state
                                .error_message
                                .insert(format!("Failed to rename {}", deletion_state.entry_type));
                        }
                    }
                    KeyCode::Delete => todo!(
                        "Delete char is not yet implemented in deletion mode yet unfortunately"
                    ),
                    KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        match ch {
                            'h' => {
                                while let Some(ch) = deletion_state.confirmation.chars().next_back()
                                    && ch == ' '
                                {
                                    deletion_state.confirmation.pop();
                                }

                                while let Some(ch) = deletion_state.confirmation.chars().next_back()
                                    && ch != ' '
                                {
                                    deletion_state.confirmation.pop();
                                }
                            }
                            'q' => self.exit(),
                            _ => {}
                        }
                    }
                    KeyCode::Char(ch) => deletion_state.confirmation.push(ch),
                    KeyCode::Esc => selection_state.reset(),
                    _ => {}
                },
            },

            Mode::Console(console) if let ConnectionState::Dead = console.state => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                    self.selection_mode()
                }
                _ => self.list_motion(key, writer).await?,
            },

            // Escaping output
            Mode::Console(console) if let ConnectionState::Escaping = console.state => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.selection_up(),
                    KeyCode::Down | KeyCode::Char('j') => self.selection_down(),

                    KeyCode::Tab => console.state.reset(),

                    KeyCode::BackTab => self.selection_mode(),

                    KeyCode::Char(':') => {
                        // TODO: implement commands
                    }

                    KeyCode::Esc => self.selection_mode(),
                    _ => {}
                }
            }

            Mode::Console(console) => match key.code {
                KeyCode::Backspace => {
                    console.input.pop();
                }
                KeyCode::Enter => {
                    console.input.push('\n');

                    write_message!(
                        writer,
                        ClientMessage::ProgramInput(mem::take(&mut console.input).into_bytes())
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
                    console.state = ConnectionState::Escaping;
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
                        console.input.push(ch);
                    }
                }
                KeyCode::Esc => {
                    console.state = ConnectionState::Escaping;
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
            self.blinking = !self.blinking;
            Either::Left(sleep(interval))
        } else {
            self.blinking = true;
            Either::Right(pending())
        });
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
            ServerMessage::AlreadyStarted => {
                log!("The server has already started apparently");
            }
            ServerMessage::Closed => {
                if let Mode::Console(console) = &mut self.mode {
                    console.input = "Press enter to return...".to_string();
                    console.state = ConnectionState::Dead;
                }
            }
            ServerMessage::History(history) => {
                if let Mode::Console(console) = &mut self.mode {
                    history
                        .into_iter()
                        .for_each(|line| console.output.push(display_bytes(&line).to_string()));
                }
            }
            ServerMessage::KillFailure(err) => log!("Failed to kill child: {err}"),
            ServerMessage::Message(message) => {
                log!("Received a message from the server: {message}");
            }
            ServerMessage::NotAttached => {
                log!("The server has not yet started apparently");
            }
            ServerMessage::Output(bytes) => {
                if let Mode::Console(console) = &mut self.mode {
                    console.output.push(display_bytes(&bytes).to_string());
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

        // let (event_sender, event_handler) = unbounded_channel();

        let blink_event = if let Some(blink_interval) = self.ti_cfg().cursor_blink {
            Either::Left(sleep(blink_interval))
        } else {
            Either::Right(pending())
        };

        pin!(blink_event);

        let mut redraw = true;

        let mut config_dir_watch: config::Watch = config::Watch::try_new(&self.config_file)
            .await
            .into_diagnostic()
            .wrap_err("Failed to create config directory event stream")?;

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

            redraw = select! {
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

                result = config_dir_watch.next() => {
                    if !(result.into_diagnostic().wrap_err("Failed to read event from inotify event stream")?) {
                        config_dir_watch
                            .restore()
                            .into_diagnostic()
                            .wrap_err("Failed to recreate config directory event stream")?;
                        continue;
                    };

                    let Ok((config, document)) = config::read_file(&self.config_file).await else {
                        log!("Erroneous config file contents");
                        // program should not crash at this point
                        continue;
                    };

                    let blinking_before = self.ti_cfg().cursor_blink.is_some();

                    self.config = config;
                    self.document = document;

                    if !blinking_before && self.ti_cfg().cursor_blink.is_some() {
                        self.handle_blink_event(&mut blink_event).await;
                    }

                    true
                }
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

    fn center_horizontal(area: Rect, x_percent: u16) -> Rect {
        Layout::horizontal([
            Constraint::Percentage(50 - x_percent / 2),
            Constraint::Percentage(x_percent),
        ])
        .split(area)[1]
    }

    fn render(&self, frame: &mut Frame) -> AbsoluteArea {
        if let Some(background_color) = self.ti_cfg().background_color {
            let background_block = Block::new().style(Style::new().bg(background_color));

            frame.render_widget(background_block, frame.area());
        }

        let frame_area = frame.area();

        const DELETION_WIDGET_LINES: u16 = 3;

        match &self.mode {
            Mode::Selecting(selection_state) => {
                frame.render_widget(
                    ServersList::new(&self.server_list.servers, self.ti_cfg())
                        .selected(self.server_list.selected)
                        .scroll(self.server_list.scroll),
                    frame_area,
                );

                match selection_state {
                    SelectionState::Normal => {}
                    SelectionState::Adding(addition_state) => {
                        let addition_area = Layout::vertical([
                            Constraint::Length((frame_area.height - 9) / 2),
                            Constraint::Length(9),
                        ])
                        .split(Self::center_horizontal(frame.area(), 90))[1];

                        frame.render_widget(
                            Addition::new(addition_state).blinking(self.blinking),
                            addition_area,
                        );
                    }
                    SelectionState::Deleting(deletion_state) => {
                        const DELETION_WIDGET_WIDTH_PERCENT: u16 = 90;

                        let area = Layout::vertical([
                            Constraint::Length((frame_area.height - DELETION_WIDGET_LINES) / 2),
                            Constraint::Length(DELETION_WIDGET_LINES),
                        ])
                        .split(Self::center_horizontal(
                            frame_area,
                            DELETION_WIDGET_WIDTH_PERCENT,
                        ))[1];

                        let deletion_widget =
                            Deletion::new(deletion_state, self.blinking, self.ti_cfg());

                        frame.render_widget(deletion_widget, area);
                    }
                }
            }

            Mode::Console(console) => {
                let output_area = if matches!(console.state, ConnectionState::Escaping) {
                    let main_sections =
                        Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)])
                            .split(frame.area());

                    frame.render_widget(
                        ServersList::new(&self.server_list.servers, self.ti_cfg())
                            .attached(console.attached_server)
                            .selected(self.server_list.selected)
                            .scroll(self.server_list.scroll),
                        main_sections[0],
                    );

                    main_sections[1]
                } else {
                    frame.area()
                };

                let widget =
                    self.draw_output_widget(output_area.into(), &console.input, &console.output);

                frame.render_widget(widget, output_area);
            }
        }

        frame_area.into()
    }

    fn selection_mode(&mut self) {
        self.mode = Mode::Selecting(SelectionState::Normal);
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

    fn selection_up(&mut self) {
        if self.server_list.selected == 0 {
            self.selection_bottom();
            return;
        }

        self.server_list.selected -= 1;

        if self.server_list.scroll > self.server_list.selected {
            self.server_list.scroll = self.server_list.selected;
        }
    }

    fn selection_down(&mut self) {
        let length = Self::count_len(&self.server_list.servers);

        if self.server_list.selected >= length.saturating_sub(1) {
            self.selection_top();
            return;
        }

        self.server_list.selected += 1;

        let height_index = self.last_area.height.saturating_sub(3);
        let last_visible = self.server_list.scroll + height_index;

        if self.server_list.selected > last_visible {
            self.server_list.scroll = self.server_list.selected - height_index;
        }
    }

    fn selection_top(&mut self) {
        self.server_list.selected = 0;
        self.server_list.scroll = 0;
    }

    fn selection_bottom(&mut self) {
        let length = Self::count_len(&self.server_list.servers);
        self.server_list.selected = length.saturating_sub(1);

        self.server_list.scroll = self
            .server_list
            .scroll
            .max(length.saturating_sub(self.last_area.height.saturating_sub(2)));
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
