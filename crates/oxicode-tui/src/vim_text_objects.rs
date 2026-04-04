//! Vim text objects for word, quote, and bracket selections.
//!
//! Each function returns `Option<(start, end)>` as char indices (exclusive end).
//! `inner_*` selects content only; `a_*` includes delimiters/surrounding whitespace.

/// Find the inner word boundary around `cursor` (word chars only, no surrounding spaces).
pub fn inner_word(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if cursor >= chars.len() {
        return None;
    }

    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
    let c = chars[cursor];

    if is_word_char(c) {
        // On a word char: select contiguous word chars.
        let mut start = cursor;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = cursor;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }
        Some((start, end))
    } else if c.is_whitespace() {
        // On whitespace: select contiguous whitespace.
        let mut start = cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let mut end = cursor;
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        Some((start, end))
    } else {
        // On punctuation/symbol: select contiguous non-word, non-whitespace chars.
        let is_punct = |ch: char| !ch.is_alphanumeric() && ch != '_' && !ch.is_whitespace();
        let mut start = cursor;
        while start > 0 && is_punct(chars[start - 1]) {
            start -= 1;
        }
        let mut end = cursor;
        while end < chars.len() && is_punct(chars[end]) {
            end += 1;
        }
        Some((start, end))
    }
}

/// Find "a word" boundary around `cursor` (word + trailing whitespace).
pub fn a_word(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let (start, mut end) = inner_word(text, cursor)?;
    let chars: Vec<char> = text.chars().collect();

    // Include trailing whitespace.
    while end < chars.len() && chars[end].is_whitespace() {
        end += 1;
    }
    Some((start, end))
}

/// Find inner quote boundary: content between matching `quote` chars around `cursor`.
pub fn inner_quote(text: &str, cursor: usize, quote: char) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if cursor >= chars.len() {
        return None;
    }

    // Find the opening quote (at or before cursor).
    let mut open = None;
    for i in (0..=cursor).rev() {
        if chars[i] == quote {
            open = Some(i);
            break;
        }
    }
    let open = open?;

    // Find the closing quote (after opening).
    let search_start = if open == cursor { open + 1 } else { cursor };
    let mut close = None;
    for i in (open + 1)..chars.len() {
        if chars[i] == quote {
            // Ensure cursor is between open and close.
            if i >= search_start {
                close = Some(i);
                break;
            }
        }
    }
    let close = close?;

    // Inner = content between quotes (exclusive).
    if close > open + 1 {
        Some((open + 1, close))
    } else {
        Some((open + 1, open + 1)) // Empty quotes
    }
}

/// Find "a quote" boundary: including the quote delimiters.
pub fn a_quote(text: &str, cursor: usize, quote: char) -> Option<(usize, usize)> {
    let (inner_start, inner_end) = inner_quote(text, cursor, quote)?;
    // Outer = opening quote .. closing quote (inclusive).
    Some((inner_start.saturating_sub(1), inner_end + 1))
}

/// Find inner bracket boundary: content between matching `open`/`close` bracket chars.
pub fn inner_bracket(text: &str, cursor: usize, open: char, close: char) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if cursor >= chars.len() {
        return None;
    }

    // Find matching opening bracket (scan left, respecting nesting).
    let mut depth = 0i32;
    let mut open_pos = None;
    for i in (0..=cursor).rev() {
        if chars[i] == close && i != cursor {
            depth += 1;
        } else if chars[i] == open {
            if depth == 0 {
                open_pos = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let open_pos = open_pos?;

    // Find matching closing bracket (scan right from open).
    let mut depth = 0i32;
    let mut close_pos = None;
    for i in (open_pos + 1)..chars.len() {
        if chars[i] == open {
            depth += 1;
        } else if chars[i] == close {
            if depth == 0 {
                close_pos = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let close_pos = close_pos?;

    Some((open_pos + 1, close_pos))
}

/// Find "a bracket" boundary: including the bracket delimiters.
pub fn a_bracket(text: &str, cursor: usize, open: char, close: char) -> Option<(usize, usize)> {
    let (inner_start, inner_end) = inner_bracket(text, cursor, open, close)?;
    Some((inner_start.saturating_sub(1), inner_end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inner_word_basic() {
        let text = "hello world foo";
        assert_eq!(inner_word(text, 0), Some((0, 5)));  // "hello"
        assert_eq!(inner_word(text, 2), Some((0, 5)));  // still "hello"
        assert_eq!(inner_word(text, 6), Some((6, 11))); // "world"
        assert_eq!(inner_word(text, 12), Some((12, 15))); // "foo"
    }

    #[test]
    fn test_a_word_basic() {
        let text = "hello world foo";
        assert_eq!(a_word(text, 0), Some((0, 6)));  // "hello " (with trailing space)
        assert_eq!(a_word(text, 6), Some((6, 12))); // "world " (with trailing space)
        assert_eq!(a_word(text, 12), Some((12, 15))); // "foo" (no trailing space)
    }

    #[test]
    fn test_inner_word_empty() {
        assert_eq!(inner_word("", 0), None);
    }

    #[test]
    fn test_inner_quote_double() {
        let text = r#"say "hello world" now"#;
        // cursor on 'h' at index 5 → inner = "hello world" (indices 5..16)
        assert_eq!(inner_quote(text, 5, '"'), Some((5, 16)));
        // cursor on the quote itself at index 4
        assert_eq!(inner_quote(text, 4, '"'), Some((5, 16)));
    }

    #[test]
    fn test_a_quote_double() {
        let text = r#"say "hello world" now"#;
        assert_eq!(a_quote(text, 5, '"'), Some((4, 17)));
    }

    #[test]
    fn test_inner_quote_single() {
        let text = "it's 'fine' really";
        assert_eq!(inner_quote(text, 7, '\''), Some((6, 10)));
    }

    #[test]
    fn test_inner_bracket_parens() {
        let text = "fn(a, b)";
        assert_eq!(inner_bracket(text, 3, '(', ')'), Some((3, 7)));
        assert_eq!(inner_bracket(text, 5, '(', ')'), Some((3, 7)));
    }

    #[test]
    fn test_a_bracket_parens() {
        let text = "fn(a, b)";
        assert_eq!(a_bracket(text, 3, '(', ')'), Some((2, 8)));
    }

    #[test]
    fn test_inner_bracket_nested() {
        let text = "a(b(c)d)e";
        // cursor on 'c' (index 4): innermost parens
        assert_eq!(inner_bracket(text, 4, '(', ')'), Some((4, 5)));
        // cursor on 'b' (index 2): outer parens
        assert_eq!(inner_bracket(text, 2, '(', ')'), Some((2, 7)));
    }

    #[test]
    fn test_inner_bracket_curly() {
        let text = "if {x > 0}";
        assert_eq!(inner_bracket(text, 5, '{', '}'), Some((4, 9)));
    }

    #[test]
    fn test_no_matching_bracket() {
        let text = "no brackets here";
        assert_eq!(inner_bracket(text, 3, '(', ')'), None);
    }

    #[test]
    fn test_no_matching_quote() {
        let text = "no quotes here";
        assert_eq!(inner_quote(text, 3, '"'), None);
    }

    #[test]
    fn test_inner_word_underscores() {
        let text = "hello_world foo";
        assert_eq!(inner_word(text, 0), Some((0, 11))); // "hello_world"
        assert_eq!(inner_word(text, 6), Some((0, 11))); // still "hello_world"
    }

    #[test]
    fn test_empty_quotes() {
        let text = r#"say "" now"#;
        assert_eq!(inner_quote(text, 4, '"'), Some((5, 5))); // empty content
    }

    #[test]
    fn test_inner_word_punctuation() {
        // Cursor on '.' should select just the '.'
        let text = "hello.world";
        assert_eq!(inner_word(text, 5), Some((5, 6))); // "."
    }

    #[test]
    fn test_inner_word_whitespace() {
        let text = "hello   world";
        assert_eq!(inner_word(text, 6), Some((5, 8))); // "   " (3 spaces)
    }

    #[test]
    fn test_inner_word_multiple_punct() {
        let text = "a::b";
        assert_eq!(inner_word(text, 1), Some((1, 3))); // "::"
    }
}
