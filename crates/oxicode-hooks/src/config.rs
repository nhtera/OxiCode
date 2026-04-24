//! Hook configuration: load hook definitions from settings.
//!
//! Each event maps to an **ordered list of matcher groups**. Each group has
//! an optional `matcher` regex (matched against `tool_name`) and a list of
//! `HookDef` entries to run when the matcher fires. `matcher: None` or empty
//! means "always match".
//!
//! This matches Claude Code's `settings.json` shape:
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       { "matcher": "Bash",     "hooks": [{"type":"command","command":"a.sh"}] },
//!       { "matcher": "FileWrite","hooks": [{"type":"command","command":"b.sh"}] },
//!       { "matcher": ".*",       "hooks": [{"type":"command","command":"c.sh"}] }
//!     ]
//!   }
//! }
//! ```
//!
//! TOML back-compat shapes all still load:
//!
//! ```toml
//! # 1) flat string — one always-on command hook.
//! [hooks]
//! session_start = "~/.oxicode/hooks/session-start.sh"
//!
//! # 2) flat HookDef table — one always-on hook with explicit type.
//! [hooks.UserPromptSubmit]
//! type = "agent"
//! instructions = "Check for PII"
//!
//! # 3) Claude-style array of matcher groups (array-of-tables).
//! [[hooks.PreToolUse]]
//! matcher = "Bash"
//! [[hooks.PreToolUse.hooks]]
//! type = "command"
//! command = "~/bin/pre-bash.sh"
//! ```
//!
//! `matcher` is a regex (Rust `regex` crate). Claude Code uses substring
//! semantics, but a substring pattern is a valid regex, so most existing
//! Claude configs work verbatim. Document this in user docs.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent_hook_executor::AgentHookConfig;
use crate::events::HookEvent;
use crate::http_hook_executor::HttpHookConfig;

/// Hook execution type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    /// Shell subprocess (default, backward compatible).
    #[default]
    Command,
    /// LLM agent call with structured output.
    Agent,
    /// HTTP POST to a URL endpoint.
    Http,
}

/// A single hook definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    /// Hook execution type (default: Command).
    #[serde(default, rename = "type")]
    pub hook_type: HookType,
    /// Shell command to execute (for Command type).
    #[serde(default)]
    pub command: String,
    /// Timeout in seconds (default 10 for command, 60 for agent, 600 for http).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Whether this hook is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Agent-specific config (only used when hook_type = Agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentHookConfig>,
    /// HTTP-specific config (only used when hook_type = Http).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpHookConfig>,

    // -- Inline agent fields (flat config alternative) --
    /// Agent instructions (shorthand: set type="agent" + instructions directly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Agent model override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    // -- Inline HTTP fields (flat config alternative) --
    /// HTTP URL (shorthand: set type="http" + url directly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP authorization header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
}

fn default_timeout() -> u64 {
    10
}

fn default_enabled() -> bool {
    true
}

impl HookDef {
    /// Build an `AgentHookConfig` from this definition (merging inline fields).
    pub fn agent_config(&self) -> AgentHookConfig {
        if let Some(ref cfg) = self.agent {
            return cfg.clone();
        }
        AgentHookConfig {
            instructions: self.instructions.clone().unwrap_or_default(),
            model: self
                .model
                .clone()
                .unwrap_or_else(|| "claude-haiku".to_string()),
            timeout_secs: self.timeout_secs,
            max_tokens: 256,
        }
    }

    /// Build an `HttpHookConfig` from this definition (merging inline fields).
    pub fn http_config(&self) -> HttpHookConfig {
        if let Some(ref cfg) = self.http {
            return cfg.clone();
        }
        HttpHookConfig {
            url: self.url.clone().unwrap_or_default(),
            authorization: self.authorization.clone(),
            env_headers: std::collections::HashMap::new(),
            timeout_secs: self.timeout_secs,
        }
    }

    /// Simple no-matcher command hook (convenience for back-compat shapes).
    fn command_only(command: String) -> Self {
        Self {
            hook_type: HookType::Command,
            command,
            timeout_secs: default_timeout(),
            enabled: true,
            agent: None,
            http: None,
            instructions: None,
            model: None,
            url: None,
            authorization: None,
        }
    }
}

/// A matcher group: one `matcher` regex + the list of hooks to run when it
/// fires. `None`/empty matcher means "always fire for this event".
///
/// The `compiled` field caches the parsed regex so we don't pay
/// `Regex::new` on every hook fire. It's populated by [`MatcherHookGroup::compile`]
/// (called by the `HooksConfig` load paths) and skipped by serde so user-facing
/// config round-trips stay clean.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatcherHookGroup {
    /// Regex matched against tool name (or arbitrary payload field in future).
    /// `None` or empty string → matches every event for this hook point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    /// Hooks that run when this group matches.
    #[serde(default)]
    pub hooks: Vec<HookDef>,
    /// Lazily compiled regex (load-time cache; invalid regexes stay `None`).
    #[serde(skip)]
    compiled: Option<regex::Regex>,
}

impl MatcherHookGroup {
    /// Build a group with a matcher + hooks, compiling the regex eagerly.
    /// Invalid patterns log a warning and leave `compiled = None` (fail-closed).
    pub fn new(matcher: Option<String>, hooks: Vec<HookDef>) -> Self {
        let mut g = Self {
            matcher,
            hooks,
            compiled: None,
        };
        g.compile();
        g
    }

    /// Compile the matcher regex once at load time; warn and discard on invalid
    /// patterns (the group will fail-closed for any tool name, matching the old
    /// per-fire behavior while avoiding the repeated warn log).
    pub fn compile(&mut self) {
        let Some(pattern) = self.matcher.as_deref() else {
            return;
        };
        if pattern.is_empty() {
            return;
        }
        match regex::Regex::new(pattern) {
            Ok(re) => self.compiled = Some(re),
            Err(e) => {
                tracing::warn!("Invalid hook matcher regex {pattern:?}: {e}");
                self.compiled = None;
            }
        }
    }

    /// Does this group's matcher accept the given `tool_name`?
    ///
    /// - No matcher or empty string → always matches.
    /// - Matcher present but `tool_name` is `None` → no match (can't evaluate).
    /// - Matcher was an invalid regex at load time → no match (fail-closed).
    pub fn matches(&self, tool_name: Option<&str>) -> bool {
        let Some(pattern) = self.matcher.as_deref() else {
            return true;
        };
        if pattern.is_empty() {
            return true;
        }
        let Some(name) = tool_name else {
            return false;
        };
        // Hot path: use the pre-compiled regex. If it's missing, the pattern
        // was invalid at load time — fail-closed (same behavior as before).
        self.compiled.as_ref().is_some_and(|re| re.is_match(name))
    }

    /// Iterate enabled hooks only.
    pub fn enabled_hooks(&self) -> impl Iterator<Item = &HookDef> {
        self.hooks.iter().filter(|h| h.enabled)
    }
}

/// All hook configurations: one `Vec<MatcherHookGroup>` per event name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(flatten)]
    pub hooks: HashMap<String, Vec<MatcherHookGroup>>,
}

impl HooksConfig {
    /// Load hooks from a TOML table value (the `[hooks]` section of settings).
    ///
    /// Accepts all three back-compat shapes (see module docs).
    pub fn from_toml_value(value: &toml::Value) -> Self {
        let Some(table) = value.as_table() else {
            return Self::default();
        };

        let mut hooks: HashMap<String, Vec<MatcherHookGroup>> = HashMap::new();
        for (key, val) in table {
            let groups = match val {
                // Simple string: `SessionStart = "cmd"` → one no-matcher group with one Command hook.
                toml::Value::String(cmd) => vec![MatcherHookGroup {
                    matcher: None,
                    hooks: vec![HookDef::command_only(cmd.clone())],
                    compiled: None,
                }],
                // Table: either a single HookDef (old flat) or a single MatcherHookGroup (new).
                // Heuristic: if the table has a `hooks` array key, treat as MatcherHookGroup.
                toml::Value::Table(t) => {
                    if matches!(t.get("hooks"), Some(toml::Value::Array(_))) {
                        let Some(g) = parse_toml::<MatcherHookGroup>(val) else {
                            tracing::warn!("Invalid hook matcher group for '{key}'");
                            continue;
                        };
                        vec![g]
                    } else {
                        let Some(def) = parse_toml::<HookDef>(val) else {
                            tracing::warn!("Invalid hook config for '{key}'");
                            continue;
                        };
                        vec![MatcherHookGroup {
                            matcher: None,
                            hooks: vec![def],
                            compiled: None,
                        }]
                    }
                }
                // Array of tables: new Claude-style multi-matcher shape.
                toml::Value::Array(arr) => {
                    let mut out = Vec::with_capacity(arr.len());
                    for v in arr {
                        if let Some(g) = parse_toml::<MatcherHookGroup>(v) {
                            out.push(g);
                        } else {
                            tracing::warn!("Invalid matcher group in '{key}' array");
                        }
                    }
                    if out.is_empty() {
                        continue;
                    }
                    out
                }
                _ => continue,
            };
            hooks.insert(key.clone(), groups);
        }

        let mut cfg = Self { hooks };
        cfg.compile_all();
        cfg
    }

    /// Compile every group's matcher regex exactly once. Called after any
    /// load/merge path so `MatcherHookGroup::matches` hits the pre-compiled
    /// cache on the hot path.
    fn compile_all(&mut self) {
        for groups in self.hooks.values_mut() {
            for g in groups {
                g.compile();
            }
        }
    }

    /// All matcher groups registered for an event. Returns empty slice if none.
    pub fn groups_for(&self, event: HookEvent) -> &[MatcherHookGroup] {
        self.hooks.get(event.as_str()).map_or(&[], Vec::as_slice)
    }

    /// All enabled hooks that should fire for `event` given `tool_name`.
    ///
    /// Groups whose matcher rejects `tool_name` are skipped. Within each
    /// matching group, hooks run in declaration order; groups themselves run
    /// in declaration order.
    pub fn matching_hooks<'a>(
        &'a self,
        event: HookEvent,
        tool_name: Option<&'a str>,
    ) -> impl Iterator<Item = &'a HookDef> {
        self.groups_for(event)
            .iter()
            .filter(move |g| g.matches(tool_name))
            .flat_map(MatcherHookGroup::enabled_hooks)
    }

    /// Does any group exist for this event? (Used by `has_hook` fast path —
    /// does NOT check matcher, since callers typically don't have the tool
    /// name handy and we'd rather fire and no-op than miss a hook.)
    pub fn has_event(&self, event: HookEvent) -> bool {
        self.hooks
            .get(event.as_str())
            .is_some_and(|groups| groups.iter().any(|g| g.enabled_hooks().next().is_some()))
    }

    /// Legacy: return the first enabled hook across all groups for this event.
    /// Retained so older call sites (e.g. `/hooks` listing, tests) keep working
    /// during the migration window.
    pub fn get(&self, event: HookEvent) -> Option<&HookDef> {
        self.hooks
            .get(event.as_str())
            .into_iter()
            .flatten()
            .flat_map(MatcherHookGroup::enabled_hooks)
            .next()
    }

    /// Load hooks config from the user settings file.
    pub fn load_from_settings_dir() -> Self {
        let config_dir =
            dirs::home_dir().map_or_else(|| PathBuf::from(".oxicode"), |h| h.join(".oxicode"));

        let settings_path = config_dir.join("settings.toml");
        let Ok(content) = std::fs::read_to_string(&settings_path) else {
            return Self::default();
        };

        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };

        parsed
            .get("hooks")
            .map(Self::from_toml_value)
            .unwrap_or_default()
    }

    /// Layered loader (Claude Code parity, oxicode-overlay-wins precedence).
    ///
    /// Read order (low → high precedence):
    /// 1. `~/.claude/settings.json` (user)
    /// 2. project `.claude/settings.json`
    /// 3. project `.claude/settings.local.json`
    /// 4. `~/.oxicode/settings.toml` overlay (oxicode wins per user choice)
    ///
    /// Each layer's hooks are merged into a single `HooksConfig`. Later
    /// layers' groups for the same event REPLACE earlier ones.
    pub fn load_layered() -> Self {
        let mut merged = Self::default();

        // Layer 1: ~/.claude/settings.json
        if let Some(home) = dirs::home_dir() {
            let user_claude = home.join(".claude").join("settings.json");
            merge_into(&mut merged, &load_claude_json(&user_claude));
        }

        // Layer 2: project .claude/settings.json (relative to cwd)
        let project_claude = std::path::PathBuf::from(".claude").join("settings.json");
        merge_into(&mut merged, &load_claude_json(&project_claude));

        // Layer 3: project .claude/settings.local.json
        let project_local = std::path::PathBuf::from(".claude").join("settings.local.json");
        merge_into(&mut merged, &load_claude_json(&project_local));

        // Layer 4: ~/.oxicode/settings.toml overlay (highest precedence).
        merge_into(&mut merged, &Self::load_from_settings_dir());

        merged
    }
}

/// Roundtrip a `toml::Value` through string form to deserialize into `T`.
/// Matches the pattern used by the old single-HookDef parse; not ideal but
/// works for any deserialize-able shape.
fn parse_toml<T: serde::de::DeserializeOwned>(value: &toml::Value) -> Option<T> {
    let s = toml::to_string(value).ok()?;
    toml::from_str::<T>(&s).ok()
}

/// Merge `src` into `dst`, with `src` entries replacing `dst` entries on conflict.
fn merge_into(dst: &mut HooksConfig, src: &HooksConfig) {
    for (key, groups) in &src.hooks {
        dst.hooks.insert(key.clone(), groups.clone());
    }
}

/// Read a Claude-Code-style `settings.json` file and convert its `hooks`
/// section to our `HooksConfig`. Returns empty config if the file is missing
/// or malformed (fail-soft — hooks should never break the app).
fn load_claude_json(path: &std::path::Path) -> HooksConfig {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HooksConfig::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        tracing::warn!("Failed to parse Claude settings JSON at {path:?}");
        return HooksConfig::default();
    };
    let Some(hooks_section) = parsed.get("hooks") else {
        return HooksConfig::default();
    };
    HooksConfig::from_claude_json(hooks_section)
}

impl HooksConfig {
    /// Parse Claude Code's `hooks` JSON section into full multi-group shape.
    ///
    /// Schema:
    /// ```json
    /// {
    ///   "PreToolUse": [
    ///     { "matcher": "Bash|Edit",
    ///       "hooks": [{"type":"command","command":"...","timeout":30}] },
    ///     { "matcher": ".*",
    ///       "hooks": [{"type":"command","command":"log.sh"}] }
    ///   ]
    /// }
    /// ```
    ///
    /// All matcher groups and all hooks within each group are preserved.
    pub fn from_claude_json(value: &serde_json::Value) -> Self {
        let Some(obj) = value.as_object() else {
            return Self::default();
        };
        let mut hooks: HashMap<String, Vec<MatcherHookGroup>> = HashMap::new();
        for (event_name, event_value) in obj {
            let Some(arr) = event_value.as_array() else {
                continue;
            };
            let mut groups = Vec::with_capacity(arr.len());
            for matcher_group in arr {
                match serde_json::from_value::<MatcherHookGroup>(matcher_group.clone()) {
                    Ok(g) => groups.push(g),
                    Err(e) => tracing::warn!("Skipping malformed hook group for {event_name}: {e}"),
                }
            }
            if !groups.is_empty() {
                hooks.insert(event_name.clone(), groups);
            }
        }
        let mut cfg = Self { hooks };
        cfg.compile_all();
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_hook(c: &HooksConfig, event: HookEvent) -> &HookDef {
        c.get(event).expect("hook configured")
    }

    #[test]
    fn test_simple_string_form_pascal_case() {
        let toml_str = r#"
SessionStart = "echo hello"
UserPromptSubmit = "~/.claude/hooks/pre-prompt.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 2);
        let hook = first_hook(&config, HookEvent::SessionStart);
        assert_eq!(hook.command, "echo hello");
        assert_eq!(hook.hook_type, HookType::Command);
    }

    #[test]
    fn test_table_form_command() {
        let toml_str = r#"
[UserPromptSubmit]
command = "python3 check.py"
timeout_secs = 5
enabled = true
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = first_hook(&config, HookEvent::UserPromptSubmit);
        assert_eq!(hook.command, "python3 check.py");
        assert_eq!(hook.timeout_secs, 5);
        assert_eq!(hook.hook_type, HookType::Command);
    }

    #[test]
    fn test_table_form_agent() {
        let toml_str = r#"
[PreToolUse]
type = "agent"
instructions = "Check for PII"
model = "claude-haiku"
timeout_secs = 30
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = first_hook(&config, HookEvent::PreToolUse);
        assert_eq!(hook.hook_type, HookType::Agent);
        let agent_cfg = hook.agent_config();
        assert_eq!(agent_cfg.instructions, "Check for PII");
        assert_eq!(agent_cfg.model, "claude-haiku");
        assert_eq!(agent_cfg.timeout_secs, 30);
    }

    #[test]
    fn test_table_form_http() {
        let toml_str = r#"
[Stop]
type = "http"
url = "https://hooks.example.com/event"
authorization = "Bearer abc"
timeout_secs = 120
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = first_hook(&config, HookEvent::Stop);
        assert_eq!(hook.hook_type, HookType::Http);
        let http_cfg = hook.http_config();
        assert_eq!(http_cfg.url, "https://hooks.example.com/event");
        assert_eq!(http_cfg.authorization.as_deref(), Some("Bearer abc"));
        assert_eq!(http_cfg.timeout_secs, 120);
    }

    #[test]
    fn test_disabled_hook_hidden_from_get() {
        let toml_str = r#"
[SessionStart]
command = "echo disabled"
enabled = false
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_hook_type_default_is_command() {
        assert_eq!(HookType::default(), HookType::Command);
    }

    #[test]
    fn test_empty_config() {
        let config = HooksConfig::default();
        assert!(config.hooks.is_empty());
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_from_non_table_value() {
        let value: toml::Value = toml::Value::String("not a table".to_string());
        let config = HooksConfig::from_toml_value(&value);
        assert!(config.hooks.is_empty());
    }

    #[test]
    fn test_default_timeout() {
        let toml_str = r#"SessionStart = "echo hello""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let hook = first_hook(&config, HookEvent::SessionStart);
        assert_eq!(hook.timeout_secs, 10);
    }

    #[test]
    fn test_unknown_event_key_stored_but_not_retrievable() {
        let toml_str = r#"NotARealEvent = "noop.sh""#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 1);
        assert!(config.get(HookEvent::SessionStart).is_none());
    }

    #[test]
    fn test_old_snake_case_keys_do_not_match_new_events() {
        let toml_str = r#"
tool_call_before = "old.sh"
pre_query = "old.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        assert_eq!(config.hooks.len(), 2, "raw keys still stored");
        assert!(config.get(HookEvent::PreToolUse).is_none());
        assert!(config.get(HookEvent::UserPromptSubmit).is_none());
    }

    #[test]
    fn test_all_events_retrievable() {
        let mut hooks: HashMap<String, Vec<MatcherHookGroup>> = HashMap::new();
        for event in HookEvent::ALL {
            hooks.insert(
                event.as_str().to_string(),
                vec![MatcherHookGroup {
                    matcher: None,
                    hooks: vec![HookDef::command_only(format!("hook-{}.sh", event.as_str()))],
                    compiled: None,
                }],
            );
        }
        let config = HooksConfig { hooks };
        for event in HookEvent::ALL {
            let hook = config.get(*event);
            assert!(hook.is_some(), "Event {event:?} should be retrievable");
            assert!(hook.unwrap().command.contains(event.as_str()));
        }
    }

    #[test]
    fn test_from_claude_json_single_matcher() {
        let json = serde_json::json!({
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "echo pre", "timeout_secs": 5}
                    ]
                }
            ],
            "SessionStart": [
                {
                    "hooks": [{"type": "command", "command": "init.sh"}]
                }
            ]
        });
        let config = HooksConfig::from_claude_json(&json);
        let pre = config.get(HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.command, "echo pre");
        let start = config.get(HookEvent::SessionStart).unwrap();
        assert_eq!(start.command, "init.sh");
    }

    #[test]
    fn test_from_claude_json_multi_matcher_preserves_all_groups() {
        let json = serde_json::json!({
            "PreToolUse": [
                {"matcher": "Bash",      "hooks": [{"type":"command","command":"a.sh"}]},
                {"matcher": "FileWrite", "hooks": [{"type":"command","command":"b.sh"}]},
                {"matcher": ".*",        "hooks": [{"type":"command","command":"c.sh"}]}
            ]
        });
        let config = HooksConfig::from_claude_json(&json);
        let groups = config.groups_for(HookEvent::PreToolUse);
        assert_eq!(groups.len(), 3, "all 3 matcher groups preserved");
        assert_eq!(groups[0].matcher.as_deref(), Some("Bash"));
        assert_eq!(groups[0].hooks[0].command, "a.sh");
        assert_eq!(groups[1].matcher.as_deref(), Some("FileWrite"));
        assert_eq!(groups[2].matcher.as_deref(), Some(".*"));
    }

    #[test]
    fn test_from_claude_json_multiple_hooks_per_group() {
        let json = serde_json::json!({
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type":"command","command":"first.sh"},
                        {"type":"command","command":"second.sh"}
                    ]
                }
            ]
        });
        let config = HooksConfig::from_claude_json(&json);
        let groups = config.groups_for(HookEvent::PreToolUse);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].hooks.len(), 2);
        assert_eq!(groups[0].hooks[0].command, "first.sh");
        assert_eq!(groups[0].hooks[1].command, "second.sh");
    }

    #[test]
    fn test_from_claude_json_ignores_malformed_entries() {
        let json = serde_json::json!({
            "PreToolUse": "not an array",
        });
        let config = HooksConfig::from_claude_json(&json);
        assert!(config.hooks.is_empty());
    }

    #[test]
    fn test_matcher_group_matches_semantics() {
        let g = MatcherHookGroup::new(None, vec![]);
        assert!(g.matches(Some("Bash")));
        assert!(
            g.matches(None),
            "no matcher → always matches, even with no tool name"
        );

        let g = MatcherHookGroup::new(Some(String::new()), vec![]);
        assert!(
            g.matches(Some("Bash")),
            "empty matcher treated as no matcher"
        );

        let g = MatcherHookGroup::new(Some("Bash".to_string()), vec![]);
        assert!(g.matches(Some("Bash")));
        assert!(!g.matches(Some("FileWrite")));
        assert!(!g.matches(None), "matcher set but no tool name → no match");

        let g = MatcherHookGroup::new(Some("Bash|Edit".to_string()), vec![]);
        assert!(g.matches(Some("Bash")));
        assert!(g.matches(Some("Edit")));
        assert!(!g.matches(Some("Read")));

        let g = MatcherHookGroup::new(Some("[invalid".to_string()), vec![]);
        assert!(!g.matches(Some("Bash")), "invalid regex fails closed");
    }

    #[test]
    fn test_matching_hooks_filters_by_matcher() {
        let json = serde_json::json!({
            "PreToolUse": [
                {"matcher": "Bash",      "hooks": [{"type":"command","command":"a.sh"}]},
                {"matcher": "FileWrite", "hooks": [{"type":"command","command":"b.sh"}]},
                {"matcher": ".*",        "hooks": [{"type":"command","command":"c.sh"}]}
            ]
        });
        let config = HooksConfig::from_claude_json(&json);

        let bash: Vec<_> = config
            .matching_hooks(HookEvent::PreToolUse, Some("Bash"))
            .map(|h| h.command.as_str())
            .collect();
        assert_eq!(bash, vec!["a.sh", "c.sh"], "Bash + .* catch-all fire");

        let write: Vec<_> = config
            .matching_hooks(HookEvent::PreToolUse, Some("FileWrite"))
            .map(|h| h.command.as_str())
            .collect();
        assert_eq!(write, vec!["b.sh", "c.sh"]);

        let read: Vec<_> = config
            .matching_hooks(HookEvent::PreToolUse, Some("Read"))
            .map(|h| h.command.as_str())
            .collect();
        assert_eq!(read, vec!["c.sh"], "only catch-all matches");
    }

    #[test]
    fn test_matching_hooks_skips_disabled() {
        let json = serde_json::json!({
            "PreToolUse": [
                {"matcher": ".*", "hooks": [
                    {"type":"command","command":"on.sh"},
                    {"type":"command","command":"off.sh","enabled":false}
                ]}
            ]
        });
        let config = HooksConfig::from_claude_json(&json);
        let fired: Vec<_> = config
            .matching_hooks(HookEvent::PreToolUse, Some("Bash"))
            .map(|h| h.command.as_str())
            .collect();
        assert_eq!(fired, vec!["on.sh"]);
    }

    #[test]
    fn test_toml_array_of_tables_multi_matcher() {
        let toml_str = r#"
[[PreToolUse]]
matcher = "Bash"
[[PreToolUse.hooks]]
type = "command"
command = "bash.sh"

[[PreToolUse]]
matcher = "FileWrite"
[[PreToolUse.hooks]]
type = "command"
command = "write.sh"
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let config = HooksConfig::from_toml_value(&value);
        let groups = config.groups_for(HookEvent::PreToolUse);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].matcher.as_deref(), Some("Bash"));
        assert_eq!(groups[0].hooks[0].command, "bash.sh");
        assert_eq!(groups[1].matcher.as_deref(), Some("FileWrite"));
        assert_eq!(groups[1].hooks[0].command, "write.sh");
    }

    #[test]
    fn test_has_event_respects_enabled() {
        let json = serde_json::json!({
            "PreToolUse": [
                {"matcher": ".*", "hooks": [
                    {"type":"command","command":"off.sh","enabled":false}
                ]}
            ]
        });
        let config = HooksConfig::from_claude_json(&json);
        assert!(!config.has_event(HookEvent::PreToolUse));
    }
}
