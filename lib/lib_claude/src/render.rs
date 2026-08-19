//! Conversation state and its projection into an FFON document.
//!
//! [`Conversation`] is an append-only log of [`Turn`]s built by folding
//! [`StreamEvent`]s through [`Conversation::apply`]. [`build`] renders that log
//! (plus the live input value) into the flat `Vec<FfonElement>` the provider
//! returns from `fetch()`.

use std::collections::{HashMap, HashSet};

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
    Assistant {
        texts: Vec<String>,
        tools: Vec<ToolUseRec>,
    },
    /// A user message we sent into the session.
    User { text: String },
    /// The result of a tool the assistant ran. `tool_use_id` links it back to
    /// the [`ToolUseRec`] in an earlier [`Turn::Assistant`], which is how
    /// [`build`] nests the output under the call that produced it.
    ToolResult {
        tool_use_id: String,
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
        self.turns.push(Turn::User {
            text: text.to_owned(),
        });
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
                            tool_use_id,
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
                    if content_block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        if let Some(name) = content_block.get("name").and_then(|n| n.as_str()) {
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
        obj.push(FfonElement::new_str(escape_markup(line)));
    }
    if lines.len() > shown {
        obj.push(FfonElement::new_str(format!(
            "… ({} more lines)",
            lines.len() - shown
        )));
    }
}

/// Escape angle brackets so text we did not write cannot be read as markup.
///
/// `<...>` in an element string is FFON inline markup: an answer that merely
/// mentions `<input>` would render as an editable `-i` field instead of a line
/// of prose, and the tag itself would be swallowed from the display. `\<` is
/// the tag syntax's own escape and `strip_display` unescapes it, so the text
/// still reads back exactly as it was written.
///
/// Applies to everything the model, the shell or the user produced — prose,
/// tool input, tool output, the prompt echo, the session's own metadata. The
/// live input slot is deliberately *not* escaped: its `<input>` is real markup
/// the app parses back out with `extract_input`.
fn escape_markup(s: &str) -> String {
    s.replace('<', "\\<").replace('>', "\\>")
}

/// Split an assistant message into the lines its label shows.
///
/// The message keeps its own line structure — paragraphs, headings and list
/// items running together into one wall of text is unreadable — with blank
/// lines dropped. The caller joins these back with `\n` into a single label:
/// the app's text renderer breaks on an explicit newline, so the message reads
/// as lines while staying one element.
///
/// Fenced code blocks pass through verbatim, fences and blank lines included:
/// inside them the whitespace is the content, not layout.
fn message_lines(texts: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    for line in texts.iter().flat_map(|t| t.lines()) {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            lines.push(escape_markup(line));
        } else if in_fence {
            lines.push(escape_markup(line));
        } else {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(escape_markup(trimmed));
            }
        }
    }
    lines
}

/// The label for one tool call: its name, its input as compact JSON, and an
/// `[error]` marker when the result came back a failure. The marker rides the
/// label so a failure is announced without stepping into the node.
fn tool_label(tool: &ToolUseRec, result: Option<(&str, bool)>) -> String {
    let compact = serde_json::to_string(&tool.input).unwrap_or_else(|_| "{}".to_owned());
    let suffix = if matches!(result, Some((_, true))) {
        "  [error]"
    } else {
        ""
    };
    format!(
        "{}: {}{suffix}",
        escape_markup(&tool.name),
        escape_markup(&truncate_chars(&compact, TOOL_INPUT_CHARS))
    )
}

/// Build `key` as an element whose children are `summary`'s lines.
///
/// The renderer derives the `+`/`-` list prefix from the element *type*, not
/// from the child count (`build_obj_label` in the app's `list.rs` always emits
/// `+`), so a childless `Obj` would announce as expandable with nothing to
/// expand into. Anything with no body must therefore be a `Str`.
fn body_element(key: String, summary: Option<&str>) -> FfonElement {
    match summary {
        Some(text) if !text.is_empty() => {
            let mut el = FfonElement::new_obj(key);
            if let Some(o) = el.as_obj_mut() {
                push_capped_lines(o, text);
            }
            el
        }
        // A call still awaiting its result, or one that returned nothing.
        _ => FfonElement::new_str(key),
    }
}

/// Render the conversation into the flat FFON element list `fetch()` returns.
///
/// `pending_input` is the value currently typed into the live input slot.
pub fn build(convo: &Conversation, pending_input: &str) -> Vec<FfonElement> {
    let mut out: Vec<FfonElement> = Vec::new();

    // --- correlate tool results with their calls -------------------------
    // Each result is rendered *inside* the `tool` node it answers rather than
    // as a sibling further down the transcript. A result whose call is not in
    // the log (a resumed session, say) has nowhere to nest and stays top-level.
    let mut used_ids: HashSet<&str> = HashSet::new();
    for turn in &convo.turns {
        if let Turn::Assistant { tools, .. } = turn {
            used_ids.extend(tools.iter().map(|t| t.id.as_str()));
        }
    }
    let mut results: HashMap<&str, (&str, bool)> = HashMap::new();
    for turn in &convo.turns {
        if let Turn::ToolResult {
            tool_use_id,
            summary,
            is_error,
            ..
        } = turn
        {
            if used_ids.contains(tool_use_id.as_str()) {
                results.insert(tool_use_id.as_str(), (summary.as_str(), *is_error));
            }
        }
    }

    // --- session header --------------------------------------------------
    if convo.session_id.is_some() || convo.model.is_some() {
        let model = convo.model.as_deref().unwrap_or("claude");
        let mode = convo.permission_mode.as_deref().unwrap_or("default");
        let key = format!(
            "session: {}  ({}, {} tools)",
            escape_markup(model),
            escape_markup(mode),
            convo.tools_count
        );
        let mut detail = String::new();
        if let Some(cwd) = &convo.cwd {
            detail.push_str(&format!("cwd: {}\n", escape_markup(cwd)));
        }
        if let Some(sid) = &convo.session_id {
            detail.push_str(&format!("session id: {}\n", escape_markup(sid)));
        }
        // `body_element` keeps it a `Str` when there is no detail to expand.
        out.push(body_element(key, Some(detail.trim_end_matches('\n'))));
    }

    // --- turns -----------------------------------------------------------
    for turn in &convo.turns {
        match turn {
            Turn::User { text } => {
                let first = text.lines().next().unwrap_or("");
                out.push(FfonElement::new_str(format!(
                    "you: {}",
                    escape_markup(first)
                )));
                for line in text.lines().skip(1) {
                    out.push(FfonElement::new_str(escape_markup(line)));
                }
            }
            Turn::Assistant { texts, tools } => {
                // The whole message rides the `claude:` label — the app's
                // text renderer breaks a label on explicit `\n`, so the lines
                // show as lines without costing a level to step into.
                let lines = message_lines(texts);

                // A turn that said nothing and ran a single tool folds the call
                // straight into the `claude:` label — an empty container above
                // one tool would cost a level and announce nothing.
                if lines.is_empty() && tools.len() == 1 {
                    let tool = &tools[0];
                    let result = results.get(tool.id.as_str()).copied();
                    out.push(body_element(
                        format!("claude: {}", tool_label(tool, result)),
                        result.map(|(summary, _)| summary),
                    ));
                    continue;
                }

                let key = if lines.is_empty() {
                    "claude:".to_owned()
                } else {
                    format!("claude: {}", lines.join("\n"))
                };
                if tools.is_empty() {
                    // Nothing to expand into, so the row must read `-`.
                    out.push(FfonElement::new_str(key));
                    continue;
                }
                let mut obj = FfonElement::new_obj(key);
                if let Some(o) = obj.as_obj_mut() {
                    for tool in tools {
                        let result = results.get(tool.id.as_str()).copied();
                        o.push(body_element(
                            tool_label(tool, result),
                            result.map(|(summary, _)| summary),
                        ));
                    }
                }
                out.push(obj);
            }
            Turn::ToolResult {
                tool_use_id,
                tool_name,
                summary,
                is_error,
            } => {
                // Already rendered inside the tool node that asked for it.
                if used_ids.contains(tool_use_id.as_str()) {
                    continue;
                }
                let suffix = if *is_error { "  [error]" } else { "" };
                out.push(body_element(
                    format!("tool result: {tool_name}{suffix}"),
                    Some(summary.as_str()),
                ));
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
                o.push(FfonElement::new_str(escape_markup(line)));
            }
            for name in &convo.partial.tools {
                o.push(FfonElement::new_str(format!(
                    "tool: {} (preparing…)",
                    escape_markup(name)
                )));
            }
        }
        out.push(obj);
    }

    // --- result footer ---------------------------------------------------
    if let Some(r) = &convo.last_result {
        let turns = r.num_turns.unwrap_or(0);
        let secs = r.duration_ms.unwrap_or(0) as f64 / 1000.0;
        let cost = r.total_cost_usd.unwrap_or(0.0);
        let label = if r.is_error {
            "result (error)"
        } else {
            "result"
        };
        out.push(FfonElement::new_str(format!(
            "{label}: {} — {} turns, {:.1}s, ${:.4}",
            escape_markup(&r.subtype),
            turns,
            secs,
            cost
        )));
    }

    // --- in-flight indicator --------------------------------------------
    // Redundant once the streaming preview is on screen.
    if convo.busy && !convo.partial.has_content() {
        out.push(FfonElement::new_str("claude is working…"));
    }

    // --- live input slot -------------------------------------------------
    // A `-i` Str: an `<input>` with nothing under it, so it reads as the leaf
    // it is. Recall history as `<button>` children is still to come; when it
    // lands this becomes a `+i` Obj on the turns that have any.
    //
    // The prompt prefix ends in `: ` so the typed text reads as following a
    // label, the way the terminal's `user@host:~$ ` prompt does, rather than
    // running straight on from the last word.
    out.push(FfonElement::new_str(format!(
        "send to claude: <input>{pending_input}</input>"
    )));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{ContentField, parse_lines};

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
            Turn::ToolResult {
                tool_use_id,
                tool_name,
                summary,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tu_7");
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
        c.turns.insert(
            0,
            Turn::User {
                text: "hello".to_owned(),
            },
        );
        let out = build(&c, "draft");

        // header
        assert!(out[0].as_obj().unwrap().key.starts_with("session: opus"));
        // user turn
        assert_eq!(out[1].as_str(), Some("you: hello"));
        // Assistant prose: the whole message in the one element, its own
        // line breaks intact, nothing pushed down a level.
        assert_eq!(out[2].as_str(), Some("claude: line one\nline two"));
        // result footer
        assert!(out[3].as_str().unwrap().starts_with("result: success"));
        // trailing `-i` live input slot — an <input> with no <radio> wrapper
        let slot = out.last().unwrap().as_str().unwrap();
        assert!(slot.contains("<input>draft</input>"));
        assert!(!slot.contains("<radio>"));
    }

    /// The slot has nothing under it, so it is a `Str` and reads `-i`.
    #[test]
    fn the_input_slot_is_a_leaf() {
        let out = build(&Conversation::default(), "draft");
        assert_eq!(
            out.last().unwrap().as_str(),
            Some("send to claude: <input>draft</input>")
        );
    }

    #[test]
    fn build_shows_working_line_while_busy() {
        let mut c = Conversation::default();
        c.push_user("q");
        let out = build(&c, "");
        assert!(out.iter().any(|e| e.as_str() == Some("claude is working…")));
    }

    /// A turn that said nothing and ran one tool collapses to a single label,
    /// and stays a `Str` while no output has come back.
    #[test]
    fn a_lone_tool_call_merges_into_the_claude_label() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        ]);
        let out = build(&c, "");
        assert_eq!(
            out[0].as_str(),
            Some(r#"claude: Bash: {"command":"echo hi"}"#)
        );
    }

    #[test]
    fn a_merged_lone_tool_carries_its_output_as_children() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{"command":"echo hi"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_x","content":"hi\nbye","is_error":false}]}}"#,
        ]);
        let out = build(&c, "");
        let merged = out[0].as_obj().unwrap();
        assert_eq!(merged.key, r#"claude: Bash: {"command":"echo hi"}"#);
        assert_eq!(merged.children[0].as_str(), Some("hi"));
        assert_eq!(merged.children[1].as_str(), Some("bye"));
        assert_eq!(merged.children.len(), 2);
    }

    /// Two tools have nothing to merge into, so the container stays.
    #[test]
    fn several_tools_stay_under_the_claude_container() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_a","name":"Bash","input":{"command":"a"}},{"type":"tool_use","id":"tu_b","name":"Read","input":{"file_path":"x"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_a","content":"out a","is_error":false}]}}"#,
        ]);
        let out = build(&c, "");
        let claude = out[0].as_obj().unwrap();
        assert_eq!(claude.key, "claude:");
        // The answered call expands; the one still running is a leaf.
        let first = claude.children[0].as_obj().unwrap();
        assert_eq!(first.key, r#"Bash: {"command":"a"}"#);
        assert_eq!(first.children[0].as_str(), Some("out a"));
        assert_eq!(
            claude.children[1].as_str(),
            Some(r#"Read: {"file_path":"x"}"#)
        );
    }

    /// No node may be a childless `Obj`: the renderer prints `+` for every
    /// `Obj` regardless of child count, so one would announce as expandable
    /// with nothing inside.
    #[test]
    fn no_element_is_a_childless_obj() {
        let mut c = convo_from(&[
            r#"{"type":"system","subtype":"init","session_id":"s1","model":"opus","tools":["Read"]}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"just talking"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"two\nlines"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{}}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"and now two"},{"type":"tool_use","id":"tu_a","name":"Bash","input":{}},{"type":"tool_use","id":"tu_b","name":"Read","input":{}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_a","content":"","is_error":false}]}}"#,
            r#"{"type":"result","subtype":"success"}"#,
        ]);
        c.turns.insert(
            0,
            Turn::User {
                text: "hi".to_owned(),
            },
        );

        fn check(elems: &[FfonElement]) {
            for e in elems {
                if let Some(o) = e.as_obj() {
                    assert!(!o.children.is_empty(), "childless Obj: {:?}", o.key);
                    check(&o.children);
                }
            }
        }
        check(&build(&c, ""));
        check(&build(&c, "draft"));
    }

    /// A single-line message has nothing below it, so it stays a leaf.
    #[test]
    fn a_one_line_message_is_a_leaf() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"just one line"}]}}"#,
        ]);
        let out = build(&c, "");
        assert_eq!(out[0].as_str(), Some("claude: just one line"));
    }

    /// Blank lines are layout, not content: they would render as empty rows to
    /// arrow past, so they are dropped outside code blocks.
    #[test]
    fn blank_lines_between_paragraphs_are_dropped() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first\n\n\nsecond\n   \nthird"}]}}"#,
        ]);
        let out = build(&c, "");
        assert_eq!(out[0].as_str(), Some("claude: first\nsecond\nthird"));
    }

    /// Inside a fenced block the whitespace is the content, so indentation,
    /// blank lines and the fences themselves survive untouched.
    #[test]
    fn fenced_code_blocks_pass_through_verbatim() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"like so:\n```rust\nfn main() {\n\n    let x = 1;\n}\n```\ndone"}]}}"#,
        ]);
        let out = build(&c, "");
        assert_eq!(
            out[0].as_str(),
            Some("claude: like so:\n```rust\nfn main() {\n\n    let x = 1;\n}\n```\ndone")
        );
    }

    /// Several text blocks in one message run on as one sequence of lines.
    #[test]
    fn several_text_blocks_join_into_one_message() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"one"},{"type":"text","text":"two"}]}}"#,
        ]);
        let out = build(&c, "");
        assert_eq!(out[0].as_str(), Some("claude: one\ntwo"));
    }

    /// An answer that merely mentions a tag must read as prose. Unescaped it
    /// renders as an editable `-i` field and the tag itself is swallowed from
    /// the display, so the sentence comes out with a hole in it.
    #[test]
    fn an_answer_mentioning_a_tag_is_not_read_as_markup() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Use <input>hello</input> for that."}]}}"#,
        ]);
        let out = build(&c, "");
        let line = out[0].as_str().unwrap();
        assert!(
            !sicompass_sdk::tags::has_input(line),
            "must not read as an input field: {line:?}"
        );
        assert_eq!(
            sicompass_sdk::tags::strip_display(line),
            "claude: Use <input>hello</input> for that.",
            "and it must display exactly what was said"
        );
    }

    /// Tool input and output are shell and model output too.
    #[test]
    fn tool_input_and_output_are_escaped() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{"command":"echo <input>"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_x","content":"<button>x</button>","is_error":false}]}}"#,
        ]);
        let out = build(&c, "");
        let merged = out[0].as_obj().unwrap();
        assert!(!sicompass_sdk::tags::has_input(&merged.key));
        assert_eq!(
            sicompass_sdk::tags::strip_display(&merged.key),
            r#"claude: Bash: {"command":"echo <input>"}"#
        );
        let body = merged.children[0].as_str().unwrap();
        assert!(!sicompass_sdk::tags::has_button(body));
        assert_eq!(
            sicompass_sdk::tags::strip_display(body),
            "<button>x</button>"
        );
    }

    /// The prompt echo is the user's own text, and just as unconstrained.
    #[test]
    fn the_prompt_echo_is_escaped() {
        let mut c = Conversation::default();
        c.push_user("what does <input> do?");
        let out = build(&c, "");
        let line = out[0].as_str().unwrap();
        assert!(!sicompass_sdk::tags::has_input(line));
        assert_eq!(
            sicompass_sdk::tags::strip_display(line),
            "you: what does <input> do?"
        );
    }

    /// The live input slot keeps its `<input>`: that one is real markup the app
    /// parses back out to read what was typed.
    #[test]
    fn the_input_slot_keeps_its_real_markup() {
        let out = build(&Conversation::default(), "draft");
        let slot = out.last().unwrap().as_str().unwrap();
        assert!(sicompass_sdk::tags::has_input(slot));
        assert_eq!(
            sicompass_sdk::tags::extract_input(slot).as_deref(),
            Some("draft")
        );
    }

    /// A header with no cwd and no session id has nothing to expand into.
    #[test]
    fn a_bare_session_header_is_a_leaf() {
        let c = convo_from(&[
            r#"{"type":"system","subtype":"init","session_id":"","model":"opus","tools":["Read"]}"#,
        ]);
        let out = build(&c, "");
        assert_eq!(out[0].as_str(), Some("session: opus  (default, 1 tools)"));
    }

    #[test]
    fn tool_result_nests_under_its_tool_use() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running it"},{"type":"tool_use","id":"tu_x","name":"Bash","input":{"command":"echo hi"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_x","content":"hi\nbye","is_error":false}]}}"#,
        ]);
        let out = build(&c, "");
        // The result is no longer a top-level sibling.
        assert!(
            !out.iter().any(|e| e
                .as_obj()
                .is_some_and(|o| o.key.starts_with("tool result:"))),
            "the result should live under its call, not beside it"
        );
        let claude = out[0].as_obj().unwrap();
        assert_eq!(claude.key, "claude: running it");
        assert_eq!(claude.children.len(), 1, "only the tool hangs below");
        let tool = claude.children[0].as_obj().unwrap();
        assert_eq!(tool.key, r#"Bash: {"command":"echo hi"}"#);
        assert_eq!(tool.children[0].as_str(), Some("hi"));
        assert_eq!(tool.children[1].as_str(), Some("bye"));
        assert_eq!(tool.children.len(), 2);
    }

    #[test]
    fn nested_tool_result_lines_are_capped() {
        let big: String = (0..100).map(|i| format!("row {i}\n")).collect();
        let mut c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{}}]}}"#,
        ]);
        c.turns.push(Turn::ToolResult {
            tool_use_id: "tu_x".to_owned(),
            tool_name: "Bash".to_owned(),
            summary: big,
            is_error: false,
        });
        let out = build(&c, "");
        // A lone tool merges into the claude label; the body hangs off it.
        let tool = out[0].as_obj().unwrap();
        assert_eq!(tool.children.len(), TOOL_RESULT_LINE_CAP + 1);
        assert!(
            tool.children
                .last()
                .unwrap()
                .as_str()
                .unwrap()
                .contains("more lines")
        );
    }

    #[test]
    fn errored_tool_result_marks_the_tool_label() {
        let c = convo_from(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu_x","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_x","content":"boom","is_error":true}]}}"#,
        ]);
        let out = build(&c, "");
        let tool = out[0].as_obj().unwrap();
        assert!(
            tool.key.ends_with("[error]"),
            "failure belongs on the label, got {:?}",
            tool.key
        );
        assert_eq!(tool.children[0].as_str(), Some("boom"));
    }

    /// A result whose `tool_use` is not in the log (a resumed session, say) has
    /// nowhere to nest, so it keeps the old top-level shape.
    #[test]
    fn orphan_tool_result_stays_top_level_and_is_capped() {
        let big: String = (0..100).map(|i| format!("row {i}\n")).collect();
        let mut c = Conversation::default();
        c.turns.push(Turn::ToolResult {
            tool_use_id: "tu_gone".to_owned(),
            tool_name: "Bash".to_owned(),
            summary: big,
            is_error: false,
        });
        let out = build(&c, "");
        let res = out[0].as_obj().unwrap();
        assert_eq!(res.key, "tool result: Bash");
        assert_eq!(res.children.len(), TOOL_RESULT_LINE_CAP + 1);
        assert!(
            res.children
                .last()
                .unwrap()
                .as_str()
                .unwrap()
                .contains("more lines")
        );
    }

    #[test]
    fn stringify_content_handles_string_array_and_value() {
        assert_eq!(stringify_content(&Value::String("hi".into())), "hi");
        let arr: Value =
            serde_json::from_str(r#"[{"type":"text","text":"a"},{"type":"text","text":"b"}]"#)
                .unwrap();
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
        let out = build(&c, "");
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
        let out = build(&c, "");
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
