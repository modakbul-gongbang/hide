//! Local copied conversation search. No provider or Memory dependency.
//! One caller owns this connection; messages and cursor commit together.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub const FILE_LIMIT: usize = 2_000;
const MESSAGE_LIMIT: usize = 25_000;
const BODY_LIMIT: usize = 64 * 1024;
const HIT_LIMIT: usize = 100;
/// The most stamps one search asks for: a page reads at most 101 rows.
pub const STAMP_LIMIT: usize = 128;

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

/// What the index holds for one session file: the conversation cursor, the
/// file's stamp and the prefix witness, each as the reader wrote it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedFile {
    pub cursor: String,
    pub stamp: String,
    pub witness: String,
}
/// One human or assistant message read from a session file, at the byte
/// offset of the record that held it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedMessage {
    pub offset: u64,
    pub role: String,
    pub at_unix_ms: u64,
    pub text: String,
}
/// One bounded read of a session file on its node, for [`SearchIndex::apply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum IndexStep {
    /// Nothing past the saved cursor.
    Done,
    /// Only the prefix witness moved.
    Witness { witness: String, more: bool },
    /// The saved prefix no longer matches: the session's rows go and it is
    /// read again from the start.
    Reset,
    /// Messages past the cursor, committed with the file's new state; with
    /// `reset` the session's older rows go first.
    Read {
        reset: bool,
        messages: Vec<IndexedMessage>,
        cursor: String,
        stamp: String,
        witness: String,
        more: bool,
    },
}
pub struct SearchIndex {
    db: Connection,
}
/// The current stamp of each asked session file, in order; `None` for one
/// that is gone or cannot be read.
pub type CurrentStamps<'a> = dyn FnMut(&[String]) -> Result<Vec<Option<String>>, String> + 'a;
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
        Ok(Self { db })
    }
    /// Moves every row of each `(old, new)` Project to its new id, in one
    /// transaction (PRD core-host-node D-23). A row whose new key is already
    /// taken was written under the new id since, so the newer row stays and
    /// the old one is dropped: a stale index row (rebuilt from the session
    /// files) or an older Copied history setting. A dropped message leaves
    /// the full-text index with it. Returns the rows moved and dropped.
    /// An index with nothing under an old id is only read, so a converted
    /// install never takes the write lock again.
    pub fn rekey_projects(&mut self, pairs: &[(String, String)]) -> Result<(usize, usize), String> {
        let mut pending = false;
        for (old, _) in pairs {
            for table in ["policy", "control_outcomes", "files", "messages"] {
                pending |= self
                    .db
                    .query_row(
                        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE project=?1)"),
                        [old],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        if !pending {
            return Ok((0, 0));
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let (mut moved, mut dropped) = (0, 0);
        for (old, new) in pairs {
            for table in ["policy", "control_outcomes", "files", "messages"] {
                moved += tx
                    .execute(
                        &format!("UPDATE OR IGNORE {table} SET project=?1 WHERE project=?2"),
                        params![new, old],
                    )
                    .map_err(|e| e.to_string())?;
            }
            for table in ["policy", "control_outcomes", "files", "messages"] {
                dropped += tx
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE project=?1"),
                        [old],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|e| e.to_string())? as usize;
            }
            erase(&tx, old, None, None)?;
            for table in ["policy", "control_outcomes", "files"] {
                tx.execute(&format!("DELETE FROM {table} WHERE project=?1"), [old])
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok((moved, dropped))
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
    /// What the index holds for `session`, which its node reads on from.
    pub fn saved(&self, project: &str, session: &str) -> Result<Option<SavedFile>, String> {
        self.db
            .query_row(
                "SELECT cursor,stamp,witness FROM files WHERE project=?1 AND session=?2",
                params![project, session],
                |r| {
                    Ok(SavedFile {
                        cursor: r.get(0)?,
                        stamp: r.get(1)?,
                        witness: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    /// Writes one read of the session file at `path` (`search_read`),
    /// keeping messages from `cutoff` on. True means the file has more.
    pub fn apply(
        &mut self,
        project: &str,
        session: &str,
        path: &str,
        cutoff: u64,
        step: IndexStep,
    ) -> Result<bool, String> {
        self.budget();
        match step {
            IndexStep::Done => Ok(false),
            IndexStep::Reset => {
                self.remove(project, Some(session))?;
                Ok(true)
            }
            IndexStep::Witness { witness, more } => {
                self.db
                    .execute(
                        "UPDATE files SET witness=?3 WHERE project=?1 AND session=?2",
                        params![project, session, witness],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(more)
            }
            IndexStep::Read {
                reset,
                messages,
                cursor,
                stamp,
                witness,
                more,
            } => {
                let tx = self.db.transaction().map_err(|e| e.to_string())?;
                if reset {
                    erase(&tx, project, Some(session), None)?;
                }
                let count: usize = tx
                    .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                let mut added = 0;
                for message in messages {
                    if message.at_unix_ms < cutoff {
                        continue;
                    }
                    if message.text.len() > BODY_LIMIT || count + added >= MESSAGE_LIMIT {
                        return Err("Search index capacity reached (25,000 messages, 64 KiB per message). Reduce retention or clear the index.".into());
                    }
                    let folded = message.text.to_lowercase();
                    tx.execute("INSERT OR IGNORE INTO messages(project,session,offset,role,at,body,folded) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![project,session,message.offset,message.role,message.at_unix_ms,message.text,folded]).map_err(|e| e.to_string())?;
                    if tx.changes() > 0 {
                        tx.execute(
                            "INSERT INTO grams(rowid,terms) VALUES(?1,?2)",
                            params![tx.last_insert_rowid(), grams(&folded)],
                        )
                        .map_err(|e| e.to_string())?;
                        added += 1;
                    }
                }
                tx.execute("INSERT INTO files VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(project,session) DO UPDATE SET path=excluded.path,cursor=excluded.cursor,stamp=excluded.stamp,witness=excluded.witness", params![project,session,path,cursor,stamp,witness]).map_err(|e| e.to_string())?;
                tx.commit().map_err(|e| e.to_string())?;
                Ok(more)
            }
        }
    }
    /// The newest matching message of each session. A hit whose file's
    /// stamp moved since it was indexed is dropped and marks the page stale;
    /// `stamps` answers the files' current stamps, `None` for one that is
    /// gone or unreadable, from the node that holds them.
    pub fn search_scoped(
        &self,
        project: &str,
        query: &str,
        cutoff: u64,
        allowed: Option<&[String]>,
        stamps: &mut CurrentStamps<'_>,
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
            let rows = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let paths = rows
                .iter()
                .map(|(_, path, _)| path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let current = if paths.is_empty() {
                Vec::new()
            } else {
                stamps(&paths)?
            };
            if current.len() != paths.len() {
                return Err("Session files answered another number of stamps than asked.".into());
            }
            let current = paths.into_iter().zip(current).collect::<BTreeMap<_, _>>();
            let mut page = SearchPage::default();
            let mut seen = BTreeSet::new();
            let count = rows.len();
            for (mut hit, path, indexed_stamp) in rows {
                if current.get(&path).cloned().flatten().as_ref() != Some(&indexed_stamp) {
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
