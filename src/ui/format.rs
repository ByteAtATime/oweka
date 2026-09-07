use std::path::Path;
use std::time::{Duration, SystemTime};

use ratatui::style::{Color, Modifier, Style};

pub(super) const PENDING: &str = "...";

pub(super) fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = units[0];
    for next in units.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

pub(super) fn relative_age(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(instant) = modified else {
        return PENDING.to_string();
    };
    let age = now.duration_since(instant).unwrap_or(Duration::ZERO);
    format_age(age)
}

pub(super) fn age_style(modified: Option<SystemTime>, now: SystemTime) -> Style {
    let Some(instant) = modified else {
        return Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM);
    };
    let age = now.duration_since(instant).unwrap_or(Duration::ZERO);
    age_bucket_style(age)
}

fn age_bucket_style(age: Duration) -> Style {
    let seconds = age.as_secs();
    if seconds < 86400 {
        Style::default().fg(Color::Green)
    } else if seconds < 86400 * 30 {
        Style::default().fg(Color::Cyan)
    } else if seconds < 86400 * 180 {
        Style::default().fg(Color::Gray)
    } else {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }
}
fn format_age(age: Duration) -> String {
    let seconds = age.as_secs();
    if seconds < 60 {
        return format!("{seconds}s ago");
    }
    if seconds < 3600 {
        return format!("{}m ago", seconds / 60);
    }
    if seconds < 86400 {
        return format!("{}h ago", seconds / 3600);
    }
    if seconds < 86400 * 30 {
        return format!("{}d ago", seconds / 86400);
    }
    if seconds < 86400 * 365 {
        return format!("{}mo ago", seconds / (86400 * 30));
    }
    format!("{}y ago", seconds / (86400 * 365))
}

pub(super) fn display_path(path: &Path) -> String {
    if let Some(home_path) = dirs::home_dir()
        && let Ok(stripped) = path.strip_prefix(&home_path)
    {
        if stripped.as_os_str().is_empty() {
            return String::from("~");
        }
        return Path::new("~").join(stripped).display().to_string();
    }

    path.display().to_string()
}

pub(super) fn truncate_left(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return String::from("…");
    }
    let tail: String = text.chars().rev().take(width - 1).collect();
    format!("…{}", tail.chars().rev().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_ascii_with_ellipsis() {
        assert_eq!(truncate_left("abcdef", 4), "…def");
    }

    #[test]
    fn truncates_multibyte_without_panicking() {
        assert_eq!(truncate_left("日本語テスト", 3), "…スト");
    }

    #[test]
    fn passes_short_input_through() {
        assert_eq!(truncate_left("hi", 4), "hi");
    }
}
