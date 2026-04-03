//! Plan slash commands: /plan list, /plan show, /plan create.

use std::fmt::Write as _;

use super::{CommandContext, CommandOutput, SlashCommand};

/// /plan [create|list|show] — manage implementation plans.
pub struct PlanCommand;

impl SlashCommand for PlanCommand {
    fn name(&self) -> &str {
        "plan"
    }
    fn description(&self) -> &str {
        "Plan mode (create/list/show)"
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandOutput {
        let plans_dir = std::path::Path::new("plans");
        let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));

        match sub.trim() {
            "create" => {
                let name = if rest.trim().is_empty() {
                    "untitled"
                } else {
                    rest.trim()
                };
                // Validate: no path traversal.
                if name.contains("..") || name.contains('/') || name.contains('\\') {
                    return CommandOutput::Error("Invalid plan name.".into());
                }
                let now = chrono::Local::now().format("%y%m%d-%H%M");
                let dir_name = format!("{now}-{name}");
                let plan_path = plans_dir.join(&dir_name);
                match std::fs::create_dir_all(&plan_path) {
                    Ok(()) => {
                        // Create a minimal plan.md
                        let plan_file = plan_path.join("plan.md");
                        let content = format!(
                            "# {name}\n\n\
                             ## Overview\n\nTODO\n\n\
                             ## Phases\n\n| Phase | Name | Status |\n|-------|------|--------|\n"
                        );
                        let _ = std::fs::write(&plan_file, content);
                        CommandOutput::Message(format!("Created plan: {}", plan_path.display()))
                    }
                    Err(e) => CommandOutput::Error(format!("Failed to create plan: {e}")),
                }
            }
            "list" | "" => {
                if !plans_dir.exists() {
                    return CommandOutput::Message("No plans/ directory found.".into());
                }
                match std::fs::read_dir(plans_dir) {
                    Ok(entries) => {
                        let mut dirs: Vec<_> = entries
                            .filter_map(Result::ok)
                            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                            .filter(|e| {
                                // Skip hidden directories and reports.
                                let name = e.file_name();
                                let n = name.to_string_lossy();
                                !n.starts_with('.') && n != "reports"
                            })
                            .collect();
                        dirs.sort_by_key(std::fs::DirEntry::file_name);

                        if dirs.is_empty() {
                            return CommandOutput::Message("No plans found.".into());
                        }
                        let mut output = String::from("Plans:\n");
                        for entry in dirs {
                            let name = entry.file_name();
                            let has_plan = entry.path().join("plan.md").exists();
                            let marker = if has_plan { "+" } else { "-" };
                            let _ = writeln!(output, "  [{marker}] {}", name.to_string_lossy());
                        }
                        let _ = writeln!(output, "\n[+] = has plan.md, [-] = no plan.md");
                        CommandOutput::Message(output)
                    }
                    Err(e) => CommandOutput::Error(format!("Failed to read plans/: {e}")),
                }
            }
            "show" => show_plan(plans_dir, rest.trim()),
            other => CommandOutput::Error(format!("Unknown: /plan {other}. Use: create, list, show")),
        }
    }

    fn completions(&self, partial: &str, _ctx: &CommandContext) -> Vec<String> {
        ["create", "list", "show"]
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| (*s).to_string())
            .collect()
    }
}

/// Handle `/plan show <name>` — read and display plan.md content.
fn show_plan(plans_dir: &std::path::Path, name: &str) -> CommandOutput {
    if name.is_empty() {
        return CommandOutput::Error("Usage: /plan show <name>".into());
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return CommandOutput::Error("Invalid plan name.".into());
    }
    let Some(dir) = find_plan_dir(plans_dir, name) else {
        return CommandOutput::Error(format!("Plan not found: {name}"));
    };
    let plan_file = dir.join("plan.md");
    if !plan_file.exists() {
        return CommandOutput::Message(format!(
            "Plan directory exists but no plan.md: {}",
            dir.display()
        ));
    }
    match std::fs::read_to_string(&plan_file) {
        Ok(content) => {
            let preview: String = content.lines().take(50).collect::<Vec<_>>().join("\n");
            let total = content.lines().count();
            let suffix = if total > 50 {
                format!("\n\n... ({total} total lines)")
            } else {
                String::new()
            };
            CommandOutput::Message(format!("{preview}{suffix}"))
        }
        Err(e) => CommandOutput::Error(format!("Failed to read plan: {e}")),
    }
}

/// Find a plan directory by partial name match.
fn find_plan_dir(plans_dir: &std::path::Path, query: &str) -> Option<std::path::PathBuf> {
    let exact = plans_dir.join(query);
    if exact.is_dir() {
        return Some(exact);
    }
    // Partial match: find directory containing query.
    std::fs::read_dir(plans_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .contains(query)
        })
        .map(|e| e.path())
}
