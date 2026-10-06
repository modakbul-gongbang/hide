//! Local copied conversation search. No provider or Memory dependency.
//! One caller owns this connection; messages and cursor commit together.
use crate::{Agent, ConversationCheckpoint, ConversationCursor, EventKind};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

pub const FILE_LIMIT: usize = 2_000;
const MESSAGE_LIMIT: usize = 25_000;
const BODY_LIMIT: usize = 64 * 1024;
const HIT_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub session_id: String,
    pub source_offset: u64,
    pub role: String,
    pub at_unix_ms: u64,
    pub snippet: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub limited: bool,
    pub stale: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateReads {
    pub cursor_bytes: u64,
    pub witness_bytes: u64,
}
pub struct SearchIndex {
    db: Connection,
    reads: UpdateReads,
}
impl SearchIndex {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if !path.exists() {
            hide_platform::fs::private::create_new_file(path).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; PRAGMA max_page_count=65536;
            CREATE TABLE IF NOT EXISTS policy(project TEXT PRIMARY KEY, days INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS control_outcomes(project TEXT PRIMARY KEY, failure TEXT);
            CREATE TABLE IF NOT EXISTS files(project TEXT NOT NULL, session TEXT NOT NULL, path TEXT NOT NULL,
                cursor TEXT NOT NULL, stamp TEXT NOT NULL, witness TEXT NOT NULL, PRIMARY KEY(project,session));
            CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY, project TEXT NOT NULL, session TEXT NOT NULL,
                offset INTEGER NOT NULL, role TEXT NOT NULL, at INTEGER NOT NULL, body TEXT NOT NULL,
                folded TEXT NOT NULL, UNIQUE(project,session,offset,role));
            CREATE INDEX IF NOT EXISTS messages_scope ON messages(project,session);
            CREATE VIRTUAL TABLE IF NOT EXISTS grams USING fts5(terms, content='');")
            .map_err(|e| e.to_string())?;
        Ok(Self {
            db,
            reads: UpdateReads::default(),
        })
    }
    /// Moves every row of each `(old, new)` Project to its new id, in one
    /// transaction (PRD core-host-node D-23). A row whose new key is already
    /// taken keeps its old id. Returns how many rows moved.
    pub fn rekey_projects(&mut self, pairs: &[(String, String)]) -> Result<usize, String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let mut moved = 0;
        for (old, new) in pairs {
            for table in ["policy", "control_outcomes", "files", "messages"] {
                moved += tx
                    .execute(
                        &format!("UPDATE OR IGNORE {table} SET project=?1 WHERE project=?2"),
                        params![new, old],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(moved)
    }
    pub fn last_update_reads(&self) -> UpdateReads {
        self.reads
    }
    fn budget(&self) {
        let started = Instant::now();
        self.db.progress_handler(
            1000,
            Some(move || started.elapsed() > Duration::from_millis(500)),
        );
    }
    pub fn days(&self, project: &str) -> Result<u16, String> {
        self.db
            .query_row("SELECT days FROM policy WHERE project=?1", [project], |r| {
                r.get(0)
            })
            .optional()
            .map(|v| v.unwrap_or(90))
            .map_err(|e| e.to_string())
    }
    pub fn control_failure(&self, project: &str) -> Result<Option<String>, String> {
        self.db
            .query_row(
                "SELECT failure FROM control_outcomes WHERE project=?1",
                [project],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|v| v.flatten())
            .map_err(|e| e.to_string())
    }
    pub fn record_control_outcome(
        &mut self,
        project: &str,
        failure: Option<&str>,
    ) -> Result<(), String> {
        if let Some(failure) = failure {
            self.db.execute("INSERT INTO control_outcomes VALUES(?1,?2) ON CONFLICT(project) DO UPDATE SET failure=excluded.failure", params![project,failure]).map_err(|e| e.to_string())?;
        } else {
            self.db
                .execute("DELETE FROM control_outcomes WHERE project=?1", [project])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn set_days(&mut self, project: &str, days: u16) -> Result<(), String> {
        self.budget();
        if ![0, 30, 90, 365].contains(&days) {
            return Err("Choose Off, 30, 90 or 365 days.".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        erase(&tx, project, None, None)?;
        tx.execute("DELETE FROM files WHERE project=?1", [project])
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO policy VALUES(?1,?2) ON CONFLICT(project) DO UPDATE SET days=excluded.days",params![project,days]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.db.execute_batch("VACUUM").map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn clear(&mut self, project: &str) -> Result<(), String> {
        self.budget();
        self.remove(project, None)?;
        self.db.execute_batch("VACUUM").map_err(|e| e.to_string())
    }
    pub fn remove(&mut self, project: &str, session: Option<&str>) -> Result<(), String> {
        self.budget();
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        erase(&tx, project, session, None)?;
        tx.execute(
            "DELETE FROM files WHERE project=?1 AND (?2 IS NULL OR session=?2)",
            params![project, session],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Remove expired copied bodies, even when source files never change.
    pub fn prune(&mut self, project: &str, cutoff: u64) -> Result<(), String> {
        self.budget();
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        erase(&tx, project, None, Some(cutoff))?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Retention applies to every copied Project, including inactive scopes.
    pub fn prune_all(&mut self, now: u64) -> Result<(), String> {
        self.budget();
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let expired = {
            let mut stmt = tx.prepare("SELECT m.id,m.folded FROM messages m LEFT JOIN policy p ON p.project=m.project WHERE m.at < ?1 - COALESCE(p.days,90)*86400000 OR p.days=0").map_err(|e| e.to_string())?;
            let mut rows = stmt.query([now]).map_err(|e| e.to_string())?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: i64 = row.get(0).map_err(|e| e.to_string())?;
                let text: String = row.get(1).map_err(|e| e.to_string())?;
                tx.execute(
                    "INSERT INTO grams(grams,rowid,terms) VALUES('delete',?1,?2)",
                    params![id, grams(&text)],
                )
                .map_err(|e| e.to_string())?;
                ids.push(id);
            }
            ids
        };
        for id in expired {
            tx.execute("DELETE FROM messages WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    /// Reconcile membership without accepting rows from another Project.
    pub fn retain(&mut self, project: &str, sessions: &[String]) -> Result<(), String> {
        let old = {
            let mut stmt = self
                .db
                .prepare("SELECT session FROM files WHERE project=?1")
                .map_err(|e| e.to_string())?;
            stmt.query_map([project], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for session in old {
            if !sessions.contains(&session) {
                self.remove(project, Some(&session))?;
            }
        }
        Ok(())
    }
    /// At most the existing 1 MiB cursor budget per call. True means more bytes.
    pub fn update(
        &mut self,
        project: &str,
        session: &str,
        agent: Agent,
        path: &Path,
        cutoff: u64,
    ) -> Result<bool, String> {
        self.reads = UpdateReads::default();
        self.budget();
        let opened = open_regular(path)?;
        self.update_opened(project, session, agent, path, cutoff, &opened)
    }
    fn update_opened(
        &mut self,
        project: &str,
        session: &str,
        agent: Agent,
        path: &Path,
        cutoff: u64,
        opened: &File,
    ) -> Result<bool, String> {
        if opened.metadata().map_err(|e| e.to_string())?.len() > crate::SESSION_READ_LIMIT_BYTES {
            return Err("Sessions larger than 64 MiB cannot be indexed or opened here.".into());
        }
        let observed_stamp = metadata_stamp(&opened.metadata().map_err(|e| e.to_string())?)?;
        if stamp(path)? != observed_stamp {
            return Err("Session source changed before indexing.".into());
        }
        let previous = self
            .db
            .query_row(
                "SELECT cursor,stamp,witness FROM files WHERE project=?1 AND session=?2",
                params![project, session],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let mut cursor = ConversationCursor::new();
        let mut reset = false;
        let mut prefix = PrefixWitness::default();
        if let Some((serialized, saved_stamp, saved_witness)) = previous.as_ref() {
            let checkpoint: ConversationCheckpoint =
                serde_json::from_str(serialized).map_err(|e| e.to_string())?;
            prefix = serde_json::from_str(saved_witness).map_err(|e| e.to_string())?;
            if saved_stamp == &observed_stamp {
                if prefix.hashed_offset < checkpoint.offset() {
                    let end = checkpoint.offset().min(prefix.hashed_offset + HASH_BLOCK);
                    prefix
                        .hashes
                        .push(hash_block(opened, prefix.hashed_offset, end)?);
                    self.reads.witness_bytes = end - prefix.hashed_offset;
                    prefix.hashed_offset = end;
                    prefix.verified_chunks = prefix.hashes.len();
                    if stamp(path)? != observed_stamp {
                        return Err("Session changed while validating its prefix.".into());
                    }
                    self.db
                        .execute(
                            "UPDATE files SET witness=?3 WHERE project=?1 AND session=?2",
                            params![
                                project,
                                session,
                                serde_json::to_string(&prefix).map_err(|e| e.to_string())?
                            ],
                        )
                        .map_err(|e| e.to_string())?;
                    return Ok(checkpoint.has_more() || prefix.hashed_offset < checkpoint.offset());
                }
                if !checkpoint.has_more() {
                    return Ok(false);
                }
            }
            if saved_stamp != &observed_stamp {
                let saved_size = saved_stamp
                    .split(':')
                    .nth(2)
                    .and_then(|v| v.parse::<u64>().ok());
                reset = prefix.hashed_offset < checkpoint.offset()
                    || saved_size.is_none_or(|size| {
                        opened.metadata().map(|m| m.len() <= size).unwrap_or(true)
                    });
                if !reset {
                    if prefix.verified_for != observed_stamp {
                        prefix.verified_for = observed_stamp.clone();
                        prefix.verified_chunks = 0;
                    }
                    if prefix.verified_chunks < prefix.hashes.len() {
                        let n = prefix.verified_chunks;
                        let end = checkpoint.offset().min((n as u64 + 1) * HASH_BLOCK);
                        self.reads.witness_bytes = end - n as u64 * HASH_BLOCK;
                        reset = hash_block(opened, n as u64 * HASH_BLOCK, end)? != prefix.hashes[n];
                        if reset {
                            self.remove(project, Some(session))?;
                            return Ok(true);
                        }
                        if !reset {
                            if stamp(path)? != observed_stamp {
                                return Err("Session changed while validating its prefix.".into());
                            }
                            prefix.verified_chunks += 1;
                            self.db
                                .execute(
                                    "UPDATE files SET witness=?3 WHERE project=?1 AND session=?2",
                                    params![
                                        project,
                                        session,
                                        serde_json::to_string(&prefix).map_err(|e| e.to_string())?
                                    ],
                                )
                                .map_err(|e| e.to_string())?;
                            return Ok(true);
                        }
                    }
                }
            }
            if !reset {
                cursor = ConversationCursor::restore(checkpoint);
            }
        }
        if reset {
            prefix = PrefixWitness::default();
        }
        let old_offset = cursor.checkpoint().offset();
        let parsed = cursor
            .read_file(agent, path, opened)
            .map_err(|e| e.to_string())?;
        reset |= parsed.rescan_reason.is_some();
        if parsed.rescan_reason.is_some() {
            prefix = PrefixWitness::default();
        }
        let checkpoint = cursor.checkpoint();
        // Only the partial previous block and newly consumed blocks are hashed.
        let first = if reset { 0 } else { old_offset / HASH_BLOCK };
        prefix.hashes.truncate(first as usize);
        prefix.hashed_offset = first * HASH_BLOCK;
        self.reads.cursor_bytes = cursor.read_bytes();
        // Small files complete in one call while the combined transcript and
        // hash reads still fit the same 1 MiB budget. Large files stage hashes.
        let hash_bytes = checkpoint.offset().saturating_sub(prefix.hashed_offset);
        if hash_bytes
            <= crate::SESSION_INCREMENT_READ_LIMIT_BYTES.saturating_sub(self.reads.cursor_bytes)
        {
            while prefix.hashed_offset < checkpoint.offset() {
                let end = checkpoint.offset().min(prefix.hashed_offset + HASH_BLOCK);
                prefix
                    .hashes
                    .push(hash_block(opened, prefix.hashed_offset, end)?);
                self.reads.witness_bytes += end - prefix.hashed_offset;
                prefix.hashed_offset = end;
            }
        }

        prefix.verified_for = observed_stamp.clone();
        prefix.verified_chunks = prefix.hashes.len();
        let new_witness = serde_json::to_string(&prefix).map_err(|e| e.to_string())?;
        // A replacement during reading cannot commit a mixed file/cursor.
        if stamp(path)? != observed_stamp {
            return Err("Session changed while indexing; retrying on the next refresh.".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        if reset {
            erase(&tx, project, Some(session), None)?;
        }
        let count: usize = tx
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let mut added = 0;
        for (event, offset) in parsed.events.into_iter().zip(parsed.event_offsets) {
            if !matches!(event.kind, EventKind::Human | EventKind::Assistant)
                || event.at_unix_ms < cutoff
            {
                continue;
            }
            if event.text.len() > BODY_LIMIT || count + added >= MESSAGE_LIMIT {
                return Err("Search index capacity reached (25,000 messages, 64 KiB per message). Reduce retention or clear the index.".into());
            }
            let folded = event.text.to_lowercase();
            tx.execute("INSERT OR IGNORE INTO messages(project,session,offset,role,at,body,folded) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![project,session,offset,event.role,event.at_unix_ms,event.text,folded]).map_err(|e| e.to_string())?;
            if tx.changes() > 0 {
                tx.execute(
                    "INSERT INTO grams(rowid,terms) VALUES(?1,?2)",
                    params![tx.last_insert_rowid(), grams(&folded)],
                )
                .map_err(|e| e.to_string())?;
                added += 1;
            }
        }
        tx.execute("INSERT INTO files VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(project,session) DO UPDATE SET path=excluded.path,cursor=excluded.cursor,stamp=excluded.stamp,witness=excluded.witness", params![project,session,path.to_string_lossy(),serde_json::to_string(&checkpoint).map_err(|e| e.to_string())?,observed_stamp,new_witness]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(cursor.has_more() || prefix.hashed_offset < checkpoint.offset())
    }
    pub fn search(&self, project: &str, query: &str, cutoff: u64) -> Result<SearchPage, String> {
        self.search_scoped(project, query, cutoff, None)
    }
    pub fn search_scoped(
        &self,
        project: &str,
        query: &str,
        cutoff: u64,
        allowed: Option<&[String]>,
    ) -> Result<SearchPage, String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(SearchPage::default());
        }
        if query.chars().count() > 256 {
            return Err("Search up to 256 characters at a time.".into());
        }
        let expression = query_terms(&query);
        let start = Instant::now();
        self.db.progress_handler(
            1000,
            Some(move || start.elapsed() > Duration::from_millis(150)),
        );
        let result = (|| {
            let mut stmt = self.db.prepare("WITH candidates AS (SELECT m.session,m.offset,m.role,m.at,m.body,f.path,f.stamp,row_number() OVER (PARTITION BY m.session ORDER BY m.at DESC,m.offset DESC) AS rank FROM grams JOIN messages m ON m.id=grams.rowid JOIN files f ON f.project=m.project AND f.session=m.session WHERE grams MATCH ?1 AND m.project=?2 AND m.at>=?3 AND instr(m.folded,?4)>0 AND (?5 IS NULL OR m.session IN (SELECT value FROM json_each(?5)))) SELECT session,offset,role,at,body,path,stamp FROM candidates WHERE rank=1 ORDER BY at DESC LIMIT 101").map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(
                    params![
                        expression,
                        project,
                        cutoff,
                        query,
                        allowed
                            .map(serde_json::to_string)
                            .transpose()
                            .map_err(|e| e.to_string())?
                    ],
                    |r| {
                        Ok((
                            SearchHit {
                                session_id: r.get(0)?,
                                source_offset: r.get(1)?,
                                role: r.get(2)?,
                                at_unix_ms: r.get(3)?,
                                snippet: r.get(4)?,
                            },
                            r.get::<_, String>(5)?,
                            r.get::<_, String>(6)?,
                        ))
                    },
                )
                .map_err(|e| e.to_string())?;
            let mut page = SearchPage::default();
            let mut seen = BTreeSet::new();
            let mut count = 0;
            for row in rows {
                count += 1;
                let (mut hit, path, indexed_stamp) = row.map_err(|e| e.to_string())?;
                if stamp(Path::new(&path)).ok().as_ref() != Some(&indexed_stamp) {
                    page.stale = true;
                    continue;
                }
                if !seen.insert(hit.session_id.clone()) {
                    continue;
                }
                if page.hits.len() >= HIT_LIMIT {
                    page.limited = true;
                    break;
                }
                hit.snippet = snippet(&hit.snippet, &query);
                page.hits.push(hit);
            }
            page.limited |= count >= 101;
            Ok(page)
        })();
        self.db.progress_handler(0, None::<fn() -> bool>);
        result
    }
}
fn erase(
    tx: &rusqlite::Transaction<'_>,
    project: &str,
    session: Option<&str>,
    cutoff: Option<u64>,
) -> Result<(), String> {
    {
        let mut stmt = tx
            .prepare(
                "SELECT id,folded FROM messages WHERE project=?1 AND (?2 IS NULL OR session=?2) AND (?3 IS NULL OR at<?3)",
            )
            .map_err(|e| e.to_string())?;
        let entries = stmt
            .query_map(params![project, session, cutoff], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for entry in entries {
            let (id, body) = entry.map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO grams(grams,rowid,terms) VALUES('delete',?1,?2)",
                params![id, grams(&body)],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.execute(
        "DELETE FROM messages WHERE project=?1 AND (?2 IS NULL OR session=?2) AND (?3 IS NULL OR at<?3)",
        params![project, session, cutoff],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
// Hex-encoded Unicode unigrams and bigrams are safe literal FTS tokens. No
// query syntax reaches MATCH; Korean 2-character substrings have postings.
fn tokens(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut terms = BTreeSet::new();
    for c in &chars {
        terms.insert(format!("u{:x}", *c as u32));
    }
    for pair in chars.windows(2) {
        terms.insert(format!("b{:x}x{:x}", pair[0] as u32, pair[1] as u32));
    }
    terms
}
fn grams(text: &str) -> String {
    tokens(text).into_iter().collect::<Vec<_>>().join(" ")
}
fn query_terms(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() == 1 {
        return format!("u{:x}", chars[0] as u32);
    }
    chars
        .windows(2)
        .map(|p| format!("b{:x}x{:x}", p[0] as u32, p[1] as u32))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" AND ")
}
fn snippet(body: &str, query: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let folded = body.to_lowercase();
    let at = folded
        .find(query)
        .map(|i| folded[..i].chars().count())
        .unwrap_or(0)
        .min(chars.len());
    let start = at.saturating_sub(45);
    let end = (start + 180).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}
fn stamp(path: &Path) -> Result<String, String> {
    metadata_stamp(&fs::metadata(path).map_err(|e| e.to_string())?)
}
fn metadata_stamp(m: &fs::Metadata) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{}:{}:{}",
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(format!(
            "{}:{:?}",
            m.len(),
            m.modified().map_err(|e| e.to_string())?
        ))
    }
}
const HASH_BLOCK: u64 = 1024 * 1024;
#[derive(Default, Serialize, Deserialize)]
struct PrefixWitness {
    hashes: Vec<String>,
    #[serde(default)]
    hashed_offset: u64,
    verified_for: String,
    verified_chunks: usize,
}
fn hash_block(opened: &File, start: u64, end: u64) -> Result<String, String> {
    let mut file = opened.try_clone().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut bytes = [0; 64 * 1024];
    let mut reader = file.take(end.saturating_sub(start));
    loop {
        let n = reader.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Session source is not a regular file.".into());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_replaced_before_path_stamp_cannot_publish_removed_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source.jsonl");
        fs::write(&path, r#"{"type":"response_item","timestamp":"2026-10-01T00:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"removed old body"}]}}"#).unwrap();
        let opened = open_regular(&path).unwrap();
        let replacement = temp.path().join("replacement");
        fs::write(&replacement, "new source").unwrap();
        fs::rename(&replacement, &path).unwrap();
        let mut index = SearchIndex::open(&temp.path().join("index.db")).unwrap();
        assert!(
            index
                .update_opened("p", "s", Agent::Codex, &path, 0, &opened)
                .unwrap_err()
                .contains("changed before")
        );
        assert!(
            index
                .search("p", "removed old body", 0)
                .unwrap()
                .hits
                .is_empty()
        );
    }
}
