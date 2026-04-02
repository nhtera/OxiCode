//! `oxicode-skills` — skill discovery, parsing, activation, and execution.
//!
//! Skills are markdown files (`SKILL.md`) with YAML frontmatter that define
//! prompt snippets injected into conversations when activation conditions are met.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use std::path::PathBuf;
//! use oxicode_skills::{SkillDiscovery, SkillExecutor, ActivationContext};
//!
//! let user_skills = PathBuf::from("/home/user/.oxicode/skills");
//! let discovery = SkillDiscovery::new(
//!     user_skills,
//!     PathBuf::from(".oxicode/skills"),
//! );
//! let skills = discovery.discover();
//! let executor = SkillExecutor::new(skills);
//!
//! let ctx = ActivationContext {
//!     current_file: Some("main.rs".to_string()),
//!     user_input: Some("help with rust".to_string()),
//! };
//!
//! if let Some(prompt) = executor.build_skills_prompt(&ctx) {
//!     println!("{prompt}");
//! }
//! ```

pub mod activation;
pub mod discovery;
pub mod executor;
pub mod parser;

// Key re-exports for external crate consumers.
pub use activation::{ActivationContext, SkillActivator};
pub use discovery::SkillDiscovery;
pub use executor::{SkillExecutor, SkillInfo};
pub use parser::{parse_skill, ActivationRule, InjectMode, Skill, SkillMetadata};
