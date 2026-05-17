use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::input::split_input_chunks;
use super::keyboard::{ENTER_KITTY_KEYBOARD_MODE, EXIT_KITTY_KEYBOARD_MODE};
use super::TuiEvent;

pub(super) struct RawTerminal {
    saved_stty: String,
}

impl RawTerminal {
    pub(super) fn enter() -> anyhow::Result<Self> {
        let saved_stty = stty_capture(&["-g"])?;
        stty_inherit(&["raw", "-echo", "min", "1", "time", "0"])?;
        let mut stdout = io::stdout();
        write!(
            stdout,
            "\x1b[?1049h{}\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?25l\x1b[2J\x1b[H",
            ENTER_KITTY_KEYBOARD_MODE
        )?;
        stdout.flush()?;
        Ok(Self { saved_stty })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = write!(
            stdout,
            "\x1b[0m{}\x1b[?2004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?25h\x1b[?1049l",
            EXIT_KITTY_KEYBOARD_MODE
        );
        let _ = stdout.flush();
        let _ = stty_inherit(&[self.saved_stty.trim()]);
    }
}

fn stty_capture(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        anyhow::bail!("stty failed: {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn stty_inherit(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("stty failed: {status}");
    }
    Ok(())
}

pub(super) fn request_terminal_palette_query() -> io::Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "\x1b]10;?\x1b\\\x1b]11;?\x1b\\")?;
    for index in 0..16 {
        write!(stdout, "\x1b]4;{index};?\x1b\\")?;
    }
    stdout.flush()
}
pub(super) fn query_terminal_palette_response(timeout: Duration) -> io::Result<Vec<u8>> {
    let _ = stty_inherit(&["raw", "-echo", "min", "0", "time", "1"]);
    request_terminal_palette_query()?;

    let started = Instant::now();
    let mut stdin = io::stdin();
    let mut buf = [0u8; 4096];
    let mut response = Vec::new();
    while started.elapsed() < timeout {
        match stdin.read(&mut buf) {
            Ok(0) => continue,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    let _ = stty_inherit(&["raw", "-echo", "min", "1", "time", "0"]);
    Ok(response)
}

pub(super) fn spawn_stdin_reader(tx: mpsc::Sender<TuiEvent>) {
    let _ = std::thread::Builder::new()
        .name("pi-harness-tui-stdin".into())
        .spawn(move || {
            let mut stdin = io::stdin();
            let mut buf = [0u8; 4096];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(n) => {
                        if send_input_chunks(&tx, &buf[..n]).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });
}

fn send_input_chunks(
    tx: &mpsc::Sender<TuiEvent>,
    bytes: &[u8],
) -> Result<(), mpsc::SendError<TuiEvent>> {
    for chunk in split_input_chunks(bytes) {
        tx.send(TuiEvent::Input(chunk))?;
    }
    Ok(())
}

#[repr(C)]
#[derive(Default)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn ioctl(fd: i32, request: usize, ...) -> i32;
}

const TIOCGWINSZ: usize = 0x5413;

pub(super) fn terminal_size() -> (u16, u16) {
    let mut size = Winsize::default();
    let fd = io::stdout().as_raw_fd();
    let ok = unsafe { ioctl(fd, TIOCGWINSZ, &mut size) } == 0;
    if ok && size.ws_col > 0 && size.ws_row > 0 {
        return (size.ws_col, size.ws_row);
    }

    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(120);
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(40);
    (cols, rows)
}
