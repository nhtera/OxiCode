//! Runtime feature flag registry.
//!
//! Provides a [`FeatureFlags`] struct that loads from the `[features]` section of
//! `settings.toml` and exposes an `is_enabled(flag)` API for runtime checks.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Known runtime feature flag names.
pub mod flag {
    pub const EXTENDED_THINKING: &str = "extended_thinking";
    pub const PROMPT_CACHING: &str = "prompt_caching";
    pub const PROACTIVE_AGENTS: &str = "proactive_agents";
    pub const REMOTE_AGENTS: &str = "remote_agents";
    pub const TEAMMATE_TASKS: &str = "teammate_tasks";
    pub const VOICE_INPUT: &str = "voice_input";
    pub const VIM_MODE: &str = "vim_mode";
    pub const LSP_INTEGRATION: &str = "lsp_integration";
    pub const TELEMETRY: &str = "telemetry";
    pub const ENTERPRISE: &str = "enterprise";
}

/// Runtime feature flags loaded from `settings.toml` `[features]` section.
///
/// Unknown flags are preserved in the `extra` map so user-defined flags work too.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeatureFlags {
    pub extended_thinking: bool,
    pub prompt_caching: bool,
    pub proactive_agents: bool,
    pub remote_agents: bool,
    pub teammate_tasks: bool,
    pub voice_input: bool,
    pub vim_mode: bool,
    pub lsp_integration: bool,
    pub telemetry: bool,
    pub enterprise: bool,
    /// Catch-all for user-defined or future flags.
    #[serde(flatten)]
    pub extra: HashMap<String, bool>,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            extended_thinking: true,
            prompt_caching: true,
            proactive_agents: false,
            remote_agents: false,
            teammate_tasks: false,
            voice_input: false,
            vim_mode: false,
            lsp_integration: false,
            telemetry: false,
            enterprise: false,
            extra: HashMap::new(),
        }
    }
}

impl FeatureFlags {
    /// Check whether a named flag is enabled.
    pub fn is_enabled(&self, name: &str) -> bool {
        match name {
            flag::EXTENDED_THINKING => self.extended_thinking,
            flag::PROMPT_CACHING => self.prompt_caching,
            flag::PROACTIVE_AGENTS => self.proactive_agents,
            flag::REMOTE_AGENTS => self.remote_agents,
            flag::TEAMMATE_TASKS => self.teammate_tasks,
            flag::VOICE_INPUT => self.voice_input,
            flag::VIM_MODE => self.vim_mode,
            flag::LSP_INTEGRATION => self.lsp_integration,
            flag::TELEMETRY => self.telemetry,
            flag::ENTERPRISE => self.enterprise,
            other => self.extra.get(other).copied().unwrap_or(false),
        }
    }

    /// Toggle a flag by name. Returns the new value, or `None` if unknown.
    pub fn toggle(&mut self, name: &str) -> Option<bool> {
        let field = match name {
            flag::EXTENDED_THINKING => &mut self.extended_thinking,
            flag::PROMPT_CACHING => &mut self.prompt_caching,
            flag::PROACTIVE_AGENTS => &mut self.proactive_agents,
            flag::REMOTE_AGENTS => &mut self.remote_agents,
            flag::TEAMMATE_TASKS => &mut self.teammate_tasks,
            flag::VOICE_INPUT => &mut self.voice_input,
            flag::VIM_MODE => &mut self.vim_mode,
            flag::LSP_INTEGRATION => &mut self.lsp_integration,
            flag::TELEMETRY => &mut self.telemetry,
            flag::ENTERPRISE => &mut self.enterprise,
            other => {
                let entry = self.extra.entry(other.to_string()).or_insert(false);
                *entry = !*entry;
                return Some(*entry);
            }
        };
        *field = !*field;
        Some(*field)
    }

    /// Set a flag to a specific value. Returns `true` if the value changed.
    pub fn set(&mut self, name: &str, value: bool) -> bool {
        let field = match name {
            flag::EXTENDED_THINKING => &mut self.extended_thinking,
            flag::PROMPT_CACHING => &mut self.prompt_caching,
            flag::PROACTIVE_AGENTS => &mut self.proactive_agents,
            flag::REMOTE_AGENTS => &mut self.remote_agents,
            flag::TEAMMATE_TASKS => &mut self.teammate_tasks,
            flag::VOICE_INPUT => &mut self.voice_input,
            flag::VIM_MODE => &mut self.vim_mode,
            flag::LSP_INTEGRATION => &mut self.lsp_integration,
            flag::TELEMETRY => &mut self.telemetry,
            flag::ENTERPRISE => &mut self.enterprise,
            other => {
                let entry = self.extra.entry(other.to_string()).or_insert(!value);
                let changed = *entry != value;
                *entry = value;
                return changed;
            }
        };
        let changed = *field != value;
        *field = value;
        changed
    }

    /// List all flags and their current values (known + extra).
    pub fn list_all(&self) -> Vec<(&str, bool)> {
        let mut flags = vec![
            (flag::EXTENDED_THINKING, self.extended_thinking),
            (flag::PROMPT_CACHING, self.prompt_caching),
            (flag::PROACTIVE_AGENTS, self.proactive_agents),
            (flag::REMOTE_AGENTS, self.remote_agents),
            (flag::TEAMMATE_TASKS, self.teammate_tasks),
            (flag::VOICE_INPUT, self.voice_input),
            (flag::VIM_MODE, self.vim_mode),
            (flag::LSP_INTEGRATION, self.lsp_integration),
            (flag::TELEMETRY, self.telemetry),
            (flag::ENTERPRISE, self.enterprise),
        ];
        // Sort extra by name for stable output.
        let mut extra: Vec<_> = self.extra.iter().collect();
        extra.sort_by_key(|(k, _)| (*k).clone());
        for (k, v) in extra {
            flags.push((k.as_str(), *v));
        }
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_flags_have_expected_values() {
        let flags = FeatureFlags::default();
        assert!(flags.is_enabled(flag::EXTENDED_THINKING));
        assert!(flags.is_enabled(flag::PROMPT_CACHING));
        assert!(!flags.is_enabled(flag::PROACTIVE_AGENTS));
        assert!(!flags.is_enabled(flag::REMOTE_AGENTS));
    }

    #[test]
    fn toggle_known_flag() {
        let mut flags = FeatureFlags::default();
        let new_val = flags.toggle(flag::PROACTIVE_AGENTS);
        assert_eq!(new_val, Some(true));
        assert!(flags.is_enabled(flag::PROACTIVE_AGENTS));
    }

    #[test]
    fn toggle_unknown_flag_creates_it() {
        let mut flags = FeatureFlags::default();
        assert!(!flags.is_enabled("custom_flag"));
        let val = flags.toggle("custom_flag");
        assert_eq!(val, Some(true));
        assert!(flags.is_enabled("custom_flag"));
    }

    #[test]
    fn set_returns_changed() {
        let mut flags = FeatureFlags::default();
        assert!(flags.set(flag::PROACTIVE_AGENTS, true));
        assert!(!flags.set(flag::PROACTIVE_AGENTS, true)); // no change
        assert!(flags.set(flag::PROACTIVE_AGENTS, false)); // changed back
    }

    #[test]
    fn list_all_includes_extra() {
        let mut flags = FeatureFlags::default();
        flags.extra.insert("my_flag".to_string(), true);
        let all = flags.list_all();
        assert!(all.iter().any(|(name, val)| *name == "my_flag" && *val));
    }

    #[test]
    fn deserialize_from_toml() {
        let toml_str = r#"
extended_thinking = false
prompt_caching = true
proactive_agents = true
remote_agents = false
teammate_tasks = false
voice_input = false
vim_mode = true
lsp_integration = false
custom_experiment = true
"#;
        let flags: FeatureFlags = toml::from_str(toml_str).unwrap();
        assert!(!flags.is_enabled(flag::EXTENDED_THINKING));
        assert!(flags.is_enabled(flag::PROACTIVE_AGENTS));
        assert!(flags.is_enabled(flag::VIM_MODE));
        assert!(flags.is_enabled("custom_experiment"));
    }
}
