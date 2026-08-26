use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use uuid::Uuid;

const SUPPORTED_IMAGE_MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];
const COMMAND_LIST_TIMEOUT: Duration = Duration::from_millis(1000);
const COMMAND_READ_TIMEOUT: Duration = Duration::from_millis(3000);
const COMMAND_POWERSHELL_TIMEOUT: Duration = Duration::from_millis(5000);
const COMMAND_MAX_BYTES: usize = 50 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ClipboardImage {
    pub(super) bytes: Vec<u8>,
    pub(super) mime_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandOutput {
    ok: bool,
    stdout: Vec<u8>,
}

pub(super) fn clipboard_image_path() -> Result<Option<PathBuf>, String> {
    let Some(image) = read_clipboard_image()? else {
        return Ok(None);
    };
    write_clipboard_image_to_temp_file(&image).map(Some)
}

pub(super) fn clipboard_image_path_from_arboard(
    clipboard: &mut Clipboard,
) -> Result<Option<PathBuf>, String> {
    let Some(image) = read_clipboard_image_from_arboard(clipboard) else {
        return Ok(None);
    };
    write_clipboard_image_to_temp_file(&image).map(Some)
}

fn read_clipboard_image() -> Result<Option<ClipboardImage>, String> {
    if env::var_os("TERMUX_VERSION").is_some() {
        return Ok(None);
    }

    let wayland = is_wayland_session();
    let wsl = is_wsl();

    if wayland || wsl {
        if let Some(image) = read_clipboard_image_via_wl_paste()? {
            return Ok(Some(image));
        }
        if let Some(image) = read_clipboard_image_via_xclip()? {
            return Ok(Some(image));
        }
    }

    if wsl {
        if let Some(image) = read_clipboard_image_via_powershell()? {
            return Ok(Some(image));
        }
    }

    if let Some(image) = read_clipboard_image_via_arboard() {
        return Ok(Some(image));
    }

    if !wayland {
        if let Some(image) = read_clipboard_image_via_xclip()? {
            return Ok(Some(image));
        }
    }

    Ok(None)
}

fn write_clipboard_image_to_temp_file(image: &ClipboardImage) -> Result<PathBuf, String> {
    let extension = extension_for_image_mime_type(&image.mime_type).unwrap_or("png");
    let path = env::temp_dir().join(format!(
        "omp-harness-clipboard-{}.{}",
        Uuid::new_v4(),
        extension
    ));
    fs::write(&path, &image.bytes)
        .map_err(|error| format!("write clipboard image {}: {error}", path.display()))?;
    Ok(path)
}

fn read_clipboard_image_via_arboard() -> Option<ClipboardImage> {
    let mut clipboard = Clipboard::new().ok()?;
    read_clipboard_image_from_arboard(&mut clipboard)
}

fn read_clipboard_image_from_arboard(clipboard: &mut Clipboard) -> Option<ClipboardImage> {
    let image = clipboard.get_image().ok()?;
    let bytes = encode_png_rgba(image.width, image.height, &image.bytes).ok()?;
    Some(ClipboardImage {
        bytes,
        mime_type: "image/png".to_string(),
    })
}

fn read_clipboard_image_via_wl_paste() -> Result<Option<ClipboardImage>, String> {
    let list = run_command(
        "wl-paste",
        &["--list-types"],
        COMMAND_LIST_TIMEOUT,
        COMMAND_MAX_BYTES,
        None,
    );
    if !list.ok {
        return Ok(None);
    }
    let types = parse_mime_type_lines(&list.stdout);
    let Some(selected_type) = select_preferred_image_mime_type(&types) else {
        return Ok(None);
    };

    let data = run_command(
        "wl-paste",
        &["--type", selected_type.as_str(), "--no-newline"],
        COMMAND_READ_TIMEOUT,
        COMMAND_MAX_BYTES,
        None,
    );
    if !data.ok || data.stdout.is_empty() {
        return Ok(None);
    }

    Ok(Some(ClipboardImage {
        bytes: data.stdout,
        mime_type: base_mime_type(&selected_type),
    }))
}

fn read_clipboard_image_via_xclip() -> Result<Option<ClipboardImage>, String> {
    let targets = run_command(
        "xclip",
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
        COMMAND_LIST_TIMEOUT,
        COMMAND_MAX_BYTES,
        None,
    );
    let candidate_types = if targets.ok {
        parse_mime_type_lines(&targets.stdout)
    } else {
        Vec::new()
    };
    let preferred = select_preferred_image_mime_type(&candidate_types);

    let mut try_types = Vec::new();
    if let Some(preferred) = preferred {
        try_types.push(preferred);
    }
    for mime_type in SUPPORTED_IMAGE_MIME_TYPES {
        if !try_types
            .iter()
            .any(|existing| base_mime_type(existing) == *mime_type)
        {
            try_types.push((*mime_type).to_string());
        }
    }

    for mime_type in try_types {
        let data = run_command(
            "xclip",
            &["-selection", "clipboard", "-t", mime_type.as_str(), "-o"],
            COMMAND_READ_TIMEOUT,
            COMMAND_MAX_BYTES,
            None,
        );
        if data.ok && !data.stdout.is_empty() {
            return Ok(Some(ClipboardImage {
                bytes: data.stdout,
                mime_type: base_mime_type(&mime_type),
            }));
        }
    }

    Ok(None)
}

fn read_clipboard_image_via_powershell() -> Result<Option<ClipboardImage>, String> {
    let path = env::temp_dir().join(format!("omp-harness-wsl-clip-{}.png", Uuid::new_v4()));
    let Some(path_str) = path.to_str() else {
        return Ok(None);
    };

    let win_path_result = run_command(
        "wslpath",
        &["-w", path_str],
        COMMAND_LIST_TIMEOUT,
        COMMAND_MAX_BYTES,
        None,
    );
    if !win_path_result.ok {
        return Ok(None);
    }
    let win_path = String::from_utf8_lossy(&win_path_result.stdout)
        .trim()
        .to_string();
    if win_path.is_empty() {
        return Ok(None);
    }

    let script = [
        "Add-Type -AssemblyName System.Windows.Forms",
        "Add-Type -AssemblyName System.Drawing",
        "$path = $env:OMP_HARNESS_WSL_CLIPBOARD_IMAGE_PATH",
        "$img = [System.Windows.Forms.Clipboard]::GetImage()",
        "if ($img) { $img.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); Write-Output 'ok' } else { Write-Output 'empty' }",
    ]
    .join("; ");
    let envs = [("OMP_HARNESS_WSL_CLIPBOARD_IMAGE_PATH", win_path.as_str())];
    let result = run_command(
        "powershell.exe",
        &["-NoProfile", "-Command", script.as_str()],
        COMMAND_POWERSHELL_TIMEOUT,
        COMMAND_MAX_BYTES,
        Some(&envs),
    );
    if !result.ok || String::from_utf8_lossy(&result.stdout).trim() != "ok" {
        let _ = fs::remove_file(&path);
        return Ok(None);
    }

    let bytes = fs::read(&path)
        .map_err(|error| format!("read WSL clipboard image {}: {error}", path.display()))?;
    let _ = fs::remove_file(&path);
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(ClipboardImage {
        bytes,
        mime_type: "image/png".to_string(),
    }))
}

fn run_command(
    command: &str,
    args: &[&str],
    timeout: Duration,
    max_bytes: usize,
    envs: Option<&[(&str, &str)]>,
) -> CommandOutput {
    let mut command = Command::new(command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(envs) = envs {
        command.envs(envs.iter().copied());
    }

    let Ok(mut child) = command.spawn() else {
        return CommandOutput {
            ok: false,
            stdout: Vec::new(),
        };
    };

    let stdout = child.stdout.take();
    let reader = stdout.map(|stdout| thread::spawn(move || read_limited(stdout, max_bytes + 1)));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                break None;
            }
        }
    };

    let stdout = reader
        .and_then(|reader| reader.join().ok())
        .and_then(Result::ok)
        .unwrap_or_default();
    let exceeded_max = stdout.len() > max_bytes;

    CommandOutput {
        ok: !timed_out && !exceeded_max && status.is_some_and(|status| status.success()),
        stdout,
    }
}

fn read_limited(mut reader: impl Read, max_bytes: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    while output.len() < max_bytes {
        let remaining = max_bytes - output.len();
        let read_len = remaining.min(buffer.len());
        let bytes_read = reader.read(&mut buffer[..read_len])?;
        if bytes_read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..bytes_read]);
    }
    Ok(output)
}

fn parse_mime_type_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn select_preferred_image_mime_type(mime_types: &[String]) -> Option<String> {
    let normalized: Vec<(String, String)> = mime_types
        .iter()
        .map(|raw| (raw.clone(), base_mime_type(raw)))
        .collect();

    for preferred in SUPPORTED_IMAGE_MIME_TYPES {
        if let Some((raw, _)) = normalized.iter().find(|(_, base)| base == preferred) {
            return Some(raw.clone());
        }
    }

    None
}

fn base_mime_type(mime_type: &str) -> String {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase()
}

fn extension_for_image_mime_type(mime_type: &str) -> Option<&'static str> {
    match base_mime_type(mime_type).as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn is_wayland_session() -> bool {
    env::var_os("WAYLAND_DISPLAY").is_some()
        || env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "wayland")
}

fn is_wsl() -> bool {
    if env::var_os("WSL_DISTRO_NAME").is_some() || env::var_os("WSLENV").is_some() {
        return true;
    }
    fs::read_to_string("/proc/version")
        .map(|version| version.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn encode_png_rgba(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("clipboard image dimensions are empty".to_string());
    }
    let width_u32 =
        u32::try_from(width).map_err(|_| "clipboard image width exceeds PNG limit".to_string())?;
    let height_u32 = u32::try_from(height)
        .map_err(|_| "clipboard image height exceeds PNG limit".to_string())?;

    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "clipboard image is too large".to_string())?;
    if rgba.len() != expected_len {
        return Err(format!(
            "clipboard image RGBA length mismatch: got {}, expected {expected_len}",
            rgba.len()
        ));
    }

    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(rgba, width_u32, height_u32, ColorType::Rgba8.into())
        .map_err(|error| format!("encode clipboard image as PNG: {error}"))?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_mime_type_uses_supported_order_and_preserves_raw_value() {
        let types = vec![
            "text/plain".to_string(),
            "image/webp;charset=utf-8".to_string(),
            "image/png;charset=utf-8".to_string(),
        ];

        assert_eq!(
            select_preferred_image_mime_type(&types).as_deref(),
            Some("image/png;charset=utf-8")
        );
    }

    #[test]
    fn preferred_mime_type_ignores_unsupported_images() {
        let types = vec!["text/plain".to_string(), "image/bmp".to_string()];
        assert_eq!(select_preferred_image_mime_type(&types), None);
    }

    #[test]
    fn png_encoder_writes_decodable_rgba_image() {
        let png = encode_png_rgba(1, 1, &[255, 0, 0, 255]).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (1, 1));
        assert_eq!(decoded.as_raw(), &[255, 0, 0, 255]);
    }

    #[test]
    fn png_encoder_rejects_wrong_rgba_length() {
        assert!(encode_png_rgba(1, 1, &[255, 0, 0]).is_err());
    }
}
