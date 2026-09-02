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

/// A name no other pane's fork can collide with.
///
/// `herdr agent new` takes the name as a required positional, and two forks of
/// the same parent are the expected case, so the parent's id alone is not
/// enough to keep them apart.
pub fn fork_name(parent_pane_id: &str, nonce: &str) -> String {
    format!("fork-{}-{}", sanitize(parent_pane_id), sanitize(nonce))
}

fn sanitize(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
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
}
