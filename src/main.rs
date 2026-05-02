mod app;
mod error;
mod gpu;
mod prelude;
mod ui;

use std::{
    io::{self, Stdout},
    time::Duration,
};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};

use crate::app::*;
use crate::prelude::*;

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", APP_NAME, env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let mut app = App::new(display);
    app.refresh().context("initial refresh failed")?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if app.quit {
            return Ok(());
        }

        let timeout = REFRESH_INTERVAL
            .checked_sub(app.last_refresh.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key)?;
        }

        if app.last_refresh.elapsed() >= REFRESH_INTERVAL
            && app.overlay.is_none()
            && let Err(err) = app.refresh()
        {
            app.push_message(format!("Refresh failed: {err:#}"));
        }
    }
}
