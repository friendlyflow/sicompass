//! SQLite-backed envelope cache for `lib_emailclient`.
//!
//! Stores message headers (not bodies) per `(folder, uid)` so that repeated
//! visits to the same folder avoid a full IMAP round-trip when nothing has
//! changed.  The cache is keyed on UIDVALIDITY: when the server reports a
//! different UIDVALIDITY for a folder, the cached envelopes for that folder
//! are flushed and rebuilt from scratch.
//!
//! DB location: `~/.cache/sicompass/email/<hex_username>.db`

use crate::MessageHeader;
use rusqlite::{params, Connection};

pub struct EnvelopeCache {
    conn: Connection,
}

impl EnvelopeCache {
    /// Open (or create) the cache DB for `username`.
    ///
    /// Returns `None` if the cache directory cannot be created or the DB
    /// cannot be opened — the caller silently falls back to uncached IMAP.
    pub fn open(username: &str) -> Option<Self> {
        let cache_dir = dirs_cache_dir()?.join("sicompass").join("email");
        std::fs::create_dir_all(&cache_dir).ok()?;
        // Safe filename: hex-encode the username bytes.
        let hex: String = username.bytes().map(|b| format!("{b:02x}")).collect();
        let db_path = cache_dir.join(format!("{hex}.db"));
        let conn = Connection::open(&db_path).ok()?;
        let cache = EnvelopeCache { conn };
        cache.init_schema().ok()?;
        Some(cache)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS folder_meta (
                folder       TEXT PRIMARY KEY,
                uidvalidity  INTEGER NOT NULL,
                count        INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS envelopes (
                folder    TEXT    NOT NULL,
                uid       INTEGER NOT NULL,
                from_addr TEXT    NOT NULL,
                subject   TEXT    NOT NULL,
                date      TEXT    NOT NULL,
                seen      INTEGER NOT NULL,
                flagged   INTEGER NOT NULL,
                PRIMARY KEY (folder, uid)
             );",
        )
    }

    // -----------------------------------------------------------------------
    // Read helpers
    // -----------------------------------------------------------------------

    /// Stored UIDVALIDITY for `folder`, or `None` if not cached yet.
    pub fn get_uidvalidity(&self, folder: &str) -> Option<u32> {
        self.conn
            .query_row(
                "SELECT uidvalidity FROM folder_meta WHERE folder = ?1",
                params![folder],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|v| v as u32)
    }

    /// Number of envelopes cached for `folder`.
    pub fn cached_count(&self, folder: &str) -> usize {
        self.conn
            .query_row(
                "SELECT count FROM folder_meta WHERE folder = ?1",
                params![folder],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|v| v as usize)
            .unwrap_or(0)
    }

    /// Highest cached UID for `folder`, or `None` if the folder is not cached.
    pub fn max_uid(&self, folder: &str) -> Option<u32> {
        self.conn
            .query_row(
                "SELECT MAX(uid) FROM envelopes WHERE folder = ?1",
                params![folder],
                |row| row.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
            .map(|v| v as u32)
    }

    /// Return the `limit` most-recent envelopes (by UID descending) for `folder`.
    pub fn get_latest(&self, folder: &str, limit: usize) -> Vec<MessageHeader> {
        let mut stmt = match self.conn.prepare(
            "SELECT uid, from_addr, subject, date, seen, flagged
               FROM envelopes
              WHERE folder = ?1
           ORDER BY uid DESC
              LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![folder, limit as i64], |row| {
            Ok(MessageHeader {
                uid: row.get::<_, i64>(0)? as u32,
                from: row.get(1)?,
                subject: row.get(2)?,
                date: row.get(3)?,
                seen: row.get::<_, i64>(4)? != 0,
                flagged: row.get::<_, i64>(5)? != 0,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Write helpers
    // -----------------------------------------------------------------------

    /// Delete all cached envelopes for `folder` and record the new UIDVALIDITY.
    pub fn invalidate_folder(&self, folder: &str, new_uidvalidity: u32) {
        let _ = self.conn.execute(
            "DELETE FROM envelopes WHERE folder = ?1",
            params![folder],
        );
        let _ = self.conn.execute(
            "INSERT INTO folder_meta (folder, uidvalidity, count)
             VALUES (?1, ?2, 0)
             ON CONFLICT(folder) DO UPDATE SET uidvalidity = excluded.uidvalidity, count = 0",
            params![folder, new_uidvalidity as i64],
        );
    }

    /// Insert or replace a batch of envelopes and update the folder count.
    pub fn upsert_all(&self, folder: &str, headers: &[MessageHeader]) {
        for h in headers {
            let _ = self.conn.execute(
                "INSERT INTO envelopes (folder, uid, from_addr, subject, date, seen, flagged)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(folder, uid) DO UPDATE SET
                   from_addr = excluded.from_addr,
                   subject   = excluded.subject,
                   date      = excluded.date,
                   seen      = excluded.seen,
                   flagged   = excluded.flagged",
                params![
                    folder,
                    h.uid as i64,
                    &h.from,
                    &h.subject,
                    &h.date,
                    h.seen as i64,
                    h.flagged as i64,
                ],
            );
        }
        // Recount.
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM envelopes WHERE folder = ?1",
                params![folder],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let _ = self.conn.execute(
            "INSERT INTO folder_meta (folder, uidvalidity, count)
             VALUES (?1, 0, ?2)
             ON CONFLICT(folder) DO UPDATE SET count = excluded.count",
            params![folder, count],
        );
    }

    /// Update seen/flagged status for a single cached envelope.
    pub fn update_flags(&self, folder: &str, uid: u32, seen: bool, flagged: bool) {
        let _ = self.conn.execute(
            "UPDATE envelopes SET seen = ?3, flagged = ?4
              WHERE folder = ?1 AND uid = ?2",
            params![folder, uid as i64, seen as i64, flagged as i64],
        );
    }

    /// Selectively update only the flags that were explicitly changed.
    ///
    /// `new_seen` / `new_flagged` are `None` when that flag was not touched.
    pub fn patch_flags(&self, folder: &str, uid: u32, new_seen: Option<bool>, new_flagged: Option<bool>) {
        if let Some(seen) = new_seen {
            let _ = self.conn.execute(
                "UPDATE envelopes SET seen = ?3 WHERE folder = ?1 AND uid = ?2",
                params![folder, uid as i64, seen as i64],
            );
        }
        if let Some(flagged) = new_flagged {
            let _ = self.conn.execute(
                "UPDATE envelopes SET flagged = ?3 WHERE folder = ?1 AND uid = ?2",
                params![folder, uid as i64, flagged as i64],
            );
        }
    }

    /// Remove a single cached envelope (after EXPUNGE).
    pub fn remove(&self, folder: &str, uid: u32) {
        let _ = self
            .conn
            .execute(
                "DELETE FROM envelopes WHERE folder = ?1 AND uid = ?2",
                params![folder, uid as i64],
            );
        // Update count.
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM envelopes WHERE folder = ?1",
                params![folder],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let _ = self.conn.execute(
            "UPDATE folder_meta SET count = ?2 WHERE folder = ?1",
            params![folder, count],
        );
    }
}

/// Portable replacement for `dirs::cache_dir()` without adding a dep.
fn dirs_cache_dir() -> Option<std::path::PathBuf> {
    // Linux/macOS: $XDG_CACHE_HOME or $HOME/.cache
    if let Ok(p) = std::env::var("XDG_CACHE_HOME") {
        let path = std::path::PathBuf::from(p);
        if path.is_absolute() {
            return Some(path);
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(std::path::PathBuf::from(home).join(".cache"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_in(dir: &std::path::Path) -> EnvelopeCache {
        let db_path = dir.join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        let cache = EnvelopeCache { conn };
        cache.init_schema().unwrap();
        cache
    }

    fn hdr(uid: u32, subject: &str) -> MessageHeader {
        MessageHeader {
            uid,
            from: "alice@example.com".to_owned(),
            subject: subject.to_owned(),
            date: "2025-01-01".to_owned(),
            seen: true,
            flagged: false,
        }
    }

    #[test]
    fn test_cache_miss_returns_none_uidvalidity() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        assert_eq!(cache.get_uidvalidity("INBOX"), None);
    }

    #[test]
    fn test_upsert_and_get_latest() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        cache.invalidate_folder("INBOX", 42);
        let msgs = vec![hdr(1, "A"), hdr(2, "B"), hdr(3, "C")];
        cache.upsert_all("INBOX", &msgs);
        let latest = cache.get_latest("INBOX", 2);
        // Most-recent-first by UID.
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].uid, 3);
        assert_eq!(latest[1].uid, 2);
    }

    #[test]
    fn test_invalidate_flushes_envelopes() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        cache.invalidate_folder("INBOX", 1);
        cache.upsert_all("INBOX", &[hdr(1, "A")]);
        assert_eq!(cache.get_latest("INBOX", 10).len(), 1);
        cache.invalidate_folder("INBOX", 2);
        assert_eq!(cache.get_latest("INBOX", 10).len(), 0);
        assert_eq!(cache.get_uidvalidity("INBOX"), Some(2));
    }

    #[test]
    fn test_cached_count_matches_upserted() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        cache.invalidate_folder("INBOX", 5);
        cache.upsert_all("INBOX", &[hdr(1, "A"), hdr(2, "B")]);
        assert_eq!(cache.cached_count("INBOX"), 2);
    }

    #[test]
    fn test_max_uid() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        cache.invalidate_folder("INBOX", 1);
        cache.upsert_all("INBOX", &[hdr(10, "A"), hdr(20, "B"), hdr(5, "C")]);
        assert_eq!(cache.max_uid("INBOX"), Some(20));
    }

    #[test]
    fn test_update_flags() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        cache.invalidate_folder("INBOX", 1);
        cache.upsert_all("INBOX", &[hdr(1, "A")]);
        cache.update_flags("INBOX", 1, false, true);
        let latest = cache.get_latest("INBOX", 1);
        assert!(!latest[0].seen);
        assert!(latest[0].flagged);
    }

    #[test]
    fn test_remove_decrements_count() {
        let dir = tempdir().unwrap();
        let cache = open_in(dir.path());
        cache.invalidate_folder("INBOX", 1);
        cache.upsert_all("INBOX", &[hdr(1, "A"), hdr(2, "B")]);
        cache.remove("INBOX", 1);
        assert_eq!(cache.cached_count("INBOX"), 1);
        assert!(cache.get_latest("INBOX", 10).iter().all(|h| h.uid == 2));
    }
}
