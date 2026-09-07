mod app;
mod format;
mod view;

use std::io::{self, Stdout};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::engine::ScanEvent;

use app::App;

pub fn run(root: &Path, events: Receiver<ScanEvent>) -> io::Result<()> {
    enter_terminal()?;
    let outcome = run_loop(root, events);
    leave_terminal()?;
    outcome
}

fn enter_terminal() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    install_restore_hook();
    Ok(())
}

fn leave_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn install_restore_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        previous(info);
    }));
}

fn run_loop(root: &Path, events: Receiver<ScanEvent>) -> io::Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal: Terminal<CrosstermBackend<Stdout>> = Terminal::new(backend)?;
    let mut app = App::new(root.to_path_buf());
    loop {
        drain_events(&events, &mut app);
        terminal.draw(|frame| view::draw(frame, &mut app))?;
        if poll_quit(&mut app)? {
            return Ok(());
        }
    }
}

fn drain_events(events: &Receiver<ScanEvent>, app: &mut App) {
    while let Ok(event) = events.try_recv() {
        app.apply(event);
    }
}

fn poll_quit(app: &mut App) -> io::Result<bool> {
    if !event::poll(Duration::from_millis(100))? {
        return Ok(false);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Release => Ok(false),
        Event::Key(key) => Ok(handle_key(app, key.code, key.modifiers)),
        _ => Ok(false),
    }
}

fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        return true;
    }
    match code {
        KeyCode::Char('q') => true,
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_cursor(1);
            false
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_cursor(-1);
            false
        }
        _ => false,
    }
}
