//! Forking an agent pane into a sibling that carries the parent conversation.
//!
//! Herdr already owns every part of this: `agent new` splits a pane, starts the
//! agent in it, and records the parent in its own lineage, all in one atomic
//! call. This module decides which panes can be forked and builds that call's
//! argument list; running it is [`crate::live`]'s job, and reading the lineage
//! back is the session sync's.

/// The agents whose own fork command this shell knows how to spell.
///
/// Both take the session id as a UUID argument, so an agent whose session Herdr
/// recorded as a path cannot be forked by either and is rejected before a
/// control is ever offered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForkableAgent {
    Claude,
    Codex,
}

impl ForkableAgent {
    pub fn parse(agent_kind: &str) -> Option<Self> {
        match agent_kind.trim().to_ascii_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn kind(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The agent's own arguments for resuming a session as a new one. These are
    /// passed through `herdr agent new`'s `--` separator, so they are the
    /// agent's vocabulary rather than Herdr's.
    fn resume_arguments(self, session_id: &str) -> Vec<String> {
        match self {
            Self::Claude => vec![
                "--resume".to_owned(),
                session_id.to_owned(),
                "--fork-session".to_owned(),
            ],
            Self::Codex => vec!["fork".to_owned(), session_id.to_owned()],
        }
    }
}

/// Everything the fork command needs, gathered before the runtime mutex is
/// released so the worker thread carries no reference back into runtime state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForkRequest {
    pub parent_pane_id: String,
    pub agent: ForkableAgent,
    pub session_id: String,
    pub cwd: Option<String>,
    pub name: String,
    pub idempotency_key: String,
}

/// Builds the `herdr agent new` argument list.
///
/// `--from-pane` is what records the parent, so the lineage Herdr reports back
/// is written by the same call that creates the pane rather than by a second
/// request that could fail on its own.
pub fn fork_arguments(request: &ForkRequest) -> Vec<String> {
    let mut arguments = vec![
        "agent".to_owned(),
        "new".to_owned(),
        request.name.clone(),
        "--kind".to_owned(),
        request.agent.kind().to_owned(),
        "--pane".to_owned(),
        request.parent_pane_id.clone(),
        "--from-pane".to_owned(),
        request.parent_pane_id.clone(),
        "--direction".to_owned(),
        "right".to_owned(),
        "--idempotency-key".to_owned(),
        request.idempotency_key.clone(),
        // The operator forked the pane they are reading; taking focus away from
        // it would undo that. This matches the existing split, which also does
        // not steal focus.
        "--no-focus".to_owned(),
    ];
    if let Some(cwd) = request.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) {
        arguments.push("--cwd".to_owned());
        arguments.push(cwd.to_owned());
    }
    arguments.push("--".to_owned());
    arguments.extend(request.agent.resume_arguments(&request.session_id));
    arguments
}

/// Herdr's own rule for an agent name, quoted from the error it answers with:
/// it must start with a lowercase letter and carry only lowercase letters,
/// digits, `-` or `_`, in 1 to 32 characters.
const MAX_NAME_CHARACTERS: usize = 32;

/// A name no other pane's fork can collide with, that Herdr will accept.
///
/// `herdr agent new` takes the name as a required positional, and two forks of
/// the same parent are the expected case, so the parent's id alone is not
/// enough to keep them apart.
///
/// Herdr rejects a name that carries a capital or runs past 32 characters, and
/// a pane id is not required to be lowercase or short: `w2X:p2F` is an
/// ordinary id, and it produced `fork-w2X-p2F-2-1788624371518`, which Herdr
/// refused with `invalid_agent_name`. The case is folded rather than replaced
/// so the id stays readable in Herdr's agent list, and a name that would run
/// long keeps a readable head and a digest of the whole name, which is what
/// keeps two forks of different panes apart after the truncation.
pub fn fork_name(parent_pane_id: &str, nonce: &str) -> String {
    let full = format!("fork-{}-{}", sanitize(parent_pane_id), sanitize(nonce));
    if full.len() <= MAX_NAME_CHARACTERS {
        return full;
    }
    let digest = digest_of(&full);
    let head = MAX_NAME_CHARACTERS - digest.len() - 1;
    format!("{}-{}", &full[..head], digest)
}

/// Twelve hex characters of a hash of the whole name, so what truncation drops
/// still tells two names apart. `DefaultHasher::new` is seeded with fixed
/// keys, so the same name gives the same digest in every process.
fn digest_of(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:012x}", hasher.finish() & 0xffff_ffff_ffff)
}

fn sanitize(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "pane".to_owned()
    } else {
        trimmed
    }
}

/// Whether a pane's agent can be forked at all, from the two facts Herdr
/// reports about it.
///
/// A session recorded as a path is not forkable, because neither agent's fork
/// command takes one. Refusing here is what keeps a control that could only
/// fail from being drawn.
pub fn is_forkable(agent_kind: Option<&str>, session_id: Option<&str>) -> bool {
    let Some(agent_kind) = agent_kind else {
        return false;
    };
    ForkableAgent::parse(agent_kind).is_some()
        && session_id.is_some_and(|session_id| !session_id.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(agent: ForkableAgent) -> ForkRequest {
        ForkRequest {
            parent_pane_id: "w1:p2".to_owned(),
            agent,
            session_id: "3f2b1c00-0000-4000-8000-000000000001".to_owned(),
            cwd: Some("/checkout".to_owned()),
            name: "fork-w1-p2-abc".to_owned(),
            idempotency_key: "key-1".to_owned(),
        }
    }

    #[test]
    fn a_claude_fork_resumes_the_parent_session_as_a_new_one() {
        let arguments = fork_arguments(&request(ForkableAgent::Claude));
        let separator = arguments.iter().position(|argument| argument == "--").unwrap();
        assert_eq!(
            &arguments[separator + 1..],
            [
                "--resume",
                "3f2b1c00-0000-4000-8000-000000000001",
                "--fork-session"
            ]
        );
    }

    #[test]
    fn a_codex_fork_uses_that_agents_own_fork_subcommand() {
        let arguments = fork_arguments(&request(ForkableAgent::Codex));
        let separator = arguments.iter().position(|argument| argument == "--").unwrap();
        assert_eq!(
            &arguments[separator + 1..],
            ["fork", "3f2b1c00-0000-4000-8000-000000000001"]
        );
    }

    #[test]
    fn the_parent_pane_is_both_the_split_target_and_the_recorded_lineage() {
        let arguments = fork_arguments(&request(ForkableAgent::Claude));
        let value_after = |flag: &str| {
            arguments
                .iter()
                .position(|argument| argument == flag)
                .map(|index| arguments[index + 1].as_str())
        };
        assert_eq!(value_after("--pane"), Some("w1:p2"));
        assert_eq!(value_after("--from-pane"), Some("w1:p2"));
        assert_eq!(value_after("--direction"), Some("right"));
        assert!(arguments.iter().any(|argument| argument == "--no-focus"));
    }

    #[test]
    fn a_blank_working_directory_is_left_to_herdr_rather_than_passed_empty() {
        let mut blank = request(ForkableAgent::Claude);
        blank.cwd = Some("   ".to_owned());
        assert!(!fork_arguments(&blank).iter().any(|argument| argument == "--cwd"));
    }

    #[test]
    fn only_the_two_agents_with_a_known_fork_command_are_forkable() {
        assert!(is_forkable(Some("claude"), Some("session")));
        assert!(is_forkable(Some("Codex"), Some("session")));
        assert!(!is_forkable(Some("gemini"), Some("session")));
        assert!(!is_forkable(None, Some("session")));
    }

    #[test]
    fn an_agent_without_a_recorded_session_is_not_forkable() {
        assert!(!is_forkable(Some("claude"), None));
        assert!(!is_forkable(Some("claude"), Some("  ")));
    }

    #[test]
    fn two_forks_of_one_parent_do_not_share_a_name() {
        assert_ne!(fork_name("w1:p2", "abc"), fork_name("w1:p2", "def"));
        assert_eq!(fork_name("w1:p2", "abc"), "fork-w1-p2-abc");
    }

    /// Herdr's own rule, from the error it answers an unacceptable name with:
    /// `agent name must start with a lowercase letter and contain only
    /// lowercase letters, digits, '-' or '_' (1-32 characters)`.
    fn herdr_accepts(name: &str) -> bool {
        let length = name.chars().count();
        (1..=32).contains(&length)
            && name.starts_with(|character: char| character.is_ascii_lowercase())
            && name.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || character == '-'
                    || character == '_'
            })
    }

    /// R12. A pane id is not required to be lowercase. `w2X:p2F` is an
    /// ordinary one, and the name it used to produce was refused by Herdr with
    /// `invalid_agent_name`, so the fork made no pane and the operator saw
    /// only the failure. Every fork in the run that shipped this happened to
    /// be from a lowercase pane id, which is why none of them hit it.
    #[test]
    fn a_pane_id_with_capitals_still_makes_a_name_herdr_accepts() {
        // Both were refused in the field: a capital in the pane part, and a
        // capital in the workspace part.
        for (pane_id, nonce, expected) in [
            ("w2X:p2F", "2-1788624371518", "fork-w2x-p2f-2-1788624371518"),
            ("w4G:pQ", "1-1788624890761", "fork-w4g-pq-1-1788624890761"),
        ] {
            let name = fork_name(pane_id, nonce);
            assert_eq!(name, expected);
            assert!(herdr_accepts(&name), "Herdr would refuse {name:?}");
        }
    }

    /// The name is also the idempotency key, so nothing may be added to it
    /// afterwards: a long pane id and a long nonce have to come back inside
    /// the rule on their own, and still tell two forks apart.
    #[test]
    fn a_long_name_is_cut_to_herdrs_limit_and_still_separates_two_forks() {
        let long_pane = "workspace-with-a-very-long-name:pane-42";
        let first = fork_name(long_pane, "17-1788624371518");
        let second = fork_name(long_pane, "18-1788624371518");
        let other_pane = fork_name("workspace-with-a-very-long-name:pane-43", "17-1788624371518");

        for name in [&first, &second, &other_pane] {
            assert!(herdr_accepts(name), "Herdr would refuse {name:?}");
        }
        assert_ne!(first, second, "two forks of one parent share a name");
        assert_ne!(first, other_pane, "forks of two parents share a name");
        assert_eq!(fork_name(long_pane, "17-1788624371518"), first, "the name is not stable");
    }
}
