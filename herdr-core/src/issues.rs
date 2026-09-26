//! Bounded GitHub issue identities. These are references, never orchestration state.
use serde::{Deserialize, Serialize};

pub const ISSUE_LIMIT: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct IssueReference {
    pub repository: String,
    pub number: u32,
}

impl IssueReference {
    pub fn parse(value: &str, current_repository: Option<&str>) -> Result<Self, String> {
        let value = value.trim();
        let (repository, number) = if let Some(path) = value.strip_prefix("https://github.com/") {
            let (repository, number) = path
                .split_once("/issues/")
                .ok_or("이슈 URL을 입력하세요.")?;
            (repository, number.trim_end_matches('/'))
        } else if let Some((repository, number)) = value.split_once('#') {
            (
                if repository.is_empty() {
                    current_repository
                        .ok_or("현재 저장소를 확인할 수 없습니다. owner/repo#번호를 입력하세요.")?
                } else {
                    repository
                },
                number,
            )
        } else {
            return Err("owner/repo#번호, #번호 또는 이슈 URL을 입력하세요.".into());
        };
        Self::validated(repository, number)
    }

    fn validated(repository: &str, number: &str) -> Result<Self, String> {
        let mut parts = repository.split('/');
        let valid = |part: &str| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        };
        if !parts.next().is_some_and(valid)
            || !parts.next().is_some_and(valid)
            || parts.next().is_some()
        {
            return Err("owner/repo 형식의 저장소를 입력하세요.".into());
        }
        let number = number
            .parse::<u32>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("양수 이슈 번호를 입력하세요.")?;
        let reference = Self {
            repository: repository.to_owned(),
            number,
        };
        if reference.token().chars().count() > 80 {
            return Err("이슈 연결은 80자 이하여야 합니다.".into());
        }
        Ok(reference)
    }

    pub fn token(&self) -> String {
        format!("{}#{}", self.repository, self.number)
    }
    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/issues/{}",
            self.repository, self.number
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ProjectIssuesSnapshot {
    pub repository: Option<String>,
    pub issues: Vec<IssueSnapshot>,
    pub overflow: bool,
    /// Why the issues' dependencies could not be read on the last pass, when
    /// they could not; each issue then keeps the blockers read before it.
    /// Reader provenance, not a wire field.
    #[serde(skip)]
    pub dependencies_failure: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IssueSnapshot {
    pub reference: IssueReference,
    pub title: String,
    pub url: String,
    pub state: String,
    pub project_status: Option<String>,
    pub updated_at_unix_ms: Option<u64>,
    /// The open issues GitHub records as blocking this one (its "blocked by"
    /// dependencies), in GitHub's order; a closed blocker no longer blocks.
    pub blocked_by: Vec<IssueReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IssueLinkSnapshot {
    pub issue: IssueSnapshot,
    pub source: String,
}

/// Only the two approved branch conventions match, never incidental digits.
pub fn branch_number(branch: &str) -> Option<u32> {
    let mut parts = branch.split('-');
    let first = parts.next()?;
    let candidate = if first.bytes().all(|c| c.is_ascii_digit()) {
        first
    } else if !first.is_empty() && first.bytes().all(|c| c.is_ascii_uppercase()) {
        parts.next()?
    } else {
        return None;
    };
    parts.next().filter(|suffix| !suffix.is_empty())?;
    candidate.parse::<u32>().ok().filter(|n| *n > 0)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedIssue {
    number: u32,
    title: String,
    url: String,
    state: String,
    #[serde(default)]
    project_items: Vec<ProjectItem>,
    updated_at: Option<String>,
}
#[derive(Deserialize)]
struct ProjectItem {
    status: Option<ProjectStatus>,
}
#[derive(Deserialize)]
struct ProjectStatus {
    name: String,
}

pub fn parse_issues(output: &str) -> Result<Vec<IssueSnapshot>, String> {
    let listed: Vec<ListedIssue> = serde_json::from_str(output)
        .map_err(|error| format!("gh issue list returned output Hide could not read: {error}"))?;
    listed
        .into_iter()
        .map(|issue| {
            let reference = IssueReference::parse(&issue.url, None)?;
            if reference.number != issue.number
                || !matches!(issue.state.as_str(), "OPEN" | "CLOSED")
            {
                return Err("GitHub returned an inconsistent issue identity or state".into());
            }
            Ok(IssueSnapshot {
                reference,
                title: issue.title,
                url: issue.url,
                state: issue.state,
                project_status: issue
                    .project_items
                    .into_iter()
                    .filter_map(|item| item.status)
                    .map(|status| status.name)
                    .find(|name| !name.trim().is_empty()),
                updated_at_unix_ms: issue
                    .updated_at
                    .as_deref()
                    .and_then(crate::github::parse_rfc3339_ms),
                blocked_by: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_github_issue_references_and_expands_local_numbers() {
        let expected = IssueReference {
            repository: "acme/project".into(),
            number: 42,
        };
        for input in [
            "acme/project#42",
            "#42",
            "https://github.com/acme/project/issues/42",
        ] {
            assert_eq!(
                IssueReference::parse(input, Some("acme/project")).unwrap(),
                expected
            );
        }
        for invalid in [
            "#0",
            "#-1",
            "#x",
            "a/b/c#2",
            "https://evil.test/a/b/issues/1",
            "https://github.com/a/b/pull/1",
            "a/b#1?x",
            "a/b#1\n#2",
        ] {
            assert!(
                IssueReference::parse(invalid, Some("acme/project")).is_err(),
                "{invalid}"
            );
        }
        assert!(IssueReference::parse("#42", None).is_err());
    }
    #[test]
    fn branch_conventions_require_a_number_prefix_and_a_suffix() {
        assert_eq!(branch_number("42-한글-작업"), Some(42));
        assert_eq!(branch_number("HOY-42-long-name"), Some(42));
        for branch in [
            "fix/42-name",
            "feature-42-name",
            "42",
            "ABC-42",
            "0-name",
            "42-",
        ] {
            assert_eq!(branch_number(branch), None, "{branch}");
        }
    }
    #[test]
    fn issue_state_and_project_status_come_from_the_response() {
        let issues = parse_issues(r#"[{"number":42,"title":"한국어 작업","url":"https://github.com/a/b/issues/42","state":"CLOSED","projectItems":[{"status":{"name":"Done","optionId":"1"},"title":"Roadmap"}],"updatedAt":"2026-09-20T00:00:00Z"}]"#).unwrap();
        assert_eq!(issues[0].state, "CLOSED");
        assert_eq!(issues[0].project_status.as_deref(), Some("Done"));
        assert!(parse_issues("not json").is_err());
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IssueCandidate {
    pub reference: IssueReference,
    pub source: String,
}
