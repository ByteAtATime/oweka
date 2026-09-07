use std::path::Path;

pub fn analyze(path: &Path) -> Option<&'static str> {
    let original = path.to_string_lossy().replace('\\', "/");
    let home = home_string();
    let cwd = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.to_string_lossy().replace('\\', "/"));
    sensitive(&original, home.as_deref(), cwd.as_deref())
}

fn home_string() -> Option<String> {
    let raw = std::env::var("HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .filter(|value| !value.is_empty())
        });
    raw.map(|value| value.replace('\\', "/"))
}

fn sensitive(original: &str, home: Option<&str>, cwd: Option<&str>) -> Option<&'static str> {
    let is_unc = original.starts_with("//");
    let is_absolute = original.starts_with('/') || has_drive_prefix(original);
    let absolute = if is_unc || is_absolute {
        original.to_string()
    } else {
        match cwd {
            None => original.to_string(),
            Some(cwd) => resolve(original, cwd),
        }
    };
    let normalized_path = normalize(&absolute);
    let normalized_original = normalize(original);
    if let Some(reason) = home_reason(&normalized_path, home) {
        return reason;
    }
    if has_app_package(&normalized_path) {
        return Some("Inside macOS .app package");
    }
    if is_unc && has_hidden_segment(&normalized_original) {
        return Some("Hidden path in network share");
    }
    if normalized_path.contains("/appdata/roaming") {
        return Some("Inside Windows AppData Roaming folder");
    }
    if normalized_path.contains("/appdata/local") {
        if has_cache_segment(&normalized_path) {
            return None;
        }
        return Some("Inside Windows AppData Local folder");
    }
    if has_program_files(&normalized_path) {
        return Some("Inside Program Files folder");
    }
    None
}

fn home_reason(normalized_path: &str, home: Option<&str>) -> Option<Option<&'static str>> {
    let home = home?;
    if home.is_empty() {
        return None;
    }
    let normalized_home = normalize(home);
    let in_home = normalized_path == normalized_home
        || normalized_path.starts_with(format!("{normalized_home}/").as_str());
    if !in_home {
        return None;
    }
    let mut rel = normalized_path[normalized_home.len()..].to_string();
    if rel.starts_with('/') {
        rel.remove(0);
    }
    if rel == ".config" || rel.starts_with(".config/") {
        return Some(Some("Contains user configuration data (~/.config)"));
    }
    if rel == ".local/share" || rel.starts_with(".local/share/") {
        return Some(Some("User data folder (~/.local/share)"));
    }
    if rel == ".cache" || rel.starts_with(".cache/") {
        return Some(Some("System-wide cache directory (~/.cache)"));
    }
    if rel == ".npm" || rel.starts_with(".npm/") || rel == ".pnpm" || rel.starts_with(".pnpm/") {
        return Some(None);
    }
    let top_level = rel.split('/').next().unwrap_or("");
    if top_level.starts_with('.') && top_level != "." && top_level != ".." {
        return Some(Some("Contains unsafe hidden folder"));
    }
    None
}

fn has_drive_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

fn normalize(value: &str) -> String {
    let lowered = value.replace('\\', "/").to_lowercase();
    if lowered.len() >= 3 {
        let bytes = lowered.as_bytes();
        if bytes[0].is_ascii_lowercase() && bytes[1] == b':' && bytes[2] == b'/' {
            return lowered[2..].to_string();
        }
    }
    lowered
}

fn resolve(original: &str, cwd: &str) -> String {
    let joined = format!("{cwd}/{original}");
    let (prefix, rest) = match drive_prefix_of(&joined) {
        Some(prefix) => (prefix.to_string(), joined[prefix.len()..].to_string()),
        None => (String::new(), joined),
    };
    let rooted = rest.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in rest.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            segments.pop();
            continue;
        }
        segments.push(segment);
    }
    if !prefix.is_empty() {
        return format!("{prefix}/{}", segments.join("/"));
    }
    if rooted {
        return format!("/{}", segments.join("/"));
    }
    segments.join("/")
}

fn drive_prefix_of(value: &str) -> Option<&str> {
    if has_drive_prefix(value) || has_drive_prefix(&value.to_lowercase()) {
        return Some(&value[..2]);
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return Some(&value[..2]);
    }
    None
}

fn has_app_package(normalized_path: &str) -> bool {
    let marker = "/applications/";
    let mut rest = normalized_path;
    while let Some(index) = rest.find(marker) {
        let after = &rest[index + marker.len()..];
        let segment_end = after.find('/').unwrap_or(after.len());
        let segment = &after[..segment_end];
        let remainder = &after[segment_end..];
        if !segment.is_empty()
            && segment.ends_with(".app")
            && remainder.starts_with('/')
            && remainder.len() > 1
        {
            return true;
        }
        rest = after;
    }
    false
}

fn has_hidden_segment(normalized_original: &str) -> bool {
    normalized_original
        .split('/')
        .any(|segment| !segment.is_empty() && segment.starts_with('.') && segment != ".")
}

fn has_cache_segment(normalized_path: &str) -> bool {
    normalized_path
        .split('/')
        .any(|segment| segment == ".cache" || segment == ".npm" || segment == ".pnpm")
}

fn has_program_files(normalized_path: &str) -> bool {
    normalized_path.contains("program files/") || normalized_path.contains("program files (x86)/")
}

#[cfg(test)]
mod tests {
    use super::sensitive;

    fn is_flagged(path: &str, home: Option<&str>, cwd: Option<&str>) -> bool {
        sensitive(path, home, cwd).is_some()
    }

    #[test]
    fn home_dir_flags_sensitive_configs_and_caches() {
        let home = Some("/home/user");

        let flagged = [
            "/home/user/.config",
            "/home/user/.config/gh/hosts.yml",
            "/home/user/.local/share",
            "/home/user/.local/share/keyrings",
            "/home/user/.cache",
            "/home/user/.cache/session",
            "/home/user/.ssh",
            "/home/user/.gnupg",
            "/home/user/.bashrc",
        ];

        for path in flagged {
            assert!(
                is_flagged(path, home, None),
                "Expected '{path}' to be flagged as sensitive in home"
            );
        }
    }

    #[test]
    fn home_dir_allows_safe_directories_and_whitelisted_package_caches() {
        let home = Some("/home/user");

        let safe = [
            "/home/user/projects/app",
            "/home/user/Documents/notes.txt",
            "/home/user/.npm",
            "/home/user/.npm/_cacache",
            "/home/user/.pnpm",
            "/home/user/.pnpm/store",
            "/home/user/projects/my-app/.git",
        ];

        for path in safe {
            assert!(
                !is_flagged(path, home, None),
                "Expected '{path}' to be allowed"
            );
        }
    }

    #[test]
    fn relative_traversal_into_sensitive_areas_is_caught() {
        let home = Some("/home/user");
        let cwd = Some("/home/user/projects/web-app");

        let malicious = [
            "../../.config/tokens.json",
            "../../../user/.config",
            "../../.ssh/id_rsa",
            "./../../.bash_history",
        ];

        for path in malicious {
            assert!(
                is_flagged(path, home, cwd),
                "Expected traversal '{path}' from '{cwd:?}' to be caught"
            );
        }
    }

    #[test]
    fn benign_relative_navigation_remains_safe() {
        let home = Some("/home/user");
        let cwd = Some("/home/user/projects/web-app");

        let safe = [
            ".",
            "..",
            "../api-service",
            "./node_modules",
            "../../projects/other-app",
        ];

        for path in safe {
            assert!(
                !is_flagged(path, home, cwd),
                "Expected relative path '{path}' to pass"
            );
        }
    }

    #[test]
    fn macos_app_bundle_internals_are_flagged() {
        assert!(is_flagged(
            "/Applications/Slack.app/Contents/MacOS/Slack",
            None,
            None
        ));
        assert!(is_flagged(
            "/Users/user/Applications/App.app/Contents/Info.plist",
            None,
            None
        ));
        assert!(!is_flagged("/Applications/Slack.app", None, None));
    }

    #[test]
    fn windows_system_directories_and_appdata() {
        assert!(is_flagged("C:/Program Files/Vendor/App", None, None));
        assert!(is_flagged(
            "c:/program files (x86)/common files",
            None,
            None
        ));

        assert!(is_flagged(
            "C:/Users/User/AppData/Roaming/Discord",
            None,
            None
        ));

        assert!(is_flagged(
            "C:/Users/User/AppData/Local/Microsoft",
            None,
            None
        ));
        assert!(!is_flagged(
            "C:/Users/User/AppData/Local/.cache",
            None,
            None
        ));
        assert!(!is_flagged("C:/Users/User/AppData/Local/.npm", None, None));
    }

    #[test]
    fn windows_unc_shares() {
        assert!(is_flagged("//server/share/.hidden_dir/file", None, None));
        assert!(!is_flagged("//server/share/public/file.txt", None, None));
    }

    #[test]
    fn handles_windows_backslashes_identically() {
        let home = Some("C:/Users/User");
        assert!(is_flagged(r"C:\Users\User\.config\foo", home, None));
        assert!(is_flagged(r"C:\Program Files\App", None, None));
        assert!(!is_flagged(r"C:\Users\User\Projects\App", home, None));
    }
}
