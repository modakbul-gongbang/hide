//! Pi 1.0.4's recorded JSONL history, not its active model context/tree.
//! Native session-manager metadata is the authority; neither folder encoding
//! nor a filename suffix identifies a checkout or session.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::{
    ConversationEvent, DiscoveryBudget, EventKind, LineResult, RootRefusal,
    SESSION_LINE_LIMIT_BYTES, SessionError, SessionIdentity,
};

pub(crate) struct Header {
    pub id: String,
    pub cwd: PathBuf,
}

/// Routing constraint of Pi's native --session <id>: its default cwd folder
/// is searched first. Global matches prompt to fork, so they cannot wake an
/// existing conversation without an operator decision.
pub(crate) fn default_directory(home: &Path, cwd: &Path) -> PathBuf {
    let spelling = cwd.to_string_lossy();
    let spelling = spelling.strip_prefix(['/', '\\']).unwrap_or(&spelling);
    let encoded: String = spelling
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | '\\' | ':') {
                '-'
            } else {
                ch
            }
        })
        .collect();
    home.join(crate::PI_SESSIONS).join(format!("--{encoded}--"))
}

/// The home itself may have a platform alias. No component below it may be
/// a link, including .pi and the sessions root. Inspect native entry identity
/// rather than relying on canonicalization to hide internal links.
pub(crate) fn inside_root(home: &Path, path: &Path) -> std::result::Result<PathBuf, RootRefusal> {
    let canonical_home =
        hide_platform::fs::identity::canonical(home).map_err(|_| RootRefusal::Unreadable)?;
    let relative = path
        .strip_prefix(home)
        .or_else(|_| path.strip_prefix(&canonical_home))
        .map_err(|_| RootRefusal::Outside)?;
    if !relative.starts_with(crate::PI_SESSIONS) || relative == Path::new(crate::PI_SESSIONS) {
        return Err(RootRefusal::Outside);
    }
    checked_path(home, path)
}

pub(crate) fn root(home: &Path) -> std::result::Result<PathBuf, RootRefusal> {
    checked_path(home, &home.join(crate::PI_SESSIONS))
}

fn checked_path(home: &Path, path: &Path) -> std::result::Result<PathBuf, RootRefusal> {
    let canonical_home =
        hide_platform::fs::identity::canonical(home).map_err(|_| RootRefusal::Unreadable)?;
    let relative = path
        .strip_prefix(home)
        .or_else(|_| path.strip_prefix(&canonical_home))
        .map_err(|_| RootRefusal::Outside)?;
    if !relative.starts_with(crate::PI_SESSIONS) {
        return Err(RootRefusal::Outside);
    }
    let mut checked = canonical_home;
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(RootRefusal::Outside);
        };
        checked.push(part);
        let native = hide_platform::fs::identity::file_id_nofollow(&checked).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RootRefusal::Missing
            } else {
                RootRefusal::Unreadable
            }
        })?;
        let followed =
            hide_platform::fs::identity::file_id(&checked).map_err(|_| RootRefusal::Unreadable)?;
        if native != followed {
            return Err(RootRefusal::Outside);
        }
    }
    hide_platform::fs::identity::canonical(&checked).map_err(|_| RootRefusal::Unreadable)
}

pub(crate) fn header(path: &Path) -> Result<Header> {
    let mut remaining = u64::MAX;
    header_budgeted(path, &mut remaining)
}

fn header_budgeted(path: &Path, remaining: &mut u64) -> Result<Header> {
    let file =
        crate::open_session_file(path).map_err(|_| anyhow!("label_session_file_unavailable"))?;
    let limit = ((SESSION_LINE_LIMIT_BYTES + 1) as u64).min(remaining.saturating_add(1));
    let mut reader = BufReader::new(file.take(limit));
    let mut bytes = Vec::new();
    reader
        .read_until(b'\n', &mut bytes)
        .map_err(|_| anyhow!("label_session_metadata_read_failed"))?;
    if bytes.len() as u64 > *remaining {
        return Err(anyhow!("session_discovery_read_capacity"));
    }
    *remaining -= bytes.len() as u64;
    if bytes.len() > SESSION_LINE_LIMIT_BYTES {
        return Err(anyhow!("label_session_metadata_line_capacity"));
    }
    if bytes.last() != Some(&b'\n') {
        return Err(anyhow!("label_session_metadata_unconfirmed"));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("label_session_metadata_unconfirmed"))?;
    if value["type"] != "session" || value["version"] != 3 {
        return Err(anyhow!("label_session_metadata_unconfirmed"));
    }
    let id = value["id"]
        .as_str()
        .filter(|id| crate::label_owner::valid_native_id(id))
        .ok_or_else(|| anyhow!("label_session_id_invalid"))?;
    let cwd = value["cwd"]
        .as_str()
        .filter(|cwd| !cwd.chars().any(char::is_control))
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute())
        .ok_or_else(|| anyhow!("label_session_cwd_unconfirmed"))?;
    Ok(Header {
        id: id.to_owned(),
        cwd,
    })
}

pub(crate) fn locate(
    home: &Path,
    identity: Option<&SessionIdentity>,
    cwd: Option<&str>,
    budget: &mut DiscoveryBudget,
) -> crate::Result<PathBuf> {
    let cwd = cwd.ok_or(SessionError::CwdUnavailable)?;
    if let Some(SessionIdentity::Path(path)) = identity {
        crate::confirm_session_file(home, crate::Agent::Pi, path, None, Some(cwd))
            .map_err(|error| SessionError::Checkpoint(error.to_string()))?;
        return Ok(path.clone());
    }
    let reported_id = match identity {
        Some(SessionIdentity::Id(id)) if crate::label_owner::valid_native_id(id) => {
            Some(id.as_str())
        }
        Some(_) => return Err(SessionError::SessionFileMissing),
        None => None,
    };
    if self::root(home).is_err() {
        return Err(SessionError::SessionFileMissing);
    }
    let mut candidates = Vec::new();
    let expected_cwd = hide_platform::fs::identity::canonical(Path::new(cwd))
        .map_err(|_| SessionError::CwdUnavailable)?;
    let directory = default_directory(home, &expected_cwd);
    if inside_root(home, &directory).is_err() {
        return Err(SessionError::SessionFileMissing);
    }
    let mut remaining = crate::SESSION_INCREMENT_READ_LIMIT_BYTES;
    for path in crate::jsonl_files(&directory, budget)? {
        if inside_root(home, &path).is_err() {
            continue;
        }
        let header = match header_budgeted(&path, &mut remaining) {
            Ok(header) => header,
            Err(error) if error.to_string() == "session_discovery_read_capacity" => {
                return Err(SessionError::Capacity {
                    resource: "discovery_read_bytes",
                    limit: crate::SESSION_INCREMENT_READ_LIMIT_BYTES,
                });
            }
            Err(_) => continue,
        };
        if reported_id.is_some_and(|id| header.id != id)
            || !hide_platform::fs::identity::canonical(&header.cwd)
                .is_ok_and(|native| native == expected_cwd)
        {
            continue;
        }
        if let Some(modified) = fs::metadata(&path).and_then(|m| m.modified()).ok() {
            candidates.push((modified, path));
        }
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let path = candidates
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .ok_or(SessionError::SessionFileMissing)?;
    crate::confirm_session_file(home, crate::Agent::Pi, &path, reported_id, Some(cwd))
        .map_err(|error| SessionError::Checkpoint(error.to_string()))?;
    Ok(path)
}

pub(crate) fn parse_line(item: &Value) -> LineResult {
    match item["type"].as_str() {
        Some("session_info") => {
            return item["name"].as_str().map_or(LineResult::Ignore, |name| {
                LineResult::CustomTitle(name.to_owned())
            });
        }
        Some("message") => (),
        // Compaction, branch summaries, extension custom messages and context
        // edits are not human turns. Pi's export retains the raw history.
        _ => return LineResult::Ignore,
    }
    let message = &item["message"];
    let role = message["role"].as_str().unwrap_or("");
    let at = match crate::timestamp_ms(item.get("timestamp").or_else(|| message.get("timestamp"))) {
        Ok(at) => at,
        Err(reason) => return LineResult::Skip(reason),
    };
    if role == "toolResult" || role == "bashExecution" {
        let text = if role == "bashExecution" {
            message["output"].as_str().map(str::to_owned)
        } else {
            crate::session_text(message.get("content"))
        };
        return text.map_or(LineResult::Ignore, |text| {
            LineResult::Sightings(crate::sightings_in(&[&text], at))
        });
    }
    let kind = match role {
        "user" => EventKind::Human,
        "assistant" if message["stopReason"] == "aborted" => EventKind::Interrupted,
        "assistant" => EventKind::Assistant,
        "custom" | "system" | "compactionSummary" | "branchSummary" => EventKind::Injected,
        _ => return LineResult::Ignore,
    };
    let images = message["content"].as_array().map_or(0, |blocks| {
        blocks
            .iter()
            .filter(|block| block["type"] == "image")
            .count() as u32
    });
    let text = crate::session_text(message.get("content")).unwrap_or_default();
    if text.is_empty() && images == 0 && kind != EventKind::Interrupted {
        return LineResult::Ignore;
    }
    let kind = if kind == EventKind::Human && crate::has_injected_prefix(&text) {
        EventKind::Injected
    } else {
        kind
    };
    LineResult::Event(
        ConversationEvent::new(
            if role == "assistant" {
                "assistant"
            } else {
                "user"
            },
            kind,
            at,
            text,
        )
        .with_images(images)
        .with_provider_injected(kind == EventKind::Injected),
    )
}
