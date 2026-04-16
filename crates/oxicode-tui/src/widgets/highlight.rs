//! Syntax highlighting engine wrapping `syntect`.
//!
//! Provides `highlight_code(code, lang)` returning styled `ratatui::text::Line`
//! values with line numbers. Uses process-global `OnceLock` singletons for the
//! grammar database and color theme — init is ~2 ms, subsequent calls are free.
//!
//! Guards: inputs > 512 KB or 10 000 lines fall back to plain unstyled text.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

// ── Global singletons ──────────────────────────────────────────────

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

/// Max input size to prevent pathological CPU usage.
const MAX_BYTES: usize = 512 * 1024;
const MAX_LINES: usize = 10_000;

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static Theme {
    THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes
            .get("base16-eighties.dark")
            .cloned()
            .unwrap_or_else(|| {
                // Fallback to any available theme.
                ts.themes
                    .values()
                    .next()
                    .cloned()
                    .expect("syntect ships at least one theme")
            })
    })
}

// ── Language resolution ────────────────────────────────────────────

/// Resolve a markdown language tag to a syntect `SyntaxReference`.
///
/// Tries (in order): extension match for alias, exact name match, extension
/// match. Extension-based lookup is preferred because syntect's name matching
/// is case-sensitive (`"rust"` won't find `"Rust"`), while extension matching
/// is reliable across grammar sets.
fn resolve_syntax(lang: &str) -> &'static SyntaxReference {
    let ss = syntax_set();
    if lang.is_empty() {
        return ss.find_syntax_plain_text();
    }

    // Map language names/aliases to file extensions for reliable lookup.
    let ext = match lang {
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => "js",
        "typescript" | "ts" | "tsx" | "mts" | "cts" => "ts",
        "python" | "py" => "py",
        "ruby" | "rb" => "rb",
        "rust" | "rs" => "rs",
        "shell" | "sh" | "bash" | "zsh" => "sh",
        "yaml" | "yml" => "yml",
        "markdown" | "md" => "md",
        "dockerfile" | "Dockerfile" => "Dockerfile",
        "c++" | "cpp" | "cc" | "cxx" => "cpp",
        "c#" | "csharp" | "cs" => "cs",
        "kotlin" | "kt" | "kts" => "kt",
        "swift" => "swift",
        "go" | "golang" => "go",
        "java" => "java",
        "json" => "json",
        "toml" => "toml",
        "xml" | "svg" | "plist" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "sql" => "sql",
        "lua" => "lua",
        "haskell" | "hs" => "hs",
        "elixir" | "ex" | "exs" => "ex",
        "erlang" | "erl" => "erl",
        "ocaml" | "ml" | "mli" => "ml",
        "php" => "php",
        "perl" | "pl" => "pl",
        "scala" => "scala",
        "clojure" | "clj" | "cljs" => "clj",
        "dart" => "dart",
        "r" | "R" => "r",
        other => other,
    };

    // Extension lookup is most reliable across different grammar sets.
    ss.find_syntax_by_extension(ext)
        .or_else(|| ss.find_syntax_by_name(lang))
        .or_else(|| ss.find_syntax_by_extension(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

// ── Color conversion ───────────────────────────────────────────────

/// Convert syntect RGBA to ratatui RGB (ignore alpha).
fn to_ratatui_color(c: SyntectColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Convert syntect `FontStyle` flags to ratatui `Modifier`.
fn to_ratatui_modifier(fs: FontStyle) -> Modifier {
    let mut m = Modifier::empty();
    if fs.contains(FontStyle::BOLD) {
        m |= Modifier::BOLD;
    }
    if fs.contains(FontStyle::ITALIC) {
        m |= Modifier::ITALIC;
    }
    if fs.contains(FontStyle::UNDERLINE) {
        m |= Modifier::UNDERLINED;
    }
    m
}

// ── Public API ─────────────────────────────────────────────────────

/// Resolve a language name from a file extension for syntax highlighting.
///
/// Returns `None` if no language can be determined.
pub fn lang_from_file_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?;
    let lang = match ext {
        "rs" => "rust",
        "py" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "rb" => "ruby",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "c++",
        "cs" => "c#",
        "sh" | "bash" | "zsh" => "bash",
        "yml" | "yaml" => "yaml",
        "json" => "json",
        "toml" => "toml",
        "xml" | "svg" | "plist" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "sql" => "sql",
        "md" | "markdown" => "markdown",
        "dockerfile" | "Dockerfile" => "dockerfile",
        "lua" => "lua",
        "r" | "R" => "r",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "hs" => "haskell",
        "ml" | "mli" => "ocaml",
        "php" => "php",
        "pl" | "pm" => "perl",
        "scala" => "scala",
        "clj" | "cljs" | "cljc" => "clojure",
        "dart" => "dart",
        "vim" => "vim",
        "tf" | "hcl" => "hcl",
        "proto" => "protobuf",
        "graphql" | "gql" => "graphql",
        "zig" => "zig",
        "nim" => "nim",
        _ => return None,
    };
    Some(lang)
}

/// Highlight `code` with language `lang`, returning styled lines with line numbers.
///
/// Returns `None` if the input exceeds safety limits (caller should fall back
/// to plain text rendering).
///
/// Background colors are intentionally omitted so the terminal's own
/// background shows through
pub fn highlight_code(code: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if code.is_empty() {
        return Some(Vec::new());
    }
    if code.len() > MAX_BYTES || code.lines().count() > MAX_LINES {
        return None;
    }

    let ss = syntax_set();
    let syntax = resolve_syntax(lang);
    let theme = theme();
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines = Vec::new();
    for (i, line_text) in LinesWithEndings::from(code).enumerate() {
        let ranges = highlighter
            .highlight_line(line_text, ss)
            .unwrap_or_default();

        let line_num = format!("{:>4} ", i + 1);
        let mut spans = vec![Span::styled(
            line_num,
            Style::default().fg(crate::render::TRANSCRIPT_MUTED),
        )];

        for (style, text) in ranges {
            let trimmed = text.trim_end_matches('\n').trim_end_matches('\r');
            if trimmed.is_empty() {
                continue;
            }
            let fg = to_ratatui_color(style.foreground);
            let modifier = to_ratatui_modifier(style.font_style);
            spans.push(Span::styled(
                trimmed.to_string(),
                Style::default().fg(fg).add_modifier(modifier),
            ));
        }

        lines.push(Line::from(spans));
    }

    Some(lines)
}

/// Highlight code for inline use in markdown rendering (no line numbers).
///
/// Returns `None` if input exceeds limits or highlighting fails.
///
/// Background colors are intentionally omitted so the terminal's own
/// background shows through (matching Codex behavior).
pub fn highlight_code_inline(code: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if code.is_empty() {
        return Some(Vec::new());
    }
    if code.len() > MAX_BYTES || code.lines().count() > MAX_LINES {
        return None;
    }

    let ss = syntax_set();
    let syntax = resolve_syntax(lang);
    let theme = theme();
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines = Vec::new();
    for line_text in LinesWithEndings::from(code) {
        let ranges = highlighter
            .highlight_line(line_text, ss)
            .unwrap_or_default();

        let mut spans: Vec<Span<'static>> = Vec::new();
        for (style, text) in ranges {
            let trimmed = text.trim_end_matches('\n').trim_end_matches('\r');
            if trimmed.is_empty() {
                continue;
            }
            let fg = to_ratatui_color(style.foreground);
            let modifier = to_ratatui_modifier(style.font_style);
            spans.push(Span::styled(
                trimmed.to_string(),
                Style::default().fg(fg).add_modifier(modifier),
            ));
        }
        lines.push(Line::from(spans));
    }

    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_from_file_path_resolves_extensions() {
        assert_eq!(lang_from_file_path("main.rs"), Some("rust"));
        assert_eq!(lang_from_file_path("/src/app.tsx"), Some("typescript"));
        assert_eq!(lang_from_file_path("config.yaml"), Some("yaml"));
        assert_eq!(lang_from_file_path("Cargo.toml"), Some("toml"));
        assert_eq!(lang_from_file_path("no_extension"), None);
        assert_eq!(lang_from_file_path("script.sh"), Some("bash"));
    }

    #[test]
    fn highlight_rust_produces_colored_spans() {
        let lines = highlight_code("fn main() {\n    println!(\"hello\");\n}", "rust");
        let lines = lines.expect("should succeed");
        assert_eq!(lines.len(), 3);

        // Should have more than just line-number + white text.
        let non_gray_spans: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| {
                matches!(s.style.fg, Some(Color::Rgb(r, g, b))
                    if !(r == 255 && g == 255 && b == 255)  // not white
                    && !(r == 128 && g == 128 && b == 128)) // not gray
                    && !s.content.trim().is_empty()
            })
            .collect();
        assert!(
            !non_gray_spans.is_empty(),
            "Rust highlighting should produce colored spans"
        );
    }

    #[test]
    fn highlight_unknown_lang_falls_back_to_plain() {
        let lines = highlight_code("some random text", "nonexistent_lang_xyz");
        assert!(lines.is_some(), "Unknown lang should not fail");
    }

    #[test]
    fn highlight_empty_code_returns_empty() {
        let lines = highlight_code("", "rust");
        assert_eq!(lines.unwrap().len(), 0);
    }

    #[test]
    fn highlight_has_line_numbers() {
        let lines = highlight_code("let x = 1;\nlet y = 2;", "rust").unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans[0].content.contains('1'));
        assert!(lines[1].spans[0].content.contains('2'));
    }

    #[test]
    fn highlight_inline_no_line_numbers() {
        let lines = highlight_code_inline("let x = 1;", "rust").unwrap();
        assert!(!lines.is_empty());
        // First span should be code content, not a line number.
        let first_content: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            first_content.contains("let"),
            "Inline highlight should contain code"
        );
    }

    #[test]
    fn highlight_large_input_returns_none() {
        let large = "x\n".repeat(MAX_LINES + 1);
        let result = highlight_code(&large, "rust");
        assert!(result.is_none(), "Should reject oversized input");
    }

    #[test]
    fn language_aliases_resolve() {
        // "rs" should resolve to rust syntax.
        let lines = highlight_code("fn foo() {}", "rs").unwrap();
        assert!(!lines.is_empty());

        // "py" should resolve to python.
        let lines = highlight_code("def foo(): pass", "py").unwrap();
        assert!(!lines.is_empty());

        // "js" should resolve to javascript.
        let lines = highlight_code("const x = 1;", "js").unwrap();
        assert!(!lines.is_empty());
    }
}
