//! The doorbell types one short line into an idle agent pane so a pending
//! letter wakes it. Herdr has no atomic composer guard, so the rule is
//! conservative: a letter waits unless every fact hide owns says the pane is
//! at rest. Nothing here reads the terminal, and nothing clears or restores
//! input; a TUI that redraws its footer or statusline cannot change a verdict.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use hide_herdr_client::{ApiConnector, request_small_response};
use serde_json::{Value, json};

use crate::runtime::Runtime;
use crate::runtime::delivery::Observation;
use crate::wire;

use super::ledger::State;
use super::worker::{Client, Effect, now};

const QUIET_MS: u64 = 30_000;
const WORK_PER_PASS: usize = 8;
const RPC_TIMEOUT: Duration = Duration::from_millis(500);

/// Why a pending letter was not belled. Each is an expected wait, so it goes
/// to the diagnostic log and never to the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hold {
    /// The agent kind is not one whose menus were seen to read `blocked`.
    Kind,
    /// No positive native session, or the pane now hosts another one.
    Session,
    /// The pane or its device is not observed.
    Absent,
    Working,
    /// Herdr reports a permission or selection menu.
    Blocked,
    /// Herdr reports a status that is not rest (or none at all).
    Status,
    /// A key hide routed after the last submission and the last start of
    /// work: an unsent draft, a prompt Esc brought back, a recalled input.
    Draft,
    /// Less than the quiet period since the last key or the last change of
    /// status.
    Quiet,
    /// The pane changed between the verdict and the input.
    Changed,
}

impl Hold {
    fn code(self) -> &'static str {
        match self {
            Self::Kind => "kind_not_belled",
            Self::Session => "session_changed",
            Self::Absent => "pane_unavailable",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Status => "status_not_at_rest",
            Self::Draft => "draft",
            Self::Quiet => "quiet_period",
            Self::Changed => "changed_before_input",
        }
    }
}

/// The agent kinds a bell may be typed into: those whose permission and
/// selection menus Herdr was observed to report as `blocked` in an isolated
/// run (`docs/delivery.md`, Verification). Herdr's own state is the only menu
/// guard, so a kind that was not observed is not in this list.
pub(crate) fn bell_target(kind: &str) -> bool {
    super::mailbox::prompt_hook(kind)
}

/// Whether the pane is at rest and a bell can be typed into it. All of these
/// must hold: Herdr reports `idle` or `done` and has for the quiet period,
/// hide has routed no key for the quiet period, no key hide routed is newer
/// than the last submission and the last start of work (a key before either
/// was consumed by it, a menu answer for one), and the pane still hosts the
/// session the letter was written for.
pub(crate) fn judge(observation: &Observation, now: u64) -> Result<(), Hold> {
    if !bell_target(&observation.actor.kind) {
        return Err(Hold::Kind);
    }
    if observation.actor.require_native_identity().is_err() {
        return Err(Hold::Session);
    }
    match observation.status.as_str() {
        "idle" | "done" if observation.state_change_seq.is_some() => {}
        "working" => return Err(Hold::Working),
        "blocked" => return Err(Hold::Blocked),
        _ => return Err(Hold::Status),
    }
    if observation.last_input_at_unix_ms
        > observation
            .last_submit_at_unix_ms
            .max(observation.entered_working_at_unix_ms)
    {
        return Err(Hold::Draft);
    }
    if now.saturating_sub(observation.last_input_at_unix_ms) < QUIET_MS
        || now.saturating_sub(observation.status_changed_at_unix_ms) < QUIET_MS
    {
        return Err(Hold::Quiet);
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct Episode {
    status: String,
    sequence: Option<u64>,
    input: u64,
    session: Option<String>,
}

impl From<&Observation> for Episode {
    fn from(value: &Observation) -> Self {
        Self {
            status: value.status.clone(),
            sequence: value.state_change_seq,
            input: value.last_input_at_unix_ms,
            session: value.actor.session.clone(),
        }
    }
}

#[allow(clippy::disallowed_methods)] // a production wait, not test code
pub(crate) fn run(runtime: Weak<Mutex<Runtime>>, client: Client, stop: Arc<AtomicBool>) {
    // The maps are bounded by the <=1024 pending letters in this pass.
    // A refusal is retried only after state/input changes.
    let mut tried = HashMap::<String, Episode>::new();
    let mut rung = HashMap::<String, Episode>::new();
    let mut held = HashMap::<String, Hold>::new();
    while !stop.load(Ordering::Acquire) {
        let Some(owner) = runtime.upgrade() else {
            break;
        };
        let context = owner
            .lock()
            .ok()
            .and_then(|guard| guard.delivery_state().ok());
        if let Some(ledger) = context {
            let mut targets = HashSet::new();
            let pending: Vec<_> = ledger
                .letters
                .iter()
                .filter(|letter| {
                    letter.state == State::Pending
                        && letter.attempts() < 3
                        && now().saturating_sub(letter.created_at_unix_ms)
                            < super::DELIVERY_EXPIRY_MS
                })
                .filter(|letter| targets.insert(letter.recipient.pane_id.clone()))
                .collect();
            let ids: HashSet<_> = pending.iter().map(|letter| letter.id.as_str()).collect();
            tried.retain(|id, _| ids.contains(id.as_str()));
            held.retain(|id, _| ids.contains(id.as_str()));
            rung.retain(|pane, _| targets.contains(pane));
            let mut count = 0;
            for letter in pending {
                if stop.load(Ordering::Acquire) || count >= WORK_PER_PASS {
                    break;
                }
                let verdict = owner
                    .lock()
                    .map(|guard| guard.delivery_bell_verdict(&letter.recipient, now()))
                    .unwrap_or(Err(Hold::Absent));
                let observed = match verdict {
                    Ok(observed) => observed,
                    Err(reason) => {
                        hold(&mut held, &letter.id, &letter.recipient.pane_id, reason);
                        continue;
                    }
                };
                let Some(connector) = owner
                    .lock()
                    .ok()
                    .and_then(|guard| guard.delivery_connector(&letter.recipient.device_id))
                else {
                    hold(
                        &mut held,
                        &letter.id,
                        &letter.recipient.pane_id,
                        Hold::Absent,
                    );
                    continue;
                };
                let episode = Episode::from(&observed);
                if tried.get(&letter.id) == Some(&episode)
                    || rung.get(&letter.recipient.pane_id) == Some(&episode)
                {
                    continue;
                }
                count += 1;
                tried.insert(letter.id.clone(), episode.clone());
                match deliver(
                    &owner,
                    &client,
                    connector.as_ref(),
                    &letter.id,
                    &observed,
                    &stop,
                ) {
                    Ok(true) => {
                        held.remove(&letter.id);
                        rung.insert(letter.recipient.pane_id.clone(), episode);
                        record(&client, &letter.id, &observed, true);
                    }
                    Ok(false) => {
                        hold(
                            &mut held,
                            &letter.id,
                            &letter.recipient.pane_id,
                            Hold::Changed,
                        );
                    }
                    Err(code) => {
                        crate::diagnostic!(json!({"component":"delivery","kind":"doorbell.failed",
                            "letter_id":letter.id,"pane_id":letter.recipient.pane_id,"code":code}));
                        record(&client, &letter.id, &observed, false);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Logs a letter's reason for waiting once per change of reason, with its id
/// and pane and never its body.
fn hold(held: &mut HashMap<String, Hold>, id: &str, pane_id: &str, reason: Hold) {
    if held.insert(id.to_owned(), reason) != Some(reason) {
        crate::diagnostic!(json!({"component":"delivery","kind":"doorbell.held",
            "letter_id":id,"pane_id":pane_id,"reason":reason.code()}));
    }
}

fn record(client: &Client, id: &str, observed: &Observation, sent: bool) {
    if let Err(code) = client.submit(
        Effect::Bell {
            id: id.to_owned(),
            recipient: observed.actor.clone(),
            sent,
        },
        Duration::from_secs(5),
    ) {
        crate::diagnostic!(
            json!({"component":"delivery","kind":"doorbell.transaction_failed",
            "letter_id":id,"code":code})
        );
    }
}

fn rpc(connector: &dyn ApiConnector, method: &str, params: Value) -> Result<Value, &'static str> {
    request_small_response(connector, method, params, RPC_TIMEOUT).map_err(|_| "herdr_call_failed")
}

fn native_matches(
    connector: &dyn ApiConnector,
    observed: &Observation,
) -> Result<bool, &'static str> {
    if observed.actor.require_native_identity().is_err() {
        return Ok(false);
    }
    let parameters =
        wire::agent_target_params(&observed.raw_pane_id).map_err(|_| "herdr_parameters")?;
    let agent = wire::delivery_agent(rpc(connector, "agent.get", parameters)?)
        .map_err(|_| "herdr_format")?;
    Ok(agent.ready
        && agent.pane_id == observed.raw_pane_id
        && agent.name == observed.actor.name
        && agent.kind.as_ref() == Some(&observed.actor.kind)
        && agent.session == observed.actor.session
        && agent.status == observed.status
        && Some(agent.state_change_seq) == observed.state_change_seq)
}

/// All delivery pane writes go through this function. The last memory check
/// cannot eliminate an external socket/TUI input race (the approved D-18 limit).
fn deliver(
    owner: &Mutex<Runtime>,
    client: &Client,
    connector: &dyn ApiConnector,
    id: &str,
    observed: &Observation,
    stop: &AtomicBool,
) -> Result<bool, &'static str> {
    if stop.load(Ordering::Acquire) || !native_matches(connector, observed)? {
        return Ok(false);
    }
    let reservation = client
        .submit(
            Effect::BellAttempt {
                id: id.to_owned(),
                observed: Box::new(observed.clone()),
            },
            Duration::from_secs(5),
        )
        .map_err(|_| "doorbell_reservation_failed")?;
    let attempt = reservation["attempt"]
        .as_u64()
        .filter(|value| (1..=3).contains(value))
        .ok_or("doorbell_reservation_format")? as u8;
    // The durable save may wait. Reinspect the native owner afterwards
    // instead of treating the pre-save answer as current evidence.
    if stop.load(Ordering::Acquire) || !native_matches(connector, observed)? {
        return Ok(false);
    }
    if stop.load(Ordering::Acquire)
        || !owner
            .lock()
            .map_err(|_| "runtime_unavailable")?
            .delivery_bell_current(id, observed, Some(attempt))
    {
        return Ok(false);
    }
    let parameters = wire::delivery_input_params(
        &observed.raw_pane_id,
        hide_agent_hooks::delivery::BELL_PROMPT,
    )
    .map_err(|_| "herdr_parameters")?;
    rpc(connector, "pane.send_input", parameters)?;
    // Arrival is not intake. Only a flushed prompt-hook confirmation clears it.
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delivery::Actor;

    const NOW: u64 = 1_000_000;

    /// A pane at rest: idle for a minute, last keys and submit a minute ago.
    fn at_rest(kind: &str) -> Observation {
        Observation {
            actor: Actor {
                pane_id: "pane".into(),
                name: "agent".into(),
                kind: kind.into(),
                device_id: "local".into(),
                session: Some("session".into()),
            },
            raw_pane_id: "pane".into(),
            status: "idle".into(),
            state_change_seq: Some(7),
            status_changed_at_unix_ms: NOW - 60_000,
            last_input_at_unix_ms: NOW - 60_000,
            last_submit_at_unix_ms: NOW - 60_000,
            entered_working_at_unix_ms: NOW - 120_000,
            session: None,
            host_scope: None,
        }
    }

    #[test]
    fn a_pane_at_rest_is_belled_for_claude_and_codex_with_idle_or_done() {
        for kind in ["claude", "codex"] {
            for status in ["idle", "done"] {
                let mut pane = at_rest(kind);
                pane.status = status.into();
                assert_eq!(judge(&pane, NOW), Ok(()), "{kind} {status}");
            }
        }
    }

    #[test]
    fn each_missing_condition_alone_holds_the_letter_for_its_own_reason() {
        type Change = fn(&mut Observation);
        let cases: [(&str, Change, Hold); 9] = [
            (
                "kind",
                |pane| pane.actor.kind = "opencode".into(),
                Hold::Kind,
            ),
            ("session", |pane| pane.actor.session = None, Hold::Session),
            (
                "working",
                |pane| pane.status = "working".into(),
                Hold::Working,
            ),
            ("menu", |pane| pane.status = "blocked".into(), Hold::Blocked),
            (
                "unknown status",
                |pane| pane.status = "unknown".into(),
                Hold::Status,
            ),
            (
                "no sequence",
                |pane| pane.state_change_seq = None,
                Hold::Status,
            ),
            (
                "draft after the last submit",
                |pane| pane.last_input_at_unix_ms = NOW - 40_000,
                Hold::Draft,
            ),
            (
                "key within the quiet period",
                |pane| {
                    pane.last_input_at_unix_ms = NOW - 5_000;
                    pane.last_submit_at_unix_ms = NOW - 5_000;
                },
                Hold::Quiet,
            ),
            (
                "status changed within the quiet period",
                |pane| pane.status_changed_at_unix_ms = NOW - 5_000,
                Hold::Quiet,
            ),
        ];
        for (name, change, expected) in cases {
            let mut pane = at_rest("claude");
            change(&mut pane);
            assert_eq!(judge(&pane, NOW), Err(expected), "{name}");
        }
    }

    #[test]
    fn a_key_before_the_pane_started_working_was_consumed_but_one_after_is_a_draft() {
        // A menu answer at NOW-90s, then the agent worked from NOW-80s.
        let mut pane = at_rest("codex");
        pane.last_input_at_unix_ms = NOW - 90_000;
        pane.last_submit_at_unix_ms = NOW - 200_000;
        pane.entered_working_at_unix_ms = NOW - 80_000;
        assert_eq!(judge(&pane, NOW), Ok(()));
        // The same key typed after work began is still a draft, minutes later.
        pane.last_input_at_unix_ms = NOW - 70_000;
        assert_eq!(judge(&pane, NOW), Err(Hold::Draft));
        assert_eq!(judge(&pane, NOW + 600_000), Err(Hold::Draft));
        // Submitting it ends the draft.
        pane.last_submit_at_unix_ms = NOW - 69_000;
        assert_eq!(judge(&pane, NOW), Ok(()));
    }

    #[test]
    fn the_bell_kinds_are_exactly_the_kinds_with_an_installed_prompt_hook() {
        for kind in ["claude", "claude-code", "claude_code", "codex"] {
            assert!(bell_target(kind), "{kind}");
        }
        for kind in ["opencode", "pi", "grok", "cursor", "gemini", "amp", ""] {
            assert!(!bell_target(kind), "{kind}");
        }
    }
}
