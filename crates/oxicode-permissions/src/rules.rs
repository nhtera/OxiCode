use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::pipeline::PermissionDecision;

/// A permission rule from config (CLAUDE.md or settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Tool name to match.
    pub tool: String,
    /// Optional regex pattern to match against tool input (stringified).
    pub input_pattern: Option<String>,
    /// Action: allow, deny, or ask.
    pub action: RuleAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleAction {
    Allow,
    Deny,
    Ask,
}

impl PermissionRule {
    pub fn allow(tool: &str, pattern: Option<&str>) -> Self {
        Self {
            tool: tool.to_string(),
            input_pattern: pattern.map(String::from),
            action: RuleAction::Allow,
        }
    }

    pub fn deny(tool: &str, pattern: Option<&str>) -> Self {
        Self {
            tool: tool.to_string(),
            input_pattern: pattern.map(String::from),
            action: RuleAction::Deny,
        }
    }

    pub fn ask(tool: &str, pattern: Option<&str>) -> Self {
        Self {
            tool: tool.to_string(),
            input_pattern: pattern.map(String::from),
            action: RuleAction::Ask,
        }
    }
}

/// Matches tool invocations against a list of permission rules.
pub struct RuleMatcher {
    rules: Vec<CompiledRule>,
}

struct CompiledRule {
    tool: String,
    input_regex: Option<Regex>,
    action: RuleAction,
}

impl RuleMatcher {
    pub fn new(rules: Vec<PermissionRule>) -> Self {
        let compiled = rules
            .into_iter()
            .map(|r| {
                let input_regex = r.input_pattern.as_ref().and_then(|p| Regex::new(p).ok());
                CompiledRule {
                    tool: r.tool,
                    input_regex,
                    action: r.action,
                }
            })
            .collect();
        Self { rules: compiled }
    }

    /// Check tool invocation against rules. First matching rule wins.
    pub fn check(&self, tool_name: &str, input: &serde_json::Value) -> Option<PermissionDecision> {
        let input_str = input.to_string();

        for rule in &self.rules {
            if rule.tool != tool_name && rule.tool != "*" {
                continue;
            }

            // If rule has an input pattern, check it.
            if let Some(ref re) = rule.input_regex {
                if !re.is_match(&input_str) {
                    continue;
                }
            }

            return Some(match rule.action {
                RuleAction::Allow => PermissionDecision::Allow,
                RuleAction::Deny => {
                    PermissionDecision::Deny(format!("Denied by rule: {tool_name}"))
                }
                RuleAction::Ask => {
                    PermissionDecision::Ask(format!("Rule requires approval for {tool_name}"))
                }
            });
        }

        None // No rule matched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let rules = vec![PermissionRule::allow("bash", None)];
        let matcher = RuleMatcher::new(rules);
        let result = matcher.check("bash", &serde_json::json!({}));
        assert_eq!(result, Some(PermissionDecision::Allow));
    }

    #[test]
    fn test_no_match() {
        let rules = vec![PermissionRule::allow("bash", None)];
        let matcher = RuleMatcher::new(rules);
        let result = matcher.check("file_write", &serde_json::json!({}));
        assert_eq!(result, None);
    }

    #[test]
    fn test_pattern_match() {
        let rules = vec![PermissionRule::allow("bash", Some("cargo test"))];
        let matcher = RuleMatcher::new(rules);

        let hit = matcher.check("bash", &serde_json::json!({"command": "cargo test"}));
        assert_eq!(hit, Some(PermissionDecision::Allow));

        let miss = matcher.check("bash", &serde_json::json!({"command": "rm -rf /"}));
        assert_eq!(miss, None);
    }

    #[test]
    fn test_wildcard_tool() {
        let rules = vec![PermissionRule::deny("*", Some("password"))];
        let matcher = RuleMatcher::new(rules);
        let result = matcher.check(
            "file_write",
            &serde_json::json!({"content": "password=secret"}),
        );
        assert!(matches!(result, Some(PermissionDecision::Deny(_))));
    }

    #[test]
    fn test_first_rule_wins() {
        let rules = vec![
            PermissionRule::deny("bash", Some("rm")),
            PermissionRule::allow("bash", None),
        ];
        let matcher = RuleMatcher::new(rules);
        let result = matcher.check("bash", &serde_json::json!({"command": "rm file"}));
        assert!(matches!(result, Some(PermissionDecision::Deny(_))));
    }
}
