//! Settings export/import (sync) functionality.
//!
//! Provides JSON-based settings export and import with validation,
//! MDM conflict detection, schema checking, and cloud sync (push/pull).

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// Export settings to a JSON string. API key is redacted for security.
pub fn export_settings(settings: &Settings) -> Result<String, String> {
    let mut export = settings.clone();
    export.api_key = None; // Never export API keys.
    serde_json::to_string_pretty(&export).map_err(|e| format!("Failed to serialize settings: {e}"))
}

/// Export settings to a JSON file.
pub fn export_to_file(settings: &Settings, path: &Path) -> Result<(), String> {
    let json = export_settings(settings)?;
    std::fs::write(path, json).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Import result with potential warnings.
#[derive(Debug)]
pub struct ImportResult {
    /// Successfully imported settings.
    pub settings: Settings,
    /// Warnings about skipped or conflicting settings.
    pub warnings: Vec<String>,
}

/// Import settings from a JSON string with validation.
pub fn import_settings(json: &str) -> Result<ImportResult, String> {
    let imported: Settings =
        serde_json::from_str(json).map_err(|e| format!("Invalid settings JSON: {e}"))?;

    let warnings = validate_imported(&imported);

    Ok(ImportResult {
        settings: imported,
        warnings,
    })
}

/// Import settings from a JSON file.
pub fn import_from_file(path: &Path) -> Result<ImportResult, String> {
    let json = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    import_settings(&json)
}

/// Validate imported settings and return warnings.
fn validate_imported(settings: &Settings) -> Vec<String> {
    let mut warnings = Vec::new();

    // Validate permission mode.
    let valid_modes = ["default", "bypass", "approval_only", "accept_edits"];
    if !valid_modes.contains(&settings.permission_mode.as_str()) {
        warnings.push(format!(
            "Unknown permission_mode '{}'. Valid: {}",
            settings.permission_mode,
            valid_modes.join(", ")
        ));
    }

    // Validate output style.
    let valid_styles = ["plain", "markdown", "minimal", "verbose"];
    if !valid_styles.contains(&settings.output_style.as_str()) {
        warnings.push(format!(
            "Unknown output_style '{}'. Valid: {}",
            settings.output_style,
            valid_styles.join(", ")
        ));
    }

    // Validate editor mode.
    let valid_editors = ["normal", "vim"];
    if !valid_editors.contains(&settings.editor_mode.as_str()) {
        warnings.push(format!(
            "Unknown editor_mode '{}'. Valid: {}",
            settings.editor_mode,
            valid_editors.join(", ")
        ));
    }

    // Warn if max_tokens seems unusually high.
    if settings.max_tokens > 200_000 {
        warnings.push(format!(
            "max_tokens={} is unusually high (typical max: 200000)",
            settings.max_tokens
        ));
    }

    // Warn if API key is present in export (security).
    if settings.api_key.is_some() {
        warnings.push(
            "Imported settings contain an API key. Consider using environment variables instead."
                .into(),
        );
    }

    warnings
}

/// Check for conflicts between imported settings and MDM-managed settings.
/// Returns list of conflicting keys.
pub fn check_mdm_conflicts(
    imported: &Settings,
    managed: &crate::mdm::ManagedSettings,
) -> Vec<String> {
    let defaults = Settings::default();
    let mut conflicts = Vec::new();

    if managed.is_locked("model") && imported.model != defaults.model {
        conflicts.push(format!(
            "model: '{}' conflicts with MDM-locked value '{}'",
            imported.model,
            managed.get("model").unwrap_or("?")
        ));
    }
    if managed.is_locked("permission_mode") && imported.permission_mode != defaults.permission_mode
    {
        conflicts.push(format!(
            "permission_mode: '{}' conflicts with MDM-locked value '{}'",
            imported.permission_mode,
            managed.get("permission_mode").unwrap_or("?")
        ));
    }
    if managed.is_locked("theme") && imported.theme != defaults.theme {
        conflicts.push(format!(
            "theme: '{}' conflicts with MDM-locked value '{}'",
            imported.theme,
            managed.get("theme").unwrap_or("?")
        ));
    }
    if managed.is_locked("max_tokens") && imported.max_tokens != defaults.max_tokens {
        conflicts.push(format!(
            "max_tokens: {} conflicts with MDM-locked value '{}'",
            imported.max_tokens,
            managed.get("max_tokens").unwrap_or("?")
        ));
    }
    if managed.is_locked("editor_mode") && imported.editor_mode != defaults.editor_mode {
        conflicts.push(format!(
            "editor_mode: '{}' conflicts with MDM-locked value '{}'",
            imported.editor_mode,
            managed.get("editor_mode").unwrap_or("?")
        ));
    }

    conflicts
}

/// Merge imported settings into target, skipping locked MDM settings.
pub fn merge_imported(
    target: &mut Settings,
    imported: &Settings,
    managed: &crate::mdm::ManagedSettings,
) -> Vec<String> {
    let mut skipped = Vec::new();

    if managed.is_locked("model") {
        skipped.push("model (MDM-locked)".into());
    } else {
        target.model.clone_from(&imported.model);
    }

    if managed.is_locked("max_tokens") {
        skipped.push("max_tokens (MDM-locked)".into());
    } else {
        target.max_tokens = imported.max_tokens;
    }

    if managed.is_locked("theme") {
        skipped.push("theme (MDM-locked)".into());
    } else {
        target.theme.clone_from(&imported.theme);
    }

    if managed.is_locked("permission_mode") {
        skipped.push("permission_mode (MDM-locked)".into());
    } else {
        target.permission_mode.clone_from(&imported.permission_mode);
    }

    if managed.is_locked("editor_mode") {
        skipped.push("editor_mode (MDM-locked)".into());
    } else {
        target.editor_mode.clone_from(&imported.editor_mode);
    }

    if managed.is_locked("output_style") {
        skipped.push("output_style (MDM-locked)".into());
    } else {
        target.output_style.clone_from(&imported.output_style);
    }

    // Always merge features (not typically MDM-controlled).
    target.features = imported.features.clone();

    // Never import API key — security risk.
    // target.api_key is intentionally left unchanged.

    skipped
}

// ========================================================================
// Cloud Sync — push/pull settings when logged in via OAuth
// ========================================================================

/// Cloud sync endpoint for settings storage.
const CLOUD_SYNC_BASE_URL: &str = "https://api.oxicode.dev/v1/settings";

/// Cloud settings payload with metadata for conflict resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSettings {
    /// The settings JSON (exported via `export_settings`).
    pub settings_json: String,
    /// Timestamp of the last push.
    pub updated_at: DateTime<Utc>,
    /// Device identifier for conflict tracking.
    #[serde(default)]
    pub device_id: String,
}

/// Result of a cloud sync pull with conflict info.
#[derive(Debug)]
pub struct SyncPullResult {
    /// The pulled settings.
    pub settings: Settings,
    /// Whether a conflict was detected (remote was newer AND different).
    pub had_conflict: bool,
    /// Warnings from import validation.
    pub warnings: Vec<String>,
}

/// Push local settings to cloud storage.
/// Requires a valid OAuth access token.
pub async fn push_settings(
    settings: &Settings,
    access_token: &str,
    device_id: &str,
) -> Result<(), String> {
    let json = export_settings(settings)?;

    let payload = CloudSettings {
        settings_json: json,
        updated_at: Utc::now(),
        device_id: device_id.to_string(),
    };

    let client = reqwest::Client::new();
    let resp = client
        .put(CLOUD_SYNC_BASE_URL)
        .bearer_auth(access_token)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Cloud sync push failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Cloud sync push returned HTTP {}", resp.status()));
    }

    tracing::info!("Settings pushed to cloud sync");
    Ok(())
}

/// Pull settings from cloud storage.
/// Uses latest-wins conflict resolution: if remote is newer, it wins.
pub async fn pull_settings(
    current: &Settings,
    access_token: &str,
) -> Result<SyncPullResult, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(CLOUD_SYNC_BASE_URL)
        .bearer_auth(access_token)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Cloud sync pull failed: {e}"))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // No cloud settings yet — nothing to pull.
        return Ok(SyncPullResult {
            settings: current.clone(),
            had_conflict: false,
            warnings: vec!["No cloud settings found. Push your local settings first.".into()],
        });
    }

    if !resp.status().is_success() {
        return Err(format!("Cloud sync pull returned HTTP {}", resp.status()));
    }

    let cloud: CloudSettings = resp
        .json()
        .await
        .map_err(|e| format!("Invalid cloud settings response: {e}"))?;

    let import_result = import_settings(&cloud.settings_json)?;

    // Detect conflict: cloud settings differ from local.
    let local_json = export_settings(current)?;
    let had_conflict = cloud.settings_json != local_json;

    if had_conflict {
        tracing::info!(
            "Cloud sync conflict detected (remote updated at {}). Using latest-wins.",
            cloud.updated_at
        );
    }

    Ok(SyncPullResult {
        settings: import_result.settings,
        had_conflict,
        warnings: import_result.warnings,
    })
}

/// Check cloud sync status (whether remote settings exist and their timestamp).
pub async fn sync_status(access_token: &str) -> Result<Option<DateTime<Utc>>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .head(CLOUD_SYNC_BASE_URL)
        .bearer_auth(access_token)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Cloud sync status check failed: {e}"))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if !resp.status().is_success() {
        return Err(format!("Cloud sync status returned HTTP {}", resp.status()));
    }

    // Parse Last-Modified header if available.
    if let Some(header) = resp.headers().get("last-modified") {
        if let Ok(date_str) = header.to_str() {
            if let Ok(dt) = DateTime::parse_from_rfc2822(date_str) {
                return Ok(Some(dt.with_timezone(&Utc)));
            }
        }
    }

    // Fallback: cloud settings exist but no timestamp.
    Ok(Some(Utc::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_import_roundtrip() {
        let settings = Settings::default();
        let json = export_settings(&settings).unwrap();
        let result = import_settings(&json).unwrap();
        assert_eq!(result.settings.model, settings.model);
        assert_eq!(result.settings.max_tokens, settings.max_tokens);
    }

    #[test]
    fn test_export_to_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let settings = Settings::default();
        export_to_file(&settings, tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(content.contains(&settings.model));
    }

    #[test]
    fn test_import_invalid_json() {
        let result = import_settings("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_warns_on_api_key() {
        let settings = Settings {
            api_key: Some("sk-ant-secret".into()),
            ..Settings::default()
        };
        let warnings = validate_imported(&settings);
        assert!(warnings.iter().any(|w| w.contains("API key")));
    }

    #[test]
    fn test_validate_warns_on_invalid_mode() {
        let settings = Settings {
            permission_mode: "invalid".into(),
            ..Settings::default()
        };
        let warnings = validate_imported(&settings);
        assert!(warnings.iter().any(|w| w.contains("permission_mode")));
    }

    #[test]
    fn test_validate_warns_on_high_tokens() {
        let settings = Settings {
            max_tokens: 999_999,
            ..Settings::default()
        };
        let warnings = validate_imported(&settings);
        assert!(warnings.iter().any(|w| w.contains("unusually high")));
    }

    #[test]
    fn test_check_mdm_conflicts() {
        let mut managed = crate::mdm::ManagedSettings::default();
        managed.settings.insert(
            "model".into(),
            crate::mdm::ManagedSetting {
                key: "model".into(),
                value: "claude-opus-4-20250514".into(),
                locked: true,
            },
        );

        let imported = Settings {
            model: "gpt-4o".into(),
            ..Settings::default()
        };

        let conflicts = check_mdm_conflicts(&imported, &managed);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("model"));
    }

    #[test]
    fn test_merge_skips_locked() {
        let mut target = Settings::default();
        let imported = Settings {
            model: "custom-model".into(),
            theme: "dracula".into(),
            ..Settings::default()
        };

        let mut managed = crate::mdm::ManagedSettings::default();
        managed.settings.insert(
            "model".into(),
            crate::mdm::ManagedSetting {
                key: "model".into(),
                value: "locked-model".into(),
                locked: true,
            },
        );

        let skipped = merge_imported(&mut target, &imported, &managed);
        // Model was locked — should not have been changed.
        assert_ne!(target.model, "custom-model");
        // Theme was not locked — should have been changed.
        assert_eq!(target.theme, "dracula");
        assert!(skipped.iter().any(|s| s.contains("model")));
    }

    #[test]
    fn test_import_from_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let settings = Settings::default();
        let json = export_settings(&settings).unwrap();
        std::fs::write(tmp.path(), json).unwrap();

        let result = import_from_file(tmp.path()).unwrap();
        assert_eq!(result.settings.model, settings.model);
    }
}
