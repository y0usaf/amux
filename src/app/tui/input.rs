use crate::config::{KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};

pub(super) fn key_stroke_for_bytes(bytes: &[u8]) -> Option<KeyStroke> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WheelDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MouseEventKind {
    Wheel(WheelDirection),
    LeftPress,
    LeftDrag,
    LeftRelease,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MouseEvent {
    pub(super) col: i32,
    pub(super) row: i32,
    pub(super) kind: MouseEventKind,
}

pub(super) fn mouse_event_for_bytes(bytes: &[u8]) -> Option<MouseEvent> {
    let text = std::str::from_utf8(bytes).ok()?;
    if !text.starts_with("\x1b[<") || !(text.ends_with('M') || text.ends_with('m')) {
        return None;
    }

    let body = text.strip_prefix("\x1b[<")?;
    let released = body.ends_with('m');
    let body = body.strip_suffix('M').or_else(|| body.strip_suffix('m'))?;
    let mut parts = body.split(';');
    let button = parts.next()?.parse::<u16>().ok()?;
    let col = parts.next()?.parse::<i32>().ok()?.saturating_sub(1);
    let row = parts.next()?.parse::<i32>().ok()?.saturating_sub(1);
    let base = button & !(4 | 8 | 16 | 32);
    let drag = button & 32 != 0;
    let kind = if released {
        MouseEventKind::LeftRelease
    } else {
        match base {
            0 if drag => MouseEventKind::LeftDrag,
            0 => MouseEventKind::LeftPress,
            64 => MouseEventKind::Wheel(WheelDirection::Up),
            65 => MouseEventKind::Wheel(WheelDirection::Down),
            _ => MouseEventKind::Other,
        }
    };
    Some(MouseEvent { col, row, kind })
}

pub(super) fn split_input_chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            if is_control_byte(bytes[index]) {
                chunks.push(vec![bytes[index]]);
                index += 1;
                continue;
            }

            let start = index;
            index += 1;
            while index < bytes.len() && bytes[index] != 0x1b && !is_control_byte(bytes[index]) {
                index += 1;
            }
            chunks.push(bytes[start..index].to_vec());
            continue;
        }

        let end = escape_sequence_end(bytes, index)
            .unwrap_or(index + 1)
            .min(bytes.len());
        chunks.push(bytes[index..end].to_vec());
        index = end;
    }
    chunks
}

fn is_control_byte(byte: u8) -> bool {
    byte < 0x20 || byte == 0x7f
}

fn escape_sequence_end(bytes: &[u8], start: usize) -> Option<usize> {
    let next = *bytes.get(start + 1)?;
    match next {
        b'[' => {
            let mut index = start + 2;
            while index < bytes.len() {
                if (0x40..=0x7e).contains(&bytes[index]) {
                    return Some(index + 1);
                }
                index += 1;
            }
            None
        }
        b']' => {
            let mut index = start + 2;
            while index < bytes.len() {
                if bytes[index] == b'\x07' {
                    return Some(index + 1);
                }
                if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                    return Some(index + 2);
                }
                index += 1;
            }
            None
        }
        b'O' => Some((start + 3).min(bytes.len())),
        _ => Some((start + 2).min(bytes.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_byte_maps_to_ctrl_stroke() {
        assert_eq!(
            key_stroke_for_bytes(&[0x11]),
            Some(char_key(
                'q',
                KeyModifiers {
                    control: true,
                    ..KeyModifiers::default()
                },
            ))
        );
    }

    #[test]
    fn csi_modifier_maps_to_shift_pageup() {
        assert_eq!(
            key_stroke_for_bytes(b"\x1b[5;2~"),
            Some(named_key(
                NamedKeyToken::PageUp,
                KeyModifiers {
                    shift: true,
                    ..KeyModifiers::default()
                },
            ))
        );
    }

    #[test]
    fn sgr_mouse_wheel_is_zero_based() {
        assert_eq!(
            mouse_event_for_bytes(b"\x1b[<64;10;20M"),
            Some(MouseEvent {
                col: 9,
                row: 19,
                kind: MouseEventKind::Wheel(WheelDirection::Up),
            })
        );
    }

    #[test]
    fn split_input_chunks_keeps_escape_sequences_together() {
        assert_eq!(
            split_input_chunks(b"ab\x1b[5;2~\x11cd"),
            vec![
                b"ab".to_vec(),
                b"\x1b[5;2~".to_vec(),
                vec![0x11],
                b"cd".to_vec(),
            ]
        );
    }
}
