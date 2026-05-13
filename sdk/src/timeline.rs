use crate::ffon::{FfonElement, IdArray};
use std::path::PathBuf;

/// Unified undo/redo record. One variant per kind of reversible action.
///
/// Replaces the old `UndoEntry` (app-side, tagged-Task) plus
/// `ProviderUndoDescriptor` (opaque provider payload) channels.
#[derive(Debug, Clone, PartialEq)]
pub enum TimelineEntry {
    Navigate {
        provider_idx: usize,
        from_id: IdArray,
        to_id: IdArray,
        from_path: Option<String>,
        to_path: Option<String>,
        kind: NavKind,
    },
    TextChunk {
        id: IdArray,
        before: FfonElement,
        after: FfonElement,
        chunk_seq: u32,
    },
    Structural {
        id: IdArray,
        op: StructuralOp,
        payload: StructuralPayload,
    },
    FsOp {
        provider_idx: usize,
        id: IdArray,
        op: FsOpKind,
        before: Option<FfonElement>,
        after: Option<FfonElement>,
        side_effect: FsSideEffect,
    },
    ImapOp {
        provider_idx: usize,
        id: IdArray,
        op: ImapOpKind,
    },
    ChatOp {
        provider_idx: usize,
        id: IdArray,
        op: ChatOpKind,
    },
    /// Catch-all for simple in-process toggles (settings radio/checkbox,
    /// sales-demo script provider, future providers). Ops with non-trivial
    /// side effects use a typed variant instead.
    ProviderOp {
        provider_idx: usize,
        command: String,
        payload: FfonElement,
        label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralOp {
    Append,
    Insert,
    Delete,
    Cut,
    Paste,
    /// Whole-element replacement driven by a non-paste UI action (e.g. radio
    /// toggle rewriting a group's children slice). Apply paths replace the
    /// element at `id` with `before` / `after`.
    Replace,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructuralPayload {
    Inserted(FfonElement),
    Removed(FfonElement),
    Pasted { before: FfonElement, after: FfonElement },
    Replaced { before: FfonElement, after: FfonElement },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsOpKind {
    Create,
    Rename,
    Delete,
    Move,
    Paste,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsSideEffect {
    None,
    TrashedFile {
        original_path: PathBuf,
        content_snapshot: Vec<u8>,
    },
    TrashedDir {
        original_path: PathBuf,
        content_tree: TrashedTree,
    },
    RenameOnly {
        from: PathBuf,
        to: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashedTree {
    File(Vec<u8>),
    Dir(Vec<(String, TrashedTree)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImapOpKind {
    /// Move-to-trash. `msg_id` (RFC 5322 Message-ID) survives the move; the
    /// folder-local UID does not.
    Trash {
        msg_id: String,
        src_folder: String,
        trash_folder: String,
    },
    Archive {
        msg_id: String,
        src_folder: String,
        archive_folder: String,
    },
    Move {
        msg_id: String,
        src_folder: String,
        dst_folder: String,
    },
    /// Flag toggles stay in the same folder, so the UID is stable.
    SetSeen {
        msg_uid: u32,
        folder: String,
        prev: bool,
        new: bool,
    },
    SetFlagged {
        msg_uid: u32,
        folder: String,
        prev: bool,
        new: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatOpKind {
    LeaveRoom {
        room_id: String,
    },
    AcceptInvite {
        room_id: String,
    },
    RejectInvite {
        room_id: String,
    },
    KickMember {
        room_id: String,
        user_id: String,
        reason: Option<String>,
    },
    BanMember {
        room_id: String,
        user_id: String,
        reason: Option<String>,
    },
    PostMessage {
        room_id: String,
        event_id: String,
        body: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(parts: &[usize]) -> IdArray {
        let mut a = IdArray::new();
        for p in parts {
            a.push(*p);
        }
        a
    }

    #[test]
    fn navigate_roundtrip() {
        let entry = TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0, 1]),
            to_id: id(&[0, 2]),
            from_path: Some("/a".into()),
            to_path: Some("/a/b".into()),
            kind: NavKind::ArrowRight,
        };
        let clone = entry.clone();
        assert_eq!(entry, clone);
        match clone {
            TimelineEntry::Navigate { kind, from_path, .. } => {
                assert_eq!(kind, NavKind::ArrowRight);
                assert_eq!(from_path.as_deref(), Some("/a"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn text_chunk_holds_before_and_after() {
        let entry = TimelineEntry::TextChunk {
            id: id(&[0, 0]),
            before: FfonElement::Str("- old".into()),
            after: FfonElement::Str("- new".into()),
            chunk_seq: 7,
        };
        match entry {
            TimelineEntry::TextChunk { before, after, chunk_seq, .. } => {
                assert_eq!(before, FfonElement::Str("- old".into()));
                assert_eq!(after, FfonElement::Str("- new".into()));
                assert_eq!(chunk_seq, 7);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn structural_payload_variants_distinguish() {
        let ins = StructuralPayload::Inserted(FfonElement::Str("x".into()));
        let rem = StructuralPayload::Removed(FfonElement::Str("x".into()));
        assert_ne!(ins, rem);
    }

    #[test]
    fn fs_side_effect_carries_snapshot() {
        let se = FsSideEffect::TrashedFile {
            original_path: PathBuf::from("/tmp/a.txt"),
            content_snapshot: b"hello".to_vec(),
        };
        match se {
            FsSideEffect::TrashedFile { content_snapshot, .. } => {
                assert_eq!(content_snapshot, b"hello");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn trashed_tree_recursive() {
        let tree = TrashedTree::Dir(vec![
            ("a.txt".into(), TrashedTree::File(b"a".to_vec())),
            (
                "sub".into(),
                TrashedTree::Dir(vec![(
                    "b.txt".into(),
                    TrashedTree::File(b"b".to_vec()),
                )]),
            ),
        ]);
        match tree {
            TrashedTree::Dir(ref children) => assert_eq!(children.len(), 2),
            _ => panic!("expected Dir at root"),
        }
    }

    #[test]
    fn imap_op_kind_typed_fields() {
        let op = ImapOpKind::Move {
            msg_id: "<abc@host>".into(),
            src_folder: "INBOX".into(),
            dst_folder: "Archive".into(),
        };
        match op {
            ImapOpKind::Move { msg_id, src_folder, dst_folder } => {
                assert_eq!(msg_id, "<abc@host>");
                assert_eq!(src_folder, "INBOX");
                assert_eq!(dst_folder, "Archive");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn chat_op_kind_post_message_carries_event_id() {
        let op = ChatOpKind::PostMessage {
            room_id: "!room:example.org".into(),
            event_id: "$evt".into(),
            body: "hi".into(),
        };
        match op {
            ChatOpKind::PostMessage { event_id, body, .. } => {
                assert_eq!(event_id, "$evt");
                assert_eq!(body, "hi");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn provider_op_keeps_legacy_shape() {
        let entry = TimelineEntry::ProviderOp {
            provider_idx: 1,
            command: "radio-toggle".into(),
            payload: FfonElement::Str("payload".into()),
            label: "toggle theme".into(),
        };
        let cloned = entry.clone();
        assert_eq!(entry, cloned);
    }
}
