use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, HighlightSpacing, Padding, Paragraph, Row as TableRow,
    Table,
};

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
    draw_confirm(frame, app);
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
        let message = empty_message(app);
        let hint = Paragraph::new(Line::from(Span::styled(
            message,
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
            let size = row_size_text(row);
            let cells = TableRow::new([
                Cell::from(path),
                Cell::from(row.artifact.matcher_id),
                Cell::from(modified),
                Cell::from(Line::from(size).alignment(Alignment::Right)),
            ]);
            if row.failed {
                cells.style(Style::default().fg(Color::Red))
            } else {
                cells
            }
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
    let table = match app.pending_confirm() {
        Some(_) => table.style(Style::default().add_modifier(Modifier::DIM)),
        None => table,
    };
    frame.render_stateful_widget(table, area, app.table_state_mut());
    app.set_render_selection(selection);
}

fn draw_confirm(frame: &mut Frame, app: &App) {
    let Some(pending) = app.pending_confirm() else {
        return;
    };

    let frame_area = frame.area();
    if frame_area.width < 24 || frame_area.height < 8 {
        return;
    }
    let max_inner = max_inner_width(frame_area) as usize;
    let now = SystemTime::now();
    let row = app.row_for(pending);

    let size = row
        .and_then(|r| r.bytes)
        .map(format_size)
        .unwrap_or_else(|| PENDING.to_string());

    let age = match row.and_then(|r| r.last_modified) {
        Some(modified) => relative_age(Some(modified), now),
        None => String::from("unknown"),
    };

    let path_str = truncate_left(&pending.path.display().to_string(), max_inner);
    let detail = format!("{size} · modified {age}");

    let meta = Line::from(vec![
        Span::styled(
            format!("{} ", pending.matcher_id),
            Style::default().bold().fg(Color::Yellow),
        ),
        Span::styled(detail, Style::default().fg(Color::Gray)),
    ]);
    let path_line = Line::from(Span::styled(
        path_str.clone(),
        Style::default().bold().fg(Color::White),
    ));
    let prompt = Line::from(Span::styled(
        "This cannot be undone.",
        Style::default().fg(Color::DarkGray),
    ));

    let title = Line::from(vec![Span::styled(
        " Confirm delete ",
        Style::default().bold().fg(Color::Red),
    )]);
    let actions = Line::from(vec![
        Span::raw(" "),
        Span::styled("y", Style::default().bold().fg(Color::Red)),
        Span::styled("es", Style::default().fg(Color::DarkGray)),
        Span::styled(" / ", Style::default().fg(Color::DarkGray)),
        Span::styled("n", Style::default().bold().fg(Color::White)),
        Span::styled("o ", Style::default().fg(Color::DarkGray)),
    ]);

    let content_width = [meta.width(), path_line.width(), prompt.width()]
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(title.width() + 4)
        .max(actions.width() + 4)
        .min(max_inner);
    let path_str = truncate_left(&pending.path.display().to_string(), content_width);
    let path_line = Line::from(Span::styled(
        path_str,
        Style::default().bold().fg(Color::White),
    ));
    let content_width = [meta.width(), path_line.width(), prompt.width()]
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(title.width() + 4)
        .max(actions.width() + 4)
        .min(max_inner);

    let lines = vec![meta, path_line, prompt];
    let dialog_width = dialog_width_for(content_width, frame_area.width);
    let dialog_height = (lines.len() as u16) + 4;
    let area = centered(frame_area, dialog_width, dialog_height);

    let modal = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Red))
            .style(Style::default().bg(Color::Rgb(24, 24, 27)))
            .title(title.alignment(Alignment::Center))
            .title_bottom(actions.alignment(Alignment::Center))
            .padding(Padding::new(2, 2, 1, 1)),
    );

    frame.render_widget(Clear, area);
    frame.render_widget(modal, area);
}

fn max_inner_width(area: Rect) -> u16 {
    area.width.saturating_sub(8).saturating_sub(6).max(10)
}

fn dialog_width_for(content_width: usize, available_width: u16) -> u16 {
    let wanted = (content_width as u16).saturating_add(6).max(28);
    let max = available_width.saturating_sub(4).max(28);
    wanted.min(max)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
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
    let potential = format_size(app.total_bytes());
    let freed = format_size(app.freed_bytes());
    let first = Line::from(Span::raw(format!(
        "Potential Space {potential} · Freed Space {freed} · errors {}",
        app.error_count()
    )));
    frame.render_widget(Paragraph::new(first), area);
}

fn empty_message(app: &App) -> &'static str {
    if app.is_done() {
        "no artifacts found"
    } else {
        "waiting for artifacts…"
    }
}

fn row_size_text(row: &super::app::Row) -> String {
    if row.deleting {
        String::from("deleting…")
    } else {
        row.bytes
            .map(format_size)
            .unwrap_or_else(|| PENDING.to_string())
    }
}
