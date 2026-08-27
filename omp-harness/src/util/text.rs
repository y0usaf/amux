use std::path::Path;

pub fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out = input.chars().take(max_chars - 1).collect::<String>();
    out.push('…');
    out
}

pub fn project_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.components()
                .rev()
                .find_map(|component| match component {
                    std::path::Component::Normal(name) => name
                        .to_str()
                        .filter(|name| !name.trim().is_empty())
                        .map(ToOwned::to_owned),
                    _ => None,
                })
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub fn session_name_from_text(text: &str) -> String {
    title_from_text(text).unwrap_or_default()
}

pub fn is_default_session_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.is_empty()
        || trimmed == "Session"
        || trimmed
            .strip_prefix("Session ")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn title_from_text(text: &str) -> Option<String> {
    let title = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("# AGENTS.md instructions"))?;
    Some(truncate_text(title, 42))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn truncate_text_handles_zero_one_and_unicode_boundaries() {
        assert_eq!(truncate_text("hello", 0), "");
        assert_eq!(truncate_text("hello", 1), "…");
        assert_eq!(truncate_text("é漢字", 2), "é…");
        assert_eq!(truncate_text("é漢", 3), "é漢");
    }

    #[test]
    fn project_name_from_path_falls_back_for_root_and_empty_names() {
        assert_eq!(project_name_from_path(Path::new("/tmp/project")), "project");
        assert_eq!(
            project_name_from_path(Path::new("/tmp/project/")),
            "project"
        );
        assert_eq!(project_name_from_path(Path::new("/")), "/");
        assert_eq!(project_name_from_path(Path::new("   ")), "   ");
    }

    #[test]
    fn session_name_uses_first_non_empty_line_and_skips_agents_header() {
        assert_eq!(
            session_name_from_text("\n\n  My session title  \nbody"),
            "My session title"
        );
        assert_eq!(
            session_name_from_text("# AGENTS.md instructions\nreal title"),
            "real title"
        );
        assert_eq!(session_name_from_text("# AGENTS.md instructions"), "");
    }

    #[test]
    fn session_name_is_truncated_to_42_chars() {
        let title = "12345678901234567890123456789012345678901234567890";
        assert_eq!(
            session_name_from_text(title),
            "12345678901234567890123456789012345678901…"
        );
    }

    #[test]
    fn default_session_name_detection_accepts_only_numeric_suffixes() {
        assert!(is_default_session_name(""));
        assert!(is_default_session_name(" Session "));
        assert!(is_default_session_name("Session 12"));
        assert!(!is_default_session_name("Session 12a"));
        assert!(is_default_session_name("Session "));
        assert!(!is_default_session_name("Other"));
    }
}
