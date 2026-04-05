//! AutoDream configuration — controls automatic suggestion generation behaviour.

use serde::{Deserialize, Serialize};

/// Configuration for the AutoDream suggestion engine.
///
/// Governs whether suggestions are produced, how long to wait between
/// generation runs, and how many suggestions to return per batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDreamConfig {
    /// Whether AutoDream is active. Set to `false` to fully disable.
    pub enabled: bool,
    /// Minimum seconds that must pass before generating the next batch.
    pub cooldown_secs: u64,
    /// Maximum number of suggestions returned per generation call.
    pub max_suggestions: usize,
}

impl Default for AutoDreamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cooldown_secs: 5,
            max_suggestions: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_correct() {
        let cfg = AutoDreamConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.cooldown_secs, 5);
        assert_eq!(cfg.max_suggestions, 3);
    }

    #[test]
    fn roundtrip_serialization() {
        let cfg = AutoDreamConfig {
            enabled: false,
            cooldown_secs: 10,
            max_suggestions: 5,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: AutoDreamConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.enabled);
        assert_eq!(back.cooldown_secs, 10);
        assert_eq!(back.max_suggestions, 5);
    }

    #[test]
    fn partial_deserialization_uses_field_defaults() {
        // Simulate config with only some fields set
        let json = r#"{"enabled": true, "cooldown_secs": 30, "max_suggestions": 1}"#;
        let cfg: AutoDreamConfig = serde_json::from_str(json).expect("deserialize");
        assert!(cfg.enabled);
        assert_eq!(cfg.cooldown_secs, 30);
        assert_eq!(cfg.max_suggestions, 1);
    }
}
