//! Clipboard image detection and encoding for image paste support.
//!
//! Platform-specific clipboard access:
//! - macOS: `osascript` (AppleScript)
//! - Linux: `xclip` / `wl-paste`
//! - Windows: PowerShell `Get-Clipboard`

use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine as _;

/// A pasted image waiting to be sent with the next message.
#[derive(Debug, Clone)]
pub struct PastedImage {
    /// Temp file path containing the PNG data.
    pub path: PathBuf,
    /// Display label (e.g. "clipboard.png").
    pub label: String,
    /// Image dimensions (width, height) if known.
    pub dimensions: Option<(u32, u32)>,
}

/// Try to read an image from the system clipboard.
///
/// Returns `Some(PastedImage)` if the clipboard contains image data,
/// `None` otherwise. The image is saved to a temp file.
pub fn read_clipboard_image() -> Option<PastedImage> {
    #[cfg(target_os = "macos")]
    {
        read_image_macos()
    }
    #[cfg(target_os = "windows")]
    {
        read_image_windows()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        read_image_linux()
    }
}

/// Base64-encode a PNG file for the Anthropic API.
pub fn encode_image_base64(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    if data.is_empty() {
        return None;
    }
    Some(base64::engine::general_purpose::STANDARD.encode(&data))
}

/// Extract width and height from a PNG file's IHDR chunk.
///
/// PNG layout: 8-byte signature, then IHDR chunk with width (bytes 16-19)
/// and height (bytes 20-23) as big-endian u32.
pub fn png_dimensions(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 24 {
        return None;
    }
    // Verify PNG signature.
    if &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    Some((w, h))
}

/// Open an image file in the system's default viewer.
///
/// Uses platform-specific commands: `open` (macOS), `xdg-open` (Linux),
/// `explorer` (Windows). Failures are silently ignored.
pub fn open_file_in_viewer(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer").arg(path).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

/// Generate a unique temp file path for a pasted PNG.
fn make_temp_png() -> PathBuf {
    let tmp_dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    tmp_dir.join(format!("oxicode-paste-{nanos}.png"))
}

// ---------------------------------------------------------------------------
// macOS: use osascript to check/extract clipboard PNG data.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn read_image_macos() -> Option<PastedImage> {
    // Check if clipboard has PNG data.
    let check = Command::new("osascript")
        .args(["-e", "the clipboard as «class PNGf»"])
        .output()
        .ok()?;
    if !check.status.success() {
        return None;
    }

    let path = make_temp_png();
    let path_str = path.to_string_lossy();

    // Write PNG bytes to temp file via AppleScript.
    let script = format!(
        "set pngData to (the clipboard as «class PNGf»)\n\
         set fp to open for access POSIX file \"{path_str}\" with write permission\n\
         write pngData to fp\n\
         close access fp"
    );
    let write = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !write.status.success() {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    // Verify file exists and has content.
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() == 0 {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    let dimensions = png_dimensions(&path);
    Some(PastedImage {
        path,
        label: "clipboard.png".to_string(),
        dimensions,
    })
}

// ---------------------------------------------------------------------------
// Linux: use xclip or wl-paste.
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_image_linux() -> Option<PastedImage> {
    if !has_linux_clipboard_image() {
        return None;
    }

    let path = make_temp_png();
    if !try_save_linux_image(&path) {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() == 0 {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    let dimensions = png_dimensions(&path);
    Some(PastedImage {
        path,
        label: "clipboard.png".to_string(),
        dimensions,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn has_linux_clipboard_image() -> bool {
    // Try xclip first.
    if let Ok(out) = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
        .output()
    {
        let targets = String::from_utf8_lossy(&out.stdout);
        if targets.contains("image/") {
            return true;
        }
    }
    // Try wl-paste (Wayland).
    if let Ok(out) = Command::new("wl-paste").args(["--list-types"]).output() {
        let types = String::from_utf8_lossy(&out.stdout);
        if types.contains("image/") {
            return true;
        }
    }
    false
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn try_save_linux_image(path: &Path) -> bool {
    // Try xclip.
    if let Ok(out) = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-o"])
        .output()
    {
        if out.status.success() && !out.stdout.is_empty() {
            if std::fs::write(path, &out.stdout).is_ok() {
                return true;
            }
        }
    }
    // Fallback: wl-paste.
    if let Ok(out) = Command::new("wl-paste")
        .args(["--type", "image/png"])
        .output()
    {
        if out.status.success() && !out.stdout.is_empty() {
            if std::fs::write(path, &out.stdout).is_ok() {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Windows: use PowerShell.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn read_image_windows() -> Option<PastedImage> {
    // Check if clipboard has an image.
    let check = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "if ((Get-Clipboard -Format Image) -ne $null) { 'yes' } else { 'no' }",
        ])
        .output()
        .ok()?;
    let answer = String::from_utf8_lossy(&check.stdout).trim().to_string();
    if answer != "yes" {
        return None;
    }

    let path = make_temp_png();
    let path_str = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$img = Get-Clipboard -Format Image; $img.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png)",
        path_str
    );
    let save = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    if !save.status.success() {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() == 0 {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    let dimensions = png_dimensions(&path);
    Some(PastedImage {
        path,
        label: "clipboard.png".to_string(),
        dimensions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn png_dimensions_valid() {
        let path = make_temp_png();
        // Minimal valid PNG header (24 bytes): signature + IHDR with 100x50.
        let mut data = Vec::new();
        data.extend_from_slice(b"\x89PNG\r\n\x1a\n"); // signature
        data.extend_from_slice(&13u32.to_be_bytes()); // IHDR length
        data.extend_from_slice(b"IHDR"); // chunk type
        data.extend_from_slice(&100u32.to_be_bytes()); // width
        data.extend_from_slice(&50u32.to_be_bytes()); // height
        std::fs::write(&path, &data).unwrap();

        let dims = png_dimensions(&path);
        assert_eq!(dims, Some((100, 50)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn png_dimensions_invalid() {
        let path = make_temp_png();
        std::fs::write(&path, b"not a png").unwrap();
        assert_eq!(png_dimensions(&path), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn png_dimensions_too_short() {
        let path = make_temp_png();
        std::fs::write(&path, b"\x89PNG").unwrap();
        assert_eq!(png_dimensions(&path), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn encode_base64_roundtrip() {
        let path = make_temp_png();
        let original = b"hello image data";
        std::fs::write(&path, original).unwrap();

        let encoded = encode_image_base64(&path).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        assert_eq!(decoded, original);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn encode_base64_empty_file_returns_none() {
        let path = make_temp_png();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"").unwrap();
        assert!(encode_image_base64(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn encode_base64_missing_file_returns_none() {
        let path = PathBuf::from("/tmp/nonexistent-oxicode-test.png");
        assert!(encode_image_base64(&path).is_none());
    }

    #[test]
    fn make_temp_png_unique() {
        let a = make_temp_png();
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let b = make_temp_png();
        assert_ne!(a, b);
    }
}
