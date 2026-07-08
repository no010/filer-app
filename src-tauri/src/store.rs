//! Local SQLite index of detected/filed downloads.
//!
//! One `records` table; the `status` column distinguishes inbox / filed /
//! ignored / error / missing / replaced / deleted. Files themselves live on
//! the local FS; this table is the book of record for what was seen and where
//! it went.
//!
//! Staleness tracking (memory-curve style retention): `last_opened_at` (filer
//! "打开" button), `last_reviewed_at` ("仍需要" bump), and `file_mtime_at_filed`
//! (baseline to detect post-filing modification) let the 回顾 view rank files
//! by days-since-last-touch into decay tiers.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

pub const STATUS_INBOX: &str = "inbox";
pub const STATUS_FILED: &str = "filed";
pub const STATUS_IGNORED: &str = "ignored";
pub const STATUS_ERROR: &str = "error";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_REPLACED: &str = "replaced";
pub const STATUS_DELETED: &str = "deleted";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Record {
    pub id: i64,
    pub sha256: String,
    pub original_path: String,
    pub original_filename: String,
    pub size_bytes: i64,
    pub detected_at: String,
    pub status: String,
    pub category: String,
    /// JSON blob: { mime, n_pages, vendor, title, dimensions, ... }
    pub sub_meta: String,
    pub rule_id: String,
    pub suggested_dest: String,
    pub suggested_filename: String,
    pub filed_path: String,
    pub filed_at: String,
    pub action: String,
    /// JSON array of user tags.
    pub tags: String,
    /// id of a prior filed record with the same sha256; 0 = not a duplicate.
    #[serde(default)]
    pub duplicate_of: i64,
    /// User's chosen dedup action: "" | skip | keep_both | replace | delete_new
    #[serde(default)]
    pub dedup_decision: String,
    /// RFC3339 of the last time the user opened the file via filer's "打开".
    #[serde(default)]
    pub last_opened_at: String,
    /// RFC3339 of the last time the user marked the file "仍需要" in 回顾.
    #[serde(default)]
    pub last_reviewed_at: String,
    /// The filed file's mtime at filing time (RFC3339). Compared later to
    /// detect post-filing modification (= "updated").
    #[serde(default)]
    pub file_mtime_at_filed: String,
    pub error: String,
}

/// A record enriched with computed staleness for the 回顾 view.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewRow {
    #[serde(flatten)]
    pub record: Record,
    /// Days since the most recent touch (max of opened/reviewed/filed).
    pub staleness_days: i64,
    /// True if the filed file's current mtime differs from the baseline
    /// (file was modified after filing).
    pub updated_since_filed: bool,
    /// True if the filed file no longer exists on disk.
    pub file_missing: bool,
}

/// Fields needed to insert a new inbox record (computed by the watcher after
/// analysis + rule matching).
pub struct NewRecord {
    pub sha256: String,
    pub original_path: String,
    pub original_filename: String,
    pub size_bytes: i64,
    pub detected_at: String,
    pub category: String,
    pub sub_meta: String,
    pub rule_id: String,
    pub suggested_dest: String,
    pub suggested_filename: String,
    /// id of a prior filed record this duplicates, or 0.
    pub duplicate_of: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

/// The SELECT column list — one source of truth for all read queries.
/// Column order MUST match `row_to_record`.
const RECORD_COLUMNS_SELECT_SQL: &str =
    "SELECT id, sha256, original_path, original_filename, size_bytes,
            detected_at, status, category, sub_meta, rule_id, suggested_dest,
            suggested_filename, filed_path, filed_at, action, tags, error,
            duplicate_of, dedup_decision, last_opened_at, last_reviewed_at,
            file_mtime_at_filed";

impl Store {
    /// Open (or create) the SQLite db at `path` and run migrations.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sha256 TEXT NOT NULL,
                original_path TEXT NOT NULL,
                original_filename TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                detected_at TEXT NOT NULL,
                status TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT '',
                sub_meta TEXT NOT NULL DEFAULT '{}',
                rule_id TEXT NOT NULL DEFAULT '',
                suggested_dest TEXT NOT NULL DEFAULT '',
                suggested_filename TEXT NOT NULL DEFAULT '',
                filed_path TEXT NOT NULL DEFAULT '',
                filed_at TEXT NOT NULL DEFAULT '',
                action TEXT NOT NULL DEFAULT '',
                tags TEXT NOT NULL DEFAULT '[]',
                error TEXT NOT NULL DEFAULT '',
                duplicate_of INTEGER NOT NULL DEFAULT 0,
                dedup_decision TEXT NOT NULL DEFAULT '',
                last_opened_at TEXT NOT NULL DEFAULT '',
                last_reviewed_at TEXT NOT NULL DEFAULT '',
                file_mtime_at_filed TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_records_status ON records(status);
            CREATE INDEX IF NOT EXISTS idx_records_sha ON records(sha256);
            PRAGMA journal_mode = WAL;",
        )?;
        // Idempotent migrations for dbs created before these columns existed.
        for (col, def) in [
            ("duplicate_of", "INTEGER NOT NULL DEFAULT 0"),
            ("dedup_decision", "TEXT NOT NULL DEFAULT ''"),
            ("last_opened_at", "TEXT NOT NULL DEFAULT ''"),
            ("last_reviewed_at", "TEXT NOT NULL DEFAULT ''"),
            ("file_mtime_at_filed", "TEXT NOT NULL DEFAULT ''"),
        ] {
            ensure_column(&conn, "records", col, def)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Insert a new inbox record. Returns the row id.
    pub fn insert_inbox(&self, n: &NewRecord) -> anyhow::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO records
                (sha256, original_path, original_filename, size_bytes, detected_at,
                 status, category, sub_meta, rule_id, suggested_dest, suggested_filename,
                 tags, duplicate_of)
             VALUES (?1, ?2, ?3, ?4, ?5, 'inbox', ?6, ?7, ?8, ?9, ?10, '[]', ?11)",
            params![
                n.sha256, n.original_path, n.original_filename, n.size_bytes,
                n.detected_at, n.category, n.sub_meta, n.rule_id, n.suggested_dest,
                n.suggested_filename, n.duplicate_of,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a single record by id.
    pub fn get(&self, id: i64) -> anyhow::Result<Option<Record>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("{RECORD_COLUMNS_SELECT_SQL} FROM records WHERE id = ?1"))?;
        let r = stmt.query_row(params![id], row_to_record).optional()?;
        Ok(r)
    }

    /// Find the most recent filed record with the same sha256 (dedup check).
    pub fn find_filed_by_sha(&self, sha: &str) -> anyhow::Result<Option<Record>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{RECORD_COLUMNS_SELECT_SQL} FROM records WHERE sha256 = ?1 AND status = 'filed'
             ORDER BY filed_at DESC LIMIT 1"
        ))?;
        let r = stmt.query_row(params![sha], row_to_record).optional()?;
        Ok(r)
    }

    /// Is there a PENDING record (status inbox/ignored) for this path? Such a
    /// record means the file is still sitting in the download dir (either
    /// awaiting triage or dismissed by the user) → don't re-insert on scan.
    /// Filed/deleted/missing records do NOT block: a filed file was moved out
    /// (path is free for a re-download), so a new file at the same path must be
    /// re-processed and dedup'd by content hash (find_filed_by_sha).
    pub fn path_pending(&self, original_path: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM records WHERE original_path = ?1 AND status IN ('inbox','ignored')",
            params![original_path],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// List inbox records, newest first.
    pub fn list_inbox(&self) -> anyhow::Result<Vec<Record>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{RECORD_COLUMNS_SELECT_SQL} FROM records WHERE status = 'inbox'
             ORDER BY detected_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map([], row_to_record)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Paginated history (filed records), newest first. Cursor = last id from
    /// the previous page (exclusive).
    pub fn list_history(&self, after_id: Option<i64>, limit: usize) -> anyhow::Result<(Vec<Record>, Option<i64>)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{RECORD_COLUMNS_SELECT_SQL} FROM records WHERE status = 'filed' AND (?1 IS NULL OR id < ?1)
             ORDER BY id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![after_id, limit as i64], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        let next = if rows.len() == limit {
            rows.last().map(|r| r.id)
        } else {
            None
        };
        Ok((rows, next))
    }

    /// Filed records ordered stales-first (most recent touch ascending), for
    /// the 回顾 view. `limit` caps the work (review scans a bounded window).
    pub fn list_review(&self, limit: usize) -> anyhow::Result<Vec<Record>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{RECORD_COLUMNS_SELECT_SQL} FROM records WHERE status = 'filed'
             ORDER BY COALESCE(NULLIF(last_opened_at, ''), NULLIF(last_reviewed_at, ''), filed_at) ASC
             LIMIT ?1"
        ))?;
        let rows = stmt.query_map(params![limit as i64], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Count filed records whose most-recent-touch is older than `cutoff_rfc`
    /// (an RFC3339 string; lexical comparison works for same-format timestamps).
    pub fn count_stale(&self, cutoff_rfc: &str) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM records WHERE status = 'filed'
             AND COALESCE(NULLIF(last_opened_at, ''), NULLIF(last_reviewed_at, ''), filed_at) < ?1",
            params![cutoff_rfc],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Count records by status.
    pub fn count_status(&self, status: &str) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM records WHERE status = ?1",
            params![status],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Count filed records filed since `since_rfc` (RFC3339, lexical compare).
    pub fn count_filed_since(&self, since_rfc: &str) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM records WHERE status = 'filed' AND filed_at >= ?1",
            params![since_rfc],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Filed records grouped by a column, count desc. Used for the stats view
    /// (by category, by action).
    pub fn group_count_filed(&self, col: &str) -> anyhow::Result<Vec<(String, usize)>> {
        // col is a hard-coded field name, not user input — safe to interpolate.
        let sql = format!(
            "SELECT {col}, COUNT(*) FROM records WHERE status = 'filed'
             GROUP BY {col} ORDER BY COUNT(*) DESC"
        );
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Search filed records by a query string across filename / category /
    /// tags / sub_meta / original_path (LIKE, case-insensitive via SQLite
    /// LOWER — ICU not needed, we lower both sides).
    pub fn search(&self, q: &str, limit: usize) -> anyhow::Result<Vec<Record>> {
        let like = format!("%{}%", q.to_lowercase());
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{RECORD_COLUMNS_SELECT_SQL} FROM records WHERE status = 'filed' AND (
                LOWER(original_filename) LIKE ?1 OR LOWER(category) LIKE ?1
                OR LOWER(tags) LIKE ?1 OR LOWER(sub_meta) LIKE ?1
                OR LOWER(original_path) LIKE ?1
            ) ORDER BY filed_at DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![like, limit as i64], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Mark a record as filed: write back the final path/time/action + any
    /// confirm-time overrides (category, tags) + the filed-file mtime baseline.
    pub fn mark_filed(
        &self, id: i64, filed_path: &str, filed_at: &str, action: &str,
        category: &str, tags: &str, file_mtime_at_filed: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE records SET status='filed', filed_path=?1, filed_at=?2,
                action=?3, category=?4, tags=?5, error='', file_mtime_at_filed=?6
             WHERE id=?7",
            params![filed_path, filed_at, action, category, tags, file_mtime_at_filed, id],
        )?;
        Ok(())
    }

    /// Move a filed record back to inbox (undo) and clear filed fields.
    pub fn revert_to_inbox(&self, id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE records SET status='inbox', filed_path='', filed_at='', action='',
                file_mtime_at_filed='' WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_ignored(&self, id: i64) -> anyhow::Result<()> {
        self.set_status(id, STATUS_IGNORED, "")
    }

    pub fn set_status(&self, id: i64, status: &str, error: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE records SET status=?1, error=?2 WHERE id=?3",
            params![status, error, id],
        )?;
        Ok(())
    }

    pub fn set_tags(&self, id: i64, tags_json: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE records SET tags=?1 WHERE id=?2", params![tags_json, id])?;
        Ok(())
    }

    /// Record the user's dedup decision for this record.
    pub fn set_dedup_decision(&self, id: i64, decision: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE records SET dedup_decision=?1 WHERE id=?2", params![decision, id])?;
        Ok(())
    }

    /// Mark a filed record as replaced by a newer duplicate: clears filed
    /// fields (the on-disk file is deleted by the caller).
    pub fn mark_replaced(&self, id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE records SET status='replaced', filed_path='', filed_at='', action='',
                file_mtime_at_filed='' WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    /// Mark a filed record as deleted (the on-disk file was removed by the caller).
    pub fn mark_deleted(&self, id: i64) -> anyhow::Result<()> {
        self.set_status(id, STATUS_DELETED, "")
    }

    /// Bump last_opened_at (filer "打开" button).
    pub fn touch_opened(&self, id: i64, ts: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE records SET last_opened_at=?1 WHERE id=?2", params![ts, id])?;
        Ok(())
    }

    /// Bump last_reviewed_at ("仍需要" in 回顾 — resets the staleness clock).
    pub fn touch_reviewed(&self, id: i64, ts: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE records SET last_reviewed_at=?1 WHERE id=?2", params![ts, id])?;
        Ok(())
    }

    /// Remove a record from the index (the on-disk file is untouched).
    pub fn delete(&self, id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM records WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Overwrite the suggestion fields (used after re-analysis).
    #[allow(dead_code)]
    pub fn set_suggestion(
        &self, id: i64, category: &str, sub_meta: &str, rule_id: &str,
        dest: &str, filename: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE records SET category=?1, sub_meta=?2, rule_id=?3,
                suggested_dest=?4, suggested_filename=?5 WHERE id=?6",
            params![category, sub_meta, rule_id, dest, filename, id],
        )?;
        Ok(())
    }
}

/// Add a column to a table if it's missing (idempotent migration).
fn ensure_column(conn: &Connection, table: &str, column: &str, def: &str) -> anyhow::Result<()> {
    let has: bool = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
        rows.iter().any(|c| c == column)
    };
    if !has {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {def}"), [])?;
    }
    Ok(())
}

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        id: r.get(0)?,
        sha256: r.get(1)?,
        original_path: r.get(2)?,
        original_filename: r.get(3)?,
        size_bytes: r.get(4)?,
        detected_at: r.get(5)?,
        status: r.get(6)?,
        category: r.get(7)?,
        sub_meta: r.get(8)?,
        rule_id: r.get(9)?,
        suggested_dest: r.get(10)?,
        suggested_filename: r.get(11)?,
        filed_path: r.get(12)?,
        filed_at: r.get(13)?,
        action: r.get(14)?,
        tags: r.get(15)?,
        error: r.get(16)?,
        duplicate_of: r.get(17)?,
        dedup_decision: r.get(18)?,
        last_opened_at: r.get(19)?,
        last_reviewed_at: r.get(20)?,
        file_mtime_at_filed: r.get(21)?,
    })
}

// Bring .optional() into scope for query_row results.
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> (Store, tempfile::TempPath) {
        let f = tempfile::Builder::new().suffix(".db").tempfile().unwrap().into_temp_path();
        let s = Store::open(&f).unwrap();
        (s, f)
    }

    fn nr(sha: &str, path: &str, dup: i64) -> NewRecord {
        NewRecord {
            sha256: sha.into(), original_path: path.into(),
            original_filename: path.rsplit(['/', '\\']).next().unwrap_or(path).into(),
            size_bytes: 100, detected_at: "2026-07-06T10:00:00+08:00".into(),
            category: "Misc".into(), sub_meta: "{}".into(), rule_id: "misc".into(),
            suggested_dest: "D:\\Filer\\2026-07".into(),
            suggested_filename: path.rsplit(['/', '\\']).next().unwrap_or(path).into(),
            duplicate_of: dup,
        }
    }

    #[test]
    fn insert_and_get() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("abc", "C:\\DL\\x.pdf", 0)).unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.sha256, "abc");
        assert_eq!(r.status, STATUS_INBOX);
        assert_eq!(r.duplicate_of, 0);
        assert_eq!(r.last_opened_at, "");
        assert_eq!(r.file_mtime_at_filed, "");
    }

    #[test]
    fn mark_filed_records_mtime_baseline() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(id, "D:\\Filer\\a.pdf", "2026-07-06T11:00:00+08:00", "move", "Misc", "[]", "2026-07-06T09:00:00+08:00").unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.status, STATUS_FILED);
        assert_eq!(r.file_mtime_at_filed, "2026-07-06T09:00:00+08:00");
    }

    #[test]
    fn list_review_orders_stales_first() {
        let (s, _f) = tmp_store();
        // a: filed long ago; b: filed recently; c: filed long ago but opened recently.
        let a = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(a, "D:\\a.pdf", "2025-01-01T00:00:00+00:00", "move", "M", "[]", "2025-01-01T00:00:00+00:00").unwrap();
        let b = s.insert_inbox(&nr("b", "C:\\DL\\b.pdf", 0)).unwrap();
        s.mark_filed(b, "D:\\b.pdf", "2026-07-01T00:00:00+00:00", "move", "M", "[]", "2026-07-01T00:00:00+00:00").unwrap();
        let c = s.insert_inbox(&nr("c", "C:\\DL\\c.pdf", 0)).unwrap();
        s.mark_filed(c, "D:\\c.pdf", "2025-01-01T00:00:00+00:00", "move", "M", "[]", "2025-01-01T00:00:00+00:00").unwrap();
        s.touch_opened(c, "2026-07-05T00:00:00+00:00").unwrap();

        let rows = s.list_review(100).unwrap();
        // a (filed 2025, never opened) is stalest → first; c (opened 2026-07) is freshest → last.
        assert_eq!(rows[0].id, a);
        assert_eq!(rows.last().unwrap().id, c);
    }

    #[test]
    fn count_stale_uses_cutoff() {
        let (s, _f) = tmp_store();
        let a = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(a, "D:\\a.pdf", "2025-01-01T00:00:00+00:00", "move", "M", "[]", "2025-01-01T00:00:00+00:00").unwrap();
        let b = s.insert_inbox(&nr("b", "C:\\DL\\b.pdf", 0)).unwrap();
        s.mark_filed(b, "D:\\b.pdf", "2026-07-01T00:00:00+00:00", "move", "M", "[]", "2026-07-01T00:00:00+00:00").unwrap();
        // cutoff 2026-01-01: a (2025) is stale, b (2026-07) is not.
        assert_eq!(s.count_stale("2026-01-01T00:00:00+00:00").unwrap(), 1);
        assert_eq!(s.count_stale("2020-01-01T00:00:00+00:00").unwrap(), 0);
    }

    #[test]
    fn touch_reviewed_resets_staleness_order() {
        let (s, _f) = tmp_store();
        let a = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(a, "D:\\a.pdf", "2025-01-01T00:00:00+00:00", "move", "M", "[]", "2025-01-01T00:00:00+00:00").unwrap();
        let b = s.insert_inbox(&nr("b", "C:\\DL\\b.pdf", 0)).unwrap();
        s.mark_filed(b, "D:\\b.pdf", "2025-01-01T00:00:00+00:00", "move", "M", "[]", "2025-01-01T00:00:00+00:00").unwrap();
        // both filed same time; touch a → a fresher → b first.
        s.touch_reviewed(a, "2026-07-05T00:00:00+00:00").unwrap();
        let rows = s.list_review(100).unwrap();
        assert_eq!(rows[0].id, b);
        assert_eq!(rows.last().unwrap().id, a);
    }

    #[test]
    fn mark_deleted_sets_status() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(id, "D:\\a.pdf", "t", "move", "M", "[]", "t").unwrap();
        s.mark_deleted(id).unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.status, STATUS_DELETED);
    }

    #[test]
    fn list_inbox_newest_first() {
        let (s, _f) = tmp_store();
        let a = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        let _b = s.insert_inbox(&nr("b", "C:\\DL\\b.pdf", 0)).unwrap();
        let rows = s.list_inbox().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, a + 1);
    }

    #[test]
    fn dedup_by_sha_finds_filed() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("dup", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(id, "D:\\a.pdf", "t", "move", "M", "[]", "t").unwrap();
        let _id2 = s.insert_inbox(&nr("dup", "C:\\DL\\a (1).pdf", 0)).unwrap();
        let found = s.find_filed_by_sha("dup").unwrap().unwrap();
        assert_eq!(found.filed_path, "D:\\a.pdf");
    }

    #[test]
    fn revert_to_inbox_clears_filed() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(id, "D:\\a.pdf", "t", "move", "M", "[]", "t").unwrap();
        s.revert_to_inbox(id).unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.status, STATUS_INBOX);
        assert_eq!(r.filed_path, "");
        assert_eq!(r.file_mtime_at_filed, "");
    }

    #[test]
    fn path_pending() {
        let (s, _f) = tmp_store();
        assert!(!s.path_pending("C:\\DL\\a.pdf").unwrap());
        // inbox → blocks re-scan (file still pending in download dir)
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        assert!(s.path_pending("C:\\DL\\a.pdf").unwrap());
        // once filed (moved out), the path is free for a re-download → NOT blocked
        s.mark_filed(id, "D:\\Filer\\a.pdf", "t", "move", "M", "[]", "t").unwrap();
        assert!(!s.path_pending("C:\\DL\\a.pdf").unwrap(), "filed record must not block a re-download at the same path");
    }

    #[test]
    fn set_tags_persists() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.set_tags(id, "[\"mcu\",\"待复核\"]").unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.tags, "[\"mcu\",\"待复核\"]");
    }

    #[test]
    fn set_dedup_decision_persists() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 7)).unwrap();
        s.set_dedup_decision(id, "keep_both").unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.dedup_decision, "keep_both");
        assert_eq!(r.duplicate_of, 7);
    }

    #[test]
    fn mark_replaced_clears_filed() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(id, "D:\\a.pdf", "t", "move", "M", "[]", "t").unwrap();
        s.mark_replaced(id).unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.status, STATUS_REPLACED);
        assert_eq!(r.filed_path, "");
    }

    #[test]
    fn touch_opened_persists() {
        let (s, _f) = tmp_store();
        let id = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.touch_opened(id, "2026-07-06T12:00:00+08:00").unwrap();
        let r = s.get(id).unwrap().unwrap();
        assert_eq!(r.last_opened_at, "2026-07-06T12:00:00+08:00");
    }

    #[test]
    fn search_finds_filed_by_filename_and_tags() {
        let (s, _f) = tmp_store();
        let a = s.insert_inbox(&nr("a", "C:\\DL\\STM32F103.pdf", 0)).unwrap();
        s.mark_filed(a, "D:\\a.pdf", "t", "move", "Datasheet", "[\"mcu\",\"待复核\"]", "t").unwrap();
        let b = s.insert_inbox(&nr("b", "C:\\DL\\invoice.pdf", 0)).unwrap();
        s.mark_filed(b, "D:\\b.pdf", "t", "move", "Receipt", "[]", "t").unwrap();
        // by filename
        assert_eq!(s.search("stm32", 100).unwrap().len(), 1);
        // by tag
        assert_eq!(s.search("待复核", 100).unwrap().len(), 1);
        // by category
        assert_eq!(s.search("receipt", 100).unwrap().len(), 1);
        // no hit
        assert_eq!(s.search("zzz", 100).unwrap().len(), 0);
    }

    #[test]
    fn group_count_filed_buckets() {
        let (s, _f) = tmp_store();
        let a = s.insert_inbox(&nr("a", "C:\\DL\\a.pdf", 0)).unwrap();
        s.mark_filed(a, "D:\\a.pdf", "t", "move", "Datasheet", "[]", "t").unwrap();
        let b = s.insert_inbox(&nr("b", "C:\\DL\\b.pdf", 0)).unwrap();
        s.mark_filed(b, "D:\\b.pdf", "t", "move", "Datasheet", "[]", "t").unwrap();
        let c = s.insert_inbox(&nr("c", "C:\\DL\\c.pdf", 0)).unwrap();
        s.mark_filed(c, "D:\\c.pdf", "t", "move", "Receipt", "[]", "t").unwrap();
        let cat = s.group_count_filed("category").unwrap();
        assert_eq!(cat.iter().find(|(k, _)| k == "Datasheet").unwrap().1, 2);
        assert_eq!(cat.iter().find(|(k, _)| k == "Receipt").unwrap().1, 1);
        // inbox-only record not counted
        let _ = s.insert_inbox(&nr("d", "C:\\DL\\d.pdf", 0)).unwrap();
        assert_eq!(s.group_count_filed("category").unwrap().iter().map(|(_, n)| n).sum::<usize>(), 3);
    }

    #[test]
    fn migration_adds_columns_to_old_schema() {
        let f = tempfile::Builder::new().suffix(".db").tempfile().unwrap().into_temp_path();
        {
            let conn = Connection::open(&f).unwrap();
            conn.execute_batch(
                "CREATE TABLE records (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    sha256 TEXT NOT NULL, original_path TEXT NOT NULL,
                    original_filename TEXT NOT NULL, size_bytes INTEGER NOT NULL,
                    detected_at TEXT NOT NULL, status TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT '', sub_meta TEXT NOT NULL DEFAULT '{}',
                    rule_id TEXT NOT NULL DEFAULT '', suggested_dest TEXT NOT NULL DEFAULT '',
                    suggested_filename TEXT NOT NULL DEFAULT '', filed_path TEXT NOT NULL DEFAULT '',
                    filed_at TEXT NOT NULL DEFAULT '', action TEXT NOT NULL DEFAULT '',
                    tags TEXT NOT NULL DEFAULT '[]', error TEXT NOT NULL DEFAULT ''
                );",
            ).unwrap();
            conn.execute(
                "INSERT INTO records (sha256, original_path, original_filename, size_bytes, detected_at, status)
                 VALUES ('abc', 'C:\\DL\\x.pdf', 'x.pdf', 1, 't', 'inbox')", [],
            ).unwrap();
        }
        let s = Store::open(&f).unwrap();
        let r = s.list_inbox().unwrap().into_iter().next().unwrap();
        assert_eq!(r.sha256, "abc");
        assert_eq!(r.duplicate_of, 0);
        assert_eq!(r.dedup_decision, "");
        assert_eq!(r.last_opened_at, "");
        assert_eq!(r.last_reviewed_at, "");
        assert_eq!(r.file_mtime_at_filed, "");
    }
}
