use serde::Serialize;

pub const FIXTURE_PREFIX: &str = "herdr-ide-verify-";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FixturePlan {
    pub workspace_name: String,
    pub branch_name: String,
    pub pane_label: String,
}

pub fn plan(name: &str) -> Result<FixturePlan, String> {
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
    Ok(FixturePlan {
        workspace_name: name.to_owned(),
        branch_name: format!("verify/{name}"),
        pane_label: format!("{name}-long-running"),
    })
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
}
