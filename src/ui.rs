mod app;
mod format;
mod view;

use std::io::{self, Stdout};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::engine::{DeleteOutcome, ScanEvent, delete_artifact, size_artifact};

use app::{App, DeleteResult};

const TICK: Duration = Duration::from_millis(100);

pub enum UiEvent {
    Tick,
    Key(KeyEvent),
    Scan(ScanEvent),
    Deleted(DeleteResult),
}

pub fn run(root: &Path, scan_events: Receiver<ScanEvent>) -> io::Result<()> {
    enter_terminal()?;
    let outcome = run_loop(root, scan_events);
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

fn run_loop(root: &Path, scan_events: Receiver<ScanEvent>) -> io::Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal: Terminal<CrosstermBackend<Stdout>> = Terminal::new(backend)?;
    let mut app = App::new(root.to_path_buf());
    let (ui_sender, ui_events) = mpsc::channel();
    spawn_event_thread(ui_sender.clone());
    spawn_scan_bridge(scan_events, ui_sender.clone());
    loop {
        terminal.draw(|frame| view::draw(frame, &mut app))?;
        match next_ui_event(&ui_events, &mut app)? {
            UiEvent::Key(key) => {
                if handle_key(&mut app, key, &ui_sender) {
                    return Ok(());
                }
            }
            UiEvent::Tick | UiEvent::Scan(_) | UiEvent::Deleted(_) => {}
        }
    }
}

fn next_ui_event(ui_events: &Receiver<UiEvent>, app: &mut App) -> io::Result<UiEvent> {
    match ui_events
        .recv()
        .map_err(|_| io::Error::other("event sources stopped"))?
    {
        UiEvent::Scan(event) => {
            app.apply(event);
            drain_ready(ui_events, app)
        }
        UiEvent::Deleted(result) => {
            app.apply_delete_result(result);
            drain_ready(ui_events, app)
        }
        other => Ok(other),
    }
}

fn drain_ready(ui_events: &Receiver<UiEvent>, app: &mut App) -> io::Result<UiEvent> {
    while let Ok(follow_up) = ui_events.try_recv() {
        match follow_up {
            UiEvent::Scan(event) => app.apply(event),
            UiEvent::Deleted(result) => app.apply_delete_result(result),
            other => return Ok(other),
        }
    }
    Ok(UiEvent::Tick)
}

fn spawn_event_thread(sender: Sender<UiEvent>) {
    thread::spawn(move || {
        let mut last_tick = Instant::now();
        loop {
            let timeout = TICK.checked_sub(last_tick.elapsed()).unwrap_or(TICK);
            if event::poll(timeout).is_ok_and(|ready| ready)
                && let Ok(Event::Key(key)) = event::read()
                && key.kind == KeyEventKind::Press
                && sender.send(UiEvent::Key(key)).is_err()
            {
                return;
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                if sender.send(UiEvent::Tick).is_err() {
                    return;
                }
            }
        }
    });
}

fn spawn_scan_bridge(scan_events: Receiver<ScanEvent>, sender: Sender<UiEvent>) {
    thread::spawn(move || {
        for event in scan_events {
            if sender.send(UiEvent::Scan(event)).is_err() {
                return;
            }
        }
    });
}

fn handle_key(app: &mut App, key: KeyEvent, sender: &Sender<UiEvent>) -> bool {
    let code = key.code;
    let modifiers = key.modifiers;
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
        KeyCode::Enter | KeyCode::Char(' ') => {
            spawn_deletion(app, sender.clone());
            false
        }
        _ => false,
    }
}

fn spawn_deletion(app: &mut App, sender: Sender<UiEvent>) {
    let Some(artifact) = app.deletion_target() else {
        return;
    };
    app.mark_deleting(&artifact);
    thread::spawn(move || {
        let outcome = delete_artifact(&artifact);
        let rescan = match &outcome {
            DeleteOutcome::Deleted => None,
            _ => Some(size_artifact(&artifact)),
        };
        let _ = sender.send(UiEvent::Deleted(DeleteResult {
            artifact,
            outcome,
            rescan,
        }));
    });
}
