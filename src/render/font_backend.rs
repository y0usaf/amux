use anyhow::{anyhow, Context, Result};
use fontdue::{Font, FontSettings};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct FontMatch {
    pub path: PathBuf,
    pub collection_index: u32,
}

#[derive(Debug)]
pub(super) struct LoadedFont {
    pub match_info: FontMatch,
    pub font: Font,
}

pub(super) fn normalize_font_pattern(font_family: Option<&str>) -> String {
    font_family
        .map(str::trim)
        .filter(|family| !family.is_empty())
        .unwrap_or("monospace")
        .to_string()
}

pub(super) fn resolve_primary_font_match(font_pattern: &str) -> Option<(String, FontMatch)> {
    if let Some(font_match) = resolve_fontconfig_match(font_pattern) {
        return Some((font_pattern.to_string(), font_match));
    }

    if font_pattern != "monospace" {
        if let Some(font_match) = resolve_fontconfig_match("monospace") {
            return Some(("monospace".to_string(), font_match));
        }
    }

    find_monospace_font().map(|font_match| ("monospace".to_string(), font_match))
}

pub(super) fn load_font_match(font_match: &FontMatch) -> Result<LoadedFont> {
    let bytes = fs::read(&font_match.path)
        .with_context(|| format!("failed to read font {}", font_match.path.display()))?;
    let settings = FontSettings {
        collection_index: font_match.collection_index,
        ..FontSettings::default()
    };
    let font = Font::from_bytes(bytes, settings).map_err(|e| {
        anyhow!(
            "failed to parse font {}#{}: {e}",
            font_match.path.display(),
            font_match.collection_index,
        )
    })?;
    Ok(LoadedFont {
        match_info: font_match.clone(),
        font,
    })
}

const FONTCONFIG_FORMAT: &str = "%{file}\u{1f}%{index}\n";

fn resolve_fontconfig_match(pattern: &str) -> Option<FontMatch> {
    fontconfig_matches(pattern, false).into_iter().next()
}

pub(super) fn fontconfig_fallback_matches(font_pattern: &str, ch: char) -> Vec<FontMatch> {
    let pattern = format!("{font_pattern}:charset={:x}", ch as u32);
    fontconfig_matches(&pattern, true)
}

fn fontconfig_matches(pattern: &str, sorted: bool) -> Vec<FontMatch> {
    let mut command = Command::new("fc-match");
    if sorted {
        command.arg("-s");
    }
    let output = command
        .arg("-f")
        .arg(FONTCONFIG_FORMAT)
        .arg(pattern)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_fontconfig_match)
        .filter(|font_match| seen.insert(font_match.clone()))
        .collect()
}

fn parse_fontconfig_match(line: &str) -> Option<FontMatch> {
    let (path, collection_index) = line.split_once('\u{1f}')?;
    let path = PathBuf::from(path.trim());
    if !path.is_file() {
        return None;
    }
    Some(FontMatch {
        path,
        collection_index: collection_index.trim().parse().ok().unwrap_or(0),
    })
}

fn find_monospace_font() -> Option<FontMatch> {
    let mut candidates = Vec::new();
    let preferred_files = [
        "JetBrainsMono-Regular.ttf",
        "JetBrainsMonoNerdFont-Regular.ttf",
        "FiraCode-Regular.ttf",
        "DejaVuSansMono.ttf",
        "LiberationMono-Regular.ttf",
    ];

    let font_roots = font_search_roots();
    for root in &font_roots {
        for name in preferred_files {
            candidates.push(root.join(name));
            candidates.push(root.join("truetype").join(name));
            candidates.push(root.join("opentype").join(name));
            candidates.push(root.join("TTF").join(name));
        }
    }

    for path in candidates {
        if path.is_file() {
            return Some(FontMatch {
                path,
                collection_index: 0,
            });
        }
    }

    for root in font_roots {
        if let Some(font_match) = find_any_font_recursive(&root) {
            return Some(font_match);
        }
    }

    None
}

pub(super) fn find_font_with_glyph(ch: char) -> Option<FontMatch> {
    for root in font_search_roots() {
        if let Some(font_match) = find_font_with_glyph_recursive(&root, ch) {
            return Some(font_match);
        }
    }
    None
}

fn font_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/run/current-system/sw/share/X11/fonts"),
        PathBuf::from("/run/current-system/sw/share/fonts"),
        PathBuf::from("/etc/profiles/per-user/root/share/fonts"),
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];

    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local/share/fonts"));
        roots.push(home.join(".fonts"));
        roots.push(home.join(".nix-profile/share/fonts"));
    }

    roots
}

fn find_any_font_recursive(root: &Path) -> Option<FontMatch> {
    let entries = fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if is_supported_font_file(&path) {
                return Some(FontMatch {
                    path,
                    collection_index: 0,
                });
            }
        } else if path.is_dir() {
            dirs.push(path);
        }
    }
    for dir in dirs {
        if let Some(font_match) = find_any_font_recursive(&dir) {
            return Some(font_match);
        }
    }
    None
}

fn find_font_with_glyph_recursive(root: &Path, ch: char) -> Option<FontMatch> {
    let entries = fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if is_supported_font_file(&path) && font_file_has_glyph(&path, ch) {
                return Some(FontMatch {
                    path,
                    collection_index: 0,
                });
            }
        } else if path.is_dir() {
            dirs.push(path);
        }
    }
    for dir in dirs {
        if let Some(font_match) = find_font_with_glyph_recursive(&dir, ch) {
            return Some(font_match);
        }
    }
    None
}

fn is_supported_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("ttf")
                || ext.eq_ignore_ascii_case("otf")
                || ext.eq_ignore_ascii_case("ttc")
                || ext.eq_ignore_ascii_case("otc")
        })
}

fn font_file_has_glyph(path: &Path, ch: char) -> bool {
    load_font_match(&FontMatch {
        path: path.to_path_buf(),
        collection_index: 0,
    })
    .map(|loaded| loaded.font.has_glyph(ch))
    .unwrap_or(false)
}
