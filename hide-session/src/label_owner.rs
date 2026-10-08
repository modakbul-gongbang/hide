//! Proven transcript ownership for labels. Other session discovery callers
//! retain their existing cwd fallback policy.

use crate::{Agent, FileIdentity, SESSION_INCREMENT_READ_LIMIT_BYTES, SESSION_LINE_LIMIT_BYTES};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// A compact, path-free token that consumers can compare with Herdr's current
/// native reference. Unsupported or unreported references prove nothing.
pub fn label_reference_token(provider: &str, kind: &str, value: &str) -> Option<String> {
    let provider = Agent::from_kind(provider)?.as_str();
    if !matches!(kind, "id" | "path")
        || value.trim().is_empty()
        || value.chars().any(char::is_control)
    {
        return None;
    }
    let mut digest = Sha256::new();
    for part in [provider, kind, value] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    Some(format!("v1:{:x}", digest.finalize()))
}

/// The durable owner uses the provider's native id, established by transcript
/// metadata. Thus an id and a path can name the same owner without trusting cwd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedLabelSession {
    pub owner: String,
    pub incarnation: String,
    pub bytes: u64,
}

/// Read at most one incremental-read budget of metadata from a regular file.
/// A native id must agree with the file; a path must contain a provider id.
/// Errors contain stable reason codes, never a transcript or personal path.
pub fn confirm_label_session(
    agent: Agent,
    path: &Path,
    reported_id: Option<&str>,
) -> Result<ConfirmedLabelSession> {
    if !std::fs::metadata(path)
        .map_err(|_| anyhow!("label_session_file_unavailable"))?
        .is_file()
    {
        return Err(anyhow!("label_session_not_regular"));
    }
    let file = File::open(path).map_err(|_| anyhow!("label_session_file_unavailable"))?;
    let metadata = file
        .metadata()
        .map_err(|_| anyhow!("label_session_stat_failed"))?;
    if !metadata.is_file() {
        return Err(anyhow!("label_session_not_regular"));
    }
    let mut reader = BufReader::new(file.take(SESSION_INCREMENT_READ_LIMIT_BYTES));
    let mut native_id = None;
    loop {
        let mut line = Vec::new();
        Read::by_ref(&mut reader)
            .take((SESSION_LINE_LIMIT_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| anyhow!("label_session_metadata_read_failed"))?;
        if line.len() > SESSION_LINE_LIMIT_BYTES {
            return Err(anyhow!("label_session_metadata_line_capacity"));
        }
        if line.last() != Some(&b'\n') {
            break;
        }
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(&line) else {
            continue;
        };
        let id = match agent {
            Agent::Codex if record["type"] == "session_meta" => record["payload"]["id"].as_str(),
            Agent::Claude if matches!(record["type"].as_str(), Some("user" | "assistant")) => {
                record["sessionId"].as_str()
            }
            _ => None,
        };
        if let Some(id) = id {
            native_id = Some(id.to_owned());
            break;
        }
    }
    let id = native_id.ok_or_else(|| anyhow!("label_session_metadata_unconfirmed"))?;
    if reported_id.is_some_and(|reported| reported != id) {
        return Err(anyhow!("label_session_id_mismatch"));
    }
    let owner = label_reference_token(agent.as_str(), "id", &id)
        .ok_or_else(|| anyhow!("label_session_id_invalid"))?;
    let physical = FileIdentity::from_metadata(&metadata);
    Ok(ConfirmedLabelSession {
        owner,
        incarnation: format!("{}:{}", physical.first, physical.second),
        bytes: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn only_provider_metadata_proves_id_and_path_equivalence() {
        for agent in [Agent::Codex, Agent::Claude] {
            let root = tempdir().unwrap();
            let path = root.path().join("session.jsonl");
            let metadata = match agent {
                Agent::Codex => {
                    serde_json::json!({"type":"session_meta","payload":{"id":"native-a"}})
                }
                Agent::Claude => {
                    serde_json::json!({"type":"user","sessionId":"native-a","message":{"role":"user","content":"content must not identify the owner"}})
                }
                Agent::OpenCode => unreachable!("OpenCode keeps no session file"),
            };
            fs::write(&path, format!("{metadata}\n")).unwrap();
            let id = confirm_label_session(agent, &path, Some("native-a")).unwrap();
            let by_path = confirm_label_session(agent, &path, None).unwrap();
            assert_eq!(id.owner, by_path.owner);
            assert_eq!(id.owner.len(), 67);
            assert!(!id.owner.contains("native-a"));
            assert_eq!(
                confirm_label_session(agent, &path, Some("native-b"))
                    .unwrap_err()
                    .to_string(),
                "label_session_id_mismatch"
            );
            fs::write(
                &path,
                "{\"type\":\"ai-title\",\"sessionId\":\"native-a\",\"title\":\"old label\"}\n",
            )
            .unwrap();
            assert!(confirm_label_session(agent, &path, None).is_err());
        }
        assert_ne!(
            label_reference_token("codex", "id", "same"),
            label_reference_token("claude", "id", "same")
        );
        for (provider, kind, value) in [
            ("other", "id", "a"),
            ("codex", "opaque", "a"),
            ("codex", "id", ""),
            ("codex", "path", "bad\npath"),
        ] {
            assert!(label_reference_token(provider, kind, value).is_none());
        }
    }

    #[test]
    fn file_replacement_and_truncation_are_visible_without_changing_a_proven_owner() {
        let root = tempdir().unwrap();
        let path = root.path().join("session.jsonl");
        let header = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"native-a\"}}\n";
        fs::write(&path, format!("{header}{}\n", "x".repeat(200))).unwrap();
        let before = confirm_label_session(Agent::Codex, &path, None).unwrap();
        fs::write(&path, header).unwrap();
        let truncated = confirm_label_session(Agent::Codex, &path, None).unwrap();
        assert_eq!(before.owner, truncated.owner);
        assert_eq!(before.incarnation, truncated.incarnation);
        assert!(truncated.bytes < before.bytes);
        let replacement = root.path().join("replacement");
        fs::write(&replacement, header).unwrap();
        fs::rename(&replacement, &path).unwrap();
        let replaced = confirm_label_session(Agent::Codex, &path, None).unwrap();
        assert_eq!(before.owner, replaced.owner);
        assert_ne!(before.incarnation, replaced.incarnation);
    }

    #[test]
    fn metadata_confirmation_is_bounded_and_never_accepts_partial_lines() {
        let root = tempdir().unwrap();
        let path = root.path().join("session.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"native-a\"}}",
        )
        .unwrap();
        assert!(confirm_label_session(Agent::Codex, &path, None).is_err());
        fs::write(&path, "x".repeat(SESSION_LINE_LIMIT_BYTES + 1)).unwrap();
        assert_eq!(
            confirm_label_session(Agent::Codex, &path, None)
                .unwrap_err()
                .to_string(),
            "label_session_metadata_line_capacity"
        );
        let mut contents = "{}\n".repeat((SESSION_INCREMENT_READ_LIMIT_BYTES / 3) as usize + 1);
        contents.push_str("{\"type\":\"session_meta\",\"payload\":{\"id\":\"too-late\"}}\n");
        fs::write(&path, contents).unwrap();
        assert!(confirm_label_session(Agent::Codex, &path, None).is_err());
        assert_eq!(
            confirm_label_session(Agent::Codex, root.path(), None)
                .unwrap_err()
                .to_string(),
            "label_session_not_regular"
        );
    }
}
