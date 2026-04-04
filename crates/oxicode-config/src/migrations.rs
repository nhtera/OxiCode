//! Config migration pipeline: versioned migrations that auto-run on startup.
//!
//! Each migration transforms a raw TOML `toml::Value` before it's parsed into
//! the `Settings` struct. Migrations are numbered sequentially and tracked via
//! the `config_version` field persisted in the config file itself.

use std::path::{Path, PathBuf};

/// Current schema version — must equal the highest migration version.
pub const CURRENT_VERSION: u32 = 4;

/// A single config migration.
pub struct Migration {
    /// Monotonically increasing version number (1, 2, 3, ...).
    pub version: u32,
    /// Short human-readable name.
    pub name: &'static str,
    /// Apply the migration to the raw TOML table.
    /// Must be idempotent — safe to re-run.
    pub apply: fn(&mut toml::Value) -> Result<(), String>,
}

/// Return all built-in migrations, ordered by version.
pub fn all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "bootstrap_config_version",
            apply: m001_bootstrap_config_version,
        },
        Migration {
            version: 2,
            name: "rename_legacy_model_names",
            apply: m002_rename_legacy_models,
        },
        Migration {
            version: 3,
            name: "add_default_features",
            apply: m003_add_default_features,
        },
        Migration {
            version: 4,
            name: "normalize_permission_mode",
            apply: m004_normalize_permission_mode,
        },
    ]
}

// ---------------------------------------------------------------------------
// Migration runner
// ---------------------------------------------------------------------------

/// Result of running migrations.
#[derive(Debug)]
pub struct MigrationResult {
    /// Number of migrations applied.
    pub applied: u32,
    /// Previous config version before migration.
    pub old_version: u32,
    /// Current config version after migration.
    pub new_version: u32,
    /// Path to backup file (if created).
    pub backup_path: Option<PathBuf>,
}

/// Run pending migrations on a TOML config file.
///
/// 1. Read the file as raw `toml::Value`.
/// 2. Determine current version from `config_version` field (default 0).
/// 3. Apply migrations with version > current, in order.
/// 4. Backup original file before first mutation.
/// 5. Write updated config back to disk.
///
/// Returns `Ok(result)` on success, `Err(msg)` on fatal error.
/// Individual migration failures are logged but don't abort — we skip
/// the failed migration and continue with the original value for that step.
pub fn run_migrations(config_path: &Path) -> Result<MigrationResult, String> {
    let content = std::fs::read_to_string(config_path).map_err(|e| format!("read config: {e}"))?;

    let mut value: toml::Value =
        toml::from_str(&content).map_err(|e| format!("parse config TOML: {e}"))?;

    let old_version = read_version(&value);
    let migrations = all_migrations();

    // Nothing to do?
    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| m.version > old_version)
        .collect();
    if pending.is_empty() {
        return Ok(MigrationResult {
            applied: 0,
            old_version,
            new_version: old_version,
            backup_path: None,
        });
    }

    // Backup before mutating.
    let backup_path = backup_config(config_path)?;

    let mut applied = 0u32;
    let mut current_version = old_version;

    for migration in &pending {
        match (migration.apply)(&mut value) {
            Ok(()) => {
                current_version = migration.version;
                applied += 1;
                tracing::info!(
                    version = migration.version,
                    name = migration.name,
                    "applied config migration"
                );
            }
            Err(e) => {
                tracing::warn!(
                    version = migration.version,
                    name = migration.name,
                    error = %e,
                    "config migration failed — skipping"
                );
                // Don't update current_version; stop applying further migrations
                // since later ones may depend on this one.
                break;
            }
        }
    }

    // Stamp the new version.
    write_version(&mut value, current_version);

    // Write back.
    let updated = toml::to_string_pretty(&value).map_err(|e| format!("serialize config: {e}"))?;
    std::fs::write(config_path, updated).map_err(|e| format!("write config: {e}"))?;

    Ok(MigrationResult {
        applied,
        old_version,
        new_version: current_version,
        backup_path: Some(backup_path),
    })
}

/// Run migrations on raw TOML content in memory (no file I/O).
/// Used when you already have the content and don't want disk side-effects.
pub fn run_migrations_in_memory(content: &str) -> Result<(String, MigrationResult), String> {
    let mut value: toml::Value = toml::from_str(content).map_err(|e| format!("parse TOML: {e}"))?;

    let old_version = read_version(&value);
    let migrations = all_migrations();
    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| m.version > old_version)
        .collect();

    if pending.is_empty() {
        return Ok((
            content.to_string(),
            MigrationResult {
                applied: 0,
                old_version,
                new_version: old_version,
                backup_path: None,
            },
        ));
    }

    let mut applied = 0u32;
    let mut current_version = old_version;

    for migration in &pending {
        match (migration.apply)(&mut value) {
            Ok(()) => {
                current_version = migration.version;
                applied += 1;
            }
            Err(_) => break,
        }
    }

    write_version(&mut value, current_version);

    let updated = toml::to_string_pretty(&value).map_err(|e| format!("serialize: {e}"))?;

    Ok((
        updated,
        MigrationResult {
            applied,
            old_version,
            new_version: current_version,
            backup_path: None,
        },
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read `config_version` from a TOML value, defaulting to 0.
fn read_version(value: &toml::Value) -> u32 {
    value
        .get("config_version")
        .and_then(|v| v.as_integer())
        .map(|v| v as u32)
        .unwrap_or(0)
}

/// Write `config_version` into a TOML table.
fn write_version(value: &mut toml::Value, version: u32) {
    if let Some(table) = value.as_table_mut() {
        table.insert(
            "config_version".to_string(),
            toml::Value::Integer(i64::from(version)),
        );
    }
}

/// Create a timestamped backup of the config file.
fn backup_config(config_path: &Path) -> Result<PathBuf, String> {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let backup_name = format!(
        "{}.bak.{ts}",
        config_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    let backup_path = config_path.with_file_name(backup_name);
    std::fs::copy(config_path, &backup_path).map_err(|e| format!("backup config: {e}"))?;
    tracing::info!(backup = %backup_path.display(), "config backup created");
    Ok(backup_path)
}

// ---------------------------------------------------------------------------
// Built-in migrations
// ---------------------------------------------------------------------------

/// Migration 1: Bootstrap — ensure `config_version` field exists.
fn m001_bootstrap_config_version(value: &mut toml::Value) -> Result<(), String> {
    // Idempotent: if already present, leave it.
    if value.get("config_version").is_some() {
        return Ok(());
    }
    write_version(value, 1);
    Ok(())
}

/// Migration 2: Rename legacy model identifiers.
///
/// Maps old model names to current canonical names.
fn m002_rename_legacy_models(value: &mut toml::Value) -> Result<(), String> {
    let model_remap: &[(&str, &str)] = &[
        ("claude-3-sonnet-20240229", "claude-sonnet-4-20250514"),
        ("claude-3-5-sonnet-20240620", "claude-sonnet-4-20250514"),
        ("claude-3-5-sonnet-20241022", "claude-sonnet-4-20250514"),
        ("claude-3-opus-20240229", "claude-opus-4-20250514"),
        ("claude-3-haiku-20240307", "claude-haiku-3-5-20241022"),
        ("claude-3-5-haiku-20241022", "claude-haiku-3-5-20241022"),
    ];

    if let Some(model_val) = value.get("model").and_then(|v| v.as_str()) {
        for (old, new) in model_remap {
            if model_val == *old {
                if let Some(table) = value.as_table_mut() {
                    table.insert("model".to_string(), toml::Value::String(new.to_string()));
                }
                break;
            }
        }
    }
    Ok(())
}

/// Migration 3: Ensure `[features]` table has expected defaults.
///
/// Adds missing feature flags with their default values without
/// overwriting existing user choices.
fn m003_add_default_features(value: &mut toml::Value) -> Result<(), String> {
    let table = value
        .as_table_mut()
        .ok_or_else(|| "config root is not a table".to_string())?;

    let defaults: &[(&str, bool)] = &[
        ("extended_thinking", true),
        ("prompt_caching", true),
        ("streaming", true),
    ];

    let features = table
        .entry("features")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

    if let Some(ftable) = features.as_table_mut() {
        for (key, default_val) in defaults {
            ftable
                .entry(key.to_string())
                .or_insert(toml::Value::Boolean(*default_val));
        }
    }

    Ok(())
}

/// Migration 4: Normalize `permission_mode` values.
///
/// Rename legacy values to canonical forms.
fn m004_normalize_permission_mode(value: &mut toml::Value) -> Result<(), String> {
    let mode_remap: &[(&str, &str)] = &[
        ("accept-edits", "accept_edits"),
        ("acceptEdits", "accept_edits"),
        ("auto-accept", "bypass"),
        ("autoAccept", "bypass"),
        ("none", "default"),
    ];

    if let Some(mode_val) = value.get("permission_mode").and_then(|v| v.as_str()) {
        for (old, new) in mode_remap {
            if mode_val == *old {
                if let Some(table) = value.as_table_mut() {
                    table.insert(
                        "permission_mode".to_string(),
                        toml::Value::String(new.to_string()),
                    );
                }
                break;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Framework tests --

    #[test]
    fn all_migrations_ordered() {
        let migrations = all_migrations();
        for (i, m) in migrations.iter().enumerate() {
            assert_eq!(
                m.version,
                (i + 1) as u32,
                "migration {} out of order",
                m.name
            );
        }
    }

    #[test]
    fn current_version_matches_last_migration() {
        let migrations = all_migrations();
        let last = migrations.last().expect("no migrations");
        assert_eq!(CURRENT_VERSION, last.version);
    }

    #[test]
    fn read_version_missing_returns_zero() {
        let value: toml::Value = toml::from_str("model = \"test\"").unwrap();
        assert_eq!(read_version(&value), 0);
    }

    #[test]
    fn read_write_version_roundtrip() {
        let mut value: toml::Value = toml::from_str("").unwrap();
        write_version(&mut value, 42);
        assert_eq!(read_version(&value), 42);
    }

    // -- Migration 1: bootstrap --

    #[test]
    fn m001_adds_config_version() {
        let mut value: toml::Value = toml::from_str("model = \"test\"").unwrap();
        m001_bootstrap_config_version(&mut value).unwrap();
        assert_eq!(read_version(&value), 1);
    }

    #[test]
    fn m001_idempotent() {
        let mut value: toml::Value =
            toml::from_str("config_version = 3\nmodel = \"test\"").unwrap();
        m001_bootstrap_config_version(&mut value).unwrap();
        assert_eq!(read_version(&value), 3); // unchanged
    }

    // -- Migration 2: model renames --

    #[test]
    fn m002_renames_legacy_sonnet() {
        let mut value: toml::Value =
            toml::from_str("model = \"claude-3-5-sonnet-20241022\"").unwrap();
        m002_rename_legacy_models(&mut value).unwrap();
        assert_eq!(
            value.get("model").unwrap().as_str().unwrap(),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn m002_leaves_current_model_alone() {
        let mut value: toml::Value =
            toml::from_str("model = \"claude-sonnet-4-20250514\"").unwrap();
        m002_rename_legacy_models(&mut value).unwrap();
        assert_eq!(
            value.get("model").unwrap().as_str().unwrap(),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn m002_no_model_field_ok() {
        let mut value: toml::Value = toml::from_str("theme = \"dark\"").unwrap();
        m002_rename_legacy_models(&mut value).unwrap(); // no panic
    }

    // -- Migration 3: default features --

    #[test]
    fn m003_adds_missing_features() {
        let mut value: toml::Value = toml::from_str("model = \"test\"").unwrap();
        m003_add_default_features(&mut value).unwrap();

        let features = value.get("features").unwrap().as_table().unwrap();
        assert_eq!(
            features.get("extended_thinking").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(
            features.get("prompt_caching").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(features.get("streaming").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn m003_preserves_existing_features() {
        let input = r#"
[features]
extended_thinking = false
custom_flag = true
"#;
        let mut value: toml::Value = toml::from_str(input).unwrap();
        m003_add_default_features(&mut value).unwrap();

        let features = value.get("features").unwrap().as_table().unwrap();
        // User's false should be preserved.
        assert_eq!(
            features.get("extended_thinking").unwrap().as_bool(),
            Some(false)
        );
        // Custom flags preserved.
        assert_eq!(features.get("custom_flag").unwrap().as_bool(), Some(true));
        // New defaults added.
        assert_eq!(
            features.get("prompt_caching").unwrap().as_bool(),
            Some(true)
        );
    }

    // -- Migration 4: permission_mode normalize --

    #[test]
    fn m004_renames_accept_edits_kebab() {
        let mut value: toml::Value = toml::from_str("permission_mode = \"accept-edits\"").unwrap();
        m004_normalize_permission_mode(&mut value).unwrap();
        assert_eq!(
            value.get("permission_mode").unwrap().as_str().unwrap(),
            "accept_edits"
        );
    }

    #[test]
    fn m004_renames_auto_accept() {
        let mut value: toml::Value = toml::from_str("permission_mode = \"autoAccept\"").unwrap();
        m004_normalize_permission_mode(&mut value).unwrap();
        assert_eq!(
            value.get("permission_mode").unwrap().as_str().unwrap(),
            "bypass"
        );
    }

    #[test]
    fn m004_leaves_canonical_values() {
        let mut value: toml::Value = toml::from_str("permission_mode = \"default\"").unwrap();
        m004_normalize_permission_mode(&mut value).unwrap();
        assert_eq!(
            value.get("permission_mode").unwrap().as_str().unwrap(),
            "default"
        );
    }

    // -- Integration tests --

    #[test]
    fn run_migrations_in_memory_full_pipeline() {
        let input = r#"
model = "claude-3-5-sonnet-20241022"
api_key = "sk-ant-test"
permission_mode = "acceptEdits"
"#;
        let (output, result) = run_migrations_in_memory(input).unwrap();
        assert_eq!(result.old_version, 0);
        assert_eq!(result.new_version, CURRENT_VERSION);
        assert_eq!(result.applied, 4);

        // Verify transformed content.
        let value: toml::Value = toml::from_str(&output).unwrap();
        assert_eq!(read_version(&value), CURRENT_VERSION);
        assert_eq!(
            value.get("model").unwrap().as_str().unwrap(),
            "claude-sonnet-4-20250514"
        );
        // api_key should remain at top level (no providers migration).
        assert_eq!(
            value.get("api_key").unwrap().as_str().unwrap(),
            "sk-ant-test"
        );
        assert_eq!(
            value.get("permission_mode").unwrap().as_str().unwrap(),
            "accept_edits"
        );
    }

    #[test]
    fn run_migrations_in_memory_already_current() {
        let input = format!("config_version = {CURRENT_VERSION}\nmodel = \"test\"");
        let (_output, result) = run_migrations_in_memory(&input).unwrap();
        assert_eq!(result.applied, 0);
        assert_eq!(result.old_version, CURRENT_VERSION);
    }

    #[test]
    fn run_migrations_file_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("settings.toml");
        std::fs::write(&config_path, "model = \"claude-3-sonnet-20240229\"").unwrap();

        let result = run_migrations(&config_path).unwrap();
        assert!(result.applied > 0);
        assert!(result.backup_path.is_some());
        assert!(result.backup_path.as_ref().unwrap().exists());

        // Original file should be updated.
        let updated = std::fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("config_version"));
    }

    #[test]
    fn run_migrations_file_no_op_when_current() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("settings.toml");
        let content = format!("config_version = {CURRENT_VERSION}\nmodel = \"test\"");
        std::fs::write(&config_path, &content).unwrap();

        let result = run_migrations(&config_path).unwrap();
        assert_eq!(result.applied, 0);
        assert!(result.backup_path.is_none()); // No backup for no-op.
    }

    #[test]
    fn run_migrations_partial_from_version_2() {
        let input = r#"
config_version = 2
model = "claude-sonnet-4-20250514"
permission_mode = "autoAccept"
"#;
        let (output, result) = run_migrations_in_memory(input).unwrap();
        assert_eq!(result.old_version, 2);
        assert_eq!(result.applied, 2); // migrations 3 and 4
        assert_eq!(result.new_version, CURRENT_VERSION);

        let value: toml::Value = toml::from_str(&output).unwrap();
        // Migration 4 should have normalized the permission_mode.
        assert_eq!(
            value.get("permission_mode").unwrap().as_str().unwrap(),
            "bypass"
        );
        // Migration 3 should have added features.
        assert!(value.get("features").is_some());
    }
}
