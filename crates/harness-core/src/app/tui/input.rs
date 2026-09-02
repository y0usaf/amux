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

/// Encode an SGR 1006 wheel event in terminal-local (0-based) cell coords.
/// Pi-only: OMP's TUI has no fullscreen mode, so the harness never forwards
/// wheel events into its PTY.
#[cfg(not(feature = "omp"))]
pub(super) fn mouse_wheel_sgr(col: i32, row: i32, up: bool) -> Vec<u8> {
    let button = if up { 64 } else { 65 };
    format!("\x1b[<{};{};{}M", button, col + 1, row + 1).into_bytes()
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
