//! Shared "insert here" placeholder used by any provider that wants a persistent
//! typing affordance at the start of a list or inside an Obj.
//!
//! Writer-side sentinel: the exact FFON Str payload is `"i <input></input>"`.
//! Render-side sentinel: `src/sicompass/src/list.rs` trims the inner content;
//! when it equals `"i"` the label becomes `"i"` instead of the default `"-i "`.
//! Both sides must stay in lock-step — centralize the literal here.

use crate::ffon::FfonElement;

/// The "insert here" placeholder used by the email client compose body and by
/// the file browser (for freshly created / empty directories).
pub const I_PLACEHOLDER: &str = "i <input></input>";

/// True when a FFON Str element's payload is the placeholder sentinel.
pub fn is_i_placeholder(s: &str) -> bool {
    s == I_PLACEHOLDER
}

/// Create a new Obj pre-seeded with [`I_PLACEHOLDER`] as its first child.
pub fn new_obj_with_i_placeholder(key: impl Into<String>) -> FfonElement {
    let mut obj = FfonElement::new_obj(key.into());
    obj.as_obj_mut()
        .unwrap()
        .push(FfonElement::new_str(I_PLACEHOLDER.to_owned()));
    obj
}

/// Recursively ensure every `Obj` in `elems` has [`I_PLACEHOLDER`] as its first
/// child so that the insert affordance is available at every nesting level.
/// Skips Objs that already start with the placeholder (idempotent).
pub fn seed_i_placeholders(elems: &mut Vec<FfonElement>) {
    for elem in elems.iter_mut() {
        if let FfonElement::Obj(o) = elem {
            let already_seeded = o.children.first()
                .map(|c| matches!(c, FfonElement::Str(s) if is_i_placeholder(s)))
                .unwrap_or(false);
            if !already_seeded {
                o.children.insert(0, FfonElement::new_str(I_PLACEHOLDER.to_owned()));
            }
            seed_i_placeholders(&mut o.children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffon::FfonElement;

    #[test]
    fn is_i_placeholder_recognises_sentinel() {
        assert!(is_i_placeholder(I_PLACEHOLDER));
    }

    #[test]
    fn is_i_placeholder_rejects_bare_input() {
        assert!(!is_i_placeholder("<input></input>"));
    }

    #[test]
    fn is_i_placeholder_rejects_empty() {
        assert!(!is_i_placeholder(""));
    }

    #[test]
    fn new_obj_with_i_placeholder_has_one_child() {
        let elem = new_obj_with_i_placeholder("key");
        let obj = elem.as_obj().unwrap();
        assert_eq!(obj.children.len(), 1);
        assert!(matches!(&obj.children[0], FfonElement::Str(s) if is_i_placeholder(s)));
    }

    #[test]
    fn new_obj_with_i_placeholder_preserves_key() {
        let elem = new_obj_with_i_placeholder("my key");
        assert_eq!(elem.as_obj().unwrap().key, "my key");
    }

    #[test]
    fn seed_i_placeholders_adds_to_unseeded_obj() {
        let mut elems = vec![{
            let mut obj = FfonElement::new_obj("k");
            obj.as_obj_mut().unwrap().push(FfonElement::new_str("child".to_owned()));
            obj
        }];
        seed_i_placeholders(&mut elems);
        let obj = elems[0].as_obj().unwrap();
        assert!(matches!(&obj.children[0], FfonElement::Str(s) if is_i_placeholder(s)));
    }

    #[test]
    fn seed_i_placeholders_is_idempotent() {
        let mut elems = vec![new_obj_with_i_placeholder("k")];
        seed_i_placeholders(&mut elems);
        let obj = elems[0].as_obj().unwrap();
        assert_eq!(obj.children.len(), 1, "should not double-add");
    }

    #[test]
    fn seed_i_placeholders_recurses_into_nested_objs() {
        let mut inner = FfonElement::new_obj("inner");
        inner.as_obj_mut().unwrap().push(FfonElement::new_str("x".to_owned()));
        let mut outer = FfonElement::new_obj("outer");
        outer.as_obj_mut().unwrap().push(inner);
        let mut elems = vec![outer];
        seed_i_placeholders(&mut elems);
        let outer_obj = elems[0].as_obj().unwrap();
        // outer gets seeded
        assert!(matches!(&outer_obj.children[0], FfonElement::Str(s) if is_i_placeholder(s)));
        // inner gets seeded too (now at index 1 after outer seeding, first child is the placeholder)
        let inner_obj = outer_obj.children[1].as_obj().unwrap();
        assert!(matches!(&inner_obj.children[0], FfonElement::Str(s) if is_i_placeholder(s)));
    }
}
