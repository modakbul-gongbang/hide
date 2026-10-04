//! Native session activity without conversation projection.
//!
//! The helper and local watcher use the same root confinement and provider
//! ownership proof as labels. Only file modification time and size cross
//! the boundary; paths, native IDs and conversation records never do.

use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::{Agent, FileIdentity, confirm_label_session, label_transcript};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionActivityRequest {
    pub agent: Agent,
    pub reference_kind: String,
    pub reference_value: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionActivity {
    pub modified_at_unix_ms: u64,
    pub bytes: u64,
}

pub fn read(home: &Path, request: &SessionActivityRequest) -> Result<SessionActivity, String> {
    let (path, before) = label_transcript::locate_confirmed(
        home,
        request.agent,
        &request.reference_kind,
        &request.reference_value,
        request.cwd.as_deref(),
    )?;
    let metadata =
        std::fs::metadata(&path).map_err(|_| "session_activity_stat_failed".to_owned())?;
    let identity = FileIdentity::from_metadata(&metadata);
    let sampled_incarnation = format!("{}:{}", identity.first, identity.second);
    let modified_at_unix_ms = metadata
        .modified()
        .map_err(|_| "session_activity_mtime_unavailable".to_owned())?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| "session_activity_mtime_invalid".to_owned())?
        .as_millis()
        .try_into()
        .map_err(|_| "session_activity_mtime_invalid".to_owned())?;
    let reported_id = (request.reference_kind == "id").then_some(request.reference_value.as_str());
    let after = confirm_label_session(request.agent, &path, reported_id)
        .map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || before.owner != after.owner
        || before.incarnation != sampled_incarnation
        || after.incarnation != sampled_incarnation
        || metadata.len() < before.bytes
        || after.bytes < metadata.len()
    {
        return Err("session_activity_read_changed".to_owned());
    }
    Ok(SessionActivity {
        modified_at_unix_ms,
        bytes: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn transcript(home: &Path, agent: Agent) -> std::path::PathBuf {
        let root = match agent {
            Agent::Claude => home.join(".claude/projects/project"),
            Agent::Codex => home.join(".codex/sessions/2026/01/01"),
            Agent::OpenCode => unreachable!("OpenCode has no transcript file"),
        };
        fs::create_dir_all(&root).unwrap();
        let path = root.join("native-a.jsonl");
        let header = match agent {
            Agent::Claude => serde_json::json!({"type":"user","sessionId":"native-a"}),
            Agent::Codex => serde_json::json!({"type":"session_meta","payload":{"id":"native-a"}}),
            Agent::OpenCode => unreachable!("OpenCode has no transcript file"),
        };
        // Conversation records are deliberately invalid: activity must not
        // parse or project them after establishing the native owner.
        fs::write(
            &path,
            format!("{header}\nprivate conversation is not JSON\n"),
        )
        .unwrap();
        path
    }

    fn request(path: &Path, agent: Agent) -> SessionActivityRequest {
        SessionActivityRequest {
            agent,
            reference_kind: "path".to_owned(),
            reference_value: path.to_string_lossy().into_owned(),
            cwd: None,
        }
    }

    #[test]
    fn both_providers_return_only_mtime_and_size_without_parsing_conversation() {
        for agent in [Agent::Claude, Agent::Codex] {
            let home = tempfile::tempdir().unwrap();
            let path = transcript(home.path(), agent);
            let result = read(home.path(), &request(&path, agent)).unwrap();
            assert_eq!(result.bytes, fs::metadata(&path).unwrap().len());
            assert!(result.modified_at_unix_ms > 0);
            let value = serde_json::to_value(result).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 2);
            assert!(value.get("modified_at_unix_ms").is_some());
            assert!(value.get("bytes").is_some());
            assert!(!value.to_string().contains("native-a"));
            assert!(!value.to_string().contains("private conversation"));
        }
    }

    #[test]
    fn missing_unsupported_and_outside_references_return_path_free_errors() {
        let home = tempfile::tempdir().unwrap();
        let path = transcript(home.path(), Agent::Codex);
        let mut input = request(&path, Agent::Codex);
        input.reference_kind = "opaque".to_owned();
        assert_eq!(
            read(home.path(), &input).unwrap_err(),
            "session_kind_unsupported"
        );
        input.reference_kind = "path".to_owned();
        input.reference_value.clear();
        assert_eq!(
            read(home.path(), &input).unwrap_err(),
            "label_session_reference_missing"
        );
        let outside = home.path().join("outside.jsonl");
        fs::copy(&path, &outside).unwrap();
        assert_eq!(
            read(home.path(), &request(&outside, Agent::Codex)).unwrap_err(),
            "label_session_outside_roots"
        );
        fs::remove_file(&path).unwrap();
        let error = read(home.path(), &request(&path, Agent::Codex)).unwrap_err();
        assert!(!error.contains(&home.path().to_string_lossy().to_string()));
        assert!(!error.contains("native-a"));
    }

    #[test]
    fn id_lookup_counts_skipped_entries_across_directories_and_refuses_capacity() {
        let home = tempfile::tempdir().unwrap();
        // Neither directory exceeds the established catalog entry limit on
        // its own. Ignored non-JSONL files must count across the whole lookup.
        for day in ["01", "02"] {
            let directory = home.path().join(format!(".codex/sessions/2026/01/{day}"));
            fs::create_dir_all(&directory).unwrap();
            for entry in 0..=crate::SESSION_DISCOVERY_LIMIT / 2 {
                fs::write(directory.join(format!("skipped-{entry}.txt")), b"").unwrap();
            }
        }
        let input = SessionActivityRequest {
            agent: Agent::Codex,
            reference_kind: "id".into(),
            reference_value: "missing-native".into(),
            cwd: None,
        };
        assert_eq!(read(home.path(), &input).unwrap_err(), "session_capacity");
        assert!(
            home.path()
                .join(".codex/sessions/2026/01/01/skipped-0.txt")
                .exists()
        );
    }

    #[test]
    fn below_capacity_id_lookup_preserves_reported_native_owner_and_fresh_activity() {
        for agent in [Agent::Claude, Agent::Codex] {
            let home = tempfile::tempdir().unwrap();
            let path = transcript(home.path(), agent);
            fs::write(path.parent().unwrap().join("ignored.txt"), b"ignored").unwrap();
            let input = SessionActivityRequest {
                agent,
                reference_kind: "id".into(),
                reference_value: "native-a".into(),
                cwd: None,
            };
            let before = read(home.path(), &input).unwrap();
            use std::io::Write;
            fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(b"new private bytes\n")
                .unwrap();
            let after = read(home.path(), &input).unwrap();
            assert!(after.bytes > before.bytes);
            assert!(after.modified_at_unix_ms >= before.modified_at_unix_ms);
            assert_eq!(after.bytes, fs::metadata(&path).unwrap().len());
            assert!(!serde_json::to_string(&after).unwrap().contains("private"));
        }
    }

    #[test]
    fn a_reference_without_native_owner_is_not_activity() {
        let home = tempfile::tempdir().unwrap();
        let path = transcript(home.path(), Agent::Codex);
        fs::write(&path, "{\"type\":\"event_msg\",\"payload\":{}}\n").unwrap();
        assert_eq!(
            read(home.path(), &request(&path, Agent::Codex)).unwrap_err(),
            "label_session_metadata_unconfirmed"
        );
    }
}
