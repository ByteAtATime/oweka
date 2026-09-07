use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, HighlightSpacing, Paragraph, Row as TableRow, Table};

use super::app::App;
use super::format::{PENDING, display_path, format_size, relative_age, truncate_left};

const PATH_MIN_WIDTH: u16 = 8;
const MATCHER_WIDTH: u16 = 13;
const MODIFIED_WIDTH: u16 = 10;
const SIZE_WIDTH: u16 = 12;
const HIGHLIGHT_SYMBOL: &str = "▸ ";
const HIGHLIGHT_WIDTH: u16 = 2;
const COLUMN_SPACING: u16 = 1;
const COLUMN_GAP_COUNT: u16 = 3;
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    draw_header(frame, app, areas[0]);
    draw_table(frame, app, areas[1]);
    draw_status(frame, app, areas[2]);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let status = scan_status(app);
    let title = Line::from(vec![
        Span::styled("oweka", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!(" · {} · {} ", app.root().display(), status)),
        Span::styled(
            format!("{} found", app.rows().len()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let rule = "─".repeat(area.width.max(1) as usize).dim();
    let block = Paragraph::new(vec![title, rule.into()]);
    frame.render_widget(block, area);
}

fn draw_table(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.is_empty() {
        let hint = Paragraph::new(Line::from(Span::styled(
            "waiting for artifacts…",
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(hint, area);
        return;
    }
    let now = SystemTime::now();
    let path_width = path_column_width(area);
    let visible_rows = area.height.saturating_sub(1) as usize;
    let len = app.rows().len();
    let selection = app.selected_index();
    let start = scrolled_start(app, len, selection, visible_rows);
    app.set_scroll(start);
    let end = (start + visible_rows).min(len);
    app.set_render_selection(selection.map(|selected| selected - start));
    let window = &app.rows()[start..end];
    let header = TableRow::new([
        Cell::from("path"),
        Cell::from("matcher"),
        Cell::from("modified"),
        Cell::from(Line::from("size").alignment(Alignment::Right)),
    ])
    .style(Style::default().add_modifier(Modifier::DIM));
    let body: Vec<TableRow> = window
        .iter()
        .map(|row| {
            let path = truncate_left(&display_path(app.root(), &row.artifact.path), path_width);
            let modified = relative_age(row.last_modified, now);
            let size = row
                .bytes
                .map(format_size)
                .unwrap_or_else(|| PENDING.to_string());
            TableRow::new([
                Cell::from(path),
                Cell::from(row.artifact.matcher_id),
                Cell::from(modified),
                Cell::from(Line::from(size).alignment(Alignment::Right)),
            ])
        })
        .collect();
    let widths = [
        Constraint::Min(PATH_MIN_WIDTH),
        Constraint::Length(MATCHER_WIDTH),
        Constraint::Length(MODIFIED_WIDTH),
        Constraint::Length(SIZE_WIDTH),
    ];
    let table = Table::new(body, widths)
        .header(header)
        .column_spacing(COLUMN_SPACING)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(table, area, app.table_state_mut());
    app.set_render_selection(selection);
}

fn scrolled_start(app: &App, len: usize, selection: Option<usize>, visible_rows: usize) -> usize {
    let mut start = app.scroll().min(len - 1);
    let Some(selected) = selection else {
        return start;
    };
    if selected < start {
        return selected;
    }
    if selected >= start + visible_rows.max(1) {
        start = selected + 1 - visible_rows.max(1);
    }
    start
}

fn path_column_width(area: Rect) -> usize {
    area.width.saturating_sub(
        HIGHLIGHT_WIDTH
            + MATCHER_WIDTH
            + MODIFIED_WIDTH
            + SIZE_WIDTH
            + COLUMN_GAP_COUNT * COLUMN_SPACING,
    ) as usize
}

fn scan_status(app: &App) -> String {
    if app.is_done() {
        let elapsed = app
            .finished()
            .map(|end| end.duration_since(app.started()).as_secs_f32())
            .unwrap_or(0.0);
        return format!("done in {elapsed:.1}s");
    }
    let tick = app.started().elapsed().as_millis() / 200 % SPINNER.len() as u128;
    format!("scanning {}", SPINNER[tick as usize])
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let total = format_size(app.total_bytes());
    let first = Line::from(Span::raw(format!(
        "total {total} · errors {}",
        app.error_count()
    )));
    frame.render_widget(Paragraph::new(first), area);
}
