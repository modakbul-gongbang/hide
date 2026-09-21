use crate::{
    Agent, ConversationEvent, EventKind, SESSION_READ_LIMIT_BYTES, parse_events, read_bounded,
};
use hide_project::ProjectIdentity;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard cap on files visited in one catalog refresh.
pub const SESSION_DISCOVERY_LIMIT: usize = 10_000;
const FIRST_LINE_LIMIT_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionFilter {
    #[default]
    All,
    Codex,
    Claude,
}

impl SessionFilter {
    pub fn includes(self, agent: Agent) -> bool {
        matches!(self, Self::All)
            || matches!(
                (self, agent),
                (Self::Codex, Agent::Codex) | (Self::Claude, Agent::Claude)
            )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAvailability {
    Available,
    Unavailable { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectSession {
    pub id: String,
    pub agent: Agent,
    pub locator: PathBuf,
    pub checkout_path: PathBuf,
    pub first_human_request: Option<String>,
    pub started_at_unix_ms: Option<u64>,
    pub updated_at_unix_ms: u64,
    pub title: Option<String>,
    pub event_count: usize,
    pub availability: SessionAvailability,
}

#[derive(Debug)]
pub enum SessionCatalogError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Capacity {
        limit: usize,
    },
}

impl Display for SessionCatalogError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => {
                write!(
                    formatter,
                    "session_catalog_{operation}:{}:{source}",
                    path.display()
                )
            }
            Self::Capacity { limit } => write!(formatter, "session_catalog_capacity:{limit}"),
        }
    }
}

impl Error for SessionCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Capacity { .. } => None,
        }
    }
}

pub struct SessionCatalog {
    home: PathBuf,
    device_id: String,
}

impl SessionCatalog {
    pub fn new(home: &Path, device_id: impl Into<String>) -> Self {
        Self {
            home: home.to_path_buf(),
            device_id: device_id.into(),
        }
    }

    pub fn project_sessions(
        &self,
        project: &ProjectIdentity,
    ) -> Result<Vec<ProjectSession>, SessionCatalogError> {
        let mut files = Vec::new();
        let mut visited = 0;
        collect_jsonl(
            &self.home.join(".claude/projects"),
            Agent::Claude,
            2,
            &mut files,
            &mut visited,
            SESSION_DISCOVERY_LIMIT,
        )?;
        collect_jsonl(
            &self.home.join(".codex/sessions"),
            Agent::Codex,
            4,
            &mut files,
            &mut visited,
            SESSION_DISCOVERY_LIMIT,
        )?;

        let mut sessions = Vec::new();
        for (agent, path) in files {
            let Some(cwd) = session_cwd(agent, &path) else {
                continue;
            };
            let Ok(identity) = hide_project::resolve(&cwd, &self.device_id) else {
                continue;
            };
            if identity.id != project.id {
                continue;
            }
            sessions.push(read_project_session(agent, path, cwd));
        }
        sessions.sort_by(|left, right| {
            right
                .updated_at_unix_ms
                .cmp(&left.updated_at_unix_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(sessions)
    }

    pub fn filtered(
        sessions: &[ProjectSession],
        filter: SessionFilter,
        query: &str,
    ) -> Vec<ProjectSession> {
        let query = query.trim().to_lowercase();
        sessions
            .iter()
            .filter(|session| filter.includes(session.agent))
            .filter(|session| {
                query.is_empty()
                    || session
                        .first_human_request
                        .as_ref()
                        .is_some_and(|value| value.to_lowercase().contains(&query))
                    || session
                        .title
                        .as_ref()
                        .is_some_and(|value| value.to_lowercase().contains(&query))
                    || session
                        .checkout_path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect()
    }
}

fn collect_jsonl(
    root: &Path,
    agent: Agent,
    depth: usize,
    output: &mut Vec<(Agent, PathBuf)>,
    visited: &mut usize,
    limit: usize,
) -> Result<(), SessionCatalogError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(SessionCatalogError::Io {
                operation: "read_directory",
                path: root.to_path_buf(),
                source,
            });
        }
    };
    for entry in entries {
        *visited = visited.saturating_add(1);
        if *visited > limit {
            return Err(SessionCatalogError::Capacity { limit });
        }
        let path = entry
            .map_err(|source| SessionCatalogError::Io {
                operation: "read_entry",
                path: root.to_path_buf(),
                source,
            })?
            .path();
        if path.is_dir() && depth > 0 {
            collect_jsonl(&path, agent, depth - 1, output, visited, limit)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            output.push((agent, path));
        }
    }
    Ok(())
}

fn session_cwd(agent: Agent, path: &Path) -> Option<PathBuf> {
    match agent {
        Agent::Codex => crate::codex_session_cwd(path).map(PathBuf::from),
        Agent::Claude => {
            let file = File::open(path).ok()?;
            let mut reader = BufReader::new(file);
            for _ in 0..128 {
                let line = match read_bounded_line(&mut reader, FIRST_LINE_LIMIT_BYTES as usize) {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(_) => return None,
                };
                let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
                    return Some(PathBuf::from(cwd));
                }
            }
            None
        }
    }
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    maximum_bytes: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::with_capacity(maximum_bytes.min(8 * 1024));
    let mut limited = std::io::Read::take(&mut *reader, maximum_bytes.saturating_add(1) as u64);
    let read = limited.read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() > maximum_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session line exceeds its fixed limit",
        ));
    }
    while line
        .last()
        .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
    {
        line.pop();
    }
    Ok(Some(line))
}

fn read_project_session(agent: Agent, path: PathBuf, cwd: PathBuf) -> ProjectSession {
    let metadata = fs::metadata(&path).ok();
    let updated_at_unix_ms = metadata
        .as_ref()
        .and_then(|value| value.modified().ok())
        .and_then(system_time_ms)
        .unwrap_or_default();
    let id = session_id(agent, &path);
    let (parsed, unavailable) = match metadata {
        Some(metadata) if metadata.len() > SESSION_READ_LIMIT_BYTES => {
            (None, Some("session_too_large".to_owned()))
        }
        Some(_) => match read_bounded(&path, SESSION_READ_LIMIT_BYTES) {
            Ok(contents) => {
                let parsed = parse_events(agent, &contents);
                let unavailable = (parsed.events.is_empty() && parsed.skipped_lines > 0)
                    .then(|| "session_malformed".to_owned());
                (Some(parsed), unavailable)
            }
            Err(error) => (None, Some(format!("session_unreadable:{error}"))),
        },
        None => (None, Some("session_missing".to_owned())),
    };
    let events = parsed
        .as_ref()
        .map(|value| value.events.as_slice())
        .unwrap_or_default();
    ProjectSession {
        id,
        agent,
        locator: path,
        checkout_path: cwd,
        first_human_request: first_human(events).map(compact_snippet),
        started_at_unix_ms: events.first().map(|event| event.at_unix_ms),
        updated_at_unix_ms: events
            .last()
            .map(|event| event.at_unix_ms)
            .unwrap_or(updated_at_unix_ms),
        title: parsed.as_ref().and_then(|value| value.title.clone()),
        event_count: events.len(),
        availability: unavailable.map_or(SessionAvailability::Available, |reason| {
            SessionAvailability::Unavailable { reason }
        }),
    }
}

fn first_human(events: &[ConversationEvent]) -> Option<&str> {
    events
        .iter()
        .find(|event| event.kind == EventKind::Human)
        .map(|event| event.text.as_str())
}

fn compact_snippet(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(160).collect()
}

fn session_id(agent: Agent, path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");
    if agent == Agent::Claude {
        return stem.to_owned();
    }
    let explicit = File::open(path).ok().and_then(|file| {
        let mut reader = BufReader::new(file);
        let line = read_bounded_line(&mut reader, FIRST_LINE_LIMIT_BYTES as usize).ok()??;
        let value: Value = serde_json::from_slice(&line).ok()?;
        value
            .pointer("/payload/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    explicit.unwrap_or_else(|| {
        let digest = Sha256::digest(path.to_string_lossy().as_bytes());
        format!("codex:{:x}", digest)
    })
}

fn system_time_ms(value: SystemTime) -> Option<u64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn mixed_provider_catalog_folds_linked_worktrees_and_keeps_order_through_filters() {
        let root = tempdir().unwrap();
        let home = root.path().join("home");
        let project_root = root.path().join("project");
        fs::create_dir_all(home.join(".claude/projects/p")).unwrap();
        fs::create_dir_all(home.join(".codex/sessions/2026/09/21")).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        fs::write(
            home.join(".claude/projects/p/claude-1.jsonl"),
            format!("{{\"type\":\"user\",\"cwd\":{},\"timestamp\":\"2026-09-21T01:00:00Z\",\"origin\":{{\"kind\":\"human\"}},\"message\":{{\"content\":\"alpha request\"}}}}\n", serde_json::to_string(&project_root).unwrap()),
        ).unwrap();
        fs::write(
            home.join(".codex/sessions/2026/09/21/rollout.jsonl"),
            format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"codex-1\",\"cwd\":{}}}}}\n{{\"type\":\"response_item\",\"timestamp\":\"2026-09-21T02:00:00Z\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"beta request\"}}]}}}}\n", serde_json::to_string(&project_root).unwrap()),
        ).unwrap();
        let catalog = SessionCatalog::new(&home, "local");
        let sessions = catalog.project_sessions(&project).unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["codex-1", "claude-1"]
        );
        assert_eq!(
            SessionCatalog::filtered(&sessions, SessionFilter::Claude, "alpha").len(),
            1
        );
        assert!(SessionCatalog::filtered(&sessions, SessionFilter::Codex, "alpha").is_empty());
    }

    #[test]
    fn claude_catalog_skips_a_malformed_line_before_the_cwd_record() {
        let root = tempdir().unwrap();
        let home = root.path().join("home");
        let project_root = root.path().join("project");
        let sessions_root = home.join(".claude/projects/p");
        fs::create_dir_all(&sessions_root).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        fs::write(
            sessions_root.join("claude-malformed.jsonl"),
            format!(
                "not-json\n{{\"type\":\"user\",\"cwd\":{},\"timestamp\":\"2026-09-21T01:00:00Z\",\"origin\":{{\"kind\":\"human\"}},\"message\":{{\"content\":\"recover me\"}}}}\n",
                serde_json::to_string(&project_root).unwrap()
            ),
        )
        .unwrap();

        let sessions = SessionCatalog::new(&home, "local")
            .project_sessions(&project)
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].first_human_request.as_deref(),
            Some("recover me")
        );
    }

    #[test]
    fn discovery_capacity_counts_every_directory_entry_not_only_sessions() {
        let root = tempdir().unwrap();
        for index in 0..4 {
            fs::write(
                root.path().join(format!("unrelated-{index}.txt")),
                "fixture",
            )
            .unwrap();
        }
        let mut output = Vec::new();
        let mut visited = 0;

        assert!(matches!(
            collect_jsonl(root.path(), Agent::Claude, 0, &mut output, &mut visited, 3,),
            Err(SessionCatalogError::Capacity { limit: 3 })
        ));
        assert!(output.is_empty());
    }

    #[test]
    fn bounded_line_reader_refuses_before_allocating_past_the_limit() {
        let mut reader = BufReader::new(std::io::Cursor::new(b"123456789\nnext\n"));
        let error = read_bounded_line(&mut reader, 8).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
