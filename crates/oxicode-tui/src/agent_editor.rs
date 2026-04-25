use std::path::Path;

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::enable_raw_mode();
    }
}

pub fn open_in_editor(path: &Path) -> std::io::Result<()> {
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".to_string());

    // Note: we deliberately do not invoke a shell. $EDITOR with spaces is
    // best-effort and won’t handle quoted paths.
    let mut parts = editor.split_whitespace();
    let cmd = parts.next().unwrap_or("vi");
    let args: Vec<&str> = parts.collect();

    crossterm::terminal::disable_raw_mode()?;
    let _guard = RawModeGuard;

    let status = std::process::Command::new(cmd)
        .args(args)
        .arg(path)
        .status()
        .map_err(std::io::Error::other)?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "editor exited with status: {status}"
        )))
    }
}
