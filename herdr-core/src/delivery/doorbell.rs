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
use hide_session::turns::Waiting;
use serde_json::{Value, json};

use crate::runtime::Runtime;
use crate::runtime::delivery::Observation;
use crate::wire::{self, Readiness};

use super::ledger::State;
use super::worker::{Client, Effect, now};

const QUIET_MS: u64 = 30_000;
const WORK_PER_PASS: usize = 8;
const RPC_TIMEOUT: Duration = Duration::from_millis(500);
/// A letter refused between its verdict and the input is tried again in the
/// same pane episode after this long, doubling up to `RETRY_LIMIT_MS`. The
/// refusal can come from a fact the episode does not carry, such as Herdr's
/// readiness or the letter itself, so waiting for the episode to move could
/// wait until the letter expires.
const RETRY_FIRST_MS: u64 = 5_000;
const RETRY_LIMIT_MS: u64 = 120_000;

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
    /// The agent's session says its last turn waits for the operator to
    /// approve a plan, a menu Herdr reads as an ordinary stop.
    AwaitingOperator,
    /// The agent's session can say it waits for the operator, and no read of
    /// it settled that for Herdr's current state.
    SessionUnread,
    /// Herdr reports a status that is not rest (or none at all).
    Status,
    /// Input hide routed after the last submission and the last start of
    /// work: an unsent draft, a prompt Esc brought back, a recalled input.
    Draft,
    /// Less than the quiet period since the last input or the last change of
    /// status.
    Quiet,
    /// Herdr reports the agent not ready for input (`interactive_ready:
    /// false`).
    NotReady,
    /// Herdr is still starting the agent it launched (`launch_pending`).
    Launching,
    /// Herdr's agent in the pane has another name, kind or native session
    /// than the one the verdict was read for.
    Identity,
    /// Herdr's status or state sequence moved after the verdict.
    Moved,
    /// Hide routed input to the pane after the verdict.
    Input,
    /// The letter was confirmed, cancelled, expired or reserved after the
    /// verdict.
    Letter,
}

impl Hold {
    fn code(self) -> &'static str {
        match self {
            Self::Kind => "kind_not_belled",
            Self::Session => "session_changed",
            Self::Absent => "pane_unavailable",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::AwaitingOperator => "awaiting_operator",
            Self::SessionUnread => "session_unread",
            Self::Status => "status_not_at_rest",
            Self::Draft => "draft",
            Self::Quiet => "quiet_period",
            Self::NotReady => "not_ready",
            Self::Launching => "launch_pending",
            Self::Identity => "identity_changed",
            Self::Moved => "sequence_moved",
            Self::Input => "input_after_verdict",
            Self::Letter => "letter_changed",
        }
    }
}

/// The agent kinds a bell may be typed into: those whose permission and
/// selection menus Herdr was observed to report as `blocked` in an isolated
/// run (`docs/delivery.md`, Verification). A menu Herdr reads as a stop
/// (Codex's plan approval) is guarded by the session read instead
/// ([`Turn`]), so a kind that was not observed is not in this list.
pub(crate) fn bell_target(kind: &str) -> bool {
    hide_agent_adapter::adapter(kind).is_some_and(|row| row.bell)
}

/// What the agent's session read says it waits for in Herdr's current state
/// (PRD codex-plan-approval-hold D-06, D-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    /// The agent's session read reports no turns, so it says nothing here.
    NotReported,
    /// The read reports turns, and none read for the current state and
    /// session settled them: a file not found, a failed read, a helper that
    /// predates the field, a state newer than the last read.
    Unread,
    Read(Waiting),
}

/// Whether the pane is at rest and a bell can be typed into it. All of these
/// must hold: Herdr reports `idle` or `done` and has for the quiet period,
/// the session read, for an agent whose read reports turns, says for that
/// state that nothing waits for the operator,
/// hide has routed no input for the quiet period, no input hide routed is
/// newer than the last submission and the last start of work (input before
/// either was consumed by it, a menu answer for one), and the pane still hosts
/// the session the letter was written for.
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
    match observation.turn {
        Turn::NotReported | Turn::Read(Waiting::Nothing) => {}
        Turn::Read(Waiting::Question | Waiting::PlanApproval) => {
            return Err(Hold::AwaitingOperator);
        }
        Turn::Unread => return Err(Hold::SessionUnread),
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

/// A letter's last attempt that did not ring: the pane episode it was made
/// in, and when the letter may be tried again in that same episode.
struct Tried {
    episode: Episode,
    retry_at: u64,
    delay: u64,
}

/// What the doorbell keeps between passes. Each map is keyed by a letter or
/// pane of the current pass and pruned to them, so it is bounded by the
/// <=1024 open letters.
#[derive(Default)]
pub(crate) struct Doorbell {
    tried: HashMap<String, Tried>,
    /// The episode each pane was belled in. The bell's own turn moves the
    /// pane, so a pane is not belled again until it has.
    rung: HashMap<String, Episode>,
    /// Why each letter waits, logged once per change.
    held: HashMap<String, Hold>,
}

/// How one attempt ended without failing.
enum Outcome {
    Rung,
    Held(Hold),
    Stopped,
}

#[allow(clippy::disallowed_methods)] // a production wait, not test code
pub(crate) fn run(runtime: Weak<Mutex<Runtime>>, client: Client, stop: Arc<AtomicBool>) {
    let mut doorbell = Doorbell::default();
    while !stop.load(Ordering::Acquire) {
        let Some(owner) = runtime.upgrade() else {
            break;
        };
        doorbell.pass(&owner, &client, &stop, now());
        drop(owner);
        thread::sleep(Duration::from_millis(250));
    }
}

impl Doorbell {
    /// One look at every pane with a pending letter, at most
    /// `WORK_PER_PASS` of which reach Herdr. The verdict is memory only;
    /// Herdr calls, the reservation and the input run off the lock.
    pub(crate) fn pass(
        &mut self,
        owner: &Mutex<Runtime>,
        client: &Client,
        stop: &AtomicBool,
        now: u64,
    ) {
        let Some(ledger) = owner
            .lock()
            .ok()
            .and_then(|guard| guard.delivery_state().ok())
        else {
            return;
        };
        let mut targets = HashSet::new();
        let pending: Vec<_> = ledger
            .letters
            .iter()
            .filter(|letter| {
                letter.state == State::Pending
                    && letter.attempts() < 3
                    && now.saturating_sub(letter.created_at_unix_ms) < super::DELIVERY_EXPIRY_MS
            })
            .filter(|letter| targets.insert(letter.recipient.pane_id.clone()))
            .collect();
        let ids: HashSet<_> = pending.iter().map(|letter| letter.id.as_str()).collect();
        self.forget(&ledger.letters, &ids, now);
        self.rung.retain(|pane, _| targets.contains(pane));
        let mut count = 0;
        for letter in pending {
            if stop.load(Ordering::Acquire) || count >= WORK_PER_PASS {
                break;
            }
            let pane = &letter.recipient.pane_id;
            let verdict = owner
                .lock()
                .map(|guard| guard.delivery_bell_verdict(&letter.recipient, now))
                .unwrap_or(Err(Hold::Absent));
            let observed = match verdict {
                Ok(observed) => observed,
                Err(reason) => {
                    self.hold(&letter.id, pane, reason);
                    continue;
                }
            };
            let Some(connector) = owner
                .lock()
                .ok()
                .and_then(|guard| guard.delivery_connector(&letter.recipient.device_id))
            else {
                self.hold(&letter.id, pane, Hold::Absent);
                continue;
            };
            let episode = Episode::from(&observed);
            if self.rung.get(pane) == Some(&episode) {
                // Nothing holds it, so a later hold for the same reason
                // is a change and is logged again.
                self.held.remove(&letter.id);
                continue;
            }
            let delay = match self.tried.get(&letter.id) {
                // Still held by its last reason, which stays logged.
                Some(last) if last.episode == episode && now < last.retry_at => continue,
                Some(last) if last.episode == episode => {
                    last.delay.saturating_mul(2).min(RETRY_LIMIT_MS)
                }
                _ => RETRY_FIRST_MS,
            };
            count += 1;
            match deliver(
                owner,
                client,
                connector.as_ref(),
                &letter.id,
                &observed,
                stop,
            ) {
                Ok(Outcome::Rung) => {
                    self.held.remove(&letter.id);
                    self.tried.remove(&letter.id);
                    self.rung.insert(pane.clone(), episode);
                    record(client, &letter.id, &observed, true);
                }
                Ok(Outcome::Held(reason)) => {
                    self.hold(&letter.id, pane, reason);
                    self.tried.insert(
                        letter.id.clone(),
                        Tried {
                            episode,
                            retry_at: now.saturating_add(delay),
                            delay,
                        },
                    );
                }
                Ok(Outcome::Stopped) => break,
                Err(code) => {
                    crate::diagnostic!(json!({"component":"delivery","kind":"doorbell.failed",
                        "letter_id":letter.id,"pane_id":pane,"code":code}));
                    record(client, &letter.id, &observed, false);
                    // A failed Herdr call may still have typed the bell, so
                    // a failure waits for the pane to move rather than for a
                    // timer that could type it twice.
                    self.tried.insert(
                        letter.id.clone(),
                        Tried {
                            episode,
                            retry_at: u64::MAX,
                            delay,
                        },
                    );
                }
            }
        }
    }

    /// Drops what the doorbell kept for letters that left the pass. A letter
    /// that reached its deadline while held is logged with the reason it last
    /// waited for, so an undelivered letter names its cause.
    fn forget(&mut self, letters: &[super::ledger::Letter], ids: &HashSet<&str>, now: u64) {
        for (id, reason) in &self.held {
            if ids.contains(id.as_str()) {
                continue;
            }
            if let Some(letter) = letters.iter().find(|letter| &letter.id == id)
                && matches!(letter.state, State::Pending | State::Undelivered)
                && now.saturating_sub(letter.created_at_unix_ms) >= super::DELIVERY_EXPIRY_MS
            {
                crate::diagnostic!(json!({"component":"delivery","kind":"doorbell.expired",
                    "letter_id":id,"pane_id":letter.recipient.pane_id,"reason":reason.code()}));
            }
        }
        self.held.retain(|id, _| ids.contains(id.as_str()));
        self.tried.retain(|id, _| ids.contains(id.as_str()));
    }

    /// Logs a letter's reason for waiting once per change of reason, with
    /// its id and pane and never its body.
    fn hold(&mut self, id: &str, pane_id: &str, reason: Hold) {
        if self.held.insert(id.to_owned(), reason) != Some(reason) {
            crate::diagnostic!(json!({"component":"delivery","kind":"doorbell.held",
                "letter_id":id,"pane_id":pane_id,"reason":reason.code()}));
        }
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

/// Whether Herdr still reports the agent the verdict was read for, at the
/// same status and sequence, and ready for input. Herdr reports readiness
/// only for an agent it started itself; an agent typed into a shell reports
/// none, and the verdict's own facts decide it: Herdr has seen it at rest for
/// the quiet period, it is the same agent and native session, and hide holds
/// no draft or recent key for it.
fn native_current(
    connector: &dyn ApiConnector,
    observed: &Observation,
) -> Result<Result<(), Hold>, &'static str> {
    if observed.actor.require_native_identity().is_err() {
        return Ok(Err(Hold::Session));
    }
    let parameters =
        wire::agent_target_params(&observed.raw_pane_id).map_err(|_| "herdr_parameters")?;
    let agent = wire::delivery_agent(rpc(connector, "agent.get", parameters)?)
        .map_err(|_| "herdr_format")?;
    Ok(match agent.readiness {
        Readiness::Launching => Err(Hold::Launching),
        Readiness::NotReady => Err(Hold::NotReady),
        Readiness::Ready | Readiness::Unreported
            if agent.pane_id != observed.raw_pane_id
                || agent.name != observed.actor.name
                || !agent.kind.as_deref().is_some_and(|kind| {
                    hide_agent_adapter::canonical_kind(kind)
                        == hide_agent_adapter::canonical_kind(&observed.actor.kind)
                })
                || agent.session != observed.actor.session =>
        {
            Err(Hold::Identity)
        }
        Readiness::Ready | Readiness::Unreported
            if agent.status != observed.status
                || Some(agent.state_change_seq) != observed.state_change_seq =>
        {
            Err(Hold::Moved)
        }
        Readiness::Ready | Readiness::Unreported => Ok(()),
    })
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
) -> Result<Outcome, &'static str> {
    if stop.load(Ordering::Acquire) {
        return Ok(Outcome::Stopped);
    }
    if let Err(reason) = native_current(connector, observed)? {
        return Ok(Outcome::Held(reason));
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
    if stop.load(Ordering::Acquire) {
        return Ok(Outcome::Stopped);
    }
    if let Err(reason) = native_current(connector, observed)? {
        return Ok(Outcome::Held(reason));
    }
    if stop.load(Ordering::Acquire) {
        return Ok(Outcome::Stopped);
    }
    if let Err(reason) = owner
        .lock()
        .map_err(|_| "runtime_unavailable")?
        .delivery_bell_current(id, observed, Some(attempt))
    {
        return Ok(Outcome::Held(reason));
    }
    let parameters = wire::delivery_input_params(
        &observed.raw_pane_id,
        hide_agent_hooks::delivery::BELL_PROMPT,
    )
    .map_err(|_| "herdr_parameters")?;
    rpc(connector, "pane.send_input", parameters)?;
    // Arrival is not intake. Only a flushed prompt-hook confirmation clears it.
    Ok(Outcome::Rung)
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
            turn: if kind == "codex" {
                Turn::Read(Waiting::Nothing)
            } else {
                Turn::NotReported
            },
        }
    }

    #[test]
    fn native_alias_identity_preserves_pane_name_session_readiness_and_sequence_guards() {
        let flags = Arc::new(Mutex::new(json!({})));
        let answer = Arc::clone(&flags);
        let herdr = crate::fake_herdr::FakeHerdr::start("bell-alias", move |method, _| {
            assert_eq!(method, "agent.get");
            let mut agent = json!({
                "pane_id": "pane", "terminal_id": "term", "workspace_id": "w1", "tab_id": "w1:t1",
                "name": "agent", "agent": "claude", "agent_status": "idle", "state_change_seq": 7,
                "focused": false, "revision": 1,
                "agent_session": {"source": "hook", "agent": "claude", "kind": "id", "value": "session"}
            });
            for (key, value) in answer.lock().unwrap().as_object().unwrap() {
                agent[key] = value.clone();
            }
            json!({"type": "agent_info", "agent": agent})
        });
        let mut observed = at_rest(" CLAUDE_CODE ");
        observed.actor.session = wire::session_digest("session");
        for (kind, canonical) in [
            (" CLAUDE-CODE ", "claude"),
            (" CLAUDE ", "claude"),
            (" CLAUDE_CODE ", "claude"),
            (" CODEX ", "codex"),
        ] {
            observed.actor.kind = kind.into();
            *flags.lock().unwrap() = json!({"agent": canonical});
            assert_eq!(
                native_current(&herdr.connector(), &observed),
                Ok(Ok(())),
                "{kind}"
            );
        }
        observed.actor.kind = " CLAUDE_CODE ".into();
        for (change, expected) in [
            (json!({"pane_id": "another-pane"}), Hold::Identity),
            (json!({"name": "another-agent"}), Hold::Identity),
            (json!({"agent": "codex"}), Hold::Identity),
            (json!({"agent": "future-agent"}), Hold::Identity),
            (
                json!({"agent_session": {"source": "hook", "agent": "claude", "kind": "id", "value": "another-session"}}),
                Hold::Identity,
            ),
            (json!({"agent_session": null}), Hold::Identity),
            (json!({"interactive_ready": false}), Hold::NotReady),
            (json!({"launch_pending": true}), Hold::Launching),
            (json!({"agent_status": "working"}), Hold::Moved),
            (json!({"state_change_seq": 8}), Hold::Moved),
        ] {
            *flags.lock().unwrap() = change;
            assert_eq!(
                native_current(&herdr.connector(), &observed),
                Ok(Err(expected))
            );
        }
        observed.actor.kind = "future-agent".into();
        *flags.lock().unwrap() = json!({"agent": "FUTURE-AGENT"});
        assert_eq!(
            native_current(&herdr.connector(), &observed),
            Ok(Err(Hold::Identity))
        );
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

    /// PRD codex-plan-approval-hold D-06, D-07, B3, B6, B7: a plan waiting
    /// for the operator's approval holds the letter at `done` or `idle`, so
    /// does a session not read for the current state, and an agent whose
    /// read reports no turns is belled as before.
    #[test]
    fn a_session_waiting_for_the_operator_or_not_read_holds_the_letter() {
        for status in ["idle", "done"] {
            let mut pane = at_rest("codex");
            pane.status = status.into();
            pane.turn = Turn::Read(Waiting::Question);
            assert_eq!(judge(&pane, NOW), Err(Hold::AwaitingOperator), "{status}");
            pane.turn = Turn::Read(Waiting::PlanApproval);
            assert_eq!(judge(&pane, NOW), Err(Hold::AwaitingOperator), "{status}");
            pane.turn = Turn::Unread;
            assert_eq!(judge(&pane, NOW), Err(Hold::SessionUnread), "{status}");
            pane.turn = Turn::Read(Waiting::Nothing);
            assert_eq!(judge(&pane, NOW), Ok(()), "{status}");
        }
        let claude = at_rest("claude");
        assert_eq!(claude.turn, Turn::NotReported);
        assert_eq!(judge(&claude, NOW), Ok(()));
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

    /// A recipient pane at rest on a Herdr whose `agent.get` answer carries
    /// the readiness flags the test sets, with one letter pending for it.
    struct Bell {
        runtime: Arc<Mutex<Runtime>>,
        client: Client,
        herdr: crate::fake_herdr::FakeHerdr,
        flags: Arc<Mutex<Value>>,
        letter: String,
        _worker: crate::delivery::worker::Worker,
        _root: tempfile::TempDir,
    }

    impl Bell {
        fn start(flags: Value) -> Self {
            use crate::delivery::Command;
            use crate::runtime::delivery::tests::{authority, fixture, recipient_at_rest};
            let root = tempfile::tempdir().unwrap();
            let (runtime, sender, _, path) = fixture(root.path());
            let flags = Arc::new(Mutex::new(flags));
            let answer = Arc::clone(&flags);
            let herdr =
                crate::fake_herdr::FakeHerdr::start("doorbell", move |method, _| match method {
                    "agent.get" => {
                        let mut agent = json!({
                            "pane_id": "recipient", "terminal_id": "terminal",
                            "workspace_id": "w1", "tab_id": "w1:t1", "focused": false,
                            "revision": 1, "name": "recipient", "agent": "codex",
                            "agent_status": "idle", "state_change_seq": 2,
                            "agent_session": {"source": "hook", "agent": "codex",
                                "kind": "id", "value": "recipient-native"},
                        });
                        for (key, value) in answer.lock().unwrap().as_object().unwrap() {
                            agent[key] = value.clone();
                        }
                        json!({"type": "agent_info", "agent": agent})
                    }
                    "pane.send_input" => json!({"type": "ok"}),
                    other => panic!("unexpected {other}"),
                });
            let recipient =
                recipient_at_rest(&mut runtime.lock().unwrap(), "recipient-native", &herdr);
            let (worker, client) = crate::delivery::worker::Worker::spawn(
                Arc::downgrade(&runtime),
                crate::handle::ChangeNotifier::noop(),
                path,
            )
            .unwrap();
            let sent = client
                .submit(
                    Effect::Command {
                        authority: authority(&sender),
                        actor: sender.clone(),
                        target: Some(Box::new(recipient)),
                        command: Box::new(Command::Send {
                            target: "recipient".into(),
                            intent: "report".into(),
                            body: "private".into(),
                            kind: "report".into(),
                        }),
                    },
                    Duration::from_secs(5),
                )
                .unwrap();
            Self {
                runtime,
                client,
                herdr,
                flags,
                letter: sent["id"].as_str().unwrap().to_owned(),
                _worker: worker,
                _root: root,
            }
        }

        /// One doorbell pass at `at`, and the reasons it logged.
        fn pass(&self, doorbell: &mut Doorbell, at: u64) -> Vec<Value> {
            let stop = AtomicBool::new(false);
            let ((), records) = crate::diagnostics::capture(|| {
                doorbell.pass(&self.runtime, &self.client, &stop, at)
            });
            records
                .into_iter()
                .filter(|record| record["letter_id"] == self.letter.as_str())
                .collect()
        }

        /// What the pane was typed, in order.
        fn typed(&self) -> Vec<Value> {
            self.herdr
                .calls()
                .into_iter()
                .filter(|(method, _)| method == "pane.send_input")
                .map(|(_, params)| params)
                .collect()
        }

        fn asked(&self) -> usize {
            self.herdr
                .methods()
                .iter()
                .filter(|method| *method == "agent.get")
                .count()
        }

        fn attempts(&self) -> u8 {
            let guard = self.runtime.lock().unwrap();
            let ledger = guard.delivery_state().unwrap();
            ledger
                .letters
                .iter()
                .find(|letter| letter.id == self.letter)
                .unwrap()
                .attempts()
        }
    }

    fn held(records: &[Value]) -> Vec<&str> {
        records
            .iter()
            .filter(|record| record["kind"] == "doorbell.held")
            .filter_map(|record| record["reason"].as_str())
            .collect()
    }

    #[test]
    fn a_pane_herdr_reports_no_readiness_for_is_belled_at_rest_like_a_started_one() {
        // A pane the operator started by typing `codex` carries neither flag;
        // one `herdr agent start` launched reports itself ready.
        for flags in [json!({}), json!({"interactive_ready": true})] {
            let bell = Bell::start(flags.clone());
            let records = bell.pass(&mut Doorbell::default(), now());
            assert_eq!(held(&records), Vec::<&str>::new(), "{flags}");
            let typed = bell.typed();
            assert_eq!(typed.len(), 1, "{flags}");
            assert_eq!(typed[0]["pane_id"], "recipient");
            assert_eq!(
                typed[0]["text"],
                hide_agent_hooks::delivery::BELL_PROMPT,
                "{flags}"
            );
            assert_eq!(typed[0]["keys"], json!(["enter"]));
            assert_eq!(bell.attempts(), 1, "{flags}");
        }
    }

    /// B3, B4: while the recipient's session waits for the operator to
    /// approve a plan, nothing is typed, the letter keeps its attempts, and
    /// the reason is logged once with the letter and pane; once the session
    /// says nothing waits, the same letter is belled.
    #[test]
    fn a_plan_waiting_for_approval_holds_the_letter_until_the_session_moves_on() {
        let bell = Bell::start(json!({}));
        let turn = |value: Turn| {
            crate::runtime::delivery::tests::recipient_turn(
                &mut bell.runtime.lock().unwrap(),
                value,
            );
        };
        turn(Turn::Read(Waiting::PlanApproval));
        let mut doorbell = Doorbell::default();
        let start = now();
        let records = bell.pass(&mut doorbell, start);
        assert_eq!(held(&records), ["awaiting_operator"]);
        let logged = records
            .iter()
            .find(|record| record["kind"] == "doorbell.held")
            .unwrap();
        assert_eq!(logged["pane_id"], "recipient");
        assert!(held(&bell.pass(&mut doorbell, start + 1_000)).is_empty());
        assert!(bell.typed().is_empty());
        assert_eq!(bell.asked(), 0, "a held verdict asks Herdr nothing");
        assert_eq!(bell.attempts(), 0);

        turn(Turn::Read(Waiting::Nothing));
        bell.pass(&mut doorbell, start + 2_000);
        assert_eq!(bell.typed().len(), 1);
        assert_eq!(bell.attempts(), 1);
    }

    #[test]
    fn herdr_reporting_an_agent_not_ready_holds_the_letter_for_that_reason_and_reserves_nothing() {
        for (flags, reason) in [
            (json!({"interactive_ready": false}), "not_ready"),
            (json!({"launch_pending": true}), "launch_pending"),
            (
                json!({"interactive_ready": true, "launch_pending": true}),
                "launch_pending",
            ),
        ] {
            let bell = Bell::start(flags.clone());
            let mut doorbell = Doorbell::default();
            let records = bell.pass(&mut doorbell, now());
            assert_eq!(held(&records), [reason], "{flags}");
            assert!(bell.typed().is_empty(), "{flags}");
            assert_eq!(bell.attempts(), 0, "{flags}");
            // Held until its deadline, it is logged with that reason.
            let records = bell.pass(&mut doorbell, now() + super::super::DELIVERY_EXPIRY_MS);
            let expired: Vec<_> = records
                .iter()
                .filter(|record| record["kind"] == "doorbell.expired")
                .collect();
            assert_eq!(expired.len(), 1, "{flags}");
            assert_eq!(expired[0]["reason"], reason);
            assert!(bell.typed().is_empty(), "{flags}");
        }
    }

    #[test]
    fn a_letter_held_by_herdr_is_belled_once_the_cause_clears_while_the_pane_stays_put() {
        let bell = Bell::start(json!({"launch_pending": true}));
        let mut doorbell = Doorbell::default();
        let start = now();
        assert_eq!(held(&bell.pass(&mut doorbell, start)), ["launch_pending"]);
        // The launch settles without the pane changing status or sequence.
        *bell.flags.lock().unwrap() = json!({"interactive_ready": true});
        // Before the retry is due Herdr is not asked again.
        bell.pass(&mut doorbell, start + RETRY_FIRST_MS - 1);
        assert_eq!(bell.asked(), 1);
        assert!(bell.typed().is_empty());
        bell.pass(&mut doorbell, start + RETRY_FIRST_MS);
        assert_eq!(bell.typed().len(), 1);
        assert_eq!(bell.attempts(), 1);
    }

    #[test]
    fn a_letter_herdr_keeps_refusing_is_asked_about_less_often_and_never_reserves() {
        let bell = Bell::start(json!({"interactive_ready": false}));
        let mut doorbell = Doorbell::default();
        let start = now();
        let mut at = start;
        let mut logged = Vec::new();
        // Twenty minutes of passes, four a second would be 4800 questions.
        while at < start + 20 * 60_000 {
            logged.extend(bell.pass(&mut doorbell, at));
            at += 250;
        }
        // At 0, 5, 15, 35, 75 and 155 s, then every two minutes.
        assert_eq!(bell.asked(), 14);
        assert_eq!(held(&logged), ["not_ready"], "logged once, not per retry");
        assert!(bell.typed().is_empty());
        assert_eq!(bell.attempts(), 0);
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
