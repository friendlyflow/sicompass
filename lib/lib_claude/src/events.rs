//! Claude Code `stream-json` event model.
//!
//! `claude --output-format stream-json` emits one JSON object per line (JSONL).
//! These types deserialize those lines. The model is **Claude-specific** — it
//! mirrors the documented event schema rather than being a generic JSON tree.
//!
//! Robustness rule: a single malformed or unexpected line must never abort the
//! session. [`parse_line`] swallows JSON errors and returns `None`; unknown
//! `type` values deserialize to [`StreamEvent::Unknown`]; unknown content
//! blocks to [`ContentBlock::Other`].

use serde::Deserialize;
use serde_json::Value;

/// One line of the `stream-json` output stream.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamEvent {
    System(SystemEvent),
    Assistant(AssistantEvent),
    User(UserEvent),
    Result(ResultEvent),
    /// `{"type":"stream_event","event":{...}}` — token-level streaming deltas,
    /// emitted only when `--include-partial-messages` is passed. The final
    /// consolidated `assistant` line still follows and supersedes them.
    #[serde(rename = "stream_event")]
    Partial(PartialEvent),
    /// Any `type` we do not model (forward compatibility).
    #[serde(other)]
    Unknown,
}

/// The `stream_event` envelope around one Anthropic streaming event.
#[derive(Debug, Clone, Deserialize)]
pub struct PartialEvent {
    pub event: PartialInner,
}

/// An inner streaming event (the Anthropic Messages API streaming schema).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartialInner {
    ContentBlockStart {
        #[serde(default)]
        content_block: Value,
    },
    ContentBlockDelta {
        delta: PartialDelta,
    },
    /// `message_start`, `content_block_stop`, `message_delta`, `message_stop`,
    /// and anything else — not needed for the live text preview.
    #[serde(other)]
    Other,
}

/// The `delta` of a `content_block_delta` event.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartialDelta {
    TextDelta {
        #[serde(default)]
        text: String,
    },
    /// `input_json_delta` (partial tool input), `thinking_delta`, … — skipped
    /// in the live preview; the consolidated event carries the final value.
    #[serde(other)]
    Other,
}

/// `{"type":"system","subtype":"init", ...}` — session bootstrap.
#[derive(Debug, Clone, Deserialize)]
pub struct SystemEvent {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
}

/// `{"type":"assistant","message":{...}}`. The `session_id` field is also
/// present but unmodeled — it never differs from the `system/init` one.
#[derive(Debug, Clone, Deserialize)]
pub struct AssistantEvent {
    pub message: ApiMessage,
}

/// `{"type":"user","message":{...}}` — tool results, mostly.
#[derive(Debug, Clone, Deserialize)]
pub struct UserEvent {
    pub message: ApiMessage,
}

/// An Anthropic API message embedded in an assistant/user event.
///
/// Only `content` is modeled — the assistant/user distinction comes from the
/// enclosing [`StreamEvent`] variant, so `role` is not needed.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiMessage {
    #[serde(default)]
    pub content: ContentField,
}

/// `content` is sometimes a bare string, sometimes an array of blocks.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ContentField {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl Default for ContentField {
    fn default() -> Self {
        ContentField::Blocks(Vec::new())
    }
}

impl ContentField {
    /// Flatten to the list of blocks, promoting a bare string to one text block.
    pub fn blocks(&self) -> Vec<ContentBlock> {
        match self {
            ContentField::Text(t) => vec![ContentBlock::Text { text: t.clone() }],
            ContentField::Blocks(b) => b.clone(),
        }
    }
}

/// A single content block inside an [`ApiMessage`].
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolUse {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        input: Value,
    },
    ToolResult {
        #[serde(default)]
        tool_use_id: String,
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    /// `thinking`, `image`, … — skipped in v1.
    #[serde(other)]
    Other,
}

/// `{"type":"result","subtype":"success", ...}` — end-of-turn summary.
///
/// The event also carries `result` (the final assistant text, already streamed
/// via `assistant` events), `session_id`, and `usage`; those are not modeled
/// because serde ignores unmodeled fields and we render only the cost line.
#[derive(Debug, Clone, Deserialize)]
pub struct ResultEvent {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub is_error: bool,
}

/// Parse one complete JSONL line into a [`StreamEvent`].
///
/// Returns `None` for blank lines and for any line that is not valid JSON of a
/// shape we recognize — a stray diagnostic line must not kill the session.
pub fn parse_line(line: &str) -> Option<StreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<StreamEvent>(trimmed).ok()
}

/// Convenience: parse a batch of lines, dropping any that fail. Test-only —
/// the provider parses lines one at a time as the reader thread delivers them.
#[cfg(test)]
pub fn parse_lines<'a, I: IntoIterator<Item = &'a str>>(lines: I) -> Vec<StreamEvent> {
    lines.into_iter().filter_map(parse_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-opus-4-7","cwd":"/tmp","permissionMode":"default","tools":["Read","Bash"],"mcp_servers":[]}"#;
        match parse_line(line) {
            Some(StreamEvent::System(s)) => {
                assert_eq!(s.subtype, "init");
                assert_eq!(s.session_id, "s1");
                assert_eq!(s.model.as_deref(), Some("claude-opus-4-7"));
                assert_eq!(s.permission_mode.as_deref(), Some("default"));
                assert_eq!(s.tools.len(), 2);
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_text() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]},"session_id":"s1"}"#;
        match parse_line(line) {
            Some(StreamEvent::Assistant(a)) => {
                let blocks = a.message.content.blocks();
                assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hello"));
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_tool_use() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"Bash","input":{"command":"ls"}}]}}"#;
        match parse_line(line) {
            Some(StreamEvent::Assistant(a)) => {
                let blocks = a.message.content.blocks();
                match &blocks[0] {
                    ContentBlock::ToolUse { id, name, .. } => {
                        assert_eq!(id, "tu_1");
                        assert_eq!(name, "Bash");
                    }
                    other => panic!("expected ToolUse, got {other:?}"),
                }
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn parses_user_tool_result() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"file.txt","is_error":false}]}}"#;
        match parse_line(line) {
            Some(StreamEvent::User(u)) => {
                let blocks = u.message.content.blocks();
                assert!(matches!(&blocks[0], ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_1"));
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn parses_partial_text_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hel"}}}"#;
        match parse_line(line) {
            Some(StreamEvent::Partial(p)) => match p.event {
                PartialInner::ContentBlockDelta {
                    delta: PartialDelta::TextDelta { text },
                } => assert_eq!(text, "hel"),
                other => panic!("expected text delta, got {other:?}"),
            },
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn parses_partial_content_block_start_tool_use() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_2","name":"Bash","input":{}}}}"#;
        match parse_line(line) {
            Some(StreamEvent::Partial(p)) => match p.event {
                PartialInner::ContentBlockStart { content_block } => {
                    assert_eq!(content_block["type"], "tool_use");
                    assert_eq!(content_block["name"], "Bash");
                }
                other => panic!("expected content_block_start, got {other:?}"),
            },
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn parses_partial_unknown_inner_as_other() {
        for line in [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#,
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{"}}}"#,
        ] {
            match parse_line(line) {
                Some(StreamEvent::Partial(_)) => {}
                other => panic!("expected Partial for {line}, got {other:?}"),
            }
        }
    }

    #[test]
    fn parses_result_success() {
        let line = r#"{"type":"result","subtype":"success","result":"done","session_id":"s1","num_turns":3,"duration_ms":12400,"total_cost_usd":0.0231,"is_error":false}"#;
        match parse_line(line) {
            Some(StreamEvent::Result(r)) => {
                assert_eq!(r.subtype, "success");
                assert_eq!(r.num_turns, Some(3));
                assert_eq!(r.total_cost_usd, Some(0.0231));
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn parses_result_error_max_turns() {
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
        match parse_line(line) {
            Some(StreamEvent::Result(r)) => {
                assert_eq!(r.subtype, "error_max_turns");
                assert!(r.is_error);
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn content_as_bare_string() {
        let line = r#"{"type":"user","message":{"role":"user","content":"plain text"}}"#;
        match parse_line(line) {
            Some(StreamEvent::User(u)) => {
                let blocks = u.message.content.blocks();
                assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "plain text"));
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_is_unknown() {
        let line = r#"{"type":"control_request","request":{"subtype":"can_use_tool"}}"#;
        assert!(matches!(parse_line(line), Some(StreamEvent::Unknown)));
    }

    #[test]
    fn unknown_content_block_is_other() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"..."}]}}"#;
        match parse_line(line) {
            Some(StreamEvent::Assistant(a)) => {
                assert!(matches!(&a.message.content.blocks()[0], ContentBlock::Other));
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn blank_and_malformed_lines_are_none() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
        assert!(parse_line("not json at all").is_none());
        assert!(parse_line("{ broken").is_none());
    }

    #[test]
    fn parse_lines_drops_failures() {
        let raw = [
            r#"{"type":"system","subtype":"init"}"#,
            "garbage",
            "",
            r#"{"type":"result","subtype":"success"}"#,
        ];
        let evs = parse_lines(raw);
        assert_eq!(evs.len(), 2);
    }
}
