use serde::{Deserialize, Serialize};

pub const FIXTURE_PREFIX: &str = "herdr-ide-verify-";

/// Synthetic fixtures live below the canonical OS temp directory. These
/// bucket spellings come from Pi 1.0.4 and omp 18.7.0, independently of the
/// product's locator, so a wrong locator cannot move the expected fixture.
#[cfg(test)]
pub(crate) fn native_session_folder(
    home: &std::path::Path,
    kind: &str,
    cwd: &std::path::Path,
) -> std::path::PathBuf {
    let encode = |path: &std::path::Path| {
        path.to_string_lossy()
            .trim_start_matches(['/', '\\'])
            .replace(['/', '\\', ':'], "-")
    };
    let bucket = match kind {
        "pi" => format!("--{}--", encode(cwd)),
        "omp" => {
            let temporary = std::env::temp_dir().canonicalize().unwrap();
            let relative = cwd.strip_prefix(temporary).unwrap();
            if relative.as_os_str().is_empty() {
                "-tmp".into()
            } else {
                format!("-tmp-{}", encode(relative))
            }
        }
        _ => panic!("not a native-file fixture"),
    };
    home.join(format!(".{kind}/agent/sessions")).join(bucket)
}

/// A Grok 1.0.46 session folder with its id-owning summary and an empty
/// conversation, at the group Grok names by URL-encoding the cwd.
#[cfg(test)]
pub(crate) fn grok_session(
    home: &std::path::Path,
    cwd: &std::path::Path,
    id: &str,
) -> std::path::PathBuf {
    let group: String = cwd
        .to_str()
        .unwrap()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    let folder = home.join(".grok/sessions").join(group).join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("summary.json"),
        serde_json::json!({"info": {"id": id, "cwd": cwd}, "session_summary": "",
            "created_at": "2026-10-03T01:00:00Z", "updated_at": "2026-10-03T01:00:00Z",
            "num_messages": 0, "current_model_id": "grok-build"})
        .to_string(),
    )
    .unwrap();
    let conversation = folder.join("updates.jsonl");
    std::fs::write(&conversation, "").unwrap();
    conversation
}

/// The pinned Cursor CLI's ordinary route and independently encoded graph.
/// The fixture owns its writer; production readers never create these files.
#[cfg(test)]
pub(crate) fn cursor_session(
    home: &std::path::Path,
    cwd: &std::path::Path,
    id: &str,
) -> std::path::PathBuf {
    let bucket = format!("{:x}", md5::compute(cwd.to_str().unwrap().as_bytes()));
    let folder = home.join(".cursor/chats").join(bucket).join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("meta.json"),
        serde_json::json!({"schemaVersion":1,"cwd":cwd,"createdAtMs":1790989200000u64,
            "hasConversation":true,"isSubagent":false})
        .to_string(),
    )
    .unwrap();
    let graph: serde_json::Value = serde_json::from_str(include_str!(
        "../../hide-session/tests/fixtures/cursor-2026.10.01/graph.json"
    ))
    .unwrap();
    let database = folder.join("store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA user_version=1; CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE blobs(id TEXT PRIMARY KEY,data BLOB);").unwrap();
    let meta = serde_json::json!({"agentId":id,"latestRootBlobId":graph["roots"]["first"],"createdAt":1790989200000u64});
    connection
        .execute(
            "INSERT INTO meta VALUES('0',?1)",
            [hex::encode(meta.to_string())],
        )
        .unwrap();
    for (key, value) in graph["blobs"].as_object().unwrap() {
        connection
            .execute(
                "INSERT INTO blobs VALUES(?1,?2)",
                rusqlite::params![key, hex::decode(value.as_str().unwrap()).unwrap()],
            )
            .unwrap();
    }
    drop(connection);
    database
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FixturePlan {
    pub workspace_name: String,
    pub branch_name: String,
    pub pane_label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ScalePlan {
    pub schema_version: u32,
    pub ownership_prefix: String,
    pub target: String,
    pub cwd: String,
    pub workspace_names: Vec<String>,
    pub requested_workspace_count: usize,
    pub requested_pane_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FixtureWorkspaceRecord {
    pub workspace_name: String,
    pub workspace_id: String,
    pub pane_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub ownership_prefix: String,
    pub target: String,
    pub cwd: String,
    pub workspaces: Vec<FixtureWorkspaceRecord>,
}

pub fn plan(name: &str) -> Result<FixturePlan, String> {
    validate_owned_name(name)?;
    Ok(FixturePlan {
        workspace_name: name.to_owned(),
        branch_name: format!("verify/{name}"),
        pane_label: format!("{name}-long-running"),
    })
}

pub fn plan_scale(
    name: &str,
    target: &str,
    cwd: &str,
    workspace_count: usize,
    pane_count: usize,
) -> Result<ScalePlan, String> {
    validate_owned_name(name)?;
    if !matches!(target, "local" | "mini") {
        return Err("Fixture target must be local or mini".to_owned());
    }
    validate_fixture_cwd(cwd)?;
    if cwd != format!("/tmp/{name}") {
        return Err("Fixture cwd must exactly match the fixture name under /tmp".to_owned());
    }
    if workspace_count == 0 || workspace_count > 32 {
        return Err("Fixture workspace count must be between 1 and 32".to_owned());
    }
    if pane_count < workspace_count || pane_count > 64 {
        return Err(
            "Fixture pane count must be at least the workspace count and at most 64".to_owned(),
        );
    }
    let workspace_names = (1..=workspace_count)
        .map(|index| format!("{name}-w{index:02}"))
        .collect();
    Ok(ScalePlan {
        schema_version: 1,
        ownership_prefix: FIXTURE_PREFIX.to_owned(),
        target: target.to_owned(),
        cwd: cwd.to_owned(),
        workspace_names,
        requested_workspace_count: workspace_count,
        requested_pane_count: pane_count,
    })
}

pub fn validate_manifest(manifest: &FixtureManifest) -> Result<(), String> {
    if manifest.schema_version != 1 || manifest.ownership_prefix != FIXTURE_PREFIX {
        return Err("Fixture manifest ownership contract does not match".to_owned());
    }
    if !matches!(manifest.target.as_str(), "local" | "mini") {
        return Err("Fixture manifest target must be local or mini".to_owned());
    }
    validate_fixture_cwd(&manifest.cwd)?;
    if manifest.workspaces.is_empty() {
        return Err("Fixture manifest must contain at least one workspace".to_owned());
    }
    for workspace in &manifest.workspaces {
        validate_owned_name(&workspace.workspace_name)?;
        if workspace.workspace_id.trim().is_empty() || workspace.pane_ids.is_empty() {
            return Err("Fixture workspace records require ids and panes".to_owned());
        }
    }
    Ok(())
}

pub fn validate_owned_name(name: &str) -> Result<(), String> {
    if !name.starts_with(FIXTURE_PREFIX) {
        return Err(format!("Fixture names must begin with {FIXTURE_PREFIX}"));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(
            "Fixture names may contain lowercase ASCII, digits, and hyphens only".to_owned(),
        );
    }
    Ok(())
}

fn validate_fixture_cwd(cwd: &str) -> Result<(), String> {
    let Some(suffix) = cwd.strip_prefix("/tmp/herdr-ide-verify-") else {
        return Err("Fixture cwd must be an owned path under /tmp".to_owned());
    };
    if suffix.is_empty()
        || !suffix.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err("Fixture cwd may contain lowercase ASCII, digits, and hyphens only".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixed_fixture_plan_is_deterministic_and_idempotent() {
        let first = plan("herdr-ide-verify-shell").unwrap();
        let second = plan("herdr-ide-verify-shell").unwrap();
        assert_eq!(first, second);
        assert!(first.branch_name.starts_with("verify/herdr-ide-verify-"));
    }

    #[test]
    fn non_prefixed_targets_are_rejected_before_any_runtime_operation() {
        assert!(plan("user-workspace").is_err());
    }

    #[test]
    fn scale_plan_has_exact_requested_counts_and_prefixes() {
        let planned = plan_scale(
            "herdr-ide-verify-scale",
            "local",
            "/tmp/herdr-ide-verify-scale",
            7,
            11,
        )
        .unwrap();
        assert_eq!(planned.workspace_names.len(), 7);
        assert_eq!(planned.requested_pane_count, 11);
        assert!(
            planned
                .workspace_names
                .iter()
                .all(|name| name.starts_with(FIXTURE_PREFIX))
        );
    }

    #[test]
    fn manifest_validation_fails_closed_before_cleanup_for_unowned_names() {
        let manifest = FixtureManifest {
            schema_version: 1,
            ownership_prefix: FIXTURE_PREFIX.to_owned(),
            target: "local".to_owned(),
            cwd: "/tmp/fixture".to_owned(),
            workspaces: vec![FixtureWorkspaceRecord {
                workspace_name: "user-workspace".to_owned(),
                workspace_id: "w1".to_owned(),
                pane_ids: vec!["w1:p1".to_owned()],
            }],
        };
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn remote_fixture_paths_reject_shell_syntax_before_process_launch() {
        assert!(
            plan_scale(
                "herdr-ide-verify-safe",
                "mini",
                "/tmp/herdr-ide-verify-safe;touch-pwned",
                1,
                1,
            )
            .is_err()
        );
        assert!(
            plan_scale(
                "herdr-ide-verify-safe",
                "mini",
                "/tmp/herdr-ide-verify-safe",
                1,
                1,
            )
            .is_ok()
        );
    }
}
