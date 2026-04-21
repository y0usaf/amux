use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

pub(crate) fn handle_key_input(
    event: &KeyEvent,
    modifiers: ModifiersState,
    rows: u16,
    screen: &vt100::Screen,
) -> KeyInput {
    if event.state != ElementState::Pressed {
        return KeyInput::Ignored;
    }

    if modifiers.shift_key() {
        if let Key::Named(named) = &event.logical_key {
            let page = i32::from(rows.saturating_sub(2).max(1));
            let scroll = match named {
                NamedKey::PageUp => Some(page),
                NamedKey::PageDown => Some(-page),
                NamedKey::Home => Some(i32::MAX),
                NamedKey::End => Some(i32::MIN),
                _ => None,
            };
            if let Some(scroll) = scroll {
                return KeyInput::Scroll(scroll);
            }
        }
    }

    translate_key(event, modifiers, screen)
        .map(KeyInput::Bytes)
        .unwrap_or(KeyInput::Ignored)
}

pub(crate) enum KeyInput {
    Ignored,
    Scroll(i32),
    Bytes(Vec<u8>),
}

fn translate_key(
    event: &KeyEvent,
    modifiers: ModifiersState,
    screen: &vt100::Screen,
) -> Option<Vec<u8>> {
    let ctrl = modifiers.control_key();
    let alt = modifiers.alt_key();
    let platform = modifiers.super_key();
    if platform && !ctrl {
        return None;
    }

    let mut bytes = match &event.logical_key {
        Key::Named(NamedKey::Enter) => vec![b'\r'],
        Key::Named(NamedKey::Backspace) => vec![0x7f],
        Key::Named(NamedKey::Tab) if modifiers.shift_key() => b"\x1b[Z".to_vec(),
        Key::Named(NamedKey::Tab) => vec![b'\t'],
        Key::Named(NamedKey::Escape) => vec![0x1b],
        Key::Named(NamedKey::ArrowUp) => cursor_key_bytes(screen.application_cursor(), b'A'),
        Key::Named(NamedKey::ArrowDown) => cursor_key_bytes(screen.application_cursor(), b'B'),
        Key::Named(NamedKey::ArrowRight) => cursor_key_bytes(screen.application_cursor(), b'C'),
        Key::Named(NamedKey::ArrowLeft) => cursor_key_bytes(screen.application_cursor(), b'D'),
        Key::Named(NamedKey::Home) => {
            if screen.application_cursor() {
                b"\x1bOH".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }
        }
        Key::Named(NamedKey::End) => {
            if screen.application_cursor() {
                b"\x1bOF".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }
        }
        Key::Named(NamedKey::Delete) => b"\x1b[3~".to_vec(),
        Key::Named(NamedKey::PageUp) => b"\x1b[5~".to_vec(),
        Key::Named(NamedKey::PageDown) => b"\x1b[6~".to_vec(),
        Key::Named(NamedKey::Space) => vec![b' '],
        Key::Character(text) => {
            let text = text.as_str();
            if ctrl {
                control_bytes(text)?
            } else {
                if text.contains('\u{7f}') || text == "\r" || text == "\n" {
                    return None;
                }
                text.as_bytes().to_vec()
            }
        }
        _ => return None,
    };

    if alt {
        let mut prefixed = vec![0x1b];
        prefixed.extend(bytes);
        bytes = prefixed;
    }

    Some(bytes)
}

fn control_bytes(key: &str) -> Option<Vec<u8>> {
    let lower = key.to_ascii_lowercase();
    if lower.len() == 1 {
        let byte = lower.as_bytes()[0];
        if byte.is_ascii_lowercase() {
            return Some(vec![byte & 0x1f]);
        }
    }

    let bytes = match lower.as_str() {
        "space" | "2" => vec![0],
        "3" | "[" => vec![0x1b],
        "4" | "\\" => vec![0x1c],
        "5" | "]" => vec![0x1d],
        "6" | "^" => vec![0x1e],
        "7" | "-" | "_" => vec![0x1f],
        "8" => vec![0x7f],
        _ => return None,
    };
    Some(bytes)
}

fn cursor_key_bytes(application_cursor: bool, suffix: u8) -> Vec<u8> {
    if application_cursor {
        vec![0x1b, b'O', suffix]
    } else {
        vec![0x1b, b'[', suffix]
    }
}
