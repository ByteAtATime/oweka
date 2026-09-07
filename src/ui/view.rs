use std::time::{Instant, SystemTime};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, ToLine};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, HighlightSpacing, Padding, Paragraph, Row as TableRow,
    Table, TableState,
};

use super::app::{App, RowView, ViewState};
use super::format::{PENDING, age_style, display_path, format_size, relative_age, truncate_left};

const PATH_MIN_WIDTH: u16 = 8;
const MATCHER_WIDTH: u16 = 10;
const MODIFIED_WIDTH: u16 = 10;
const SIZE_WIDTH: u16 = 12;
const HIGHLIGHT_SYMBOL: &str = "▸ ";
const HIGHLIGHT_WIDTH: u16 = 2;
const COLUMN_SPACING: u16 = 1;
const COLUMN_GAP_COUNT: u16 = 3;

pub fn draw(frame: &mut Frame, app: &mut App, now: Instant, wall: SystemTime) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let view = app.view_state(areas[1].height, now, wall);
    draw_header(frame, &view, areas[0]);
    draw_table(frame, &view, areas[1], wall);
    draw_status(frame, &view, areas[2]);
    draw_confirm(frame, &view, wall);
}

fn draw_header(frame: &mut Frame, view: &ViewState, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let title = Line::from(vec![
        Span::styled("oweka", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!(
            " · {} · {} ",
            view.root.display(),
            view.scan_status
        )),
        Span::styled(
            format!("{} found", view.row_count),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let hint = header_hint(view);
    let hint_width = hint.width() as u16;
    let top = Rect::new(area.x, area.y, area.width, 1);
    if hint_width == 0 || area.width < hint_width + 12 {
        let block = Paragraph::new(vec![title, rule_line(area.width)]);
        frame.render_widget(block, area);
        return;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(hint_width)])
        .split(top);
    frame.render_widget(Paragraph::new(title), columns[0]);
    frame.render_widget(Paragraph::new(hint.alignment(Alignment::Right)), columns[1]);
    if area.height > 1 {
        let rule_area = Rect::new(area.x, area.y + 1, area.width, 1);
        frame.render_widget(Paragraph::new(rule_line(area.width)), rule_area);
    }
}

fn header_hint(view: &ViewState) -> Line<'static> {
    if view.confirm.is_some() {
        hint_line(&[("y", "confirm"), ("n", "cancel")])
    } else if !view.done {
        hint_line(&[("q", "quit")])
    } else {
        hint_line(&[
            ("j/k/arrows", "move"),
            ("enter/space", "delete"),
            ("o", "open"),
            ("q", "quit"),
        ])
    }
}

fn hint_line(pairs: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::with_capacity(pairs.len() * 2);
    for (index, (key, label)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                " · ",
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    Line::from(spans)
}

fn rule_line(width: u16) -> Line<'static> {
    Line::from("─".repeat(width.max(1) as usize).dim())
}

fn draw_table(frame: &mut Frame, view: &ViewState, area: Rect, wall: SystemTime) {
    if view.row_count == 0 {
        let message = empty_message(view);
        let hint = Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().add_modifier(Modifier::DIM),
        )));
        frame.render_widget(hint, area);
        return;
    }
    let path_width = path_column_width(area);
    let header = TableRow::new([
        Cell::from("path"),
        Cell::from("matcher"),
        Cell::from("modified"),
        Cell::from(Line::from("size").alignment(Alignment::Right)),
    ])
    .style(Style::default().add_modifier(Modifier::DIM));
    let body: Vec<TableRow> = view
        .rows
        .iter()
        .map(|row| {
            let path = truncate_left(&display_path(&row.path), path_width);
            let modified = relative_age(row.last_modified, wall);
            let modified_style = age_style(row.last_modified, wall);
            let size = row_size_text(row);
            let modified_cell = if row.failed {
                Cell::from(modified)
            } else {
                Cell::from(Span::styled(modified, modified_style))
            };
            let cells = TableRow::new([
                Cell::from(path),
                Cell::from(row.matcher_id),
                modified_cell,
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
    let scanning = !view.done;
    let row_highlight = if scanning {
        Style::default()
            .fg(Color::Gray)
            .bg(Color::Black)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD)
    };
    let table = Table::new(body, widths)
        .header(header)
        .column_spacing(COLUMN_SPACING)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(row_highlight);
    let table = match view.confirm {
        Some(_) => table.style(Style::default().add_modifier(Modifier::DIM)),
        None if scanning => table.style(Style::default().add_modifier(Modifier::DIM)),
        None => table,
    };
    let mut state = TableState::default().with_selected(view.selection);
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_confirm(frame: &mut Frame, view: &ViewState, wall: SystemTime) {
    let Some(pending) = view.confirm.as_ref() else {
        return;
    };

    let frame_area = frame.area();
    if frame_area.width < 24 || frame_area.height < 8 {
        return;
    }
    let max_inner = max_inner_width(frame_area) as usize;
    let size = pending
        .bytes
        .map(format_size)
        .unwrap_or_else(|| PENDING.to_string());

    let age = match pending.last_modified {
        Some(modified) => relative_age(Some(modified), wall),
        None => String::from("unknown"),
    };
    let age_color = age_style(pending.last_modified, wall);

    let meta = Line::from(vec![
        Span::styled(
            format!("{} ", pending.matcher_id),
            Style::default().bold().fg(Color::Yellow),
        ),
        Span::styled(
            format!("{size} · modified "),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(age, age_color),
    ]);
    let prompt = Line::from(Span::styled(
        "This cannot be undone.",
        Style::default().fg(Color::DarkGray),
    ));
    let note_line = pending.note.map(|text| {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(Color::Yellow),
        ))
    });
    let note_width = note_line.as_ref().map(|line| line.width()).unwrap_or(0);

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

    let path_first = truncate_left(&pending.path.display().to_string(), max_inner);
    let first_width = confirm_content_width(
        [
            meta.width(),
            Line::from(path_first.as_str()).width(),
            prompt.width(),
            note_width,
            title.width() + 4,
            actions.width() + 4,
        ],
        max_inner,
    );
    let path_line = Line::from(Span::styled(
        truncate_left(&pending.path.display().to_string(), first_width),
        Style::default().bold().fg(Color::White),
    ));
    let content_width = confirm_content_width(
        [
            meta.width(),
            path_line.width(),
            prompt.width(),
            note_width,
            title.width() + 4,
            actions.width() + 4,
        ],
        max_inner,
    );

    let mut lines = vec![meta, path_line];
    if let Some(note_line) = note_line {
        lines.push("".to_line());
        lines.push(note_line);
    }
    lines.push(prompt);
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

fn confirm_content_width(widths: [usize; 6], max_inner: usize) -> usize {
    widths.into_iter().max().unwrap_or(0).min(max_inner)
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

fn path_column_width(area: Rect) -> usize {
    area.width.saturating_sub(
        HIGHLIGHT_WIDTH
            + MATCHER_WIDTH
            + MODIFIED_WIDTH
            + SIZE_WIDTH
            + COLUMN_GAP_COUNT * COLUMN_SPACING,
    ) as usize
}

fn draw_status(frame: &mut Frame, view: &ViewState, area: Rect) {
    let first = Line::from(Span::raw(format!(
        "Potential Space {} · Freed Space {} · errors {}",
        view.potential, view.freed, view.error_count
    )))
    .dim();
    frame.render_widget(Paragraph::new(first), area);
}

fn empty_message(view: &ViewState) -> &'static str {
    if view.done {
        "no artifacts found"
    } else {
        "waiting for artifacts…"
    }
}

fn row_size_text(row: &RowView) -> String {
    if row.deleting {
        String::from("deleting…")
    } else {
        row.bytes
            .map(format_size)
            .unwrap_or_else(|| PENDING.to_string())
    }
}
