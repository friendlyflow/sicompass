//! Conversation state and its projection into an FFON document.
//!
//! [`Conversation`] is an append-only log of [`Turn`]s built by folding
//! [`StreamEvent`]s through [`Conversation::apply`]. [`build`] renders that log
//! (plus the recall history and the live input value) into the flat
//! `Vec<FfonElement>` the provider returns from `fetch()`.

use serde_json::Value;
use sicompass_sdk::FfonElement;

use crate::events::{ContentBlock, PartialDelta, PartialInner, ResultEvent, StreamEvent};

/// A tool invocation requested by the assistant.
#[derive(Debug, Clone)]
pub struct ToolUseRec {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// One entry in the conversation log.
#[derive(Debug, Clone)]
pub enum Turn {
    /// An assistant message: prose plus any tools it asked to run.
    Assistant { texts: Vec<String>, tools: Vec<ToolUseRec> },
    /// A user message we sent into the session.
    User { text: String },
    /// The result of a tool the assistant ran.
    ToolResult {
        tool_name: String,
        summary: String,
        is_error: bool,
    },
}

/// A live, in-progress assistant message reconstructed from `--include-partial-
/// messages` token deltas. Superseded by the consolidated `assistant` event.
#[derive(Debug, Default)]
pub struct PartialAssistant {
    /// `true` once any partial event for the current message has arrived.
    pub active: bool,
    /// Text accumulated from `text_delta` events across the message's blocks.
    pub text: String,
    /// Names of `tool_use` blocks the message has started.
    pub tools: Vec<String>,
}

impl PartialAssistant {
    fn clear(&mut self) {
        *self = PartialAssistant::default();
    }

    /// Whether there is anything worth showing as a live preview.
    fn has_content(&self) -> bool {
        self.active && (!self.text.is_empty() || !self.tools.is_empty())
    }
}

/// The full state of one streaming `claude` session.
#[derive(Debug, Default)]
pub struct Conversation {
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub permission_mode: Option<String>,
    pub tools_count: usize,
    pub turns: Vec<Turn>,
    pub last_result: Option<ResultEvent>,
    /// `true` between sending a user message and receiving its `result` event.
    pub busy: bool,
    /// Live token-level preview of the assistant message currently streaming.
    pub partial: PartialAssistant,
}

impl Conversation {
    /// Record a user message we just sent. Called from `commit_edit`, not from
    /// the event stream — the stream echoes our input back as a `user` event,
    /// which [`apply`](Self::apply) deliberately ignores to avoid double-render.
    pub fn push_user(&mut self, text: &str) {
        self.turns.push(Turn::User { text: text.to_owned() });
        self.busy = true;
        self.partial.clear();
    }

    /// Fold one stream event into the conversation state.
    pub fn apply(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::System(s) => {
                if s.subtype == "init" {
                    if !s.session_id.is_empty() {
                        self.session_id = Some(s.session_id);
                    }
                    self.model = s.model;
                    self.cwd = s.cwd;
                    self.permission_mode = s.permission_mode;
                    self.tools_count = s.tools.len();
                }
            }
            StreamEvent::Assistant(a) => {
                let mut texts = Vec::new();
                let mut tools = Vec::new();
                for block in a.message.content.blocks() {
                    match block {
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                texts.push(text);
                            }
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            tools.push(ToolUseRec { id, name, input });
                        }
                        ContentBlock::ToolResult { .. } | ContentBlock::Other => {}
                    }
                }
                if !texts.is_empty() || !tools.is_empty() {
                    self.turns.push(Turn::Assistant { texts, tools });
                }
                // The consolidated event is authoritative — drop the live
                // preview now that the real turn is recorded.
                self.partial.clear();
            }
            StreamEvent::User(u) => {
                // A `user` event carries tool results (and an echo of our own
                // text input, which we skip — `push_user` already logged it).
                for block in u.message.content.blocks() {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = block
                    {
                        let tool_name = self.tool_name_for(&tool_use_id);
                        self.turns.push(Turn::ToolResult {
                            tool_name,
                            summary: stringify_content(&content),
                            is_error,
                        });
                    }
                }
            }
            StreamEvent::Partial(p) => match p.event {
                PartialInner::ContentBlockStart { content_block } => {
                    self.partial.active = true;
                    if content_block.get("type").and_then(|t| t.as_str())
                        == Some("tool_use")
                    {
                        if let Some(name) =
                            content_block.get("name").and_then(|n| n.as_str())
                        {
                            self.partial.tools.push(name.to_owned());
                        }
                    }
                }
                PartialInner::ContentBlockDelta { delta } => {
                    self.partial.active = true;
                    if let PartialDelta::TextDelta { text } = delta {
                        self.partial.text.push_str(&text);
                    }
                }
                PartialInner::Other => {}
            },
            StreamEvent::Result(r) => {
                self.busy = false;
                self.last_result = Some(r);
                self.partial.clear();
            }
            StreamEvent::Unknown => {}
        }
    }

    /// Resolve a `tool_use_id` to the tool's name by scanning prior assistant
    /// turns; falls back to the raw id when no match is found.
    fn tool_name_for(&self, tool_use_id: &str) -> String {
        for turn in self.turns.iter().rev() {
            if let Turn::Assistant { tools, .. } = turn {
                if let Some(t) = tools.iter().find(|t| t.id == tool_use_id) {
                    return t.name.clone();
                }
            }
        }
        tool_use_id.to_owned()
    }
}

/// Lines past this cap are collapsed to a single "… N more" line.
const TOOL_RESULT_LINE_CAP: usize = 40;
/// Compact-JSON tool input is truncated to this many characters.
const TOOL_INPUT_CHARS: usize = 200;

/// Stringify a tool-result `content` value: a bare string passes through, a
/// block array joins its text blocks, anything else becomes compact JSON.
pub fn stringify_content(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(text.to_owned());
                } else if let Some(s) = item.as_str() {
                    parts.push(s.to_owned());
                }
            }
            if parts.is_empty() {
                v.to_string()
            } else {
                parts.join("\n")
            }
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Truncate a string to `max` characters, appending `…` when cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Push each line of `text` as its own navigable `Str`, capped at
/// `TOOL_RESULT_LINE_CAP` with a trailing "… N more" marker.
fn push_capped_lines(obj: &mut sicompass_sdk::FfonObject, text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    let shown = lines.len().min(TOOL_RESULT_LINE_CAP);
    for line in &lines[..shown] {
        obj.push(FfonElement::new_str(*line));
    }
    if lines.len() > shown {
        obj.push(FfonElement::new_str(format!(
            "… ({} more lines)",
            lines.len() - shown
        )));
    }
}

/// Render the conversation into the flat FFON element list `fetch()` returns.
///
/// `history` is the list of past prompts (oldest first); `pending_input` is the
/// value currently typed into the live input slot.
pub fn build(convo: &Conversation, history: &[String], pending_input: &str) -> Vec<FfonElement> {
    let mut out: Vec<FfonElement> = Vec::new();

    // --- session header --------------------------------------------------
    if convo.session_id.is_some() || convo.model.is_some() {
        let model = convo.model.as_deref().unwrap_or("claude");
        let mode = convo.permission_mode.as_deref().unwrap_or("default");
        let mut header = FfonElement::new_obj(format!(
            "session: {model}  ({mode}, {} tools)",
            convo.tools_count
        ));
        if let Some(obj) = header.as_obj_mut() {
            if let Some(cwd) = &convo.cwd {
                obj.push(FfonElement::new_str(format!("cwd: {cwd}")));
            }
            if let Some(sid) = &convo.session_id {
                obj.push(FfonElement::new_str(format!("session id: {sid}")));
            }
        }
        out.push(header);
    }

    // --- turns -----------------------------------------------------------
    for turn in &convo.turns {
        match turn {
            Turn::User { text } => {
                let first = text.lines().next().unwrap_or("");
                out.push(FfonElement::new_str(format!("you: {first}")));
                for line in text.lines().skip(1) {
                    out.push(FfonElement::new_str(line.to_owned()));
                }
            }
            Turn::Assistant { texts, tools } => {
                let mut obj = FfonElement::new_obj("claude:");
                if let Some(o) = obj.as_obj_mut() {
                    for text in texts {
                        for line in text.lines() {
                            o.push(FfonElement::new_str(line.to_owned()));
                        }
                    }
                    for tool in tools {
                        let mut t = FfonElement::new_obj(format!("tool: {}", tool.name));
                        if let Some(to) = t.as_obj_mut() {
                            to.push(FfonElement::new_str(format!("<id>{}</id>", tool.id)));
                            let compact = serde_json::to_string(&tool.input)
                                .unwrap_or_else(|_| "{}".to_owned());
                            to.push(FfonElement::new_str(format!(
                                "input: {}",
                                truncate_chars(&compact, TOOL_INPUT_CHARS)
                            )));
                        }
                        o.push(t);
                    }
                }
                out.push(obj);
            }
            Turn::ToolResult {
                tool_name,
                summary,
                is_error,
            } => {
                let suffix = if *is_error { "  [error]" } else { "" };
                let mut obj =
                    FfonElement::new_obj(format!("tool result: {tool_name}{suffix}"));
                if let Some(o) = obj.as_obj_mut() {
                    push_capped_lines(o, summary);
                }
                out.push(obj);
            }
        }
    }

    // --- live streaming preview -----------------------------------------
    // The in-progress assistant message, reconstructed from token deltas.
    // Cleared the moment the consolidated `assistant` turn lands above.
    if convo.partial.has_content() {
        let mut obj = FfonElement::new_obj("claude: (streaming…)");
        if let Some(o) = obj.as_obj_mut() {
            for line in convo.partial.text.lines() {
                o.push(FfonElement::new_str(line.to_owned()));
            }
            for name in &convo.partial.tools {
                o.push(FfonElement::new_str(format!("tool: {name} (preparing…)")));
            }
        }
        out.push(obj);
    }

    // --- result footer ---------------------------------------------------
    if let Some(r) = &convo.last_result {
        let turns = r.num_turns.unwrap_or(0);
        let secs = r.duration_ms.unwrap_or(0) as f64 / 1000.0;
        let cost = r.total_cost_usd.unwrap_or(0.0);
        let label = if r.is_error { "result (error)" } else { "result" };
        out.push(FfonElement::new_str(format!(
            "{label}: {} — {} turns, {:.1}s, ${:.4}",
            r.subtype, turns, secs, cost
        )));
    }

    // --- in-flight indicator --------------------------------------------
    // Redundant once the streaming preview is on screen.
    if convo.busy && !convo.partial.has_content() {
        out.push(FfonElement::new_str("claude is working…"));
    }

    // --- live input slot (terminal-style +iR: <input> inside <radio>) ----
    let mut slot = FfonElement::new_obj(format!(
        "<radio>send to claude<input>{pending_input}</input></radio>"
    ));
    if let Some(obj) = slot.as_obj_mut() {
        for prompt in history.iter().rev() {
            obj.push(FfonElement::new_str(prompt.clone()));
        }
    }
    out.push(slot);

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{parse_lines, ContentField};

    fn convo_from(lines: &[&str]) -> Conversation {
        let mut c = Conversation::default();
        for ev in parse_lines(lines.iter().copied()) {
            c.apply(ev);
        }
        c
    }

    #[test]
    fn apply_fills_session_metadata() {
        let c = convo_from(&[
            r#"{"type":"system","subtype":"init","session_id":"s9","model":"opus","cwd":"/w","permissionMode":"plan","tools":["Read","Bash","Edit"]}"#,
        ]);
        assert_eq!(c.session_id.as_deref(), Some("s9"));
        assert_eq!(c.model.as_deref(), Some("opus"));
        assert_eq!(c.permission_mode.as_deref(), Some("plan"));
        assert_eq!(c.tools_count, 3);
    }

    #[test]
    fn apply_collects_assistant_text_and_tools() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"on it"},{"type":"tool_use","id":"tu_1","name":"Bash","input":{"command":"ls"}}]}}"#,
        ]);
        assert_eq!(c.turns.len(), 1);
        match &c.turns[0] {
            Turn::Assistant { texts, tools } => {
                assert_eq!(texts, &["on it".to_owned()]);
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].name, "Bash");
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_resolves_tool_name_from_prior_use() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_7","name":"Grep","input":{}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_7","content":"3 matches","is_error":false}]}}"#,
        ]);
        let last = c.turns.last().unwrap();
        match last {
            Turn::ToolResult { tool_name, summary, is_error } => {
                assert_eq!(tool_name, "Grep");
                assert_eq!(summary, "3 matches");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn result_event_clears_busy_and_stores_summary() {
        let mut c = Conversation::default();
        c.push_user("hi");
        assert!(c.busy);
        for ev in parse_lines([
            r#"{"type":"result","subtype":"success","num_turns":2,"duration_ms":3400,"total_cost_usd":0.01}"#,
        ]) {
            c.apply(ev);
        }
        assert!(!c.busy);
        assert!(c.last_result.is_some());
    }

    #[test]
    fn user_text_echo_is_not_double_rendered() {
        // We log the user turn via push_user; the stream's echoed user text
        // event must not add a second turn.
        let mut c = Conversation::default();
        c.push_user("do the thing");
        for ev in parse_lines([
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"do the thing"}]}}"#,
        ]) {
            c.apply(ev);
        }
        assert_eq!(c.turns.len(), 1);
        assert!(matches!(&c.turns[0], Turn::User { .. }));
    }

    #[test]
    fn build_emits_header_turns_footer_and_input_slot() {
        let mut c = convo_from(&[
            r#"{"type":"system","subtype":"init","session_id":"s1","model":"opus","tools":["Read"]}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"line one\nline two"}]}}"#,
            r#"{"type":"result","subtype":"success","num_turns":1,"duration_ms":1200,"total_cost_usd":0.002}"#,
        ]);
        c.turns.insert(0, Turn::User { text: "hello".to_owned() });
        let history = vec!["older".to_owned(), "hello".to_owned()];
        let out = build(&c, &history, "draft");

        // header
        assert!(out[0].as_obj().unwrap().key.starts_with("session: opus"));
        // user turn
        assert_eq!(out[1].as_str(), Some("you: hello"));
        // assistant turn with two text lines as children
        let claude = out[2].as_obj().unwrap();
        assert_eq!(claude.key, "claude:");
        assert_eq!(claude.children.len(), 2);
        // result footer
        assert!(out[3].as_str().unwrap().starts_with("result: success"));
        // trailing input slot
        let slot = out.last().unwrap().as_obj().unwrap();
        assert!(slot.key.contains("<input>draft</input>"));
        assert!(slot.key.contains("<radio>"));
        // history newest-first
        assert_eq!(slot.children[0].as_str(), Some("hello"));
        assert_eq!(slot.children[1].as_str(), Some("older"));
    }

    #[test]
    fn build_shows_working_line_while_busy() {
        let mut c = Conversation::default();
        c.push_user("q");
        let out = build(&c, &[], "");
        assert!(out.iter().any(|e| e.as_str() == Some("claude is working…")));
    }

    #[test]
    fn build_tool_use_renders_id_and_input() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        ]);
        let out = build(&c, &[], "");
        let claude = out[0].as_obj().unwrap();
        let tool = claude.children[0].as_obj().unwrap();
        assert_eq!(tool.key, "tool: Bash");
        assert_eq!(tool.children[0].as_str(), Some("<id>tu_x</id>"));
        assert!(tool.children[1].as_str().unwrap().starts_with("input: "));
    }

    #[test]
    fn tool_result_lines_are_capped() {
        let big: String = (0..100).map(|i| format!("row {i}\n")).collect();
        let mut c = Conversation::default();
        c.turns.push(Turn::ToolResult {
            tool_name: "Bash".to_owned(),
            summary: big,
            is_error: false,
        });
        let out = build(&c, &[], "");
        let res = out[0].as_obj().unwrap();
        assert_eq!(res.children.len(), TOOL_RESULT_LINE_CAP + 1);
        assert!(res.children.last().unwrap().as_str().unwrap().contains("more lines"));
    }

    #[test]
    fn stringify_content_handles_string_array_and_value() {
        assert_eq!(stringify_content(&Value::String("hi".into())), "hi");
        let arr: Value = serde_json::from_str(r#"[{"type":"text","text":"a"},{"type":"text","text":"b"}]"#).unwrap();
        assert_eq!(stringify_content(&arr), "a\nb");
        assert_eq!(stringify_content(&Value::Null), "");
    }

    // ContentField is exercised indirectly above; keep an explicit smoke test.
    #[test]
    fn content_field_default_is_empty_blocks() {
        assert!(matches!(ContentField::default(), ContentField::Blocks(b) if b.is_empty()));
    }

    // --- v2: partial / live streaming -----------------------------------

    #[test]
    fn partial_text_deltas_accumulate() {
        let c = convo_from(&[
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo!"}}}"#,
        ]);
        assert!(c.partial.active);
        assert_eq!(c.partial.text, "Hello!");
        assert!(c.turns.is_empty());
    }

    #[test]
    fn partial_tool_use_start_records_name() {
        let c = convo_from(&[
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_9","name":"Edit","input":{}}}}"#,
        ]);
        assert_eq!(c.partial.tools, vec!["Edit".to_owned()]);
    }

    #[test]
    fn consolidated_assistant_event_clears_partial_preview() {
        let c = convo_from(&[
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"par"}}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"partial then full"}]}}"#,
        ]);
        assert!(!c.partial.active);
        assert!(c.partial.text.is_empty());
        assert_eq!(c.turns.len(), 1);
        assert!(matches!(&c.turns[0], Turn::Assistant { .. }));
    }

    #[test]
    fn result_event_clears_partial_preview() {
        let c = convo_from(&[
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}}}"#,
            r#"{"type":"result","subtype":"success"}"#,
        ]);
        assert!(!c.partial.active);
    }

    #[test]
    fn build_renders_streaming_preview() {
        let c = convo_from(&[
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"thinking out loud"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"Bash","input":{}}}}"#,
        ]);
        let out = build(&c, &[], "");
        let preview = out
            .iter()
            .find_map(|e| e.as_obj())
            .filter(|o| o.key == "claude: (streaming…)")
            .expect("streaming preview obj");
        assert_eq!(preview.children[0].as_str(), Some("thinking out loud"));
        assert_eq!(
            preview.children[1].as_str(),
            Some("tool: Bash (preparing…)")
        );
    }

    #[test]
    fn streaming_preview_suppresses_working_line() {
        let mut c = Conversation::default();
        c.push_user("q");
        assert!(c.busy);
        for ev in parse_lines([
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}}"#,
        ]) {
            c.apply(ev);
        }
        let out = build(&c, &[], "");
        assert!(
            !out.iter().any(|e| e.as_str() == Some("claude is working…")),
            "working line should be hidden once the preview is visible"
        );
    }

    #[test]
    fn push_user_clears_a_stale_partial() {
        let mut c = convo_from(&[
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"old"}}}"#,
        ]);
        assert!(c.partial.active);
        c.push_user("new question");
        assert!(!c.partial.active);
        assert!(c.partial.text.is_empty());
    }
}
