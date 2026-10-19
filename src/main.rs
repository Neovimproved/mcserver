mod app;
mod error;

use std::{
    io,
    ops::{Deref, DerefMut},
};

use app::App;
use ratatui::DefaultTerminal;

struct GuardedTerminal {
    terminal: DefaultTerminal,
}

impl Drop for GuardedTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

impl Deref for GuardedTerminal {
    type Target = DefaultTerminal;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for GuardedTerminal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut guarded_terminal = GuardedTerminal {
        terminal: ratatui::init(),
    };

    let mut app = App::new();

    app.run(&mut guarded_terminal).await?;

    Ok(())
}
