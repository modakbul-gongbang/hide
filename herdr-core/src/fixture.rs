use serde::{Deserialize, Serialize};

pub const FIXTURE_PREFIX: &str = "herdr-ide-verify-";

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
