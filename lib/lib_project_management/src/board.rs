//! The in-memory board: columns, the cards in them, and their stable ids.
//!
//! # Two levels, on purpose
//!
//! A kanban board is a tree exactly two levels deep. The provider root lists the
//! columns, a column lists its cards, and a card is a leaf. Modelling it as a
//! general recursive tree the way `lib_notes` does would buy nothing and cost
//! the depth guard: every insert path would have to re-assert "not deeper than
//! two", and one that forgot would silently grow a third level that the board
//! view has nowhere to draw. Here the type system says it instead — a `Card` has
//! no children field to fill in.

/// A card's or column's local identity. Minted once, never reused.
///
/// Position cannot serve as identity: inserting a card above another renumbers
/// it, and anything holding a position — a navigation path, a pending rename, a
/// timeline entry waiting to be undone — would silently start naming a different
/// card. The app persists and restores `current_path()` across a refresh, an
/// undo and a restart, so this matters beyond one frame.
pub type Id = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: Id,
    pub text: String,
}

impl Card {
    pub fn new(id: Id, text: impl Into<String>) -> Self {
        Card {
            id,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub id: Id,
    pub title: String,
    pub cards: Vec<Card>,
}

impl Column {
    pub fn new(id: Id, title: impl Into<String>) -> Self {
        Column {
            id,
            title: title.into(),
            cards: Vec::new(),
        }
    }
}

/// The whole board, plus the id counter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Board {
    pub columns: Vec<Column>,
    /// The column cards are archived into, if one has been made yet.
    ///
    /// Private, because two invariants hang off it and methods are the only
    /// place they can be kept: the column it names is pinned **last**, and an id
    /// naming nothing reads as no archive at all.
    ///
    /// An id and not a position, for the reason [`Id`] exists: the list view
    /// reorders columns freely, so a position would name a different column the
    /// moment one was inserted above. And not a title either, because the user
    /// can rename the archive column and must still have an archive afterwards.
    archive: Option<Id>,
    next_id: Id,
}

impl Board {
    pub fn new() -> Self {
        Board {
            columns: Vec::new(),
            archive: None,
            next_id: 1,
        }
    }

    /// The archive column's id, or `None`.
    ///
    /// Self-healing: an id that no longer names a column on the board reads as
    /// no archive. The user can delete the archive column in the list like any
    /// other, and nothing should have to notice before the next read.
    pub fn archive_id(&self) -> Option<Id> {
        self.archive.filter(|id| self.column_index(*id).is_some())
    }

    pub fn is_archive(&self, id: Id) -> bool {
        self.archive_id() == Some(id)
    }

    /// How many columns the board view draws.
    ///
    /// The archive is pinned last, so this is a prefix of [`Board::columns`] and
    /// every index below it names a real column. That is what lets the board's
    /// drawing and navigation stay plain index arithmetic: keep the cursor under
    /// this number and the archive is unreachable, with no per-column test
    /// anywhere.
    pub fn visible_len(&self) -> usize {
        self.columns.len() - usize::from(self.archive_id().is_some())
    }

    /// Adopt `id` as the archive, and pin it last.
    pub fn set_archive(&mut self, id: Id) {
        self.archive = Some(id);
        self.pin_archive_last();
    }

    /// Re-establish both halves of the invariant: forget an id that names
    /// nothing, and move the archive column to the end.
    ///
    /// Idempotent, and it has to run after anything that rebuilds `columns` from
    /// the outside. `reconcile_columns` is the one that matters: it replaces the
    /// whole vector with the rows the app hands back, in the app's order, so
    /// without this a single edit anywhere in the list could leave the archive
    /// sitting in the middle of the board, visible.
    pub fn pin_archive_last(&mut self) {
        let Some(id) = self.archive_id() else {
            self.archive = None;
            return;
        };
        let i = self
            .column_index(id)
            .expect("archive_id only answers for a column that is on the board");
        let last = self.columns.len() - 1;
        if i != last {
            let col = self.columns.remove(i);
            self.columns.push(col);
        }
    }

    /// Lift the counter clear of every id on the board.
    ///
    /// Monotonic, never a reset. After a load that is the same thing, because the
    /// counter starts at 1 and everything on the board is above it. After an
    /// **undo** it is not: the board shrinks, and a plain reset would hand the
    /// removed card's id straight back out. The next new card would then collide
    /// with whatever a redo is still holding, and `locate_card` would find
    /// whichever copy came first. Ids are minted once and never reused, and this
    /// is the line that has to hold for that to be true.
    pub fn reseat_counter(&mut self) {
        self.next_id = self.next_id.max(self.max_id() + 1);
    }

    pub fn mint_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn max_id(&self) -> Id {
        self.columns
            .iter()
            .map(|c| c.id.max(c.cards.iter().map(|k| k.id).max().unwrap_or(0)))
            .max()
            .unwrap_or(0)
    }

    pub fn column(&self, id: Id) -> Option<&Column> {
        self.columns.iter().find(|c| c.id == id)
    }

    pub fn column_mut(&mut self, id: Id) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.id == id)
    }

    pub fn column_index(&self, id: Id) -> Option<usize> {
        self.columns.iter().position(|c| c.id == id)
    }

    /// The column holding the card with this id, and the card's position in it.
    pub fn locate_card(&self, id: Id) -> Option<(usize, usize)> {
        for (ci, col) in self.columns.iter().enumerate() {
            if let Some(ki) = col.cards.iter().position(|k| k.id == id) {
                return Some((ci, ki));
            }
        }
        None
    }

    pub fn card(&self, id: Id) -> Option<&Card> {
        let (ci, ki) = self.locate_card(id)?;
        self.columns[ci].cards.get(ki)
    }

    /// Total cards across every column, for the board's status line.
    pub fn card_count(&self) -> usize {
        self.columns.iter().map(|c| c.cards.len()).sum()
    }

    /// Give an id to anything that came off disk without one — a store written
    /// by an older version, or one a user edited by hand.
    ///
    /// Minting during the load itself would risk colliding with an id further
    /// down that has not been read yet, so this runs once the whole board is in.
    pub fn assign_missing_ids(&mut self) {
        let mut next = self.max_id() + 1;
        for col in self.columns.iter_mut() {
            if col.id == 0 {
                col.id = next;
                next += 1;
            }
            for card in col.cards.iter_mut() {
                if card.id == 0 {
                    card.id = next;
                    next += 1;
                }
            }
        }
        self.reseat_counter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Board {
        let mut b = Board::new();
        let mut todo = Column::new(1, "To do");
        todo.cards.push(Card::new(2, "fix login"));
        todo.cards.push(Card::new(3, "write docs"));
        let mut doing = Column::new(4, "Doing");
        doing.cards.push(Card::new(5, "kanban ui"));
        b.columns.push(todo);
        b.columns.push(doing);
        b.reseat_counter();
        b
    }

    #[test]
    fn minted_ids_are_never_reused() {
        let mut b = sample();
        let first = b.mint_id();
        let second = b.mint_id();
        assert!(first > b.max_id().min(first - 1));
        assert_ne!(first, second);
        assert!(first > 5, "must clear every id already on the board");
    }

    #[test]
    fn a_reload_cannot_hand_out_an_id_that_is_already_on_disk() {
        // What a load does: a fresh board, whose counter starts at 1, filled from
        // disk and then reseated.
        let mut b = Board::new();
        b.columns = sample().columns;
        b.reseat_counter();
        assert_eq!(b.mint_id(), 6);
    }

    #[test]
    fn the_counter_never_walks_backwards() {
        // Undo shrinks the board. If the counter followed it down, the next new
        // card would take the id of the one undo removed, and a redo would then
        // insert a second card claiming the same identity.
        let mut b = sample();
        let minted = b.mint_id();
        b.columns[0].cards.push(Card::new(minted, "new"));
        b.reseat_counter();

        b.columns[0].cards.pop(); // as an undo would
        b.reseat_counter();
        assert!(
            b.mint_id() > minted,
            "an id handed out once must never be handed out again"
        );
    }

    #[test]
    fn locate_card_finds_the_owning_column() {
        let b = sample();
        assert_eq!(b.locate_card(3), Some((0, 1)));
        assert_eq!(b.locate_card(5), Some((1, 0)));
        assert_eq!(b.locate_card(99), None);
    }

    #[test]
    fn card_count_spans_every_column() {
        assert_eq!(sample().card_count(), 3);
    }

    // ---- The archive ----------------------------------------------------

    #[test]
    fn the_archive_is_pinned_last_however_it_got_there() {
        let mut b = sample();
        b.columns.insert(0, Column::new(9, "Archive"));
        b.set_archive(9);
        assert_eq!(b.columns.last().unwrap().id, 9);
        assert_eq!(b.visible_len(), 2, "the board sees only the real columns");

        // As `reconcile_columns` would: the app hands the rows back in its own
        // order, archive in the middle.
        b.columns.swap(1, 2);
        b.pin_archive_last();
        assert_eq!(b.columns.last().unwrap().id, 9);
    }

    #[test]
    fn pinning_is_idempotent() {
        let mut b = sample();
        b.columns.push(Column::new(9, "Archive"));
        b.set_archive(9);
        let once = b.columns.clone();
        b.pin_archive_last();
        assert_eq!(b.columns, once);
    }

    #[test]
    fn an_archive_id_naming_a_deleted_column_reads_as_no_archive() {
        // The user can delete the archive column in the list like any other, and
        // nothing should have to notice before the next read.
        let mut b = sample();
        b.columns.push(Column::new(9, "Archive"));
        b.set_archive(9);
        b.columns.retain(|c| c.id != 9);
        assert_eq!(b.archive_id(), None);
        assert!(!b.is_archive(9));
        assert_eq!(b.visible_len(), b.columns.len());
        b.pin_archive_last(); // must not panic on the dangling id
        assert_eq!(b.archive_id(), None);
    }

    #[test]
    fn a_board_that_is_only_an_archive_has_nothing_visible() {
        let mut b = Board::new();
        b.columns.push(Column::new(9, "Archive"));
        b.set_archive(9);
        assert_eq!(b.visible_len(), 0);
    }

    #[test]
    fn an_archived_cards_id_is_still_never_handed_out_again() {
        // `max_id` deliberately spans the archive too. An id that went in there
        // must not be re-minted and collide with a redo still holding it.
        let mut b = sample();
        let mut archive = Column::new(9, "Archive");
        archive.cards.push(Card::new(42, "shipped"));
        b.columns.push(archive);
        b.set_archive(9);
        b.reseat_counter();
        assert!(b.mint_id() > 42);
    }

    #[test]
    fn a_card_in_the_archive_is_located_like_any_other() {
        // This is what makes `BoardOp`'s "lift it from wherever it is" reverse an
        // archive for free.
        let mut b = sample();
        let mut archive = Column::new(9, "Archive");
        archive.cards.push(Card::new(42, "shipped"));
        b.columns.push(archive);
        b.set_archive(9);
        assert_eq!(b.locate_card(42), Some((2, 0)));
        assert_eq!(b.card(42).map(|c| c.text.as_str()), Some("shipped"));
    }

    #[test]
    fn ids_read_off_disk_without_one_get_filled_in() {
        let mut b = Board::new();
        let mut col = Column::new(0, "Ideas");
        col.cards.push(Card::new(0, "dark mode"));
        col.cards.push(Card::new(7, "sync"));
        b.columns.push(col);
        b.assign_missing_ids();
        let col = &b.columns[0];
        assert!(col.id > 7);
        assert!(col.cards[0].id > 7);
        assert_eq!(col.cards[1].id, 7, "an id already on disk is kept");
        assert_ne!(col.id, col.cards[0].id);
    }
}
