//! The work ledger shares the link writer, version, size cap and retention.

use super::{LinkStore, SessionKey, failed};
use crate::labels::analysis::LabelEnd;
use crate::links::ledger::*;
use crate::links::{CREATED_AFTER_MS, CREATED_BEFORE_MS, SUMMARY_SESSION_LIMIT};
use rusqlite::{OptionalExtension, Transaction, params};
use std::collections::BTreeSet;

pub(super) const SCHEMA: &str = "
CREATE TABLE ledger_requests(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 offset INTEGER NOT NULL, at INTEGER NOT NULL, text TEXT NOT NULL, author TEXT NOT NULL,
 chapter TEXT, task_ids TEXT NOT NULL DEFAULT '[]', judged INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(device,agent,id,offset));
CREATE INDEX ledger_requests_time ON ledger_requests(device,agent,id,at);
CREATE TABLE ledger_chapters(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 chapter TEXT NOT NULL, goal TEXT NOT NULL, at INTEGER NOT NULL,
 PRIMARY KEY(device,agent,id,chapter));
CREATE TABLE ledger_tasks(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 task TEXT NOT NULL, chapter TEXT NOT NULL, name TEXT NOT NULL, at INTEGER NOT NULL,
 last_at INTEGER NOT NULL, end TEXT NOT NULL, PRIMARY KEY(device,agent,id,task));
CREATE TABLE ledger_missed(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 missed TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL, status TEXT NOT NULL,
 at INTEGER NOT NULL, turn_offset INTEGER NOT NULL, issue_repo TEXT, issue_number INTEGER,
 PRIMARY KEY(device,agent,id,missed));
CREATE TABLE ledger_turns(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 offset INTEGER NOT NULL, start_at INTEGER NOT NULL, end_at INTEGER NOT NULL,
 end TEXT NOT NULL, task_ids TEXT NOT NULL, output_task TEXT,
 PRIMARY KEY(device,agent,id,offset));
CREATE TABLE ledger_outputs(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 kind TEXT NOT NULL, repo TEXT NOT NULL, number INTEGER NOT NULL, title TEXT NOT NULL,
 url TEXT NOT NULL, state TEXT NOT NULL, created_at INTEGER NOT NULL,
 sighted_at INTEGER NOT NULL, task TEXT, PRIMARY KEY(device,agent,id,kind,repo,number));
CREATE TABLE ledger_issue_attempts(device TEXT NOT NULL, agent TEXT NOT NULL, id TEXT NOT NULL,
 missed TEXT NOT NULL, repo TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
 started_at INTEGER NOT NULL, state TEXT NOT NULL, number INTEGER, url TEXT, failure TEXT,
 PRIMARY KEY(device,agent,id,missed));
";

fn encoded(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|_| "ledger_value_invalid".to_owned())
}

fn decoded<T: serde::de::DeserializeOwned>(value: String) -> Result<T, String> {
    serde_json::from_str(&value).map_err(|_| "ledger_record_invalid".to_owned())
}

fn identity(key: &SessionKey) -> SessionIdentity {
    SessionIdentity {
        device_id: key.device.clone(),
        agent: key.agent.clone(),
        session_id: key.id.clone(),
    }
}

fn key(session: &SessionIdentity) -> SessionKey {
    SessionKey {
        device: session.device_id.clone(),
        agent: session.agent.clone(),
        id: session.session_id.clone(),
    }
}

fn cap(
    tx: &Transaction<'_>,
    session: &SessionIdentity,
    table: &str,
    limit: usize,
) -> Result<(), String> {
    let count: usize = tx
        .query_row(
            &format!("SELECT count(*) FROM {table} WHERE device=?1 AND agent=?2 AND id=?3"),
            params![session.device_id, session.agent, session.session_id],
            |row| row.get(0),
        )
        .map_err(failed)?;
    if count > limit {
        tx.execute(
            &format!(
                "DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} \
                WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at,rowid LIMIT ?4)"
            ),
            params![
                session.device_id,
                session.agent,
                session.session_id,
                (count - limit) as i64
            ],
        )
        .map_err(failed)?;
        crate::diagnostic!(serde_json::json!({
            "component":"links", "kind":"ledger.capacity", "table":table,
            "device_id":session.device_id, "agent":session.agent,
            "session_id":session.session_id, "removed":count-limit,
        }));
    }
    Ok(())
}

impl LinkStore {
    pub fn apply_ledger_update(&mut self, update: &LedgerUpdate) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(failed)?;
        match update {
            LedgerUpdate::Requests {
                session,
                cwd,
                path,
                requests,
            } => {
                if requests.len() > REQUEST_LIMIT {
                    return Err("ledger_request_capacity".to_owned());
                }
                tx.execute("INSERT INTO sessions(device,agent,id,path,cwd) VALUES(?1,?2,?3,?4,?5) \
                    ON CONFLICT(device,agent,id) DO UPDATE SET cwd=COALESCE(excluded.cwd,cwd),path=COALESCE(excluded.path,path)",
                    params![session.device_id,session.agent,session.session_id,path,cwd]).map_err(failed)?;
                for request in requests {
                    let text = hide_session::links::cut(&request.text);
                    tx.execute(
                        "INSERT INTO ledger_requests(device,agent,id,offset,at,text,author) \
                        VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(device,agent,id,offset) DO UPDATE \
                        SET text=excluded.text,author=excluded.author,at=excluded.at",
                        params![
                            session.device_id,
                            session.agent,
                            session.session_id,
                            request.offset as i64,
                            request.at_unix_ms as i64,
                            text,
                            encoded(&request.author)?
                        ],
                    )
                    .map_err(failed)?;
                }
                cap(&tx, session, "ledger_requests", REQUEST_LIMIT)?;
            }
            LedgerUpdate::Chapter {
                session,
                goal,
                at_unix_ms,
            } => {
                let previous: Option<String> = tx
                    .query_row(
                        "SELECT goal FROM ledger_chapters \
                    WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at DESC,rowid DESC LIMIT 1",
                        params![session.device_id, session.agent, session.session_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(failed)?;
                if previous.as_deref() != Some(goal) {
                    tx.execute(
                        "INSERT OR IGNORE INTO ledger_chapters VALUES(?1,?2,?3,?4,?5,?6)",
                        params![
                            session.device_id,
                            session.agent,
                            session.session_id,
                            format!("chapter-{at_unix_ms}"),
                            goal,
                            *at_unix_ms as i64
                        ],
                    )
                    .map_err(failed)?;
                    cap(&tx, session, "ledger_chapters", TASK_LIMIT)?;
                }
            }
            LedgerUpdate::TurnEnd {
                session,
                offset,
                at_unix_ms,
                end,
                work_author,
                judgment,
            } => {
                let exists: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM ledger_turns \
                    WHERE device=?1 AND agent=?2 AND id=?3 AND offset=?4)",
                        params![
                            session.device_id,
                            session.agent,
                            session.session_id,
                            *offset as i64
                        ],
                        |row| row.get(0),
                    )
                    .map_err(failed)?;
                if exists {
                    return Ok(());
                }
                if judgment.missed.len() > NEW_MISSED_LIMIT
                    || judgment.resolved.len() > MISSED_LIMIT
                {
                    return Err("ledger_judgment_capacity".to_owned());
                }
                let chapter: Option<String> = tx
                    .query_row(
                        "SELECT chapter FROM ledger_chapters \
                    WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at DESC,rowid DESC LIMIT 1",
                        params![session.device_id, session.agent, session.session_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(failed)?;
                let start: Option<u64> = tx
                    .query_row(
                        "SELECT at FROM ledger_requests \
                    WHERE device=?1 AND agent=?2 AND id=?3 AND offset=?4",
                        params![
                            session.device_id,
                            session.agent,
                            session.session_id,
                            *offset as i64
                        ],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(failed)?;
                let mut tasks = Vec::new();
                if *work_author {
                    match &judgment.task {
                        TaskDecision::New { name } => {
                            let chapter = chapter.as_deref().ok_or("ledger_chapter_missing")?;
                            if name.trim().is_empty() || name.chars().count() > MISSED_CHARS {
                                return Err("ledger_task_invalid".to_owned());
                            }
                            let task = format!("task-{offset}");
                            tx.execute(
                                "INSERT INTO ledger_tasks VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                                params![
                                    session.device_id,
                                    session.agent,
                                    session.session_id,
                                    task,
                                    chapter,
                                    name,
                                    start.unwrap_or(*at_unix_ms) as i64,
                                    *at_unix_ms as i64,
                                    encoded(end)?
                                ],
                            )
                            .map_err(failed)?;
                            tasks.push(task);
                        }
                        TaskDecision::Continue { ids } => {
                            if ids.is_empty() || ids.len() > CONTINUED_TASK_LIMIT {
                                return Err("ledger_task_capacity".to_owned());
                            }
                            for task in ids {
                                let changed = tx
                                    .execute(
                                        "UPDATE ledger_tasks SET end=?5,last_at=?6 \
                                    WHERE device=?1 AND agent=?2 AND id=?3 AND task=?4",
                                        params![
                                            session.device_id,
                                            session.agent,
                                            session.session_id,
                                            task,
                                            encoded(end)?,
                                            *at_unix_ms as i64
                                        ],
                                    )
                                    .map_err(failed)?;
                                if changed == 0 {
                                    return Err("ledger_task_unknown".to_owned());
                                }
                                if !tasks.contains(task) {
                                    tasks.push(task.clone());
                                }
                            }
                        }
                        TaskDecision::None => {}
                    }
                }
                let output_task = if tasks.len() == 1 {
                    tasks.first().cloned()
                } else {
                    judgment
                        .output_task_id
                        .as_ref()
                        .filter(|id| tasks.contains(id))
                        .cloned()
                };
                tx.execute(
                    "INSERT INTO ledger_turns VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        session.device_id,
                        session.agent,
                        session.session_id,
                        *offset as i64,
                        start.unwrap_or(*at_unix_ms) as i64,
                        *at_unix_ms as i64,
                        encoded(end)?,
                        encoded(&tasks)?,
                        output_task
                    ],
                )
                .map_err(failed)?;
                tx.execute(
                    "UPDATE ledger_requests SET judged=1,chapter=?5,task_ids=?6 \
                    WHERE device=?1 AND agent=?2 AND id=?3 AND offset=?4",
                    params![
                        session.device_id,
                        session.agent,
                        session.session_id,
                        *offset as i64,
                        chapter,
                        encoded(&tasks)?
                    ],
                )
                .map_err(failed)?;
                for (index, missed) in judgment.missed.iter().enumerate() {
                    if missed.text.trim().is_empty() || missed.text.chars().count() > MISSED_CHARS {
                        return Err("ledger_missed_invalid".to_owned());
                    }
                    tx.execute("INSERT INTO ledger_missed(device,agent,id,missed,kind,text,status,at,turn_offset) \
                        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![session.device_id,session.agent,session.session_id,format!("missed-{offset}-{index}"),
                            encoded(&missed.kind)?,missed.text,encoded(&MissedStatus::Open)?,*at_unix_ms as i64,*offset as i64]).map_err(failed)?;
                }
                for resolved in &judgment.resolved {
                    // A printed existing issue is insufficient: only this
                    // session's verified creation can close as an issue.
                    let created: Option<String> = match resolved.issue_number {
                        Some(number) => tx
                            .query_row(
                                "SELECT repo FROM ledger_outputs WHERE device=?1 AND agent=?2 \
                            AND id=?3 AND kind='issue' AND number=?4 LIMIT 1",
                                params![
                                    session.device_id,
                                    session.agent,
                                    session.session_id,
                                    number as i64
                                ],
                                |row| row.get(0),
                            )
                            .optional()
                            .map_err(failed)?,
                        None => None,
                    };
                    let status = if created.is_some() {
                        MissedStatus::Issue
                    } else {
                        MissedStatus::Resolved
                    };
                    tx.execute(
                        "UPDATE ledger_missed SET status=?5,issue_repo=?6,issue_number=?7 \
                        WHERE device=?1 AND agent=?2 AND id=?3 AND missed=?4 AND status=?8",
                        params![
                            session.device_id,
                            session.agent,
                            session.session_id,
                            resolved.id,
                            encoded(&status)?,
                            created,
                            created
                                .as_ref()
                                .and(resolved.issue_number)
                                .map(|n| n as i64),
                            encoded(&MissedStatus::Open)?
                        ],
                    )
                    .map_err(failed)?;
                }
                cap(&tx, session, "ledger_tasks", TASK_LIMIT)?;
                cap(&tx, session, "ledger_missed", MISSED_LIMIT)?;
                // Turns use their end timestamp for bounded retention.
                tx.execute("DELETE FROM ledger_turns WHERE rowid IN (SELECT rowid FROM ledger_turns \
                    WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY end_at DESC LIMIT -1 OFFSET ?4)",
                    params![session.device_id,session.agent,session.session_id,REQUEST_LIMIT as i64]).map_err(failed)?;
                assign_outputs(&tx, session)?;
            }
            LedgerUpdate::MissedStatus {
                session,
                id,
                status,
            } => {
                if !matches!(status, MissedStatus::Open | MissedStatus::Dismissed) {
                    return Err("ledger_status_invalid".to_owned());
                }
                let changed=tx.execute("UPDATE ledger_missed SET status=?5,issue_repo=NULL,issue_number=NULL \
                    WHERE device=?1 AND agent=?2 AND id=?3 AND missed=?4 AND status!=?6 \
                    AND NOT EXISTS(SELECT 1 FROM ledger_issue_attempts a WHERE a.device=?1 AND a.agent=?2 \
                    AND a.id=?3 AND a.missed=?4 AND a.state='creating')",
                    params![session.device_id,session.agent,session.session_id,id,encoded(status)?,encoded(&MissedStatus::Issue)?]).map_err(failed)?;
                if changed == 0 {
                    return Err("ledger_missed_unavailable".to_owned());
                }
            }
        }
        tx.commit().map_err(failed)
    }

    pub fn ledger_record(&self, session: &SessionIdentity) -> Result<SessionRecord, String> {
        let mut visited = BTreeSet::new();
        self.record_tree(session, &mut visited, 0)
    }

    fn record_tree(
        &self,
        session: &SessionIdentity,
        visited: &mut BTreeSet<SessionIdentity>,
        depth: usize,
    ) -> Result<SessionRecord, String> {
        if depth > 32 || visited.len() >= SUMMARY_SESSION_LIMIT {
            return Err("ledger_descendant_capacity".to_owned());
        }
        if !visited.insert(session.clone()) {
            return Err("ledger_parent_cycle".to_owned());
        }
        let row = self
            .session(&key(session))?
            .ok_or("ledger_session_missing")?;
        let args = params![session.device_id, session.agent, session.session_id];
        let mut record = SessionRecord {
            session: session.clone(),
            cwd: row.cwd,
            path: row.path,
            started_at_unix_ms: row.started_at,
            ended_at_unix_ms: row.ended_at,
            ..SessionRecord::default()
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT chapter,goal,at FROM ledger_chapters \
            WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at",
            )
            .map_err(failed)?;
        record.chapters = statement
            .query_map(args, |r| {
                Ok(WorkChapter {
                    id: r.get(0)?,
                    goal: r.get(1)?,
                    at_unix_ms: r.get(2)?,
                })
            })
            .map_err(failed)?
            .collect::<Result<_, _>>()
            .map_err(failed)?;
        record.title = record
            .chapters
            .last()
            .map(|chapter| chapter.goal.clone())
            .unwrap_or_else(|| session.session_id.clone());
        let mut statement=self.connection.prepare("SELECT offset,at,text,author,task_ids,chapter,judged \
            FROM ledger_requests WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at DESC,offset DESC LIMIT ?4").map_err(failed)?;
        let requests = statement
            .query_map(
                params![
                    session.device_id,
                    session.agent,
                    session.session_id,
                    REQUEST_LIMIT as i64
                ],
                |r| {
                    Ok((
                        r.get::<_, u64>(0)?,
                        r.get::<_, u64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, bool>(6)?,
                    ))
                },
            )
            .map_err(failed)?;
        for request in requests {
            let (offset, at, text, author, tasks, chapter, judged) = request.map_err(failed)?;
            record.requests.push(RecordedRequest {
                offset,
                at_unix_ms: at,
                text,
                author: decoded(author)?,
                task_ids: decoded(tasks)?,
                chapter_id: chapter,
                judged,
            });
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT task,chapter,name,at,last_at,end FROM ledger_tasks \
            WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at DESC",
            )
            .map_err(failed)?;
        let tasks = statement
            .query_map(
                params![session.device_id, session.agent, session.session_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, u64>(3)?,
                        r.get::<_, u64>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                },
            )
            .map_err(failed)?;
        for task in tasks {
            let (id, chapter, name, at, last, end) = task.map_err(failed)?;
            record.tasks.push(WorkTask {
                id,
                chapter_id: chapter,
                name,
                at_unix_ms: at,
                last_at_unix_ms: last,
                end: decoded(end)?,
                done: false,
            });
        }
        let mut statement=self.connection.prepare("SELECT missed,kind,text,status,at,turn_offset,issue_repo,issue_number, \
            EXISTS(SELECT 1 FROM ledger_issue_attempts a WHERE a.device=m.device AND a.agent=m.agent AND a.id=m.id \
            AND a.missed=m.missed AND a.state='creating') FROM ledger_missed m WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY at DESC").map_err(failed)?;
        let missed = statement
            .query_map(
                params![session.device_id, session.agent, session.session_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, u64>(4)?,
                        r.get::<_, u64>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<u64>>(7)?,
                        r.get::<_, bool>(8)?,
                    ))
                },
            )
            .map_err(failed)?;
        for missed in missed {
            let (id, kind, text, status, at, offset, repo, number, creating) =
                missed.map_err(failed)?;
            record.missed.push(MissedWork {
                id,
                kind: decoded(kind)?,
                text,
                status: decoded(status)?,
                at_unix_ms: at,
                turn_offset: offset,
                issue_repository: repo,
                issue_number: number,
                creating,
            });
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT kind,repo,number,title,url,state,created_at,task \
            FROM ledger_outputs WHERE device=?1 AND agent=?2 AND id=?3 ORDER BY created_at DESC",
            )
            .map_err(failed)?;
        record.outputs = statement
            .query_map(
                params![session.device_id, session.agent, session.session_id],
                |r| {
                    Ok(WorkOutput {
                        kind: r.get(0)?,
                        repository: r.get(1)?,
                        number: r.get(2)?,
                        title: r.get(3)?,
                        url: r.get(4)?,
                        state: r.get(5)?,
                        created_at_unix_ms: r.get(6)?,
                        task_id: r.get(7)?,
                        source: session.clone(),
                        source_name: record.title.clone(),
                    })
                },
            )
            .map_err(failed)?
            .collect::<Result<_, _>>()
            .map_err(failed)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT agent,id FROM session_parents \
            WHERE device=?1 AND parent_agent=?2 AND parent_id=?3 LIMIT ?4",
            )
            .map_err(failed)?;
        let children = statement
            .query_map(
                params![
                    session.device_id,
                    session.agent,
                    session.session_id,
                    SUMMARY_SESSION_LIMIT as i64
                ],
                |r| {
                    Ok(SessionIdentity {
                        device_id: session.device_id.clone(),
                        agent: r.get(0)?,
                        session_id: r.get(1)?,
                    })
                },
            )
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?;
        for child in children {
            record
                .children
                .push(self.record_tree(&child, visited, depth + 1)?);
        }
        for task in &mut record.tasks {
            task.done = task.end == LabelEnd::Done
                && record
                    .outputs
                    .iter()
                    .filter(|o| o.kind == "pr" && o.task_id.as_deref() == Some(&task.id))
                    .all(|o| o.state == "merged");
        }
        Ok(record)
    }

    pub fn ledger_context(&self, session: &SessionIdentity) -> Result<LedgerContext, String> {
        let record = self.ledger_record(session)?;
        Ok(LedgerContext {
            open_tasks: record.tasks.into_iter().filter(|task| !task.done).collect(),
            open_missed: record
                .missed
                .into_iter()
                .filter(|missed| missed.status == MissedStatus::Open)
                .collect(),
        })
    }

    pub fn ledger_summaries(&self, project: &str) -> Result<Vec<SessionWorkSummary>, String> {
        let project = self.project(project)?.ok_or("ledger_project_missing")?;
        let paths = self.project_paths(&project)?;
        let mut summaries = Vec::new();
        for session in self
            .all_sessions()?
            .into_iter()
            .filter(|session| session.inside(&paths))
            .take(SUMMARY_SESSION_LIMIT)
        {
            let record = self.ledger_record(&identity(&session.key))?;
            fn count(record: &SessionRecord) -> usize {
                record
                    .missed
                    .iter()
                    .filter(|missed| missed.status == MissedStatus::Open)
                    .count()
                    + record.children.iter().map(count).sum::<usize>()
            }
            summaries.push(SessionWorkSummary {
                session: record.session.clone(),
                title: record.title.clone(),
                cwd: record.cwd.clone(),
                ended_at_unix_ms: record.ended_at_unix_ms,
                open_missed: record
                    .missed
                    .iter()
                    .filter(|missed| missed.status == MissedStatus::Open)
                    .count(),
                subtree_open_missed: count(&record),
            });
        }
        Ok(summaries)
    }

    pub fn ledger_search(
        &self,
        query: &str,
        summaries: bool,
    ) -> Result<Vec<SessionSearchHit>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if query.chars().count() > 200 {
            return Err("ledger_query_capacity".to_owned());
        }
        let pattern = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let sql = if summaries {
            "SELECT s.device,s.agent,s.id,COALESCE((SELECT goal FROM ledger_chapters c WHERE c.device=s.device AND c.agent=s.agent AND c.id=s.id ORDER BY at DESC LIMIT 1),s.id),r.text,r.at,s.cwd \
            FROM sessions s JOIN (SELECT device,agent,id,text,at FROM ledger_requests UNION ALL SELECT device,agent,id,name,at FROM ledger_tasks UNION ALL SELECT device,agent,id,goal,at FROM ledger_chapters) r \
            ON s.device=r.device AND s.agent=r.agent AND s.id=r.id WHERE r.text LIKE ?1 ESCAPE '\\' ORDER BY r.at DESC LIMIT ?2"
        } else {
            "SELECT s.device,s.agent,s.id,s.id,r.text,r.at,s.cwd FROM sessions s JOIN ledger_requests r \
            ON s.device=r.device AND s.agent=r.agent AND s.id=r.id WHERE r.text LIKE ?1 ESCAPE '\\' ORDER BY r.at DESC LIMIT ?2"
        };
        let mut statement = self.connection.prepare(sql).map_err(failed)?;
        statement
            .query_map(params![pattern, SEARCH_LIMIT as i64], |r| {
                Ok(SessionSearchHit {
                    session: SessionIdentity {
                        device_id: r.get(0)?,
                        agent: r.get(1)?,
                        session_id: r.get(2)?,
                    },
                    title: r.get(3)?,
                    matched: r.get(4)?,
                    at_unix_ms: r.get(5)?,
                    cwd: r.get(6)?,
                })
            })
            .map_err(failed)?
            .collect::<Result<_, _>>()
            .map_err(failed)
    }

    /// Apply verified PR creation sightings after each project refresh.
    pub fn refresh_ledger_outputs(&mut self) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(failed)?;
        tx.execute("INSERT INTO ledger_outputs(device,agent,id,kind,repo,number,title,url,state,created_at,sighted_at) \
            SELECT s.device,s.agent,s.id,'pr',p.repo,p.number,p.title,p.url,CASE WHEN p.merged_at IS NOT NULL THEN 'merged' \
            WHEN p.closed_at IS NOT NULL THEN 'closed' ELSE 'open' END,p.created_at,s.at \
            FROM session_prs s JOIN repo_names n ON n.name=s.repo_name JOIN prs p ON p.repo=n.repo AND p.number=s.number \
            WHERE p.created_at IS NOT NULL AND s.at>=p.created_at-?1 AND s.at<=p.created_at+?2 \
            ON CONFLICT(device,agent,id,kind,repo,number) DO UPDATE SET title=excluded.title,url=excluded.url,state=excluded.state",
            params![CREATED_BEFORE_MS as i64,CREATED_AFTER_MS as i64]).map_err(failed)?;
        tx.execute("UPDATE ledger_outputs SET task=(SELECT output_task FROM ledger_turns t WHERE t.device=ledger_outputs.device \
            AND t.agent=ledger_outputs.agent AND t.id=ledger_outputs.id AND ledger_outputs.sighted_at BETWEEN t.start_at AND t.end_at \
            ORDER BY t.start_at DESC LIMIT 1) WHERE kind='pr'",[]).map_err(failed)?;
        tx.commit().map_err(failed)
    }
}

fn assign_outputs(tx: &Transaction<'_>, session: &SessionIdentity) -> Result<(), String> {
    tx.execute("UPDATE ledger_outputs SET task=(SELECT output_task FROM ledger_turns t WHERE t.device=?1 \
        AND t.agent=?2 AND t.id=?3 AND ledger_outputs.sighted_at BETWEEN t.start_at AND t.end_at ORDER BY t.start_at DESC LIMIT 1) \
        WHERE device=?1 AND agent=?2 AND id=?3",params![session.device_id,session.agent,session.session_id]).map_err(failed)?;
    Ok(())
}

#[cfg(test)]
#[path = "ledger_store_tests.rs"]
mod tests;
