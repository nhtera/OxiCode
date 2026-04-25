//! Integration tests for project memory injection.
//!
//! Verifies that the memory-building pipeline (memdir + memory_scanner)
//! produces the correct output for injection into the system prompt.
//! Tests replicate the logic of `build_project_memory` using the public APIs.

use oxicode_session::{memdir, memory_scanner};

/// Helper: compute a combined memory string the same way `build_project_memory` does.
fn assemble_memory(memory_dir: &std::path::Path) -> Option<String> {
    if !memory_dir.exists() {
        return None;
    }

    let entrypoint = memdir::read_entrypoint(memory_dir)
        .map(|(content, _truncated)| content)
        .unwrap_or_default();

    let headers = memory_scanner::scan_memory_files(memory_dir).unwrap_or_default();

    if entrypoint.is_empty() && headers.is_empty() {
        return None;
    }

    let mut combined = String::with_capacity(entrypoint.len() + 512);

    if !entrypoint.is_empty() {
        combined.push_str(&entrypoint);
    }

    if !headers.is_empty() {
        let index = memory_scanner::build_memory_index(&headers);
        if !index.is_empty() {
            let prefix = if combined.is_empty() {
                "## Additional Memory Files\n\n"
            } else {
                "\n\n## Additional Memory Files\n\n"
            };
            combined.push_str(prefix);
            combined.push_str(&index);
        }
    }

    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

#[test]
fn entrypoint_only_returns_content() {
    let tmp = tempfile::tempdir().unwrap();
    let mem_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();

    let content = "# Project Memory\n\nUse snake_case for all identifiers.";
    std::fs::write(mem_dir.join("MEMORY.md"), content).unwrap();

    let result = assemble_memory(&mem_dir).expect("should return Some");
    assert!(
        result.contains("snake_case"),
        "returned string should contain entrypoint text"
    );
}

#[test]
fn missing_memory_dir_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let absent_dir = tmp.path().join("no_such_dir");

    let result = assemble_memory(&absent_dir);
    assert!(result.is_none(), "should return None for missing dir");
}

#[test]
fn empty_memory_dir_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let mem_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();
    // No files written.

    let result = assemble_memory(&mem_dir);
    assert!(result.is_none(), "should return None for empty dir");
}

#[test]
fn additional_md_files_appear_in_index() {
    let tmp = tempfile::tempdir().unwrap();
    let mem_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();

    // Entrypoint.
    std::fs::write(mem_dir.join("MEMORY.md"), "# Root\nProject context.").unwrap();

    // Additional memory file with frontmatter.
    std::fs::write(
        mem_dir.join("arch.md"),
        "---\ntype: decision\ndescription: \"Use async Rust\"\n---\nBody.",
    )
    .unwrap();

    let result = assemble_memory(&mem_dir).expect("should have content");
    assert!(
        result.contains("Project context."),
        "entrypoint should be present"
    );
    assert!(
        result.contains("Use async Rust"),
        "additional file index entry should appear"
    );
}

#[test]
fn additional_files_without_entrypoint_returns_index() {
    let tmp = tempfile::tempdir().unwrap();
    let mem_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();

    // No MEMORY.md — only additional files.
    std::fs::write(
        mem_dir.join("pref.md"),
        "---\ntype: preference\ndescription: \"Prefer snake_case\"\n---\nBody.",
    )
    .unwrap();

    let result = assemble_memory(&mem_dir).expect("should return index even without entrypoint");
    assert!(result.contains("Prefer snake_case"));
}

#[test]
fn malformed_frontmatter_does_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let mem_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();

    std::fs::write(mem_dir.join("MEMORY.md"), "# Root").unwrap();
    // Malformed YAML frontmatter.
    std::fs::write(mem_dir.join("broken.md"), "---\n{{{bad yaml\n---\nBody.").unwrap();

    // Must not panic.
    let result = assemble_memory(&mem_dir);
    assert!(
        result.is_some(),
        "should still return entrypoint despite broken file"
    );
}

#[test]
fn project_memory_dir_key_is_deterministic() {
    let root = std::path::Path::new("/home/user/my-project");
    let dir1 = memdir::project_memory_dir(root);
    let dir2 = memdir::project_memory_dir(root);
    assert_eq!(
        dir1, dir2,
        "memory dir should be deterministic for the same root"
    );
}
