//! `/teleport` command — export and import sessions across machines.
//!
//! - `/teleport export [--local <path>]` — package session + memories to tar.gz
//! - `/teleport import <path>` — restore session + memories from tar.gz
//!
//! Security: path traversal validation, size limits, no symlink following.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{CommandContext, CommandOutput, SlashCommand};

/// Maximum teleport package size: 50 MB.
const MAX_PACKAGE_SIZE: u64 = 50 * 1024 * 1024;

/// Maximum number of files allowed in a teleport archive.
const MAX_FILE_COUNT: usize = 1_000;

/// Maximum decompressed size per entry: 100 MB.
const MAX_ENTRY_SIZE: u64 = 100 * 1024 * 1024;

/// /teleport — export/import sessions across machines.
pub struct TeleportCommand;

impl SlashCommand for TeleportCommand {
    fn name(&self) -> &str {
        "teleport"
    }

    fn description(&self) -> &str {
        "Export/import sessions for cross-machine transfer"
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandOutput {
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts.first().copied() {
            Some("export") => handle_export(&parts[1..], ctx),
            Some("import") => handle_import(&parts[1..]),
            Some("help") | None => CommandOutput::Message(
                "Usage:\n  \
                 /teleport export [--local <path>]  — Package session + memories\n  \
                 /teleport import <path>            — Restore from package\n\n\
                 Exports session messages, settings, and memory entries.\n\
                 Use --local to save to a specific path (default: ~/.oxicode/teleport/)."
                    .to_string(),
            ),
            Some(other) => {
                CommandOutput::Error(format!("Unknown subcommand: {other}. Use: export, import"))
            }
        }
    }
}

/// Handle `/teleport export [--local <path>]`.
fn handle_export(args: &[&str], ctx: &CommandContext) -> CommandOutput {
    // Determine output path.
    let output_path = if args.len() >= 2 && args[0] == "--local" {
        PathBuf::from(args[1])
    } else {
        let teleport_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oxicode/teleport");
        if let Err(e) = fs::create_dir_all(&teleport_dir) {
            return CommandOutput::Error(format!("Failed to create teleport dir: {e}"));
        }
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        teleport_dir.join(format!(
            "session-{}-{timestamp}.tar.gz",
            &ctx.session_id[..8.min(ctx.session_id.len())]
        ))
    };

    // Collect session state as JSON.
    let state = ctx.state_store.current();
    let session_json = match serde_json::to_string_pretty(&state) {
        Ok(j) => j,
        Err(e) => return CommandOutput::Error(format!("Failed to serialize session: {e}")),
    };

    // Collect memory files.
    let memory_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode/memory");

    let memory_files = collect_memory_files(&memory_dir);

    // Collect settings.
    let settings_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode/settings.toml");

    // Build tar.gz archive.
    match build_archive(&output_path, &session_json, &memory_files, &settings_path) {
        Ok(size) => {
            let size_kb = size / 1024;
            CommandOutput::Message(format!(
                "Session exported successfully!\n\
                 File: {}\n\
                 Size: {} KB\n\
                 Contents: 1 session, {} memories, settings\n\n\
                 Transfer this file to the target machine and run:\n  \
                 /teleport import {}",
                output_path.display(),
                size_kb,
                memory_files.len(),
                output_path.display()
            ))
        }
        Err(e) => CommandOutput::Error(format!("Export failed: {e}")),
    }
}

/// Handle `/teleport import <path>`.
fn handle_import(args: &[&str]) -> CommandOutput {
    let path = match args.first() {
        Some(p) => PathBuf::from(p),
        None => return CommandOutput::Error("Usage: /teleport import <path>".to_string()),
    };

    if !path.exists() {
        return CommandOutput::Error(format!("File not found: {}", path.display()));
    }

    // Check file size.
    match fs::metadata(&path) {
        Ok(meta) if meta.len() > MAX_PACKAGE_SIZE => {
            return CommandOutput::Error(format!(
                "Package too large: {} MB (max: {} MB)",
                meta.len() / (1024 * 1024),
                MAX_PACKAGE_SIZE / (1024 * 1024)
            ));
        }
        Err(e) => return CommandOutput::Error(format!("Cannot read file: {e}")),
        _ => {}
    }

    // Extract and validate archive.
    match extract_archive(&path) {
        Ok(stats) => CommandOutput::Message(format!(
            "Session imported successfully!\n\
             Messages found: {} (load via session file)\n\
             Memories imported: {} (skipped {} duplicates)\n\
             Settings: {}\n\n\
             Session data saved to ~/.oxicode/ — restart OxiCode to use.",
            stats.messages,
            stats.memories_imported,
            stats.memories_skipped,
            if stats.settings_restored {
                "restored"
            } else {
                "not found in package"
            },
        )),
        Err(e) => CommandOutput::Error(format!("Import failed: {e}")),
    }
}

/// Collect all .json files from the memory directory.
fn collect_memory_files(memory_dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    if !memory_dir.exists() {
        return files;
    }

    let Ok(entries) = fs::read_dir(memory_dir) else {
        return files;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(content) = fs::read(&path) {
                if let Some(name) = path.file_name() {
                    files.push((name.to_string_lossy().to_string(), content));
                }
            }
        }
    }
    files
}

/// Build a tar.gz archive containing session, memories, and settings.
fn build_archive(
    output_path: &Path,
    session_json: &str,
    memory_files: &[(String, Vec<u8>)],
    settings_path: &Path,
) -> Result<u64, String> {
    // Ensure parent directory exists.
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create output dir: {e}"))?;
    }

    let file = fs::File::create(output_path).map_err(|e| format!("Cannot create archive: {e}"))?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(enc);

    // Add session.json.
    add_bytes_to_tar(&mut archive, "session.json", session_json.as_bytes())?;

    // Add memory files under memory/ prefix.
    for (name, content) in memory_files {
        let tar_path = format!("memory/{name}");
        add_bytes_to_tar(&mut archive, &tar_path, content)?;
    }

    // Add settings.toml if it exists.
    if settings_path.exists() {
        if let Ok(content) = fs::read(settings_path) {
            add_bytes_to_tar(&mut archive, "settings.toml", &content)?;
        }
    }

    let enc = archive
        .into_inner()
        .map_err(|e| format!("Archive finalization failed: {e}"))?;
    enc.finish()
        .map_err(|e| format!("Gzip finalization failed: {e}"))?;

    let size = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    Ok(size)
}

/// Add raw bytes as a file entry in a tar archive.
fn add_bytes_to_tar<W: Write>(
    archive: &mut tar::Builder<W>,
    path: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();

    archive
        .append_data(&mut header, path, data)
        .map_err(|e| format!("Failed to add {path}: {e}"))
}

/// Statistics from an import operation.
#[derive(Debug)]
struct ImportStats {
    messages: usize,
    memories_imported: usize,
    memories_skipped: usize,
    settings_restored: bool,
}

/// Extract and validate a teleport archive, restoring memories and settings.
fn extract_archive(archive_path: &Path) -> Result<ImportStats, String> {
    let file = fs::File::open(archive_path).map_err(|e| format!("Cannot open archive: {e}"))?;
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);

    let memory_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oxicode/memory");
    fs::create_dir_all(&memory_dir).map_err(|e| format!("Cannot create memory dir: {e}"))?;

    let mut stats = ImportStats {
        messages: 0,
        memories_imported: 0,
        memories_skipped: 0,
        settings_restored: false,
    };

    let mut file_count = 0usize;

    let entries = archive
        .entries()
        .map_err(|e| format!("Cannot read archive entries: {e}"))?;

    for entry_result in entries {
        let mut entry = entry_result.map_err(|e| format!("Corrupt archive entry: {e}"))?;

        file_count += 1;
        if file_count > MAX_FILE_COUNT {
            return Err(format!(
                "Archive contains too many files (max: {MAX_FILE_COUNT})"
            ));
        }

        // Security: reject symlinks.
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err("Archive contains symlinks — rejected for security".to_string());
        }

        let path = entry
            .path()
            .map_err(|e| format!("Invalid entry path: {e}"))?
            .to_path_buf();

        // Security: reject path traversal.
        let path_str = path.to_string_lossy();
        if path_str.contains("..") || path_str.starts_with('/') {
            return Err(format!(
                "Path traversal detected: {path_str} — rejected for security"
            ));
        }

        // Security: reject oversized entries (decompression bomb protection).
        let entry_size = entry.header().size().unwrap_or(0);
        if entry_size > MAX_ENTRY_SIZE {
            tracing::warn!(
                "Skipping oversized entry '{}': {} bytes (max: {} bytes)",
                path_str,
                entry_size,
                MAX_ENTRY_SIZE
            );
            continue;
        }

        // Read entry content (with runtime size guard).
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|e| format!("Failed to read entry {path_str}: {e}"))?;

        // Double-check actual decompressed size in case header lied.
        if content.len() as u64 > MAX_ENTRY_SIZE {
            tracing::warn!(
                "Skipping entry '{}': actual size {} exceeds max {}",
                path_str,
                content.len(),
                MAX_ENTRY_SIZE
            );
            continue;
        }

        if path_str == "session.json" {
            // Validate JSON is parseable and count messages.
            let session: serde_json::Value = serde_json::from_slice(&content)
                .map_err(|e| format!("Invalid session.json: {e}"))?;
            stats.messages = session["messages"].as_array().map_or(0, Vec::len);
        } else if let Some(name) = path_str.strip_prefix("memory/") {
            // Security: only allow flat filenames — reject subdirectory paths.
            if name.contains('/') || name.contains('\\') {
                return Err(format!(
                    "Memory path contains subdirectory: {name} — rejected for security"
                ));
            }
            // Import memory file with deduplication by SHA-256.
            let target = memory_dir.join(name);
            if target.exists() {
                // Compare hashes to skip duplicates.
                if let Ok(existing) = fs::read(&target) {
                    let existing_hash = Sha256::digest(&existing);
                    let new_hash = Sha256::digest(&content);
                    if existing_hash == new_hash {
                        stats.memories_skipped += 1;
                        continue;
                    }
                }
            }
            fs::write(&target, &content)
                .map_err(|e| format!("Failed to write memory {name}: {e}"))?;
            stats.memories_imported += 1;
        } else if path_str == "settings.toml" {
            let settings_path = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oxicode/settings.toml");
            // Don't overwrite existing settings — save as .imported.
            let target = if settings_path.exists() {
                settings_path.with_extension("toml.imported")
            } else {
                settings_path
            };
            fs::write(&target, &content).map_err(|e| format!("Failed to write settings: {e}"))?;
            stats.settings_restored = true;
        }
        // Ignore unknown entries silently.
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "test".to_string(),
            provider_name: "test".to_string(),
            session_id: "test-session-12345678".to_string(),
        }
    }

    #[test]
    fn test_teleport_help() {
        let cmd = TeleportCommand;
        let ctx = make_ctx();
        let output = cmd.execute("", &ctx);
        match output {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("export"));
                assert!(msg.contains("import"));
            }
            _ => panic!("Expected message output"),
        }
    }

    #[test]
    fn test_teleport_unknown_subcommand() {
        let cmd = TeleportCommand;
        let ctx = make_ctx();
        let output = cmd.execute("invalid", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Unknown subcommand")),
            _ => panic!("Expected error output"),
        }
    }

    #[test]
    fn test_teleport_import_missing_path() {
        let cmd = TeleportCommand;
        let ctx = make_ctx();
        let output = cmd.execute("import", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("Usage")),
            _ => panic!("Expected error output"),
        }
    }

    #[test]
    fn test_teleport_import_nonexistent_file() {
        let cmd = TeleportCommand;
        let ctx = make_ctx();
        let output = cmd.execute("import /nonexistent/file.tar.gz", &ctx);
        match output {
            CommandOutput::Error(msg) => assert!(msg.contains("not found")),
            _ => panic!("Expected error output"),
        }
    }

    #[test]
    fn test_export_and_import_roundtrip() {
        let ctx = make_ctx();
        let tmp = tempfile::TempDir::new().unwrap();
        let archive_path = tmp.path().join("test-export.tar.gz");

        // Export to local file.
        let output = handle_export(&["--local", archive_path.to_str().unwrap()], &ctx);
        match &output {
            CommandOutput::Message(msg) => assert!(msg.contains("exported successfully")),
            CommandOutput::Error(e) => panic!("Export failed: {e}"),
            _ => panic!("Expected message output"),
        }

        // Verify file exists.
        assert!(archive_path.exists());

        // Import back.
        let import_result = extract_archive(&archive_path);
        assert!(import_result.is_ok());
    }

    #[test]
    fn test_archive_path_traversal_rejected() {
        use std::io::Write as _;
        let tmp = tempfile::TempDir::new().unwrap();
        let archive_path = tmp.path().join("malicious.tar.gz");

        // Craft raw tar bytes with a malicious "../etc/passwd" path.
        // The tar crate's set_path() rejects ".." and absolute paths,
        // so we write the name bytes directly into the raw 512-byte header.
        let file = fs::File::create(&archive_path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());

        let data = b"malicious content";
        let data_len = data.len();

        // Build a minimal GNU tar header manually (512 bytes).
        let mut raw = [0u8; 512];
        let name = b"../etc/passwd";
        raw[..name.len()].copy_from_slice(name); // offset 0: name
                                                 // mode at offset 100 (8 bytes octal)
        raw[100..107].copy_from_slice(b"0000644");
        // uid at offset 108
        raw[108..115].copy_from_slice(b"0001000");
        // gid at offset 116
        raw[116..123].copy_from_slice(b"0001000");
        // size at offset 124 (12 bytes octal)
        let size_str = format!("{data_len:011o}");
        raw[124..135].copy_from_slice(size_str.as_bytes());
        // mtime at offset 136
        raw[136..147].copy_from_slice(b"14633466453");
        // typeflag at offset 156: '0' = regular file
        raw[156] = b'0';
        // magic at offset 257: "ustar\0"
        raw[257..263].copy_from_slice(b"ustar\0");
        // version at offset 263
        raw[263..265].copy_from_slice(b"00");

        // Compute checksum: sum of all bytes in header, treating
        // checksum field (offset 148..156) as spaces.
        let mut cksum: u32 = 0;
        for (i, &b) in raw.iter().enumerate() {
            if (148..156).contains(&i) {
                cksum += u32::from(b' ');
            } else {
                cksum += u32::from(b);
            }
        }
        let cksum_str = format!("{cksum:06o}\0 ");
        raw[148..156].copy_from_slice(cksum_str.as_bytes());

        // Write raw header + data + padding to fill 512-byte block.
        let mut output = enc;
        output.write_all(&raw).unwrap();
        output.write_all(data).unwrap();
        // Pad to 512-byte boundary.
        let padding = 512 - (data_len % 512);
        if padding < 512 {
            output.write_all(&vec![0u8; padding]).unwrap();
        }
        // Two zero blocks mark end of archive.
        output.write_all(&[0u8; 1024]).unwrap();
        output.finish().unwrap();

        let result = extract_archive(&archive_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("traversal"),
            "Expected path traversal rejection, got: {err}"
        );
    }

    #[test]
    fn test_collect_memory_files_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let files = collect_memory_files(tmp.path());
        assert!(files.is_empty());
    }

    #[test]
    fn test_collect_memory_files_with_entries() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("mem1.json"), r#"{"key":"val"}"#).unwrap();
        fs::write(tmp.path().join("mem2.json"), r#"{"key":"val2"}"#).unwrap();
        fs::write(tmp.path().join("not-json.txt"), "skip me").unwrap();

        let files = collect_memory_files(tmp.path());
        assert_eq!(files.len(), 2);
    }
}
