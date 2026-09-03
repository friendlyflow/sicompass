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
    next_id: Id,
}

impl Board {
    pub fn new() -> Self {
        Board {
            columns: Vec::new(),
            next_id: 1,
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
