//! Proven transcript ownership for labels. Other session discovery callers
//! retain their existing cwd fallback policy.

use crate::{Agent, FileIdentity, SESSION_INCREMENT_READ_LIMIT_BYTES, SESSION_LINE_LIMIT_BYTES};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    /// Native metadata, never the reported path. Older helpers omit it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_id"
    )]
    pub native_session_id: Option<String>,
    pub incarnation: String,
    pub bytes: u64,
}

fn deserialize_native_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    let id = Option::<String>::deserialize(deserializer)?;
    if id.as_deref().is_some_and(|id| !valid_native_id(id)) {
        return Err(serde::de::Error::custom("label_session_id_invalid"));
    }
    Ok(id)
}

/// Bounded native ID syntax. This validates spelling, never ownership.
pub fn valid_native_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= crate::turns::NATIVE_ID_LIMIT_BYTES
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Read at most one incremental-read budget of metadata from a regular file.
/// A native id must agree with the file; a path must contain a provider id.
/// Errors contain stable reason codes, never a transcript or personal path.
pub fn confirm_label_session(
    agent: Agent,
    path: &Path,
    reported_id: Option<&str>,
) -> Result<ConfirmedLabelSession> {
    confirm_metadata(agent, path, reported_id, None, None)
}

/// Shared file proof for consumers, including idle/paged reads. Pi additionally
/// requires its exact checkout and refuses every linked entry under home.
pub fn confirm_session_file(
    home: &Path,
    agent: Agent,
    path: &Path,
    reported_id: Option<&str>,
    cwd: Option<&str>,
) -> Result<ConfirmedLabelSession> {
    crate::inside_session_root(home, &[agent], path)
        .map_err(|_| anyhow!("label_session_outside_roots"))?;
    confirm_metadata(agent, path, reported_id, cwd, Some(home))
}

fn confirm_metadata(
    agent: Agent,
    path: &Path,
    reported_id: Option<&str>,
    cwd: Option<&str>,
    home: Option<&Path>,
) -> Result<ConfirmedLabelSession> {
    if !std::fs::metadata(path)
        .map_err(|_| anyhow!("label_session_file_unavailable"))?
        .is_file()
    {
        return Err(anyhow!("label_session_not_regular"));
    }
    let file =
        crate::open_session_file(path).map_err(|_| anyhow!("label_session_file_unavailable"))?;
    let metadata = file
        .metadata()
        .map_err(|_| anyhow!("label_session_stat_failed"))?;
    if !metadata.is_file() {
        return Err(anyhow!("label_session_not_regular"));
    }
    if agent == Agent::Pi
        && hide_platform::fs::identity::link_count(&file)
            .map_err(|_| anyhow!("label_session_stat_failed"))?
            != 1
    {
        return Err(anyhow!("label_session_linked"));
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
            if agent == Agent::Pi {
                return Err(anyhow!("label_session_metadata_unconfirmed"));
            }
            continue;
        };
        if agent == Agent::Pi {
            if record["type"] != "session" || record["version"] != 3 {
                return Err(anyhow!("label_session_metadata_unconfirmed"));
            }
            let expected = cwd.ok_or_else(|| anyhow!("label_session_cwd_unconfirmed"))?;
            let native = record["cwd"]
                .as_str()
                .ok_or_else(|| anyhow!("label_session_cwd_unconfirmed"))?;
            if !Path::new(native).is_absolute() || native.chars().any(char::is_control) {
                return Err(anyhow!("label_session_cwd_unconfirmed"));
            }
            let expected = hide_platform::fs::identity::canonical(Path::new(expected))
                .map_err(|_| anyhow!("label_session_cwd_unconfirmed"))?;
            let native = hide_platform::fs::identity::canonical(Path::new(native))
                .map_err(|_| anyhow!("label_session_cwd_unconfirmed"))?;
            if native != expected {
                return Err(anyhow!("label_session_cwd_mismatch"));
            }
            let home = home.ok_or_else(|| anyhow!("label_session_outside_roots"))?;
            let native_directory = crate::pi::default_directory(home, &native);
            let actual_directory = path
                .parent()
                .and_then(|parent| hide_platform::fs::identity::canonical(parent).ok());
            if actual_directory.is_none()
                || hide_platform::fs::identity::canonical(&native_directory).ok()
                    != actual_directory
            {
                return Err(anyhow!("label_session_default_directory_required"));
            }
            if record["id"].as_str().is_none() {
                return Err(anyhow!("label_session_id_invalid"));
            }
        }
        let id = match agent {
            Agent::Pi => record["id"].as_str(),
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
    if !valid_native_id(&id) {
        return Err(anyhow!("label_session_id_invalid"));
    }
    if agent == Agent::Pi && id.ends_with(".jsonl") {
        return Err(anyhow!("label_session_id_unresumable"));
    }
    if reported_id.is_some_and(|reported| reported != id) {
        return Err(anyhow!("label_session_id_mismatch"));
    }
    let owner = label_reference_token(agent.as_str(), "id", &id)
        .ok_or_else(|| anyhow!("label_session_id_invalid"))?;
    let physical = FileIdentity::from_metadata(&metadata);
    Ok(ConfirmedLabelSession {
        owner,
        native_session_id: Some(id),
        incarnation: format!("{}:{}", physical.first, physical.second),
        bytes: metadata.len(),
    })
}

/// Run a bounded archive/search read between the same native proofs. These
/// consumers discover checkout metadata from the file itself; a live pane
/// instead supplies its cwd to confirm_session_file. No proof is persisted.
pub fn read_session_file<T>(
    home: &Path,
    agent: Agent,
    path: &Path,
    read: impl FnOnce() -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    if agent != Agent::Pi {
        return read();
    }
    crate::inside_session_root(home, &[agent], path)
        .map_err(|_| "label_session_outside_roots".to_owned())?;
    let header = crate::pi::header(path).map_err(|e| e.to_string())?;
    let before = confirm_session_file(home, agent, path, Some(&header.id), header.cwd.to_str())
        .map_err(|e| e.to_string())?;
    let stamp = crate::search_read::stamp_at(path);
    let result = read()?;
    let after = confirm_session_file(home, agent, path, Some(&header.id), header.cwd.to_str())
        .map_err(|e| e.to_string())?;
    if after.owner != before.owner
        || after.incarnation != before.incarnation
        || after.bytes < before.bytes
        || (after.bytes == before.bytes && crate::search_read::stamp_at(path) != stamp)
    {
        return Err("label_session_read_changed".to_owned());
    }
    Ok(result)
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
                Agent::Pi | Agent::OpenCode => unreachable!("legacy metadata fixtures"),
            };
            fs::write(&path, format!("{metadata}\n")).unwrap();
            let id = confirm_label_session(agent, &path, Some("native-a")).unwrap();
            let by_path = confirm_label_session(agent, &path, None).unwrap();
            assert_eq!(id.owner, by_path.owner);
            assert_eq!(by_path.native_session_id.as_deref(), Some("native-a"));
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

    #[test]
    fn previous_answers_omit_native_id_and_malformed_native_ids_are_refused() {
        let old: ConfirmedLabelSession = serde_json::from_value(serde_json::json!({
            "owner":"v1:old", "incarnation":"1:2", "bytes":10
        }))
        .unwrap();
        assert!(old.native_session_id.is_none());
        for id in ["", "..", "outside/session", "bad\nvalue", &"a".repeat(257)] {
            assert!(
                serde_json::from_value::<ConfirmedLabelSession>(serde_json::json!({
                    "owner":"v1:old", "native_session_id":id, "incarnation":"1:2", "bytes":10
                }))
                .is_err()
            );
        }
    }
}
