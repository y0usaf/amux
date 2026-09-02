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
