use crate::app::theme::TerminalPalette;

pub(super) fn is_terminal_palette_response(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x1b]10;")
        || bytes.starts_with(b"\x1b]11;")
        || bytes.starts_with(b"\x1b]4;")
}

pub(super) fn apply_terminal_palette_response(
    response: &[u8],
    palette: &mut TerminalPalette,
) -> bool {
    let text = String::from_utf8_lossy(response);
    let mut changed = false;
    for token in text.split(['\x1b', '\x07']) {
        let token = token.trim_matches(['\\']);
        if let Some(rest) = token.strip_prefix("]10;") {
            if let Some(color) = parse_osc_rgb(rest) {
                changed |= palette.fg != color;
                palette.fg = color;
            }
        } else if let Some(rest) = token.strip_prefix("]11;") {
            if let Some(color) = parse_osc_rgb(rest) {
                changed |= palette.bg != color;
                palette.bg = color;
            }
        } else if let Some(rest) = token.strip_prefix("]4;") {
            let mut parts = rest.splitn(2, ';');
            if let (Some(index), Some(color_text)) = (parts.next(), parts.next()) {
                if let (Ok(index), Some(color)) =
                    (index.parse::<usize>(), parse_osc_rgb(color_text))
                {
                    if index < 16 {
                        changed |= palette.ansi[index] != color;
                        palette.ansi[index] = color;
                    }
                }
            }
        }
    }
    changed
}
pub(super) fn parse_terminal_palette_response(response: &[u8]) -> Option<TerminalPalette> {
    let text = String::from_utf8_lossy(response);
    let mut palette = TerminalPalette::fallback();
    let mut got_fg = false;
    let mut got_bg = false;
    let mut got_ansi = [false; 16];

    for token in text.split(['\x1b', '\x07']) {
        let token = token.trim_matches(['\\']);
        if let Some(rest) = token.strip_prefix("]10;") {
            if let Some(color) = parse_osc_rgb(rest) {
                palette.fg = color;
                got_fg = true;
            }
        } else if let Some(rest) = token.strip_prefix("]11;") {
            if let Some(color) = parse_osc_rgb(rest) {
                palette.bg = color;
                got_bg = true;
            }
        } else if let Some(rest) = token.strip_prefix("]4;") {
            let mut parts = rest.splitn(2, ';');
            if let (Some(index), Some(color_text)) = (parts.next(), parts.next()) {
                if let (Ok(index), Some(color)) =
                    (index.parse::<usize>(), parse_osc_rgb(color_text))
                {
                    if index < 16 {
                        palette.ansi[index] = color;
                        got_ansi[index] = true;
                    }
                }
            }
        }
    }

    (got_fg && got_bg && got_ansi.iter().any(|got| *got)).then_some(palette)
}

fn parse_osc_rgb(value: &str) -> Option<crate::render::Color> {
    let rgb = value.split(['\x1b', '\x07', '\\']).next()?.trim();
    let rgb = rgb.strip_prefix("rgb:")?;
    let mut parts = rgb.split('/');
    let r = parse_osc_rgb_component(parts.next()?)?;
    let g = parse_osc_rgb_component(parts.next()?)?;
    let b = parse_osc_rgb_component(parts.next()?)?;
    Some(crate::render::Color::rgb(r, g, b))
}

fn parse_osc_rgb_component(value: &str) -> Option<u8> {
    let digits = value.trim();
    if digits.is_empty() {
        return None;
    }
    let value = u16::from_str_radix(digits, 16).ok()?;
    let max = (1u32 << (digits.len().min(4) * 4)) - 1;
    Some(((u32::from(value) * 255 + max / 2) / max) as u8)
}
