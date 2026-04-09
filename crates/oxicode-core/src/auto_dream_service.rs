//! AutoDream service — generates context-aware project suggestions.
//!
//! Suggestions are produced from keyword analysis of the supplied context
//! strings. LLM integration will be wired in a future iteration; for now
//! the engine uses a deterministic keyword→suggestion map.

use std::collections::HashSet;
use std::time::Instant;

use crate::auto_dream_config::AutoDreamConfig;

/// A single actionable suggestion produced by the AutoDream engine.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Short title for display in the suggestion panel (≤60 chars recommended).
    pub title: String,
    /// Longer description explaining what the suggestion entails.
    pub description: String,
}

/// Drives the AutoDream suggestion engine.
///
/// Maintains cooldown state so the engine is not called more frequently
/// than `config.cooldown_secs` allows.
#[derive(Debug)]
pub struct AutoDreamService {
    config: AutoDreamConfig,
    /// Instant at which the last generation batch completed.
    last_generated: Option<Instant>,
}

impl AutoDreamService {
    /// Create a new service with the supplied configuration.
    pub fn new(config: AutoDreamConfig) -> Self {
        Self {
            config,
            last_generated: None,
        }
    }

    /// Whether the cooldown has elapsed and a new generation is allowed.
    pub fn should_generate(&self) -> bool {
        if !self.config.enabled {
            return false;
        }
        match self.last_generated {
            None => true,
            Some(last) => last.elapsed().as_secs() >= self.config.cooldown_secs,
        }
    }

    /// Generate suggestions derived from `context` keywords.
    ///
    /// Returns an empty `Vec` when disabled or within the cooldown window.
    /// At most `config.max_suggestions` entries are returned.
    pub fn generate_suggestions(&mut self, context: &[String]) -> Vec<Suggestion> {
        if !self.should_generate() {
            tracing::debug!("AutoDream: within cooldown, skipping generation");
            return Vec::new();
        }

        let joined = context.join(" ").to_lowercase();
        let mut suggestions = build_suggestions(&joined);

        // Deduplicate by title (handles non-adjacent duplicates).
        let mut seen = HashSet::new();
        suggestions.retain(|s| seen.insert(s.title.clone()));
        suggestions.truncate(self.config.max_suggestions);

        tracing::debug!(
            count = suggestions.len(),
            "AutoDream: generated suggestions"
        );

        self.last_generated = Some(Instant::now());
        suggestions
    }
}

/// Map context keywords to relevant suggestions.
///
/// The match order is intentional: more specific checks come first so they
/// shadow generic fallbacks when both would apply.
fn build_suggestions(context: &str) -> Vec<Suggestion> {
    let mut out = Vec::new();

    // ── Rust / Cargo ────────────────────────────────────────────────────────
    if context.contains("rust") || context.contains("cargo") || context.contains(".rs") {
        out.push(Suggestion {
            title: "Run `cargo clippy --workspace`".to_string(),
            description: "Catch common Rust mistakes and style issues across the entire workspace."
                .to_string(),
        });
        out.push(Suggestion {
            title: "Add `#[must_use]` to pure functions".to_string(),
            description: "Prevent callers from silently ignoring return values.".to_string(),
        });
        out.push(Suggestion {
            title: "Check for unused dependencies with `cargo machete`".to_string(),
            description: "Remove unused crates to speed up compile times.".to_string(),
        });
    }

    // ── JavaScript / TypeScript / Node ──────────────────────────────────────
    if context.contains("javascript")
        || context.contains("typescript")
        || context.contains("node")
        || context.contains("npm")
        || context.contains(".ts")
        || context.contains(".js")
    {
        out.push(Suggestion {
            title: "Run `npm audit` to check for vulnerabilities".to_string(),
            description: "Identify and fix known security issues in dependencies.".to_string(),
        });
        out.push(Suggestion {
            title: "Enable strict TypeScript mode".to_string(),
            description: "Add `\"strict\": true` to tsconfig.json for stronger type safety."
                .to_string(),
        });
        out.push(Suggestion {
            title: "Add `eslint --max-warnings 0` to CI".to_string(),
            description: "Prevent lint warnings from accumulating unnoticed.".to_string(),
        });
    }

    // ── Python ───────────────────────────────────────────────────────────────
    if context.contains("python")
        || context.contains(".py")
        || context.contains("pip")
        || context.contains("uv")
    {
        out.push(Suggestion {
            title: "Run `ruff check .` for fast Python linting".to_string(),
            description: "Ruff is orders-of-magnitude faster than flake8 / pylint.".to_string(),
        });
        out.push(Suggestion {
            title: "Add type annotations with `mypy --strict`".to_string(),
            description: "Catch type errors before runtime.".to_string(),
        });
        out.push(Suggestion {
            title: "Pin dependencies in `requirements.lock`".to_string(),
            description: "Ensures reproducible environments across machines.".to_string(),
        });
    }

    // ── Git / version control ────────────────────────────────────────────────
    if context.contains("git") || context.contains("commit") || context.contains("branch") {
        out.push(Suggestion {
            title: "Enable branch protection rules on `main`".to_string(),
            description: "Require PR reviews and passing CI before merging.".to_string(),
        });
    }

    // ── Generic fallback ─────────────────────────────────────────────────────
    if out.is_empty() {
        out.push(Suggestion {
            title: "Add a README with setup instructions".to_string(),
            description: "Help new contributors get started quickly.".to_string(),
        });
        out.push(Suggestion {
            title: "Set up a CI pipeline".to_string(),
            description: "Automate builds and tests on every push.".to_string(),
        });
        out.push(Suggestion {
            title: "Add a `.editorconfig` file".to_string(),
            description: "Enforce consistent indentation across editors and IDEs.".to_string(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, cooldown: u64, max: usize) -> AutoDreamConfig {
        AutoDreamConfig {
            enabled,
            cooldown_secs: cooldown,
            max_suggestions: max,
        }
    }

    #[test]
    fn generates_rust_suggestions() {
        let mut svc = AutoDreamService::new(cfg(true, 0, 5));
        let ctx = vec!["cargo.toml".to_string(), "rust workspace".to_string()];
        let suggestions = svc.generate_suggestions(&ctx);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.title.contains("clippy")));
    }

    #[test]
    fn respects_max_suggestions_limit() {
        let mut svc = AutoDreamService::new(cfg(true, 0, 2));
        let ctx = vec!["rust cargo npm python git".to_string()];
        let suggestions = svc.generate_suggestions(&ctx);
        assert!(suggestions.len() <= 2);
    }

    #[test]
    fn returns_empty_when_disabled() {
        let mut svc = AutoDreamService::new(cfg(false, 0, 5));
        let suggestions = svc.generate_suggestions(&["rust".to_string()]);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn cooldown_prevents_rapid_regeneration() {
        let mut svc = AutoDreamService::new(cfg(true, 9999, 5));
        let ctx = vec!["rust".to_string()];
        let first = svc.generate_suggestions(&ctx);
        assert!(!first.is_empty()); // first call goes through

        let second = svc.generate_suggestions(&ctx);
        assert!(second.is_empty()); // cooldown not elapsed
    }

    #[test]
    fn fallback_suggestions_for_unknown_context() {
        let mut svc = AutoDreamService::new(cfg(true, 0, 5));
        let suggestions = svc.generate_suggestions(&["some random words".to_string()]);
        assert!(!suggestions.is_empty());
        // Generic fallback should mention README or CI
        let titles: Vec<_> = suggestions.iter().map(|s| s.title.as_str()).collect();
        assert!(titles
            .iter()
            .any(|t| t.contains("README") || t.contains("CI")));
    }
}
