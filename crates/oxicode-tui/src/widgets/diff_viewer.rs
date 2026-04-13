//! Interactive diff viewer — state management and diff parsing.
//!
//! Two-pane modal overlay showing file-level and line-level diffs.
//! Supports GitDiff (from `git diff HEAD`) with structured hunk/line data.
//! Rendering lives in `diff_viewer_render.rs`.

use std::process::Command;

// ── Data structures ──────────────────────────────────────────────────────────

/// Which pane is active for keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffPane {
    FileList,
    Detail,
}

/// Line classification within a diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    Header,
}

/// A single line in a diff hunk.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_line_no: Option<u32>,
    pub new_line_no: Option<u32>,
}

/// A contiguous group of changes with surrounding context.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

/// Per-file diff statistics and structured hunks.
#[derive(Debug, Clone)]
pub struct FileDiffStats {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
    pub is_new_file: bool,
    pub hunks: Vec<DiffHunk>,
}

impl FileDiffStats {
    /// Total displayable lines across all hunks.
    pub fn total_lines(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len()).sum()
    }
}

// ── State ────────────────────────────────────────────────────────────────────

/// Persistent state for the diff viewer overlay.
#[derive(Debug)]
pub struct DiffViewerState {
    visible: bool,
    pub files: Vec<FileDiffStats>,
    pub selected_file: usize,
    pub active_pane: DiffPane,
    pub detail_scroll: u16,
    pub collapsed: Vec<bool>,
}

impl DiffViewerState {
    pub fn new() -> Self {
        Self {
            visible: false,
            files: Vec::new(),
            selected_file: 0,
            active_pane: DiffPane::FileList,
            detail_scroll: 0,
            collapsed: Vec::new(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Open the diff viewer with git diff from the given directory.
    pub fn open(&mut self, cwd: &str) {
        let diff_output = load_git_diff(cwd);
        self.files = parse_unified_diff(&diff_output);
        self.collapsed = vec![false; self.files.len()];
        self.selected_file = 0;
        self.active_pane = DiffPane::FileList;
        self.detail_scroll = 0;
        self.visible = true;
    }

    /// Open with pre-built file diffs (for turn-based diffs).
    pub fn open_with_files(&mut self, files: Vec<FileDiffStats>) {
        self.collapsed = vec![false; files.len()];
        self.files = files;
        self.selected_file = 0;
        self.active_pane = DiffPane::FileList;
        self.detail_scroll = 0;
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Select previous file in list.
    pub fn prev_file(&mut self) {
        if self.selected_file > 0 {
            self.selected_file -= 1;
            self.detail_scroll = 0;
        }
    }

    /// Select next file in list.
    pub fn next_file(&mut self) {
        if !self.files.is_empty() && self.selected_file < self.files.len() - 1 {
            self.selected_file += 1;
            self.detail_scroll = 0;
        }
    }

    /// Switch between FileList and Detail panes.
    pub fn toggle_pane(&mut self) {
        self.active_pane = match self.active_pane {
            DiffPane::FileList => DiffPane::Detail,
            DiffPane::Detail => DiffPane::FileList,
        };
    }

    /// Toggle collapse state for the selected file.
    pub fn toggle_collapse(&mut self) {
        if self.selected_file < self.collapsed.len() {
            self.collapsed[self.selected_file] = !self.collapsed[self.selected_file];
            self.detail_scroll = 0;
        }
    }

    /// Whether the selected file is collapsed.
    pub fn is_selected_collapsed(&self) -> bool {
        self.collapsed.get(self.selected_file).copied().unwrap_or(false)
    }

    /// Scroll detail pane up by `lines` lines.
    pub fn scroll_up(&mut self, lines: u16) {
        self.detail_scroll = self.detail_scroll.saturating_sub(lines);
    }

    /// Scroll detail pane down by `lines` lines, clamped to content height.
    pub fn scroll_down(&mut self, lines: u16, visible_height: u16) {
        let total = self.selected_total_lines() as u16;
        let max_scroll = total.saturating_sub(visible_height);
        self.detail_scroll = (self.detail_scroll + lines).min(max_scroll);
    }

    /// Total displayable lines for the currently selected file.
    fn selected_total_lines(&self) -> usize {
        self.files
            .get(self.selected_file)
            .map_or(0, FileDiffStats::total_lines)
    }

    /// Summary stats: (total_files, total_added, total_removed).
    pub fn summary_stats(&self) -> (usize, u32, u32) {
        let added: u32 = self.files.iter().map(|f| f.added).sum();
        let removed: u32 = self.files.iter().map(|f| f.removed).sum();
        (self.files.len(), added, removed)
    }
}

// ── Git diff loading ─────────────────────────────────────────────────────────

/// Run `git diff HEAD --unified=3` in the given directory.
fn load_git_diff(cwd: &str) -> String {
    let output = Command::new("git")
        .args(["diff", "HEAD", "--unified=3"])
        .current_dir(cwd)
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            // Fallback: try without HEAD (for repos with no commits).
            let fallback = Command::new("git")
                .args(["diff", "--unified=3"])
                .current_dir(cwd)
                .output();
            match fallback {
                Ok(f) => String::from_utf8_lossy(&f.stdout).into_owned(),
                Err(_) => String::from_utf8_lossy(&o.stdout).into_owned(),
            }
        }
        Err(_) => String::new(),
    }
}

// ── Unified diff parser ──────────────────────────────────────────────────────

/// Parse unified diff text into structured `FileDiffStats`.
pub fn parse_unified_diff(input: &str) -> Vec<FileDiffStats> {
    let mut files: Vec<FileDiffStats> = Vec::new();
    let mut current_file: Option<FileDiffStats> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line: u32 = 0;
    let mut new_line: u32 = 0;

    for line in input.lines() {
        if line.starts_with("diff --git") {
            // Flush previous hunk and file.
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }
            if let Some(file) = current_file.take() {
                files.push(file);
            }
            // Extract path from "diff --git a/path b/path".
            let path = extract_diff_path(line);
            current_file = Some(FileDiffStats {
                path,
                added: 0,
                removed: 0,
                binary: false,
                is_new_file: false,
                hunks: Vec::new(),
            });
        } else if line.starts_with("new file mode") {
            if let Some(ref mut file) = current_file {
                file.is_new_file = true;
            }
        } else if line.starts_with("Binary files") {
            if let Some(ref mut file) = current_file {
                file.binary = true;
            }
        } else if line.starts_with("@@") {
            // Flush previous hunk.
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }
            let (os, oc, ns, nc) = parse_hunk_header(line);
            old_line = os;
            new_line = ns;
            current_hunk = Some(DiffHunk {
                old_start: os,
                old_count: oc,
                new_start: ns,
                new_count: nc,
                lines: vec![DiffLine {
                    kind: DiffLineKind::Header,
                    content: line.to_string(),
                    old_line_no: None,
                    new_line_no: None,
                }],
            });
        } else if let Some(ref mut hunk) = current_hunk {
            if let Some(first_char) = line.chars().next() {
                match first_char {
                    '+' if !line.starts_with("+++") => {
                        hunk.lines.push(DiffLine {
                            kind: DiffLineKind::Added,
                            content: line[1..].to_string(),
                            old_line_no: None,
                            new_line_no: Some(new_line),
                        });
                        new_line += 1;
                        if let Some(ref mut file) = current_file {
                            file.added += 1;
                        }
                    }
                    '-' if !line.starts_with("---") => {
                        hunk.lines.push(DiffLine {
                            kind: DiffLineKind::Removed,
                            content: line[1..].to_string(),
                            old_line_no: Some(old_line),
                            new_line_no: None,
                        });
                        old_line += 1;
                        if let Some(ref mut file) = current_file {
                            file.removed += 1;
                        }
                    }
                    ' ' => {
                        hunk.lines.push(DiffLine {
                            kind: DiffLineKind::Context,
                            content: line[1..].to_string(),
                            old_line_no: Some(old_line),
                            new_line_no: Some(new_line),
                        });
                        old_line += 1;
                        new_line += 1;
                    }
                    _ => {} // Ignore other lines (e.g., "\ No newline at end of file").
                }
            }
        }
    }

    // Flush final hunk and file.
    if let Some(hunk) = current_hunk {
        if let Some(ref mut file) = current_file {
            file.hunks.push(hunk);
        }
    }
    if let Some(file) = current_file {
        files.push(file);
    }

    files
}

/// Extract file path from "diff --git a/path b/path".
fn extract_diff_path(line: &str) -> String {
    // Format: "diff --git a/some/path b/some/path"
    if let Some(b_idx) = line.rfind(" b/") {
        line[b_idx + 3..].to_string()
    } else {
        // Fallback: try to extract after "a/".
        line.split("a/")
            .nth(1)
            .and_then(|s| s.split(" b/").next())
            .unwrap_or("unknown")
            .to_string()
    }
}

/// Parse hunk header "@@ -old_start,old_count +new_start,new_count @@".
fn parse_hunk_header(line: &str) -> (u32, u32, u32, u32) {
    // Strip leading "@@ " and trailing " @@..." suffix.
    let trimmed = line
        .trim_start_matches("@@ ")
        .split(" @@")
        .next()
        .unwrap_or("");

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let (old_start, old_count) = parse_range(parts.first().unwrap_or(&"-1,1"));
    let (new_start, new_count) = parse_range(parts.get(1).unwrap_or(&"+1,1"));

    (old_start, old_count, new_start, new_count)
}

/// Parse a range like "-12,5" or "+3,7" into (start, count).
fn parse_range(s: &str) -> (u32, u32) {
    let s = s.trim_start_matches(['-', '+']);
    if let Some((start, count)) = s.split_once(',') {
        (
            start.parse().unwrap_or(1),
            count.parse().unwrap_or(1),
        )
    } else {
        (s.parse().unwrap_or(1), 1)
    }
}

// ── Turn-based diff building via `similar` crate ─────────────────────────────

/// Build `FileDiffStats` from before/after text pairs using the `similar` crate.
pub fn build_file_diff(path: &str, before: &str, after: &str) -> FileDiffStats {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(before, after);
    let mut hunks = Vec::new();
    let mut added: u32 = 0;
    let mut removed: u32 = 0;

    for group in diff.grouped_ops(3) {
        let first_op = group.first();
        let last_op = group.last();
        let old_start = first_op.map_or(0, |op| op.old_range().start as u32);
        let new_start = first_op.map_or(0, |op| op.new_range().start as u32);
        let old_end = last_op.map_or(0, |op| op.old_range().end as u32);
        let new_end = last_op.map_or(0, |op| op.new_range().end as u32);

        let mut lines = Vec::new();
        // Use 1-based line numbers for display.
        let mut old_line = old_start + 1;
        let mut new_line = new_start + 1;

        // Add hunk header.
        let old_count = old_end.saturating_sub(old_start);
        let new_count = new_end.saturating_sub(new_start);
        let display_old_start = old_start + 1;
        let display_new_start = new_start + 1;
        lines.push(DiffLine {
            kind: DiffLineKind::Header,
            content: format!("@@ -{display_old_start},{old_count} +{display_new_start},{new_count} @@"),
            old_line_no: None,
            new_line_no: None,
        });

        for op in &group {
            for change in diff.iter_changes(op) {
                let text = change.value().trim_end_matches('\n').to_string();
                match change.tag() {
                    ChangeTag::Equal => {
                        lines.push(DiffLine {
                            kind: DiffLineKind::Context,
                            content: text,
                            old_line_no: Some(old_line),
                            new_line_no: Some(new_line),
                        });
                        old_line += 1;
                        new_line += 1;
                    }
                    ChangeTag::Delete => {
                        lines.push(DiffLine {
                            kind: DiffLineKind::Removed,
                            content: text,
                            old_line_no: Some(old_line),
                            new_line_no: None,
                        });
                        old_line += 1;
                        removed += 1;
                    }
                    ChangeTag::Insert => {
                        lines.push(DiffLine {
                            kind: DiffLineKind::Added,
                            content: text,
                            old_line_no: None,
                            new_line_no: Some(new_line),
                        });
                        new_line += 1;
                        added += 1;
                    }
                }
            }
        }

        hunks.push(DiffHunk {
            old_start: display_old_start,
            old_count,
            new_start: display_new_start,
            new_count,
            lines,
        });
    }

    FileDiffStats {
        path: path.to_string(),
        added,
        removed,
        binary: false,
        is_new_file: before.is_empty() && !after.is_empty(),
        hunks,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,6 @@
 fn main() {
-    println!("old");
+    println!("new");
+    println!("added");
     let x = 1;
 }
diff --git a/src/lib.rs b/src/lib.rs
new file mode 100644
--- /dev/null
+++ b/src/lib.rs
@@ -0,0 +1,3 @@
+pub fn hello() {
+    println!("hello");
+}
"#;

    #[test]
    fn parse_returns_two_files() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[1].path, "src/lib.rs");
    }

    #[test]
    fn parse_counts_added_removed() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        assert_eq!(files[0].added, 2);
        assert_eq!(files[0].removed, 1);
        assert_eq!(files[1].added, 3);
        assert_eq!(files[1].removed, 0);
    }

    #[test]
    fn parse_detects_new_file() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        assert!(!files[0].is_new_file);
        assert!(files[1].is_new_file);
    }

    #[test]
    fn parse_creates_hunks() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[1].hunks.len(), 1);
    }

    #[test]
    fn parse_hunk_has_correct_lines() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        let hunk = &files[0].hunks[0];
        // Header + 1 context + 1 removed + 2 added + 1 context + 1 context = 7
        // Actually: header, " fn main() {", "-    println", "+    println", "+    println", "     let x", " }"
        assert!(hunk.lines.len() >= 5, "Hunk should have header + diff lines");

        let added: Vec<_> = hunk.lines.iter().filter(|l| l.kind == DiffLineKind::Added).collect();
        let removed: Vec<_> = hunk.lines.iter().filter(|l| l.kind == DiffLineKind::Removed).collect();
        assert_eq!(added.len(), 2);
        assert_eq!(removed.len(), 1);
    }

    #[test]
    fn parse_context_lines_have_both_line_numbers() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        let context_lines: Vec<_> = files[0].hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Context)
            .collect();
        for line in &context_lines {
            assert!(line.old_line_no.is_some(), "Context line should have old line number");
            assert!(line.new_line_no.is_some(), "Context line should have new line number");
        }
    }

    #[test]
    fn parse_added_line_has_only_new_line_no() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        let added_lines: Vec<_> = files[0].hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Added)
            .collect();
        for line in &added_lines {
            assert!(line.old_line_no.is_none());
            assert!(line.new_line_no.is_some());
        }
    }

    #[test]
    fn parse_removed_line_has_only_old_line_no() {
        let files = parse_unified_diff(SAMPLE_DIFF);
        let removed_lines: Vec<_> = files[0].hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Removed)
            .collect();
        for line in &removed_lines {
            assert!(line.old_line_no.is_some());
            assert!(line.new_line_no.is_none());
        }
    }

    #[test]
    fn parse_empty_diff() {
        let files = parse_unified_diff("");
        assert!(files.is_empty());
    }

    #[test]
    fn parse_binary_file() {
        let input = "diff --git a/image.png b/image.png\nBinary files a/image.png and b/image.png differ\n";
        let files = parse_unified_diff(input);
        assert_eq!(files.len(), 1);
        assert!(files[0].binary);
    }

    #[test]
    fn hunk_header_parsing() {
        let (os, oc, ns, nc) = parse_hunk_header("@@ -12,5 +14,7 @@ fn main() {");
        assert_eq!(os, 12);
        assert_eq!(oc, 5);
        assert_eq!(ns, 14);
        assert_eq!(nc, 7);
    }

    #[test]
    fn hunk_header_single_line() {
        let (os, oc, ns, nc) = parse_hunk_header("@@ -1 +1 @@");
        assert_eq!(os, 1);
        assert_eq!(oc, 1);
        assert_eq!(ns, 1);
        assert_eq!(nc, 1);
    }

    // ── State tests ──────────────────────────────────────────────────────────

    #[test]
    fn state_new_not_visible() {
        let state = DiffViewerState::new();
        assert!(!state.is_visible());
    }

    #[test]
    fn state_open_with_files_makes_visible() {
        let mut state = DiffViewerState::new();
        let files = vec![FileDiffStats {
            path: "test.rs".to_string(),
            added: 1,
            removed: 0,
            binary: false,
            is_new_file: false,
            hunks: vec![],
        }];
        state.open_with_files(files);
        assert!(state.is_visible());
        assert_eq!(state.files.len(), 1);
    }

    #[test]
    fn state_close_hides() {
        let mut state = DiffViewerState::new();
        state.open_with_files(vec![]);
        state.close();
        assert!(!state.is_visible());
    }

    #[test]
    fn state_navigate_files() {
        let mut state = DiffViewerState::new();
        let files: Vec<_> = (0..3)
            .map(|i| FileDiffStats {
                path: format!("file{i}.rs"),
                added: 0,
                removed: 0,
                binary: false,
                is_new_file: false,
                hunks: vec![],
            })
            .collect();
        state.open_with_files(files);

        assert_eq!(state.selected_file, 0);
        state.next_file();
        assert_eq!(state.selected_file, 1);
        state.next_file();
        assert_eq!(state.selected_file, 2);
        state.next_file(); // Should not go past last.
        assert_eq!(state.selected_file, 2);
        state.prev_file();
        assert_eq!(state.selected_file, 1);
        state.prev_file();
        assert_eq!(state.selected_file, 0);
        state.prev_file(); // Should not go below 0.
        assert_eq!(state.selected_file, 0);
    }

    #[test]
    fn state_toggle_pane() {
        let mut state = DiffViewerState::new();
        assert_eq!(state.active_pane, DiffPane::FileList);
        state.toggle_pane();
        assert_eq!(state.active_pane, DiffPane::Detail);
        state.toggle_pane();
        assert_eq!(state.active_pane, DiffPane::FileList);
    }

    #[test]
    fn state_toggle_collapse() {
        let mut state = DiffViewerState::new();
        let files = vec![FileDiffStats {
            path: "test.rs".to_string(),
            added: 0,
            removed: 0,
            binary: false,
            is_new_file: false,
            hunks: vec![],
        }];
        state.open_with_files(files);
        assert!(!state.is_selected_collapsed());
        state.toggle_collapse();
        assert!(state.is_selected_collapsed());
        state.toggle_collapse();
        assert!(!state.is_selected_collapsed());
    }

    #[test]
    fn state_scroll_clamps() {
        let mut state = DiffViewerState::new();
        state.scroll_up(10);
        assert_eq!(state.detail_scroll, 0);
        state.scroll_down(100, 20);
        // With no files, total_lines=0, max_scroll=0.
        assert_eq!(state.detail_scroll, 0);
    }

    #[test]
    fn state_summary_stats() {
        let mut state = DiffViewerState::new();
        let files = vec![
            FileDiffStats { path: "a.rs".into(), added: 5, removed: 2, binary: false, is_new_file: false, hunks: vec![] },
            FileDiffStats { path: "b.rs".into(), added: 3, removed: 1, binary: false, is_new_file: false, hunks: vec![] },
        ];
        state.open_with_files(files);
        let (count, added, removed) = state.summary_stats();
        assert_eq!(count, 2);
        assert_eq!(added, 8);
        assert_eq!(removed, 3);
    }

    // ── build_file_diff tests ────────────────────────────────────────────────

    #[test]
    fn build_diff_detects_additions() {
        let before = "line1\nline2\n";
        let after = "line1\nline2\nline3\n";
        let stats = build_file_diff("test.rs", before, after);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.removed, 0);
        assert!(!stats.hunks.is_empty());
    }

    #[test]
    fn build_diff_detects_removals() {
        let before = "line1\nline2\nline3\n";
        let after = "line1\nline2\n";
        let stats = build_file_diff("test.rs", before, after);
        assert_eq!(stats.added, 0);
        assert_eq!(stats.removed, 1);
    }

    #[test]
    fn build_diff_detects_changes() {
        let before = "hello world\n";
        let after = "hello rust\n";
        let stats = build_file_diff("test.rs", before, after);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.removed, 1);
    }

    #[test]
    fn build_diff_new_file() {
        let stats = build_file_diff("new.rs", "", "pub fn main() {}\n");
        assert!(stats.is_new_file);
        assert_eq!(stats.added, 1);
    }

    #[test]
    fn build_diff_identical() {
        let text = "no changes here\n";
        let stats = build_file_diff("same.rs", text, text);
        assert_eq!(stats.added, 0);
        assert_eq!(stats.removed, 0);
        assert!(stats.hunks.is_empty());
    }

    #[test]
    fn extract_path_standard() {
        let path = extract_diff_path("diff --git a/src/main.rs b/src/main.rs");
        assert_eq!(path, "src/main.rs");
    }

    #[test]
    fn file_total_lines() {
        let file = FileDiffStats {
            path: "test.rs".to_string(),
            added: 0,
            removed: 0,
            binary: false,
            is_new_file: false,
            hunks: vec![
                DiffHunk {
                    old_start: 1,
                    old_count: 3,
                    new_start: 1,
                    new_count: 3,
                    lines: vec![
                        DiffLine { kind: DiffLineKind::Header, content: "@@".into(), old_line_no: None, new_line_no: None },
                        DiffLine { kind: DiffLineKind::Context, content: "a".into(), old_line_no: Some(1), new_line_no: Some(1) },
                        DiffLine { kind: DiffLineKind::Added, content: "b".into(), old_line_no: None, new_line_no: Some(2) },
                    ],
                },
            ],
        };
        assert_eq!(file.total_lines(), 3);
    }
}
