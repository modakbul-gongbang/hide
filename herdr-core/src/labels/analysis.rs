//! What one analysis is asked and when a pane owes one.
//!
//! The context carries the operator's requests only (PRD overview-request-view
//! D-09): the first three and last eight or, once a goal exists, the new ones
//! and the latest exchange; it is masked and cut at 4000 characters. A turn
//! costs at most two requests, one at its start and one at its end, and one
//! more only when the end's timed out (D-33); a `goal_changed: false` answer
//! keeps the goal verbatim.

use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;
use std::time::Duration;

use hide_ai::AiError;
use hide_session::label_transcript::{LabelEvent, LabelEventKind};
use regex::Regex;

/// Upper bound on the analysis context. This also guards against one
/// enormous turn while the initial or rolling sections are being assembled.
pub(crate) const MAX_ANALYSIS_CONTEXT_CHARS: usize = 4_000;
pub(crate) const MAX_GOAL_CHARS: usize = 30;
/// The longest `line` kept. Every surface draws it in one line; the prompt
/// asks for this length and the parser cuts whatever came back.
pub(crate) const MAX_LINE_CHARS: usize = 40;
/// The initial view includes a small head and a bounded tail of human turns.
pub(crate) const INITIAL_FIRST_USER_TURNS: usize = 3;
/// How many of the session's latest human turns feed the initial task
/// context; unbounded, a long session would grow every request.
pub(crate) const MAX_USER_REQUEST_TURNS: usize = 8;
/// The most events a pane keeps between reads besides the human turns the
/// context needs.
const MAX_RETAINED_EVENTS: usize = 512;
/// How long a pane waits before asking again after the provider layer said
/// no provider can answer now (not logged in, usage limit, none connected).
/// Without it the exhausted state was rediscovered on every event, which
/// once wrote 42828 identical lines in a day.
pub(crate) const PROVIDER_RECOVERY_INTERVAL: Duration = Duration::from_secs(600);

/// One provider verdict, already judged usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Analysis {
    /// What the session is to have made when it ends (D-38).
    pub(crate) goal: String,
    pub(crate) goal_changed: bool,
    /// This turn's work while it runs, its result once over, or what the
    /// operator is to answer or do when it ended on a question.
    pub(crate) line: String,
    pub(crate) end: LabelEnd,
}

/// How the turn stands by the agent's last word (D-08). Only a question,
/// an unfinished turn and a wait on something other than a pull request
/// move a row's verb; the rest come from Herdr and GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelEnd {
    Working,
    /// The agent asks the operator something specific.
    Question,
    Done,
    /// It waits on something other than a pull request: a build, another
    /// agent.
    Waiting,
    /// It stopped before the request was done, without asking.
    Unfinished,
}

/// Why one analysis produced no verdict.
#[derive(Debug)]
pub(crate) enum AnalysisFailure {
    /// The provider layer gave up after its own retries, or none answers.
    Provider(AiError),
    /// A well-formed answer whose content the feature refuses.
    Invalid(String),
    /// The analysis thread itself failed; a bug, reported rather than lost.
    Worker(String),
    /// The daemon is stopping and cancelled the request. Nothing is
    /// recorded, so the next process asks again.
    Stopped,
}

impl AnalysisFailure {
    /// A wait means the same context can succeed once the environment
    /// changes (a login, a usage window). `None` is a failure the input
    /// settles, and the turn is parked with it rather than asked again.
    pub(crate) fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Provider(AiError::UsageLimited { retry_after }) => {
                Some(retry_after.unwrap_or(PROVIDER_RECOVERY_INTERVAL))
            }
            Self::Provider(
                AiError::NotAuthenticated
                | AiError::NoProvider(_)
                | AiError::ProviderUnavailable(_)
                | AiError::OverBudget { .. },
            ) => Some(PROVIDER_RECOVERY_INTERVAL),
            Self::Provider(_) | Self::Invalid(_) | Self::Worker(_) | Self::Stopped => None,
        }
    }

    /// A provider that ran out of time may answer the same context when
    /// asked again; a turn's end gets that one more request (D-33).
    pub(crate) fn timed_out(&self) -> bool {
        matches!(self, Self::Provider(AiError::Timeout))
    }

    /// A class for the diagnostic log: an error class or a reason code,
    /// never provider output.
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Provider(error) => error.to_string(),
            Self::Invalid(reason) | Self::Worker(reason) => reason.clone(),
            Self::Stopped => "stopped".to_owned(),
        }
    }
}

/// Cut a goal to the display budget without splitting a word.
pub(crate) fn truncate_goal(text: &str) -> String {
    if text.chars().count() <= MAX_GOAL_CHARS {
        return text.to_owned();
    }
    let budget: String = text.chars().take(MAX_GOAL_CHARS - 1).collect();
    let head = budget
        .rsplit_once(char::is_whitespace)
        .map(|(head, _)| head)
        .filter(|head| head.chars().count() * 2 >= MAX_GOAL_CHARS)
        .unwrap_or(&budget);
    format!("{}…", head.trim_end())
}

pub(crate) fn normalize_goal(raw: &str) -> Option<String> {
    let candidate = raw
        .rsplit("</think>")
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches(|character| matches!(character, '`' | '*' | '"' | '#'))
        .trim();
    if candidate.chars().count() < 8 || candidate.chars().any(char::is_control) {
        return None;
    }
    Some(truncate_goal(candidate))
}

/// Normalize a one-line non-title field without silently accepting a
/// multiline answer. An empty `line` is valid when the boundary has nothing
/// more to say.
pub(crate) fn normalize_text_field(raw: &str, allow_empty: bool) -> Option<String> {
    let candidate = raw.trim();
    if (!allow_empty && candidate.is_empty()) || candidate.chars().any(char::is_control) {
        return None;
    }
    Some(candidate.to_owned())
}

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)sk-[a-z0-9_-]{8,}|(?:api[_-]?key|token|password|secret)\s*[=:]\s*[^\s,;]+")
        .expect("valid secret expression")
});
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").expect("valid email expression")
});
static FILE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:(?:/Users|/home|/tmp|/var|/etc)/)[^\s'"`]+"#).expect("valid path expression")
});

fn conversation(events: &[LabelEvent]) -> Vec<&LabelEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                LabelEventKind::Human | LabelEventKind::Assistant
            )
        })
        .collect()
}

fn render(events: &[&LabelEvent]) -> String {
    events
        .iter()
        .map(|event| format!("{}: {}", event.kind.role(), event.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn latest_exchange(events: &[LabelEvent]) -> String {
    let conversation = conversation(events);
    if conversation.is_empty() {
        return String::new();
    }
    let last = conversation
        .iter()
        .rposition(|event| event.kind == LabelEventKind::Human)
        .unwrap_or(0);
    let start = conversation[..last]
        .iter()
        .rposition(|event| event.kind == LabelEventKind::Human)
        .unwrap_or(last);
    render(&conversation[start..])
}

fn latest_turn_exchange(events: &[LabelEvent]) -> String {
    let conversation = conversation(events);
    let Some(start) = conversation
        .iter()
        .rposition(|event| event.kind == LabelEventKind::Human)
    else {
        return String::new();
    };
    render(&conversation[start..])
}

fn initial_operator_requests(
    events: &[LabelEvent],
    operator: &dyn Fn(&LabelEvent) -> bool,
) -> Option<String> {
    let requests = events
        .iter()
        .filter(|event| event.kind == LabelEventKind::Human && operator(event))
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return None;
    }
    let recent_start = requests.len().saturating_sub(MAX_USER_REQUEST_TURNS);
    let first_count = requests.len().min(INITIAL_FIRST_USER_TURNS);
    let omitted = recent_start.saturating_sub(first_count);
    let mut lines = requests[..first_count]
        .iter()
        .map(|event| format!("user: {}", event.text))
        .collect::<Vec<_>>();
    lines.push(format!(
        "<omitted-requests>{omitted}개 요청 생략</omitted-requests>"
    ));
    lines.extend(
        requests[recent_start..]
            .iter()
            .skip(first_count.saturating_sub(recent_start))
            .map(|event| format!("user: {}", event.text)),
    );
    let lines = lines.join("\n---\n");
    Some(format!(
        "<initial-operator-requests>\n{lines}\n</initial-operator-requests>"
    ))
}

/// The initial text handed to the provider: the first three and last eight
/// of the operator's requests (`operator` says which person's messages are)
/// with an explicit omission marker, and the latest exchange.
pub(crate) fn analysis_context(
    events: &[LabelEvent],
    operator: &dyn Fn(&LabelEvent) -> bool,
) -> String {
    let Some(initial) = initial_operator_requests(events, operator) else {
        return String::new();
    };
    let transcript = latest_exchange(events);
    let combined = format!("{initial}\n<latest-exchange>\n{transcript}\n</latest-exchange>");
    redact(&strip_code_fences(&combined))
}

/// The rolling text for a goal that already exists: the operator's new
/// requests as goal evidence, and the latest exchange for the line and the
/// end.
pub(crate) fn rolling_analysis_context(
    previous_goal: &str,
    new_requests: &[LabelEvent],
    events: &[LabelEvent],
) -> String {
    let delta = new_requests
        .iter()
        .filter(|event| event.kind == LabelEventKind::Human)
        .map(|event| format!("user: {}", event.text))
        .collect::<Vec<_>>()
        .join("\n---\n");
    let delta = if delta.is_empty() {
        "<none/>".to_owned()
    } else {
        delta
    };
    let combined = format!(
        "<previous-goal>{previous_goal}</previous-goal>\n<new-operator-requests>\n{delta}\n</new-operator-requests>\n<latest-exchange>\n{}\n</latest-exchange>",
        latest_turn_exchange(events)
    );
    redact(&strip_code_fences(&combined))
}

/// Which of a turn's two boundaries an analysis is answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisPhase {
    /// The user has just spoken; naming the task here is what lets a
    /// working pane say what it is working on.
    TurnStart,
    /// The agent has answered and stopped; whether it waits on the user can
    /// only be judged once its last word is in.
    TurnEnd,
}

impl AnalysisPhase {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::TurnStart => "start",
            Self::TurnEnd => "end",
        }
    }
}

fn hash_event(event: &LabelEvent) -> u64 {
    let mut hasher = DefaultHasher::new();
    event.at_unix_ms.hash(&mut hasher);
    event.text.hash(&mut hasher);
    hasher.finish()
}

/// Identity of the turn a transcript is in, taken from the user's own last
/// message: fixed for the whole turn and changing exactly once, at the
/// boundary. Hashing the growing context instead once spent a day's request
/// budget by midday. `None` means there is no turn to analyze yet.
pub(crate) fn turn_key(events: &[LabelEvent]) -> Option<u64> {
    let last = events
        .iter()
        .rposition(|event| event.kind == LabelEventKind::Human)?;
    Some(context_fingerprint(&events[last].text))
}

/// The last human event that fed task analysis. Unlike the byte cursor it is
/// about the semantic input boundary, and survives a restart.
pub(crate) fn task_input_cursor(events: &[LabelEvent]) -> Option<u64> {
    events
        .iter()
        .rev()
        .find(|event| event.kind == LabelEventKind::Human)
        .map(hash_event)
}

pub(crate) fn new_human_turns(
    events: &[LabelEvent],
    previous_cursor: Option<u64>,
) -> Vec<LabelEvent> {
    let humans = events
        .iter()
        .filter(|event| event.kind == LabelEventKind::Human)
        .cloned()
        .collect::<Vec<_>>();
    let Some(previous_cursor) = previous_cursor else {
        return humans;
    };
    let Some(previous_index) = humans
        .iter()
        .rposition(|event| hash_event(event) == previous_cursor)
    else {
        // The bounded retention may have dropped the old boundary; the
        // retained human history is the available delta rather than a
        // silent claim that no new request arrived.
        return humans;
    };
    humans.into_iter().skip(previous_index + 1).collect()
}

/// The boundary this pane sits on, or `None` when the turn's two calls are
/// spent. Level-triggered: the condition holds until the call lands, so a
/// call deferred by a provider wait is made later instead of lost.
pub(crate) fn analysis_phase(
    newest_user_is_last: bool,
    working: bool,
    start_done: bool,
    end_done: bool,
) -> Option<AnalysisPhase> {
    if newest_user_is_last || working {
        return (!start_done).then_some(AnalysisPhase::TurnStart);
    }
    (!end_done).then_some(AnalysisPhase::TurnEnd)
}

/// Whether the newest human, interruption or assistant event is the human.
pub(crate) fn newest_user_is_last(events: &[LabelEvent]) -> bool {
    events
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event.kind,
                LabelEventKind::Human | LabelEventKind::Interrupted | LabelEventKind::Assistant
            )
        })
        .is_some_and(|event| event.kind == LabelEventKind::Human)
}

/// The user tore the current turn down: the newest human-side event is an
/// interruption.
pub(crate) fn interrupted(events: &[LabelEvent]) -> bool {
    events
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event.kind,
                LabelEventKind::Human | LabelEventKind::Interrupted
            )
        })
        .is_some_and(|event| event.kind == LabelEventKind::Interrupted)
}

fn strip_code_fences(text: &str) -> String {
    let mut inside = false;
    text.lines()
        .filter(|line| {
            if line.trim_start().starts_with("```") {
                inside = !inside;
                return false;
            }
            !inside
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact(text: &str) -> String {
    let masked = SECRET.replace_all(text, "[redacted-secret]");
    let masked = EMAIL.replace_all(&masked, "[redacted-personal]");
    let masked = FILE_PATH.replace_all(&masked, "[redacted-path]");
    let masked = masked.trim();
    let start = masked
        .char_indices()
        .rev()
        .nth(MAX_ANALYSIS_CONTEXT_CHARS)
        .map_or(0, |(index, _)| index);
    masked[start..].to_owned()
}

pub(crate) fn context_fingerprint(context: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    context.hash(&mut hasher);
    hasher.finish()
}

/// Keeps what the context can still use: the first three human turns, the
/// last eight, and everything from the second-last human on, capped at
/// `MAX_RETAINED_EVENTS` so a long streamed answer cannot grow a pane.
pub(crate) fn retain_bounded(events: &mut VecDeque<LabelEvent>) {
    let human_positions = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (event.kind == LabelEventKind::Human).then_some(index))
        .collect::<Vec<_>>();
    let second_last_human = human_positions.iter().rev().nth(1).copied();
    let mut retained = VecDeque::new();
    for (index, event) in events.drain(..).enumerate() {
        let keep = if event.kind == LabelEventKind::Human {
            // The first turns the initial context names, and the last ones.
            index
                <= human_positions
                    .get(INITIAL_FIRST_USER_TURNS.saturating_sub(1))
                    .copied()
                    .unwrap_or(usize::MAX)
                || human_positions
                    .iter()
                    .rev()
                    .take(MAX_USER_REQUEST_TURNS)
                    .any(|position| *position == index)
        } else {
            match human_positions.as_slice() {
                [] => false,
                [first_human] => index >= *first_human,
                _ => second_last_human.is_some_and(|boundary| index >= boundary),
            }
        };
        if keep {
            retained.push_back(event);
        }
    }
    *events = retained;
    while events.len() > MAX_RETAINED_EVENTS {
        let Some(index) = events
            .iter()
            .position(|event| event.kind != LabelEventKind::Human)
        else {
            break;
        };
        events.remove(index);
    }
}

#[cfg(test)]
pub(crate) fn human(text: &str, at: u64) -> LabelEvent {
    LabelEvent {
        kind: LabelEventKind::Human,
        at_unix_ms: at,
        text: text.to_owned(),
        offset: at,
        images: 0,
        sender: None,
    }
}

#[cfg(test)]
pub(crate) fn assistant(text: &str, at: u64) -> LabelEvent {
    LabelEvent {
        kind: LabelEventKind::Assistant,
        at_unix_ms: at,
        text: text.to_owned(),
        offset: at,
        images: 0,
        sender: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_truncates_on_a_word_boundary_and_rejects_short_or_multiline_titles() {
        assert_eq!(
            truncate_goal("Refactor the authentication middleware to use JWT tokens"),
            "Refactor the authentication…"
        );
        assert_eq!(normalize_goal("짧음"), None);
        assert_eq!(normalize_goal("two\nlines here ok"), None);
        assert_eq!(
            normalize_goal("**라벨 생성기 옮기기**").as_deref(),
            Some("라벨 생성기 옮기기")
        );
    }

    #[test]
    fn initial_context_spans_the_first_three_and_last_eight_operator_requests() {
        let events = (0..15)
            .map(|index| human(&format!("turn {index}"), index))
            .collect::<Vec<_>>();
        let context = analysis_context(&events, &|_| true);
        for kept in [0, 1, 2, 7, 14] {
            assert!(
                context.contains(&format!("user: turn {kept}\n"))
                    || context.contains(&format!("user: turn {kept}"))
            );
        }
        assert!(!context.contains("user: turn 5\n"));
        assert!(context.contains("<omitted-requests>4개 요청 생략</omitted-requests>"));
    }

    #[test]
    fn another_agents_request_never_reaches_the_goal_evidence() {
        let events = vec![
            human("요청 보기 만들기", 1),
            assistant("착수", 2),
            human("테스트 결과 알려줘", 3),
            assistant("결과", 4),
        ];
        let context = analysis_context(&events, &|event| event.offset != 3);
        let (requests, exchange) = context
            .split_once("<latest-exchange>")
            .expect("the context has both sections");
        assert!(requests.contains("user: 요청 보기 만들기"));
        assert!(!requests.contains("테스트 결과"));
        // The exchange still carries it: the line and the end judge the turn.
        assert!(exchange.contains("테스트 결과 알려줘"));
    }

    #[test]
    fn rolling_context_carries_only_the_previous_goal_and_the_new_requests() {
        let events = vec![
            human("old request", 1),
            assistant("done", 2),
            human("new request", 3),
        ];
        let cursor = task_input_cursor(&events[..1]);
        let delta = new_human_turns(&events, cursor);
        let context = rolling_analysis_context("이전 작업 제목", &delta, &events);
        assert!(context.contains("<previous-goal>이전 작업 제목</previous-goal>"));
        assert!(context.contains("user: new request"));
        assert!(!context.contains("user: old request"));
    }

    #[test]
    fn turn_identity_holds_while_output_grows_and_moves_only_at_a_boundary() {
        let mut events = vec![human("first", 1), assistant("a", 2)];
        let key = turn_key(&events);
        events.push(assistant("b", 3));
        assert_eq!(turn_key(&events), key);
        events.push(human("second", 4));
        assert_ne!(turn_key(&events), key);
        assert_eq!(turn_key(&[assistant("only output", 1)]), None);
    }

    #[test]
    fn the_phase_stays_offered_until_its_call_actually_lands() {
        assert_eq!(
            analysis_phase(true, false, false, false),
            Some(AnalysisPhase::TurnStart)
        );
        assert_eq!(analysis_phase(false, true, true, false), None);
        assert_eq!(
            analysis_phase(false, false, true, false),
            Some(AnalysisPhase::TurnEnd)
        );
        assert_eq!(analysis_phase(false, false, true, true), None);
    }

    #[test]
    fn masking_removes_secrets_emails_and_home_paths_and_keeps_prose() {
        let context = analysis_context(
            &[human(
                "token=abc123 메일 me@example.com 파일 /Users/example/project 그대로 두기",
                1,
            )],
            &|_| true,
        );
        assert!(context.contains("[redacted-secret]"));
        assert!(context.contains("[redacted-personal]"));
        assert!(context.contains("[redacted-path]"));
        assert!(context.contains("그대로 두기"));
        assert!(!context.contains("abc123"));
    }

    #[test]
    fn retention_keeps_the_human_turns_the_context_needs() {
        let mut events = VecDeque::new();
        for index in 0..20 {
            events.push_back(human(&format!("h{index}"), index * 2));
            events.push_back(assistant(&format!("a{index}"), index * 2 + 1));
        }
        retain_bounded(&mut events);
        let humans = events
            .iter()
            .filter(|event| event.kind == LabelEventKind::Human)
            .map(|event| event.text.clone())
            .collect::<Vec<_>>();
        assert_eq!(humans.first().map(String::as_str), Some("h0"));
        assert_eq!(humans.last().map(String::as_str), Some("h19"));
        assert_eq!(humans.len(), 3 + 8);
        assert_eq!(events.back().map(|event| event.text.as_str()), Some("a19"));
    }
}
