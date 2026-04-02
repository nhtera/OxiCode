use crate::command_security::CommandSecurityChecker;
use crate::dangerous::DangerousPatternDetector;
use crate::rules::{PermissionRule, RuleMatcher};
use crate::tracker::DenialTracker;

/// Result of the permission pipeline evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Tool execution is allowed.
    Allow,
    /// Tool execution is denied with a reason.
    Deny(String),
    /// User must be prompted for approval.
    Ask(String),
}

/// Permission mode set by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Normal mode — proceed through all layers.
    Default,
    /// Bypass all permission checks (trust everything).
    Bypass,
    /// Always ask for approval on non-readonly tools.
    ApprovalOnly,
}

impl PermissionMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "bypass" => Self::Bypass,
            "approval_only" | "accept_edits" => Self::ApprovalOnly,
            _ => Self::Default,
        }
    }
}

/// Permission level of the tool being checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionLevel {
    ReadOnly,
    FileWrite,
    ShellExec,
    System,
}

/// 6-layer permission pipeline.
pub struct PermissionPipeline {
    mode: PermissionMode,
    rule_matcher: RuleMatcher,
    dangerous_detector: DangerousPatternDetector,
    command_security: CommandSecurityChecker,
    tracker: DenialTracker,
}

impl PermissionPipeline {
    pub fn new(mode: PermissionMode, rules: Vec<PermissionRule>) -> Self {
        Self {
            mode,
            rule_matcher: RuleMatcher::new(rules),
            dangerous_detector: DangerousPatternDetector::new(),
            command_security: CommandSecurityChecker::new(),
            tracker: DenialTracker::new(),
        }
    }

    /// Evaluate permission for a tool invocation.
    pub fn check(
        &self,
        tool_name: &str,
        tool_level: ToolPermissionLevel,
        input: &serde_json::Value,
    ) -> PermissionDecision {
        // Layer 1: Safe allowlist — read-only tools are always allowed.
        if tool_level == ToolPermissionLevel::ReadOnly {
            return PermissionDecision::Allow;
        }

        // Layer 2: Command security (hard deny — ALWAYS checked, even in bypass).
        if tool_level == ToolPermissionLevel::ShellExec {
            if let Some(reason) = self.command_security.check(input) {
                return PermissionDecision::Deny(reason);
            }
        }

        // Layer 3: Dangerous pattern detection (ALWAYS checked, even in bypass).
        if let Some(reason) = self.dangerous_detector.check(tool_name, input) {
            // In bypass mode, log warning but allow (Ask becomes Allow).
            if self.mode == PermissionMode::Bypass {
                tracing::warn!(
                    "Bypass mode: dangerous pattern detected for {tool_name}: {reason}"
                );
                return PermissionDecision::Allow;
            }
            return PermissionDecision::Ask(reason);
        }

        // Layer 4: Permission mode (only reached after security checks pass).
        match self.mode {
            PermissionMode::Bypass => return PermissionDecision::Allow,
            PermissionMode::ApprovalOnly => {
                return PermissionDecision::Ask(format!(
                    "Approval required for {tool_name} (approval-only mode)"
                ));
            }
            PermissionMode::Default => {}
        }

        // Layer 5: Rule matching from config (user rules only apply to safe inputs).
        if let Some(decision) = self.rule_matcher.check(tool_name, input) {
            return decision;
        }

        // Layer 6: Default — ask for non-readonly tools.
        PermissionDecision::Ask(format!("Approve {tool_name}?"))
    }

    /// Record a denial for tracking/analytics.
    pub fn record_denial(&self, tool_name: &str, reason: &str) {
        self.tracker.record(tool_name, reason);
    }

    /// Get denial history.
    pub fn denial_history(&self) -> Vec<(String, String, chrono::DateTime<chrono::Utc>)> {
        self.tracker.history()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readonly_always_allowed() {
        let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);
        let decision = pipeline.check(
            "file_read",
            ToolPermissionLevel::ReadOnly,
            &serde_json::json!({}),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_bypass_mode() {
        let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({}),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_approval_only_mode() {
        let pipeline = PermissionPipeline::new(PermissionMode::ApprovalOnly, vec![]);
        let decision = pipeline.check(
            "file_write",
            ToolPermissionLevel::FileWrite,
            &serde_json::json!({}),
        );
        assert!(matches!(decision, PermissionDecision::Ask(_)));
    }

    #[test]
    fn test_default_mode_asks() {
        let pipeline = PermissionPipeline::new(PermissionMode::Default, vec![]);
        let decision = pipeline.check(
            "file_write",
            ToolPermissionLevel::FileWrite,
            &serde_json::json!({}),
        );
        assert!(matches!(decision, PermissionDecision::Ask(_)));
    }

    #[test]
    fn test_rule_allow() {
        let rules = vec![PermissionRule::allow("bash", Some("echo.*"))];
        let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({"command": "echo hello"}),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_rule_deny() {
        // Use a non-dangerous command so the deny rule fires (not dangerous detector).
        let rules = vec![PermissionRule::deny("bash", Some("deploy"))];
        let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({"command": "deploy production"}),
        );
        assert!(matches!(decision, PermissionDecision::Deny(_)));
    }

    #[test]
    fn test_dangerous_overrides_allow_rule() {
        // Even with an allow-all rule, dangerous commands are still caught.
        let rules = vec![PermissionRule::allow("bash", None)];
        let pipeline = PermissionPipeline::new(PermissionMode::Default, rules);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({"command": "rm -rf /"}),
        );
        // Should be Ask (dangerous pattern), NOT Allow (rule).
        assert!(matches!(decision, PermissionDecision::Ask(_)));
    }

    #[test]
    fn test_bypass_mode_still_blocks_command_security() {
        // Bypass mode must NOT skip command security hard-denies.
        let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({"command": "export LD_PRELOAD=/evil.so && ./app"}),
        );
        assert!(matches!(decision, PermissionDecision::Deny(_)));
    }

    #[test]
    fn test_bypass_mode_allows_dangerous_with_warning() {
        // Bypass mode logs warning for dangerous patterns but still allows.
        let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({"command": "rm -rf /"}),
        );
        // Dangerous pattern detected, but bypass overrides Ask→Allow.
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_bypass_mode_allows_safe_commands() {
        let pipeline = PermissionPipeline::new(PermissionMode::Bypass, vec![]);
        let decision = pipeline.check(
            "bash",
            ToolPermissionLevel::ShellExec,
            &serde_json::json!({"command": "echo hello"}),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }
}
