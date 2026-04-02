use oxicode_common::{ContentBlock, Message};

/// Lines kept from head and tail of large tool results.
const TOOL_RESULT_KEEP_LINES: usize = 50;

/// Layer-2 defense: in-place compression of message content blocks.
///
/// Actions performed (in order):
/// 1. Compress `ToolResult` content: keep first/last 50 lines, insert truncation marker.
/// 2. Remove all `Thinking` blocks entirely.
/// 3. Collapse consecutive `Text` blocks into one.
pub fn microcompact_messages(messages: &mut [Message]) {
    let mut thinking_removed: usize = 0;
    let mut tool_results_compressed: usize = 0;
    let mut text_blocks_collapsed: usize = 0;

    for msg in messages.iter_mut() {
        // Step 1 & 2: compress tool results and strip thinking blocks.
        let mut new_content: Vec<ContentBlock> = Vec::with_capacity(msg.content.len());

        for block in msg.content.drain(..) {
            match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let compressed = compress_tool_result(&content);
                    if compressed != content {
                        tool_results_compressed += 1;
                    }
                    new_content.push(ContentBlock::ToolResult {
                        tool_use_id,
                        content: compressed,
                        is_error,
                    });
                }
                ContentBlock::Thinking { .. } => {
                    thinking_removed += 1;
                    // Drop entirely.
                }
                other => new_content.push(other),
            }
        }

        // Step 3: collapse consecutive Text blocks.
        let collapsed = collapse_text_blocks(new_content, &mut text_blocks_collapsed);
        msg.content = collapsed;
    }

    tracing::info!(
        thinking_removed,
        tool_results_compressed,
        text_blocks_collapsed,
        "L2: microcompact complete"
    );
}

/// Compress a tool result string: keep first/last N lines, insert marker.
fn compress_tool_result(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let keep = TOOL_RESULT_KEEP_LINES;

    // Only compress when there are more lines than 2×keep.
    if total <= keep * 2 {
        return content.to_string();
    }

    let head = lines[..keep].join("\n");
    let tail = lines[total - keep..].join("\n");
    let dropped = total - keep * 2;

    format!("{head}\n... [truncated {dropped} lines] ...\n{tail}")
}

/// Merge runs of consecutive `Text` blocks into a single block.
fn collapse_text_blocks(
    blocks: Vec<ContentBlock>,
    collapsed_count: &mut usize,
) -> Vec<ContentBlock> {
    let mut result: Vec<ContentBlock> = Vec::with_capacity(blocks.len());

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                if let Some(ContentBlock::Text { text: prev }) = result.last_mut() {
                    prev.push_str(&text);
                    *collapsed_count += 1;
                } else {
                    result.push(ContentBlock::Text { text });
                }
            }
            other => result.push(other),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_thinking_blocks() {
        let mut msgs = vec![Message::user("")];
        msgs[0].content = vec![
            ContentBlock::Text {
                text: "hello".to_string(),
            },
            ContentBlock::Thinking {
                thinking: "internal reasoning".to_string(),
            },
        ];
        microcompact_messages(&mut msgs);
        assert_eq!(msgs[0].content.len(), 1);
        assert!(matches!(&msgs[0].content[0], ContentBlock::Text { text } if text == "hello"));
    }

    #[test]
    fn compresses_long_tool_result() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
        let content = lines.join("\n");
        let result = compress_tool_result(&content);
        assert!(result.contains("truncated"));
        assert!(result.contains("line 0"));
        assert!(result.contains("line 199"));
        let result_lines: Vec<&str> = result.lines().collect();
        // head(50) + marker(1) + tail(50) = 101
        assert_eq!(result_lines.len(), 101);
    }

    #[test]
    fn short_tool_result_unchanged() {
        let content = "line 1\nline 2\nline 3";
        assert_eq!(compress_tool_result(content), content);
    }

    #[test]
    fn collapses_consecutive_text_blocks() {
        let mut msgs = vec![Message::user("")];
        msgs[0].content = vec![
            ContentBlock::Text {
                text: "foo ".to_string(),
            },
            ContentBlock::Text {
                text: "bar".to_string(),
            },
        ];
        microcompact_messages(&mut msgs);
        assert_eq!(msgs[0].content.len(), 1);
        assert!(
            matches!(&msgs[0].content[0], ContentBlock::Text { text } if text == "foo bar")
        );
    }
}
