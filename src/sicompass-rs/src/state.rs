//! FFON mutation — mirrors `update.c` (`updateState`, `updateIds`, `updateFfon`,
//! `updateHistory`, `handleHistoryAction`).

use crate::app_state::{AppRenderer, Coordinate, History, Task, UndoEntry};
use crate::list;
use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray, next_layer_exists};

const UNDO_HISTORY_SIZE: usize = 100;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Commit a task into the FFON tree, record undo history, rebuild the list.
pub fn update_state(r: &mut AppRenderer, task: Task, history: History) {
    // Capture position before modification (needed for undo of DELETE)
    let history_id = r.current_id.clone();

    // Capture element BEFORE modification (for undo of DELETE / INPUT / PASTE)
    let prev_element = if history == History::None
        && matches!(task, Task::Delete | Task::Input | Task::Paste)
    {
        get_element_at(&r.ffon, &r.current_id).cloned()
    } else {
        None
    };

    // Determine "line" — what we're editing/inserting
    let (line, current_elem_is_obj) = get_current_line(r);
    let is_key = is_line_key(&line) || current_elem_is_obj;

    update_ids(r, is_key, task, history);
    update_ffon(r, &line, is_key, task, history);

    // Capture element AFTER modification (for undo of APPEND / INSERT / INPUT / PASTE)
    let new_element = if history == History::None
        && matches!(
            task,
            Task::Append
                | Task::AppendAppend
                | Task::Insert
                | Task::InsertInsert
                | Task::Input
                | Task::Paste
        ) {
        get_element_at(&r.ffon, &r.current_id).cloned()
    } else {
        None
    };

    let record_id = if matches!(task, Task::Delete | Task::Cut | Task::Paste) {
        history_id
    } else {
        r.current_id.clone()
    };
    update_history(r, task, &record_id, prev_element, new_element, history);

    list::create_list_current_layer(r);
}

// ---------------------------------------------------------------------------
// updateIds
// ---------------------------------------------------------------------------

pub fn update_ids(r: &mut AppRenderer, is_key: bool, task: Task, history: History) {
    r.previous_id = r.current_id.clone();

    if matches!(history, History::Undo | History::Redo) {
        return;
    }

    let max_id = get_ffon_max_id_at_path(&r.ffon, &r.current_id);
    let current_idx = r.current_id.last().unwrap_or(0);
    let depth = r.current_id.depth();

    match task {
        Task::ArrowUp => {
            if current_idx > 0 {
                r.current_id.set_last(current_idx - 1);
            }
        }
        Task::ArrowDown => {
            if current_idx < max_id {
                r.current_id.set_last(current_idx + 1);
            }
        }
        Task::ArrowLeft => {
            if depth > 1 {
                r.current_id.pop();
            }
        }
        Task::ArrowRight => {
            if next_layer_exists(&r.ffon, &r.previous_id) {
                r.current_id.push(0);
            }
        }
        Task::Append => {
            let in_editor = is_editor_general_or_operator_general(r.coordinate);
            if in_editor {
                if !is_key {
                    r.current_id.set_last(current_idx + 1);
                } else if next_layer_exists(&r.ffon, &r.previous_id) {
                    r.current_id.set_last(current_idx + 1);
                } else {
                    r.current_id.push(0);
                }
            }
        }
        Task::AppendAppend => {
            if is_editor_general_or_operator_general(r.coordinate) {
                r.current_id.set_last(max_id + 1);
            }
        }
        Task::Insert => {
            // Position stays the same
        }
        Task::InsertInsert => {
            if is_editor_general_or_operator_general(r.coordinate) {
                r.current_id.set_last(0);
            }
        }
        Task::Delete => {
            // Position handled in update_ffon
        }
        Task::Input => {
            // Position stays the same
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// updateFfon
// ---------------------------------------------------------------------------

pub fn update_ffon(r: &mut AppRenderer, line: &str, is_key: bool, task: Task, history: History) {
    if r.previous_id.depth() == 0 {
        return;
    }

    // Handle empty root structure
    if r.ffon.is_empty() && r.previous_id.depth() == 1 {
        if matches!(
            task,
            Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert | Task::Input
        ) {
            let new_elem = if is_key {
                let mut obj = FfonElement::new_obj(strip_trailing_colon(line));
                obj.as_obj_mut().unwrap().push(FfonElement::new_str(""));
                obj
            } else {
                FfonElement::new_str(line)
            };
            r.ffon.push(new_elem);
        }
        return;
    }

    // Paste: replace current element with clipboard content
    if task == Task::Paste {
        let prev_id = r.previous_id.clone();
        let prev_idx = match prev_id.last() { Some(i) => i, None => return };
        if let Some(elem) = r.clipboard.clone() {
            replace_at(&mut r.ffon, &prev_id, prev_idx, elem);
        }
        return;
    }

    let is_editor = is_editor_coordinate(r.coordinate);

    // Clone the ids so we can still mutate r.current_id
    let prev_id = r.previous_id.clone();
    let cur_id = r.current_id.clone();

    // Get the target index (last component of prev_id)
    let prev_idx = match prev_id.last() {
        Some(i) => i,
        None => return,
    };
    let cur_idx_last = cur_id.last().unwrap_or(0);

    // Navigate to the parent array (the array containing the element at prev_idx)
    // We need to get a mutable reference. We do this in a separate scope to avoid
    // fighting with the borrow checker on `r`.
    // Strategy: get depth, clone the path, then do the mutation on r.ffon.

    let depth = prev_id.depth();

    if is_key && is_editor {
        // Check if element at prev_idx is an Obj
        let is_obj_at_prev = {
            let arr = navigate_to_slice(&r.ffon, &prev_id);
            arr.and_then(|a| a.get(prev_idx)).map_or(false, |e| e.is_obj())
        };

        if matches!(task, Task::Delete | Task::Cut) {
            // Remove element at prev_idx
            let removed = remove_at(&mut r.ffon, &prev_id, prev_idx);
            let new_len = get_parent_len(&r.ffon, &prev_id);
            if depth != 1 && new_len == 0 {
                // Insert empty string if this was the only child
                insert_at(&mut r.ffon, &prev_id, 0, FfonElement::new_str(""));
            } else if new_len > 0 {
                // Move cursor to previous element
                if prev_idx > 0 {
                    r.current_id.set_last(prev_idx - 1);
                }
            }
        } else if is_obj_at_prev {
            // Element is already an Obj — re-key it
            let new_key = strip_trailing_colon(line);
            rekey_obj_at(&mut r.ffon, &prev_id, prev_idx, new_key);

            if matches!(task, Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert)
                && history != History::Redo
            {
                // Insert a new empty sibling at cur_idx_last
                insert_at(&mut r.ffon, &prev_id, cur_idx_last, FfonElement::new_str(""));
            }
        } else {
            // Element is a string — convert it to an Obj
            let new_key = strip_trailing_colon(line);
            let has_children = next_layer_exists(&r.ffon, &prev_id);
            let new_elem = if has_children {
                // Preserve existing children
                let children = {
                    let arr = navigate_to_slice(&r.ffon, &prev_id);
                    arr.and_then(|a| a.get(prev_idx))
                        .and_then(|e| e.as_obj())
                        .map(|o| o.children.clone())
                        .unwrap_or_default()
                };
                let mut obj = FfonElement::new_obj(new_key);
                for c in children {
                    obj.as_obj_mut().unwrap().push(c);
                }
                obj
            } else {
                let mut obj = FfonElement::new_obj(new_key);
                obj.as_obj_mut().unwrap().push(FfonElement::new_str(""));
                obj
            };
            replace_at(&mut r.ffon, &prev_id, prev_idx, new_elem);

            if matches!(task, Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert)
                && history != History::Redo
            {
                insert_at(&mut r.ffon, &prev_id, cur_idx_last, FfonElement::new_str(""));
            }
        }
    } else if !is_key && is_editor {
        if matches!(task, Task::Delete | Task::Cut) {
            remove_at(&mut r.ffon, &prev_id, prev_idx);
            let new_len = get_parent_len(&r.ffon, &prev_id);
            // If cursor is at 0 and list is now empty at non-root level, insert placeholder
            if cur_id.last().unwrap_or(1) == 0 && new_len == 0 && depth != 1 {
                insert_at(&mut r.ffon, &prev_id, prev_id.last().unwrap_or(0), FfonElement::new_str(""));
            }
            if r.current_id.last().unwrap_or(0) > 0 {
                let cur = r.current_id.last().unwrap_or(1);
                r.current_id.set_last(cur - 1);
            }
        } else if matches!(
            task,
            Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert
        ) {
            // Set the current element to `line`
            replace_at(&mut r.ffon, &prev_id, prev_idx, FfonElement::new_str(line));
            // Insert empty string sibling at cur_idx_last
            if history != History::Redo {
                insert_at(&mut r.ffon, &prev_id, cur_idx_last, FfonElement::new_str(""));
            }
        } else {
            // TASK_INPUT / navigation — update the value
            replace_at(&mut r.ffon, &prev_id, prev_idx, FfonElement::new_str(line));
        }
    } else if matches!(task, Task::Delete | Task::Cut) {
        // Non-editor delete/cut (e.g. file browser in OperatorGeneral)
        remove_at(&mut r.ffon, &prev_id, prev_idx);
        let new_len = get_parent_len(&r.ffon, &prev_id);
        if new_len == 0 {
            // Insert placeholder
            let placeholder = FfonElement::new_str("<input></input>");
            insert_at(&mut r.ffon, &prev_id, 0, placeholder);
            r.current_id.set_last(0);
        } else if r.current_id.last().unwrap_or(0) > 0 {
            let cur = r.current_id.last().unwrap_or(1);
            r.current_id.set_last(cur - 1);
        }
    }
}

// ---------------------------------------------------------------------------
// updateHistory
// ---------------------------------------------------------------------------

pub fn update_history(
    r: &mut AppRenderer,
    task: Task,
    id: &IdArray,
    prev_element: Option<FfonElement>,
    new_element: Option<FfonElement>,
    history: History,
) {
    if history != History::None {
        return;
    }

    if !matches!(
        task,
        Task::Append
            | Task::AppendAppend
            | Task::Insert
            | Task::InsertInsert
            | Task::Delete
            | Task::Input
            | Task::Cut
            | Task::Paste
            | Task::FsCreate
    ) {
        return;
    }

    // Truncate redo entries beyond current position
    if r.undo_position > 0 {
        let new_count = r.undo_history.len().saturating_sub(r.undo_position);
        r.undo_history.truncate(new_count);
    }

    // Remove oldest entry if at capacity
    if r.undo_history.len() >= UNDO_HISTORY_SIZE {
        r.undo_history.remove(0);
    }

    r.undo_history.push(UndoEntry {
        id: id.clone(),
        task,
        prev_element,
        new_element,
    });
    r.undo_position = 0;
}

// ---------------------------------------------------------------------------
// handleHistoryAction (undo/redo)
// ---------------------------------------------------------------------------

pub fn handle_history_action(r: &mut AppRenderer, history: History) {
    if r.undo_history.is_empty() {
        r.error_message = "No undo history".to_owned();
        return;
    }

    crate::handlers::handle_escape(r);

    if history == History::Undo {
        if r.undo_position >= r.undo_history.len() {
            r.error_message = "Nothing to undo".to_owned();
            return;
        }

        r.undo_position += 1;
        let entry_idx = r.undo_history.len() - r.undo_position;
        let entry_id = r.undo_history[entry_idx].id.clone();
        let entry_task = r.undo_history[entry_idx].task;
        let entry_prev = r.undo_history[entry_idx].prev_element.clone();

        match entry_task {
            Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert => {
                r.current_id = entry_id;
                crate::handlers::handle_delete(r, History::Undo);
                if r.current_id.last().unwrap_or(0) > 0 {
                    let cur = r.current_id.last().unwrap_or(1);
                    r.current_id.set_last(cur - 1);
                }
            }
            Task::Delete | Task::Cut => {
                if let Some(elem) = entry_prev {
                    insert_element_at_id(r, &entry_id, elem);
                    r.current_id = entry_id;
                }
            }
            Task::Input | Task::Paste => {
                if let Some(elem) = entry_prev {
                    replace_element_at_id(r, &entry_id, elem);
                    r.current_id = entry_id;
                }
            }
            Task::FsCreate => {
                let undo_elem = r.undo_history[entry_idx].new_element.clone();
                if let Some(elem) = undo_elem {
                    let name = match &elem {
                        sicompass_sdk::ffon::FfonElement::Str(s) => s.clone(),
                        sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.clone(),
                    };
                    r.current_id = entry_id.clone();
                    crate::provider::delete_item_by_name(r, &name);
                    crate::provider::refresh_current_directory(r);
                    // Clamp cursor if out of bounds
                    let parent_len = get_parent_len(&r.ffon, &entry_id);
                    if let Some(idx) = r.current_id.last() {
                        if idx >= parent_len && parent_len > 0 {
                            r.current_id.set_last(parent_len - 1);
                        }
                    }
                }
            }
            _ => {}
        }
    } else if history == History::Redo {
        if r.undo_position == 0 {
            r.error_message = "Nothing to redo".to_owned();
            return;
        }

        let entry_idx = r.undo_history.len() - r.undo_position;
        r.undo_position -= 1;
        let entry_id = r.undo_history[entry_idx].id.clone();
        let entry_task = r.undo_history[entry_idx].task;
        let entry_new = r.undo_history[entry_idx].new_element.clone();

        match entry_task {
            Task::Append | Task::AppendAppend | Task::Insert | Task::InsertInsert => {
                if let Some(elem) = entry_new {
                    insert_element_at_id(r, &entry_id, elem);
                    r.current_id = entry_id;
                }
            }
            Task::Delete | Task::Cut => {
                r.current_id = entry_id.clone();
                crate::handlers::handle_delete(r, History::Redo);
                let count = get_parent_len(&r.ffon, &entry_id);
                if let Some(idx) = r.current_id.last() {
                    if idx >= count && count > 0 {
                        r.current_id.set_last(count - 1);
                    }
                }
            }
            Task::Input | Task::Paste => {
                if let Some(elem) = entry_new {
                    replace_element_at_id(r, &entry_id, elem);
                    r.current_id = entry_id;
                }
            }
            Task::FsCreate => {
                if let Some(elem) = entry_new {
                    let name = match &elem {
                        sicompass_sdk::ffon::FfonElement::Str(s) => s.clone(),
                        sicompass_sdk::ffon::FfonElement::Obj(o) => o.key.clone(),
                    };
                    let is_dir = matches!(elem, sicompass_sdk::ffon::FfonElement::Obj(_));
                    r.current_id = entry_id.clone();
                    if is_dir {
                        crate::provider::create_directory(r, &name);
                    } else {
                        crate::provider::create_file(r, &name);
                    }
                    crate::provider::refresh_current_directory(r);
                    r.current_id = entry_id;
                }
            }
            _ => {}
        }
    }

    list::create_list_current_layer(r);
}

// ---------------------------------------------------------------------------
// Helpers — FFON mutation primitives
// ---------------------------------------------------------------------------

/// True if the last character of `line` is `':'`.
pub fn is_line_key(line: &str) -> bool {
    line.ends_with(':')
}

/// Strip a trailing `':'` from a string (for object key naming).
pub fn strip_trailing_colon(s: &str) -> String {
    if s.ends_with(':') {
        s[..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}

fn is_editor_coordinate(coord: Coordinate) -> bool {
    matches!(
        coord,
        Coordinate::EditorGeneral
            | Coordinate::EditorInsert
            | Coordinate::EditorNormal
            | Coordinate::EditorVisual
    )
}

fn is_editor_general_or_operator_general(coord: Coordinate) -> bool {
    matches!(
        coord,
        Coordinate::EditorGeneral | Coordinate::OperatorGeneral
    )
}

/// Get the element at `id` (immutable).
fn get_element_at<'a>(ffon: &'a [FfonElement], id: &IdArray) -> Option<&'a FfonElement> {
    let arr = navigate_to_slice(ffon, id)?;
    let idx = id.last()?;
    arr.get(idx)
}

/// Navigate to the parent slice (the array containing the element pointed to by `id`).
/// Returns `None` if navigation fails.
fn navigate_to_slice<'a>(ffon: &'a [FfonElement], id: &IdArray) -> Option<&'a [FfonElement]> {
    if id.depth() == 0 {
        return None;
    }
    let mut current = ffon;
    for d in 0..id.depth() - 1 {
        let idx = id.get(d)?;
        match current.get(idx)? {
            FfonElement::Obj(obj) => current = &obj.children,
            _ => return None,
        }
    }
    Some(current)
}

/// Navigate to the parent Vec (mutable) — public for use by handlers.
pub fn navigate_to_slice_pub<'a>(
    ffon: &'a mut Vec<FfonElement>,
    id: &IdArray,
) -> Option<&'a mut Vec<FfonElement>> {
    navigate_to_slice_mut(ffon, id)
}

/// Navigate to the parent Vec (mutable).
fn navigate_to_slice_mut<'a>(
    ffon: &'a mut Vec<FfonElement>,
    id: &IdArray,
) -> Option<&'a mut Vec<FfonElement>> {
    if id.depth() == 0 {
        return None;
    }
    let mut current = ffon;
    for d in 0..id.depth() - 1 {
        let idx = id.get(d)?;
        match current.get_mut(idx)? {
            FfonElement::Obj(obj) => current = &mut obj.children,
            _ => return None,
        }
    }
    Some(current)
}

/// Get the length of the parent array at `id`.
fn get_parent_len(ffon: &[FfonElement], id: &IdArray) -> usize {
    navigate_to_slice(ffon, id)
        .map(|s| s.len())
        .unwrap_or(0)
}

/// Maximum valid index at the current navigation path.
fn get_ffon_max_id_at_path(ffon: &[FfonElement], id: &IdArray) -> usize {
    navigate_to_slice(ffon, id)
        .map(|s| s.len().saturating_sub(1))
        .unwrap_or(0)
}

/// Replace element at `(parent_id, idx)` with `new_elem`.
fn replace_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize, new_elem: FfonElement) {
    if let Some(arr) = navigate_to_slice_mut(ffon, parent_id) {
        if idx < arr.len() {
            arr[idx] = new_elem;
        }
    }
}

/// Remove element at `(parent_id, idx)`.
fn remove_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize) -> Option<FfonElement> {
    let arr = navigate_to_slice_mut(ffon, parent_id)?;
    if idx < arr.len() {
        Some(arr.remove(idx))
    } else {
        None
    }
}

/// Insert element at `(parent_id, idx)`.
fn insert_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize, elem: FfonElement) {
    if let Some(arr) = navigate_to_slice_mut(ffon, parent_id) {
        let idx = idx.min(arr.len());
        arr.insert(idx, elem);
    }
}

/// Re-key an existing Obj at `(parent_id, idx)`.
fn rekey_obj_at(ffon: &mut Vec<FfonElement>, parent_id: &IdArray, idx: usize, new_key: String) {
    if let Some(arr) = navigate_to_slice_mut(ffon, parent_id) {
        if let Some(FfonElement::Obj(obj)) = arr.get_mut(idx) {
            obj.key = new_key;
        }
    }
}

/// Insert a cloned element at `id` (used by undo/redo).
fn insert_element_at_id(r: &mut AppRenderer, id: &IdArray, elem: FfonElement) {
    let insert_idx = id.last().unwrap_or(0);
    insert_at(&mut r.ffon, id, insert_idx, elem);
}

/// Replace element at `id` with a clone (used by undo/redo).
fn replace_element_at_id(r: &mut AppRenderer, id: &IdArray, elem: FfonElement) {
    let idx = id.last().unwrap_or(0);
    replace_at(&mut r.ffon, id, idx, elem);
}

// ---------------------------------------------------------------------------
// get_current_line — build the "line" for updateFfon
// ---------------------------------------------------------------------------

fn get_current_line(r: &AppRenderer) -> (String, bool) {
    if matches!(
        r.coordinate,
        Coordinate::EditorInsert | Coordinate::OperatorInsert
    ) {
        return (r.input_buffer.clone(), false);
    }

    let arr = match navigate_to_slice(&r.ffon, &r.current_id) {
        Some(a) => a,
        None => return (String::new(), false),
    };
    let idx = r.current_id.last().unwrap_or(0);
    match arr.get(idx) {
        Some(FfonElement::Str(s)) => (s.clone(), false),
        Some(FfonElement::Obj(obj)) => (obj.key.clone(), true),
        None => (String::new(), false),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{AppRenderer, Coordinate, History, Task};
    use sicompass_sdk::ffon::{FfonElement, IdArray};

    fn make_renderer(ffon: Vec<FfonElement>) -> AppRenderer {
        let mut r = AppRenderer::new();
        r.ffon = ffon;
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        r
    }

    // --- is_line_key ---

    #[test]
    fn is_line_key_with_colon() {
        assert!(is_line_key("Section:"));
    }

    #[test]
    fn is_line_key_without_colon() {
        assert!(!is_line_key("plain text"));
    }

    #[test]
    fn is_line_key_empty() {
        assert!(!is_line_key(""));
    }

    // --- strip_trailing_colon ---

    #[test]
    fn strip_colon_removes_trailing() {
        assert_eq!(strip_trailing_colon("Section:"), "Section");
    }

    #[test]
    fn strip_colon_no_change() {
        assert_eq!(strip_trailing_colon("plain"), "plain");
    }

    // --- update_ids ---

    #[test]
    fn update_ids_arrow_up_decrements() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("a"),
            FfonElement::new_str("b"),
        ]);
        r.current_id.set_last(1);
        update_ids(&mut r, false, Task::ArrowUp, History::None);
        assert_eq!(r.current_id.last(), Some(0));
    }

    #[test]
    fn update_ids_arrow_down_increments() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("a"),
            FfonElement::new_str("b"),
        ]);
        r.current_id.set_last(0);
        update_ids(&mut r, false, Task::ArrowDown, History::None);
        assert_eq!(r.current_id.last(), Some(1));
    }

    #[test]
    fn update_ids_arrow_down_clamps_at_max() {
        let mut r = make_renderer(vec![FfonElement::new_str("only")]);
        r.current_id.set_last(0);
        update_ids(&mut r, false, Task::ArrowDown, History::None);
        assert_eq!(r.current_id.last(), Some(0)); // no change, already at max
    }

    // --- update_ffon (basic) ---

    #[test]
    fn update_ffon_input_string_value() {
        let mut r = make_renderer(vec![FfonElement::new_str("old")]);
        r.coordinate = Coordinate::EditorInsert;
        r.input_buffer = "new value".to_owned();
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "new value", false, Task::Input, History::None);
        assert_eq!(r.ffon[0].as_str(), Some("new value"));
    }

    #[test]
    fn update_ffon_input_empty_root_creates_string() {
        let mut r = make_renderer(vec![]);
        r.coordinate = Coordinate::EditorInsert;
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "hello", false, Task::Input, History::None);
        assert_eq!(r.ffon.len(), 1);
        assert_eq!(r.ffon[0].as_str(), Some("hello"));
    }

    #[test]
    fn update_ffon_input_empty_root_creates_obj_for_key() {
        let mut r = make_renderer(vec![]);
        r.coordinate = Coordinate::EditorInsert;
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "Section:", true, Task::Input, History::None);
        assert_eq!(r.ffon.len(), 1);
        assert!(r.ffon[0].is_obj());
        assert_eq!(r.ffon[0].as_obj().unwrap().key, "Section");
    }

    #[test]
    fn update_ffon_delete_string_removes_element() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("keep"),
            FfonElement::new_str("remove"),
        ]);
        r.coordinate = Coordinate::EditorGeneral;
        r.current_id.set_last(1);
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "remove", false, Task::Delete, History::None);
        assert_eq!(r.ffon.len(), 1);
        assert_eq!(r.ffon[0].as_str(), Some("keep"));
    }

    // --- update_state integration ---

    #[test]
    fn update_state_input_commits_value() {
        let mut r = make_renderer(vec![FfonElement::new_str("old")]);
        r.coordinate = Coordinate::EditorInsert;
        r.input_buffer = "updated".to_owned();
        update_state(&mut r, Task::Input, History::None);
        assert_eq!(r.ffon[0].as_str(), Some("updated"));
    }

    #[test]
    fn update_state_records_undo_for_input() {
        let mut r = make_renderer(vec![FfonElement::new_str("before")]);
        r.coordinate = Coordinate::EditorInsert;
        r.input_buffer = "after".to_owned();
        update_state(&mut r, Task::Input, History::None);
        assert_eq!(r.undo_history.len(), 1);
        assert_eq!(r.undo_history[0].task, Task::Input);
    }

    // --- is_line_key (additional) ---

    #[test]
    fn is_line_key_just_colon() {
        assert!(is_line_key(":"));
    }

    #[test]
    fn is_line_key_colon_in_middle() {
        assert!(!is_line_key("key:value"));
    }

    #[test]
    fn is_line_key_multiple_colons() {
        assert!(is_line_key("a:b:c:"));
    }

    #[test]
    fn is_line_key_spaces_before_colon() {
        assert!(is_line_key("my key :"));
    }

    // --- strip_trailing_colon (additional) ---

    #[test]
    fn strip_colon_just_colon() {
        assert_eq!(strip_trailing_colon(":"), "");
    }

    #[test]
    fn strip_colon_empty_string() {
        assert_eq!(strip_trailing_colon(""), "");
    }

    // --- update_history ---

    #[test]
    fn update_history_skips_non_none_history() {
        let mut r = AppRenderer::new();
        let id = IdArray::new();
        update_history(&mut r, Task::Input, &id, None, None, History::Undo);
        assert_eq!(r.undo_history.len(), 0);
    }

    #[test]
    fn update_history_skips_navigation_tasks() {
        let mut r = AppRenderer::new();
        let id = IdArray::new();
        update_history(&mut r, Task::ArrowUp, &id, None, None, History::None);
        update_history(&mut r, Task::ArrowDown, &id, None, None, History::None);
        assert_eq!(r.undo_history.len(), 0);
    }

    #[test]
    fn update_history_adds_entry() {
        let mut r = AppRenderer::new();
        let mut id = IdArray::new();
        id.push(0);
        id.push(2);
        let prev = FfonElement::new_str("old");
        let new = FfonElement::new_str("new");
        update_history(&mut r, Task::Input, &id, Some(prev), Some(new), History::None);
        assert_eq!(r.undo_history.len(), 1);
        assert_eq!(r.undo_history[0].task, Task::Input);
    }

    #[test]
    fn update_history_multiple_entries() {
        let mut r = AppRenderer::new();
        let id = IdArray::new();
        for _ in 0..5 {
            update_history(&mut r, Task::Input, &id, None, None, History::None);
        }
        assert_eq!(r.undo_history.len(), 5);
    }

    #[test]
    fn update_history_null_elements_stored() {
        let mut r = AppRenderer::new();
        let id = IdArray::new();
        update_history(&mut r, Task::Delete, &id, None, None, History::None);
        assert_eq!(r.undo_history.len(), 1);
        assert!(r.undo_history[0].prev_element.is_none());
        assert!(r.undo_history[0].new_element.is_none());
    }

    // --- update_ids (additional boundary cases) ---

    #[test]
    fn update_ids_move_up_at_top_stays() {
        let mut r = make_renderer(vec![FfonElement::new_str("only")]);
        r.current_id.set_last(0);
        update_ids(&mut r, false, Task::ArrowUp, History::None);
        assert_eq!(r.current_id.last(), Some(0));
    }

    #[test]
    fn update_ids_move_down_at_bottom_stays() {
        let mut r = make_renderer(vec![
            FfonElement::new_str("a"),
            FfonElement::new_str("b"),
        ]);
        r.current_id.set_last(1);
        update_ids(&mut r, false, Task::ArrowDown, History::None);
        assert_eq!(r.current_id.last(), Some(1));
    }

    // --- update_ffon (object key modification) ---

    #[test]
    fn update_ffon_input_key_modifies_object_key() {
        let mut r = make_renderer(vec![{
            let mut root = FfonElement::new_obj("root:");
            let child = FfonElement::new_obj("old key:");
            root.as_obj_mut().unwrap().push(child);
            root
        }]);
        r.coordinate = Coordinate::EditorInsert;
        r.current_id = {
            let mut id = IdArray::new();
            id.push(0);
            id.push(0);
            id
        };
        r.previous_id = r.current_id.clone();
        update_ffon(&mut r, "new key:", true, Task::Input, History::None);
        // The child at [0][0] should now have key "new key"
        let root_obj = r.ffon[0].as_obj().unwrap();
        assert_eq!(root_obj.children[0].as_obj().unwrap().key, "new key");
    }
}
