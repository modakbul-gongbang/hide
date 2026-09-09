//! Bounded Git ancestry for the selected project's Overview, read by WorktreeReader.
use serde::Serialize;
use std::path::Path;

pub const COMMIT_LIMIT: usize = 512;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GitHistorySnapshot {
    pub commits: Vec<GitCommit>,
    pub truncated: bool,
    pub continuation: Vec<String>,
    pub shallow_boundaries: Vec<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub subject: String,
    pub decorations: String,
}

pub fn read(root: &Path, heads: &[String], shared_git: Option<&Path>) -> GitHistorySnapshot {
    if heads.is_empty() {
        return GitHistorySnapshot::default();
    }
    let shallow = match shared_git.map(|p| std::fs::read_to_string(p.join("shallow"))) {
        Some(Ok(text)) => {
            let ids: Vec<_> = text.lines().map(str::to_owned).collect();
            if !ids.iter().all(|sha| valid_sha(sha)) {
                return GitHistorySnapshot {
                    unavailable_reason: Some(
                        "Invalid shallow repository boundary. Repair the repository and refresh."
                            .into(),
                    ),
                    ..Default::default()
                };
            }
            ids
        }
        Some(Err(error)) if error.kind() != std::io::ErrorKind::NotFound => {
            return GitHistorySnapshot {
                unavailable_reason: Some("Shallow repository boundary is unreadable.".into()),
                ..Default::default()
            };
        }
        _ => Vec::new(),
    };
    let count = format!("--max-count={}", COMMIT_LIMIT + 1);
    let mut args = vec![
        "log",
        "--topo-order",
        "--decorate=full",
        "--format=%H%x00%P%x00%D%x00%s",
        &count,
    ];
    args.extend(heads.iter().map(String::as_str));
    args.push("--");
    match super::git(root, &args).and_then(|text| parse(&text)) {
        Ok(mut value) => {
            value.shallow_boundaries = shallow;
            value
        }
        Err(reason) => GitHistorySnapshot {
            unavailable_reason: Some(reason),
            ..Default::default()
        },
    }
}

fn parse(text: &str) -> Result<GitHistorySnapshot, String> {
    let mut commits = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.is_empty())
        .take(COMMIT_LIMIT + 1)
    {
        let fields: Vec<_> = line.splitn(4, '\0').collect();
        if fields.len() != 4
            || !valid_sha(fields[0])
            || !fields[1].split_whitespace().all(valid_sha)
        {
            return Err("Git returned an invalid commit relationship. Refresh Overview.".into());
        }
        commits.push(GitCommit {
            sha: fields[0].into(),
            parents: fields[1].split_whitespace().map(str::to_owned).collect(),
            decorations: fields[2].into(),
            subject: fields[3].into(),
        });
    }
    let truncated = commits.len() > COMMIT_LIMIT;
    let next = commits.get(COMMIT_LIMIT).map(|c| c.sha.clone());
    commits.truncate(COMMIT_LIMIT);
    let visible: std::collections::HashSet<_> = commits.iter().map(|c| c.sha.as_str()).collect();
    let mut continuation: Vec<_> = commits
        .iter()
        .flat_map(|c| &c.parents)
        .filter(|p| !visible.contains(p.as_str()))
        .cloned()
        .collect();
    continuation.extend(next);
    continuation.sort();
    continuation.dedup();
    // Parents outside this window remain named: the renderer must show a
    // continuation, never turn an incomplete relationship into a Git root.
    Ok(GitHistorySnapshot {
        commits,
        truncated,
        continuation,
        ..Default::default()
    })
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_history_retains_all_merge_parents_and_boundary_identity() {
        let b = "b".repeat(40);
        let c = "c".repeat(40);
        let text = (0..=COMMIT_LIMIT)
            .map(|i| format!("{i:040x}\0{b} {c}\0\0Merge\n"))
            .collect::<String>();
        let history = parse(&text).unwrap();
        assert_eq!(history.commits.len(), COMMIT_LIMIT);
        assert!(history.truncated);
        assert_eq!(history.commits.last().unwrap().parents, [b, c]);
        assert!(parse("invalid\0parent\0subject").is_err());
    }
}
