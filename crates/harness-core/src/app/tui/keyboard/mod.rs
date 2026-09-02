use crate::config::{KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};

pub(super) const ENTER_KITTY_KEYBOARD_MODE: &str = "\x1b[>1u";
pub(super) const EXIT_KITTY_KEYBOARD_MODE: &str = "\x1b[<1u";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KeyInput {
    pub(super) stroke: KeyStroke,
    /// Bytes to forward to the embedded Pi PTY if this key is not consumed by
    /// the harness UI. For Kitty keyboard protocol input, preserve the original
    /// enhanced sequence so Pi can still see modified keys such as Shift+Enter.
    pub(super) terminal_bytes: Vec<u8>,
}

pub(super) fn decode_key_input(bytes: &[u8]) -> Option<KeyInput> {
    if let Some(stroke) = key_stroke_for_kitty_csi_u(bytes) {
        return Some(KeyInput {
            stroke,
            terminal_bytes: bytes.to_vec(),
        });
    }

    let stroke = legacy_key_stroke_for_bytes(bytes)?;
    Some(KeyInput {
        stroke,
        terminal_bytes: bytes.to_vec(),
    })
}

pub(super) fn key_stroke_for_bytes(bytes: &[u8]) -> Option<KeyStroke> {
    decode_key_input(bytes).map(|input| input.stroke)
}

pub(super) fn is_ctrl_char(bytes: &[u8], target: char) -> bool {
    let target = target.to_ascii_lowercase().to_string();
    key_stroke_for_bytes(bytes).is_some_and(|stroke| {
        stroke.modifiers.control
            && !stroke.modifiers.shift
            && !stroke.modifiers.alt
            && matches!(stroke.key, KeyToken::Character(ref key) if key == &target)
    })
}

/// Ctrl+? — terminals in Kitty disambiguate mode report the unshifted base key,
/// so accept Ctrl+Shift+'/' as well as a literal shifted '?'.
pub(super) fn is_ctrl_question(bytes: &[u8]) -> bool {
    key_stroke_for_bytes(bytes).is_some_and(|stroke| {
        stroke.modifiers.control
            && !stroke.modifiers.alt
            && match stroke.key {
                KeyToken::Character(ref key) => {
                    key == "?" || (stroke.modifiers.shift && key == "/")
                }
                _ => false,
            }
    })
}

fn legacy_key_stroke_for_bytes(bytes: &[u8]) -> Option<KeyStroke> {
    if let Some(stroke) = key_stroke_for_escape_sequence(bytes) {
        return Some(stroke);
    }
    if bytes.len() != 1 {
        return None;
    }

    let byte = bytes[0];
    match byte {
        b'\r' => Some(named_key(NamedKeyToken::Enter, KeyModifiers::default())),
        b'\n' => Some(char_key(
            'j',
            KeyModifiers {
                control: true,
                ..KeyModifiers::default()
            },
        )),
        b'\t' => Some(named_key(NamedKeyToken::Tab, KeyModifiers::default())),
        // Only DEL (^?, 0x7f) is the physical Backspace key in legacy byte mode.
        // BS (^H, 0x08) is genuinely Ctrl+H: let it fall through to the ctrl-letter
        // range below so keybinds like "ctrl+h" keep working on terminals without
        // the kitty keyboard protocol.
        0x7f => Some(named_key(NamedKeyToken::Backspace, KeyModifiers::default())),
        0x1b => Some(named_key(NamedKeyToken::Escape, KeyModifiers::default())),
        0x01..=0x1a => {
            let ch = (b'a' + byte - 1) as char;
            Some(char_key(
                ch,
                KeyModifiers {
                    control: true,
                    ..KeyModifiers::default()
                },
            ))
        }
        b' ' => Some(named_key(NamedKeyToken::Space, KeyModifiers::default())),
        0x21..=0x7e => {
            let ch = byte as char;
            if ch.is_ascii_uppercase() {
                Some(char_key(
                    ch.to_ascii_lowercase(),
                    KeyModifiers {
                        shift: true,
                        ..KeyModifiers::default()
                    },
                ))
            } else {
                Some(char_key(ch, KeyModifiers::default()))
            }
        }
        _ => None,
    }
}

fn key_stroke_for_kitty_csi_u(bytes: &[u8]) -> Option<KeyStroke> {
    let text = std::str::from_utf8(bytes).ok()?;
    let body = text.strip_prefix("\x1b[")?;
    let params = body.strip_suffix('u')?;
    let mut parts = params.split(';');
    let key_codes = parse_kitty_numbers(parts.next()?)?;
    let mut modifiers = parts
        .next()
        .and_then(parse_kitty_number)
        .and_then(|value| u16::try_from(value).ok())
        .and_then(modifiers_from_csi_param)
        .unwrap_or_default();
    if key_codes
        .iter()
        .filter_map(|code| char::from_u32(*code))
        .any(|ch| ch.is_ascii_uppercase())
    {
        modifiers.shift = true;
    }

    let key = kitty_key_token_from_candidates(&key_codes, modifiers)?;
    Some(KeyStroke { modifiers, key })
}

fn parse_kitty_number(text: &str) -> Option<u32> {
    text.parse::<u32>().ok()
}

fn parse_kitty_numbers(text: &str) -> Option<Vec<u32>> {
    // Kitty can include alternate key/event metadata separated by ':'. Some
    // terminals/multiplexers place the control-character text code first and
    // the physical key's printable code later (for example Ctrl+J as 10:106),
    // so keep the full candidate list instead of only the first number.
    let numbers: Option<Vec<u32>> = text.split(':').map(parse_kitty_number).collect();
    numbers.filter(|values| !values.is_empty())
}

fn kitty_key_token_from_candidates(key_codes: &[u32], modifiers: KeyModifiers) -> Option<KeyToken> {
    let primary = *key_codes.first()?;
    if modifiers.control && is_kitty_control_code(primary) {
        if let Some(key) = key_codes[1..]
            .iter()
            .find_map(|code| match kitty_key_token(*code) {
                Some(KeyToken::Character(text)) => Some(KeyToken::Character(text)),
                _ => None,
            })
        {
            return Some(key);
        }
    }

    key_codes.iter().find_map(|code| kitty_key_token(*code))
}

fn is_kitty_control_code(key_code: u32) -> bool {
    key_code < 0x20 || key_code == 0x7f
}

fn kitty_key_token(key_code: u32) -> Option<KeyToken> {
    let named = match key_code {
        9 | 57346 => Some(NamedKeyToken::Tab),
        13 | 57345 => Some(NamedKeyToken::Enter),
        27 | 57344 => Some(NamedKeyToken::Escape),
        32 => Some(NamedKeyToken::Space),
        127 | 57347 => Some(NamedKeyToken::Backspace),
        57348 => Some(NamedKeyToken::Insert),
        57349 => Some(NamedKeyToken::Delete),
        57350 => Some(NamedKeyToken::Left),
        57351 => Some(NamedKeyToken::Right),
        57352 => Some(NamedKeyToken::Up),
        57353 => Some(NamedKeyToken::Down),
        57354 => Some(NamedKeyToken::PageUp),
        57355 => Some(NamedKeyToken::PageDown),
        57356 => Some(NamedKeyToken::Home),
        57357 => Some(NamedKeyToken::End),
        _ => None,
    };
    if let Some(named) = named {
        return Some(KeyToken::Named(named));
    }

    let ch = char::from_u32(key_code)?;
    if ch.is_control() {
        return None;
    }
    let canonical = if ch.is_ascii_uppercase() {
        ch.to_ascii_lowercase()
    } else {
        ch
    };
    Some(KeyToken::Character(canonical.to_string()))
}

fn key_stroke_for_escape_sequence(bytes: &[u8]) -> Option<KeyStroke> {
    if let Some(stroke) = key_stroke_for_csi_sequence(bytes) {
        return Some(stroke);
    }

    match bytes {
        b"\x1bOA" => Some(named_key(NamedKeyToken::Up, KeyModifiers::default())),
        b"\x1bOB" => Some(named_key(NamedKeyToken::Down, KeyModifiers::default())),
        b"\x1bOC" => Some(named_key(NamedKeyToken::Right, KeyModifiers::default())),
        b"\x1bOD" => Some(named_key(NamedKeyToken::Left, KeyModifiers::default())),
        b"\x1bOH" => Some(named_key(NamedKeyToken::Home, KeyModifiers::default())),
        b"\x1bOF" => Some(named_key(NamedKeyToken::End, KeyModifiers::default())),
        _ => None,
    }
}

fn key_stroke_for_csi_sequence(bytes: &[u8]) -> Option<KeyStroke> {
    let text = std::str::from_utf8(bytes).ok()?;
    let body = text.strip_prefix("\x1b[")?;
    let final_char = body.chars().last()?;
    let params = &body[..body.len().saturating_sub(final_char.len_utf8())];

    let (key, modifier) = match final_char {
        'A' => (NamedKeyToken::Up, modifier_param(params)),
        'B' => (NamedKeyToken::Down, modifier_param(params)),
        'C' => (NamedKeyToken::Right, modifier_param(params)),
        'D' => (NamedKeyToken::Left, modifier_param(params)),
        'H' => (NamedKeyToken::Home, modifier_param(params)),
        'F' => (NamedKeyToken::End, modifier_param(params)),
        '~' => key_and_modifier_for_tilde(params)?,
        _ => return None,
    };

    Some(named_key(key, modifier.unwrap_or_default()))
}

fn key_and_modifier_for_tilde(params: &str) -> Option<(NamedKeyToken, Option<KeyModifiers>)> {
    let mut parts = params.split(';');
    let key_code = parts.next()?.parse::<u16>().ok()?;
    let modifier = parts.next().and_then(|value| value.parse::<u16>().ok());
    let key = match key_code {
        1 | 7 => NamedKeyToken::Home,
        2 => NamedKeyToken::Insert,
        3 => NamedKeyToken::Delete,
        4 | 8 => NamedKeyToken::End,
        5 => NamedKeyToken::PageUp,
        6 => NamedKeyToken::PageDown,
        _ => return None,
    };
    Some((key, modifier.and_then(modifiers_from_csi_param)))
}

fn modifier_param(params: &str) -> Option<KeyModifiers> {
    let mut parts = params.split(';');
    let first = parts.next();
    let second = parts.next();
    match (first, second) {
        (Some(value), None) => value.parse::<u16>().ok().and_then(modifiers_from_csi_param),
        (_, Some(value)) => value.parse::<u16>().ok().and_then(modifiers_from_csi_param),
        _ => None,
    }
}

fn modifiers_from_csi_param(param: u16) -> Option<KeyModifiers> {
    if param <= 1 {
        return Some(KeyModifiers::default());
    }
    let bits = param - 1;
    Some(KeyModifiers {
        shift: bits & 1 != 0,
        alt: bits & 2 != 0,
        control: bits & 4 != 0,
    })
}

fn named_key(key: NamedKeyToken, modifiers: KeyModifiers) -> KeyStroke {
    KeyStroke {
        modifiers,
        key: KeyToken::Named(key),
    }
}

fn char_key(key: char, modifiers: KeyModifiers) -> KeyStroke {
    KeyStroke {
        modifiers,
        key: KeyToken::Character(key.to_string()),
    }
}
