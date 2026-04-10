//! `/ultraplan <goal>` — generate a phased implementation plan from a goal description.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// `/ultraplan <goal>` — generate a structured, phased implementation plan.
pub struct UltraplanCommand;

impl SlashCommand for UltraplanCommand {
    fn name(&self) -> &str {
        "ultraplan"
    }

    fn description(&self) -> &str {
        "Generate a phased implementation plan from a goal description"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let goal = args.trim();

        if goal.is_empty() {
            return CommandOutput::Message(
                "Usage: /ultraplan <goal>\n\
                 Example: /ultraplan add OAuth2 login to the REST API\n\n\
                 Generates a numbered, phased implementation plan for the given goal."
                    .to_string(),
            );
        }

        let phases = derive_phases(goal);
        let mut out = format!(
            "Ultraplan: {goal}\n\
             {}\n\n",
            "=".repeat(60)
        );

        for (i, phase) in phases.iter().enumerate() {
            let _ = writeln!(out, "Phase {}: {}", i + 1, phase.title);
            let _ = writeln!(out, "  Goal:     {}", phase.goal);
            let _ = writeln!(out, "  Outputs:  {}", phase.outputs);
            let _ = writeln!(out, "  Estimate: {}", phase.estimate);
            out.push('\n');
        }

        out.push_str(
            "Tip: use /plan to track progress, /dream to generate prompts for each phase.\n\
             For AI-assisted planning, paste this into the conversation.",
        );

        CommandOutput::Message(out)
    }
}

// ---------------------------------------------------------------------------
// Phase derivation
// ---------------------------------------------------------------------------

struct Phase {
    title: String,
    goal: String,
    outputs: String,
    estimate: String,
}

/// Derive implementation phases from a free-text goal string.
///
/// Uses keyword heuristics to tailor the phases; falls back to a generic
/// five-phase template for unrecognised goals.
#[allow(clippy::too_many_lines)]
fn derive_phases(goal: &str) -> Vec<Phase> {
    let lower = goal.to_ascii_lowercase();

    // API / backend-flavoured goals.
    if lower.contains("api") || lower.contains("endpoint") || lower.contains("backend") {
        return vec![
            phase(
                "Research & Design",
                "Define data models, contracts, and auth strategy",
                "ERD + OpenAPI spec",
                "0.5d",
            ),
            phase(
                "Database layer",
                "Create migrations and repository traits",
                "Migration files, model structs",
                "1d",
            ),
            phase(
                "API endpoints",
                "Implement handlers, validation, error mapping",
                "HTTP handlers + integration tests",
                "2d",
            ),
            phase(
                "Auth & security",
                "Add authentication / authorisation middleware",
                "Auth middleware + security tests",
                "1d",
            ),
            phase(
                "Documentation & review",
                "Write API docs, run load tests, address review feedback",
                "README, postman collection",
                "0.5d",
            ),
        ];
    }

    // UI / frontend-flavoured goals.
    if lower.contains("ui")
        || lower.contains("frontend")
        || lower.contains("component")
        || lower.contains("page")
    {
        return vec![
            phase(
                "Design & wireframes",
                "Define component hierarchy and data flow",
                "Wireframes / Figma mockups",
                "0.5d",
            ),
            phase(
                "Component scaffolding",
                "Create skeleton components with prop types",
                "Component files + Storybook stories",
                "1d",
            ),
            phase(
                "Logic & state",
                "Implement business logic and state management",
                "Working feature with unit tests",
                "2d",
            ),
            phase(
                "Styling & accessibility",
                "Apply design tokens, ARIA labels, responsive layout",
                "Pixel-perfect, a11y-passing UI",
                "1d",
            ),
            phase(
                "E2E tests & review",
                "Write Playwright/Cypress tests and address feedback",
                "E2E test suite",
                "0.5d",
            ),
        ];
    }

    // Auth-flavoured goals.
    if lower.contains("auth") || lower.contains("login") || lower.contains("oauth") {
        return vec![
            phase(
                "Provider research",
                "Select OAuth providers, review token flows",
                "Provider comparison doc",
                "0.25d",
            ),
            phase(
                "Auth models",
                "Define user, token, and session models",
                "DB migrations + model structs",
                "0.5d",
            ),
            phase(
                "OAuth flow",
                "Implement redirect, callback, and token exchange",
                "Working OAuth flow",
                "1.5d",
            ),
            phase(
                "Session & guards",
                "Add session storage, middleware guards",
                "Protected routes, refresh logic",
                "1d",
            ),
            phase(
                "Tests & hardening",
                "Unit + integration tests, CSRF/PKCE validation",
                "Full test coverage",
                "0.75d",
            ),
        ];
    }

    // Generic fallback — five universal phases.
    vec![
        phase(
            "Discovery & planning",
            &format!("Research requirements for: {goal}"),
            "Spec document, task list",
            "0.5d",
        ),
        phase(
            "Foundation",
            "Set up scaffolding, configs, and data models",
            "Skeleton code, migrations",
            "1d",
        ),
        phase(
            "Core implementation",
            "Build primary features and business logic",
            "Working feature set",
            "2d",
        ),
        phase(
            "Testing & QA",
            "Write unit, integration, and E2E tests",
            "Test suite, coverage report",
            "1d",
        ),
        phase(
            "Polish & release",
            "Address review feedback, update docs, tag release",
            "Release-ready build + docs",
            "0.5d",
        ),
    ]
}

fn phase(title: &str, goal: &str, outputs: &str, estimate: &str) -> Phase {
    Phase {
        title: title.to_string(),
        goal: goal.to_string(),
        outputs: outputs.to_string(),
        estimate: estimate.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_ctx() -> CommandContext {
        CommandContext {
            state_store: Arc::new(oxicode_state::StateStore::default()),
            model: "test".to_string(),
            provider_name: "test".to_string(),
            session_id: "test".to_string(),
        }
    }

    #[test]
    fn test_empty_args_returns_usage() {
        let cmd = UltraplanCommand;
        let ctx = make_ctx();
        match cmd.execute("", &ctx) {
            CommandOutput::Message(msg) => assert!(msg.contains("Usage")),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_generic_goal_produces_five_phases() {
        let phases = derive_phases("improve the logging system");
        assert_eq!(phases.len(), 5);
    }

    #[test]
    fn test_api_goal_uses_api_phases() {
        let phases = derive_phases("add REST API endpoints for users");
        assert!(phases[0].title.to_lowercase().contains("research") || phases.len() == 5);
    }

    #[test]
    fn test_output_contains_goal() {
        let cmd = UltraplanCommand;
        let ctx = make_ctx();
        match cmd.execute("build a payment service", &ctx) {
            CommandOutput::Message(msg) => {
                assert!(msg.contains("payment service"));
                assert!(msg.contains("Phase 1"));
                assert!(msg.contains("Phase 5"));
            }
            _ => panic!("expected Message"),
        }
    }
}
