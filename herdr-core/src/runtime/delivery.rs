//! Memory-only admission and observations. Disk, process and helper work is
//! owned by delivery workers, never by the owner-thread caller.

use std::collections::HashSet;
use std::sync::Arc;

use crate::delivery::doorbell::Turn;
use crate::delivery::ledger::Ledger;
use crate::delivery::worker::{Authority, Client, Prepared, PreparedHuman};
use crate::delivery::{Actor, Command};
use crate::labels::overlay::LabelOverlay;
use crate::sidebar::SessionSnapshotPayload;
use crate::workspace_control::{Caller, Context, Query};

use super::{Runtime, unix_milliseconds};

pub(crate) const OBSERVATION_LIMIT: usize = 2048;

#[derive(Clone)]
pub(crate) struct Observation {
    pub actor: Actor,
    pub raw_pane_id: String,
    pub status: String,
    pub state_change_seq: Option<u64>,
    pub status_changed_at_unix_ms: u64,
    /// The last input hide routed to this pane: a key, a paste, a phone
    /// write or an agent-find key.
    pub last_input_at_unix_ms: u64,
    /// The last moment the pane's input was proven submitted: a prompt hook
    /// of the pane's own session, or a phone reply. No key proves it. Input
    /// after it and after `entered_working_at_unix_ms` is an unsent draft.
    pub last_submit_at_unix_ms: u64,
    /// The snapshot that first showed the pane `working` in its current run.
    /// Input hide sent before it was consumed by that work, such as the
    /// answer to a menu.
    pub entered_working_at_unix_ms: u64,
    pub session: Option<hide_session::session_activity::SessionActivityRequest>,
    pub host_scope: Option<String>,
    pub turn: Turn,
}

impl Observation {
    /// A Factory's recipient as a letter target: no pane, no status.
    pub(crate) fn code_owned(actor: Actor) -> Self {
        Self {
            raw_pane_id: actor.pane_id.clone(),
            actor,
            status: "idle".into(),
            state_change_seq: None,
            status_changed_at_unix_ms: 0,
            last_input_at_unix_ms: 0,
            last_submit_at_unix_ms: 0,
            entered_working_at_unix_ms: 0,
            session: None,
            host_scope: None,
            turn: Turn::NotReported,
        }
    }
}

impl Runtime {
    /// `labels` is the device's label overlay, whose session reads say what
    /// an agent waits for; without one, an agent whose read reports turns is
    /// not known to wait for nothing.
    pub(crate) fn observe_delivery(
        &mut self,
        device: &str,
        payload: &SessionSnapshotPayload,
        host_scope: Option<&str>,
        labels: Option<&LabelOverlay>,
    ) {
        let now = unix_milliseconds();
        let mut present = HashSet::new();
        let mut excess = false;
        for agent in &payload.agents {
            let (Some(pane), Some(name), Some(kind), Some(status)) = (
                agent.pane_id.as_deref(),
                agent.id.as_deref(),
                agent.agent.as_deref(),
                agent.agent_status.as_deref(),
            ) else {
                continue;
            };
            if ![pane, name, kind, status]
                .into_iter()
                .all(crate::delivery::valid_key)
            {
                continue;
            }
            let pane_id = if self.node == device {
                pane.to_owned()
            } else {
                format!("remote:{device}:pane:{pane}")
            };
            if !self.delivery_observations.contains_key(&pane_id)
                && self.delivery_observations.len() >= OBSERVATION_LIMIT
            {
                excess = true;
                continue;
            }
            present.insert(pane_id.clone());
            let actor = Actor {
                pane_id: pane_id.clone(),
                name: name.to_owned(),
                kind: kind.to_owned(),
                device_id: device.to_owned(),
                session: agent.lineage_session.clone(),
            };
            if !actor.valid() {
                continue;
            }
            let previous = self
                .delivery_observations
                .get(&pane_id)
                .filter(|old| old.actor.same_identity(&actor));
            let persisted = self.delivery_ledger.as_ref().ok().and_then(|ledger| {
                ledger.watches.iter().find(|watch| {
                    watch.target.same_identity(&actor)
                        && watch.last_status == status
                        && watch.last_state_change_seq == agent.state_change_seq
                })
            });
            let changed_at = previous
                .filter(|old| {
                    old.status == status && old.state_change_seq == agent.state_change_seq
                })
                .map(|old| old.status_changed_at_unix_ms)
                .or_else(|| persisted.map(|watch| watch.status_changed_at_unix_ms))
                .unwrap_or(now);
            // The same reading of the kind the bell target uses, so a pane
            // the bell reaches is never one whose session read is skipped.
            let session_kind = crate::agent_hooks::runtime_of(kind).map(|runtime| match runtime {
                hide_agent_hooks::runtime::AgentRuntime::Codex => hide_session::Agent::Codex,
                hide_agent_hooks::runtime::AgentRuntime::ClaudeCode => hide_session::Agent::Claude,
            });
            let turn = match session_kind {
                Some(kind) if kind.reports_turns() => labels
                    .and_then(|labels| labels.waiting(agent))
                    .map_or(Turn::Unread, Turn::Read),
                _ => Turn::NotReported,
            };
            let session = session_kind
                .zip(agent.agent_session.as_ref())
                .filter(|(_, reference)| {
                    reference.value.len() <= 4096
                        && matches!(reference.kind.as_str(), "id" | "path")
                })
                .map(|(agent_kind, reference)| {
                    hide_session::session_activity::SessionActivityRequest {
                        agent: agent_kind,
                        reference_kind: reference.kind.clone(),
                        reference_value: reference.value.clone(),
                        cwd: agent.cwd.clone().filter(|cwd| cwd.len() <= 4096),
                    }
                });
            let observation = Observation {
                actor,
                raw_pane_id: pane.to_owned(),
                status: status.to_owned(),
                state_change_seq: agent.state_change_seq,
                status_changed_at_unix_ms: changed_at,
                // A pane hide has not seen before has no key it knows of, so
                // every clock starts at this observation: a 30 second grace,
                // and no draft assumed.
                last_input_at_unix_ms: previous.map(|old| old.last_input_at_unix_ms).unwrap_or(now),
                last_submit_at_unix_ms: previous
                    .map(|old| old.last_submit_at_unix_ms)
                    .unwrap_or(now),
                entered_working_at_unix_ms: match previous {
                    Some(old) if status != "working" || old.status == "working" => {
                        old.entered_working_at_unix_ms
                    }
                    _ => now,
                },
                session,
                host_scope: host_scope
                    .filter(|scope| scope.len() <= 4096)
                    .map(str::to_owned),
                turn,
            };
            self.delivery_observations.insert(pane_id, observation);
        }
        self.delivery_observations.retain(|pane, observation| {
            observation.actor.device_id != device || present.contains(pane)
        });
        if excess {
            self.delivery_overflow.insert(device.to_owned());
        } else {
            self.delivery_overflow.remove(device);
        }
        self.delivery_connected.insert(device.to_owned());
        self.observe_registration_panes(device, payload, host_scope);
    }

    /// A host's session sync is about to ask Herdr for a fresh snapshot.
    /// Until that snapshot publishes, the host's panes are unknown and no
    /// registration on it is ended.
    pub(crate) fn begin_delivery_pane_read(&mut self, device: &str) {
        if !self.delivery_pane_owner_registered(device) {
            return;
        }
        let floor = self
            .delivery_ledger
            .as_ref()
            .ok()
            .map(|ledger| ledger.next_id);
        self.delivery_panes.insert(
            device.to_owned(),
            crate::coordination::PaneRead {
                host_scope: None,
                floor,
                panes: None,
            },
        );
    }

    /// Records the host's published panes and, when they changed, which live
    /// registrations on it lost their pane. A publish whose panes did not
    /// move costs one borrowed set and no copy; the pass over the
    /// registrations runs only when they moved.
    fn observe_registration_panes(
        &mut self,
        device: &str,
        payload: &SessionSnapshotPayload,
        host_scope: Option<&str>,
    ) {
        if !self.delivery_pane_owner_registered(device) {
            return;
        }
        // An agent's pane is one of the panes; a payload of agents alone
        // (a device read before its layout) still names them.
        let panes: HashSet<&str> = payload
            .panes
            .iter()
            .map(|pane| pane.pane_id.as_str())
            .chain(
                payload
                    .agents
                    .iter()
                    .filter_map(|agent| agent.pane_id.as_deref()),
            )
            .collect();
        let read = self.delivery_panes.get(device);
        if read.is_some_and(|read| {
            read.host_scope.as_deref() == host_scope
                && read.panes.as_ref().is_some_and(|known| {
                    known.len() == panes.len() && panes.iter().all(|pane| known.contains(*pane))
                })
        }) {
            return;
        }
        // A transition judged without the ledger would be lost, so the
        // previous read stays until a ledger can judge it.
        let Ok(ledger) = self.delivery_ledger.as_ref() else {
            return;
        };
        let read =
            self.delivery_panes
                .entry(device.to_owned())
                .or_insert(crate::coordination::PaneRead {
                    host_scope: None,
                    floor: None,
                    panes: None,
                });
        read.host_scope = host_scope.map(str::to_owned);
        let previous = read
            .panes
            .replace(panes.into_iter().map(str::to_owned).collect());
        self.registrations_gone
            .extend(crate::coordination::gone_registrations(
                ledger,
                device,
                read,
                previous.as_ref(),
            ));
    }

    /// A retired remote coordinator may finish before its off-lock join,
    /// but only a registered device still owns a pane read.
    fn delivery_pane_owner_registered(&self, device: &str) -> bool {
        device == self.node.as_str()
            || self
                .snapshot
                .ui_state
                .device_registrations
                .iter()
                .any(|registration| registration.id == device)
    }

    /// The registrations the delivery store has still to end.
    pub(crate) fn delivery_registrations_gone(
        &self,
    ) -> std::collections::BTreeMap<String, crate::coordination::PaneGone> {
        self.registrations_gone.clone()
    }

    /// A key, paste or phone write hide routed to a pane: a draft may have
    /// grown there, so the quiet period restarts. No key ends a draft, not
    /// even Enter: a built-in picker (`/model`, `/resume`) is opened with
    /// Enter and reads `done` to Herdr, so only a prompt hook of the pane's
    /// own session, a phone reply, or the pane entering work proves the
    /// composer was sent.
    pub(crate) fn note_delivery_key(&mut self, pane_id: &str) {
        if let Some(observation) = self.delivery_observations.get_mut(pane_id) {
            observation.last_input_at_unix_ms = unix_milliseconds();
        }
    }

    /// The phone's reply is text hide sent and submitted in one write to an
    /// agent waiting on the operator, so it is a key and a submit.
    pub(crate) fn note_delivery_reply(&mut self, pane_id: &str) {
        if let Some(observation) = self.delivery_observations.get_mut(pane_id) {
            let now = unix_milliseconds();
            observation.last_input_at_unix_ms = now;
            observation.last_submit_at_unix_ms = now;
        }
    }

    /// A prompt hook ran in the pane, so whatever was in its composer was
    /// submitted, whoever typed it and wherever it was typed.
    /// A hook that runs while the pane is already `working` is a queued
    /// prompt being taken up, not the composer being sent: the operator's
    /// newer draft is still there, and the turn's own entry to `working`
    /// already covers whatever was typed before it.
    fn note_prompt_submitted(&mut self, pane_id: &str) {
        if let Some(observation) = self
            .delivery_observations
            .get_mut(pane_id)
            .filter(|observation| observation.status != "working")
        {
            observation.last_submit_at_unix_ms = unix_milliseconds();
        }
    }

    pub(crate) fn prepare_delivery(
        &mut self,
        device: &str,
        caller: &str,
        expected: &Context,
        hint: Option<&str>,
        command: Command,
    ) -> Result<Prepared, String> {
        let context = self
            .workspace_control_query(device, caller, Query::Info)
            .map_err(|refusal| refusal.reason.to_owned())?
            .context;
        if context != *expected {
            return Err("caller_context_changed".into());
        }
        let qualify = |pane: &str| {
            if self.node == device || pane.starts_with(&format!("remote:{device}:pane:")) {
                pane.to_owned()
            } else {
                format!("remote:{device}:pane:{pane}")
            }
        };
        // Only Hide's pane attestation names the agent a command acts as; a
        // checkout-bound caller has no pane, and a hint cannot lend it one.
        let Caller::Pane(actor_pane) = Caller::parse(caller) else {
            return Err("agent_pane_required".into());
        };
        if hint.is_some_and(|hint| qualify(hint) != actor_pane) {
            return Err("caller_identity_conflict".into());
        }
        let actor = self
            .delivery_observations
            .get(actor_pane)
            .ok_or("agent_pane_required")?
            .actor
            .clone();
        actor.require_native_identity()?;
        if crate::delivery::reserved_name(&actor.name) {
            // A pane named like a Factory would read as one (D-14).
            return Err("reserved_name".into());
        }
        // Only the pane's own session proves its composer was submitted, so a
        // stray `hide inbox --hook` from a tool in the pane clears no draft.
        // The id is not a secret (D-18 external input stays outside this). The id is checked before it is
        // hashed because this runs under the runtime lock.
        if let Command::Pull {
            session: Some(session),
            ..
        } = &command
        {
            if !crate::delivery::valid_key(session) {
                return Err("session_invalid".into());
            }
            if crate::wire::session_digest(session) == actor.session {
                self.note_prompt_submitted(&actor.pane_id);
            }
        }
        let resolves_to_caller = |key: &str| {
            key == actor.pane_id
                || key == actor.name
                || self.delivery_ledger.as_ref().is_ok_and(|ledger| {
                    crate::coordination::resolve_actor(ledger, key)
                        .is_some_and(|registered| registered.same_identity(&actor))
                })
        };
        match &command {
            Command::WatchStart {
                actor: declared,
                observer,
                ..
            } => {
                if declared
                    .as_deref()
                    .is_some_and(|key| !resolves_to_caller(key))
                    || observer
                        .as_deref()
                        .is_some_and(|key| !resolves_to_caller(key))
                {
                    return Err("caller_identity_conflict".into());
                }
            }
            Command::WatchAssign {
                actor: declared, ..
            } if declared
                .as_deref()
                .is_some_and(|key| !resolves_to_caller(key)) =>
            {
                return Err("caller_identity_conflict".into());
            }
            _ => {}
        }
        let repeated = match &command {
            Command::Send { intent, .. } => self.delivery_ledger.as_ref().is_ok_and(|ledger| {
                crate::delivery::mailbox::existing_intent(
                    ledger,
                    &actor,
                    intent,
                    unix_milliseconds(),
                )
                .is_some()
            }),
            _ => false,
        };
        let target = match &command {
            Command::Send { .. } if repeated => None,
            Command::Send { target, .. }
            | Command::WatchStart { target, .. }
            | Command::WatchAssign {
                observer: target, ..
            } if target.starts_with(crate::delivery::FACTORY_PREFIX) => {
                // A Factory is addressed by its code-owned name only; no
                // pane can stand in for it.
                let actor = Actor::factory(
                    &target[crate::delivery::FACTORY_PREFIX.len()..],
                    self.node.as_str(),
                );
                if !self.factory_recipient_current(&actor) {
                    return Err("target_unavailable".into());
                }
                Some(Observation::code_owned(actor))
            }
            Command::Send { target, .. }
            | Command::WatchStart { target, .. }
            | Command::WatchAssign {
                observer: target, ..
            } => {
                let registered = self
                    .delivery_ledger
                    .as_ref()
                    .ok()
                    .and_then(|ledger| crate::coordination::resolve_actor(ledger, target));
                let mut matches = self.delivery_observations.values().filter(|observation| {
                    observation.actor.pane_id == *target
                        || observation.actor.name == *target
                        || registered
                            .is_some_and(|registered| registered.same_identity(&observation.actor))
                });
                let first = matches.next().cloned();
                if matches.next().is_some() {
                    return Err("target_ambiguous".into());
                }
                if first.is_none() && !self.delivery_overflow.is_empty() {
                    return Err("capacity".into());
                }
                first
            }
            _ => None,
        };
        if (matches!(&command, Command::Send { .. }) && !repeated)
            || matches!(&command, Command::WatchAssign { .. })
        {
            target
                .as_ref()
                .ok_or("target_unavailable")?
                .actor
                .require_native_identity()?;
        }
        Ok(Prepared::new(
            self.delivery_client.clone().ok_or("delivery_unavailable")?,
            Authority {
                caller: caller.to_owned(),
                context,
            },
            actor,
            target,
            command,
        ))
    }

    pub(crate) fn install_delivery_client(&mut self, client: Client) {
        self.delivery_client = Some(client);
    }

    pub(crate) fn prepare_delivery_human(&self) -> Result<PreparedHuman, String> {
        self.delivery_state()?;
        Ok(PreparedHuman::new(
            self.delivery_client.clone().ok_or("delivery_unavailable")?,
        ))
    }

    pub(crate) fn delivery_state(&self) -> Result<Arc<Ledger>, String> {
        self.delivery_ledger.clone()
    }

    pub(crate) fn invalidate_delivery(&mut self) {
        // Preserve installed bytes for startup recovery. In particular no
        // subsequent pull, tick or bell may acknowledge our old memory image.
        self.delivery_ledger = Err("ledger_unavailable".into());
    }

    pub(crate) fn publish_delivery(&mut self, ledger: Arc<Ledger>, transitions: bool) -> bool {
        let before = self.delivery_ledger.as_ref().ok().map(|ledger| {
            ledger
                .watches
                .iter()
                .map(|watch| watch.view())
                .collect::<Vec<_>>()
        });
        let after = ledger
            .watches
            .iter()
            .map(|watch| watch.view())
            .collect::<Vec<_>>();
        let changed = transitions && before.as_ref() != Some(&after);
        if changed {
            self.snapshot.delivery_watches = after;
        }
        if !self.registrations_gone.is_empty() {
            self.registrations_gone.retain(|id, _| {
                ledger
                    .agents
                    .iter()
                    .any(|record| &record.id == id && !record.ended)
            });
        }
        self.delivery_ledger = Ok(ledger);
        self.feed_link_parents();
        changed
    }

    /// The Factory host publishes which Factories exist.
    /// The open Factories, each with the inactivity window its workers'
    /// watches use.
    pub(crate) fn set_factory_recipients(
        &mut self,
        factories: std::collections::BTreeMap<String, u64>,
    ) {
        self.factory_recipients = factories;
    }

    /// A code-owned recipient is current while its Factory exists and the
    /// actor is exactly the one the host constructs.
    pub(crate) fn factory_recipient_current(&self, actor: &Actor) -> bool {
        actor
            .pane_id
            .strip_prefix(crate::delivery::FACTORY_PREFIX)
            .is_some_and(|id| {
                self.factory_recipients.contains_key(id)
                    && *actor == Actor::factory(id, self.node.as_str())
            })
    }

    /// The authority the Factory host acts with: its own recipient, never a
    /// pane's.
    pub(crate) fn factory_authority_current(&self, caller: &str, actor: &Actor) -> bool {
        caller == actor.pane_id && self.factory_recipient_current(actor)
    }

    /// A delivery client and authority for the Factory host (D-14).
    pub(crate) fn factory_delivery(
        &self,
        id: &str,
    ) -> Result<(crate::delivery::worker::Client, Authority, Actor), String> {
        let actor = Actor::factory(id, self.node.as_str());
        if !self.factory_recipient_current(&actor) {
            return Err("factory_unavailable".into());
        }
        let context = Context {
            device_id: self.node.as_str().into(),
            workspace_id: actor.pane_id.clone(),
            checkout_id: actor.pane_id.clone(),
            checkout_path: String::new(),
        };
        Ok((
            self.delivery_client.clone().ok_or("delivery_unavailable")?,
            Authority {
                caller: actor.pane_id.clone(),
                context,
            },
            actor,
        ))
    }

    pub(crate) fn delivery_identity_current(&self, actor: &Actor) -> bool {
        self.delivery_observations
            .get(&actor.pane_id)
            .is_some_and(|observation| observation.actor.same_identity(actor))
    }

    pub(crate) fn delivery_connector(
        &self,
        device: &str,
    ) -> Option<Arc<dyn hide_herdr_client::ApiConnector>> {
        if self.node == device {
            self.live.as_ref().map(|live| live.api_connector.clone())
        } else {
            self.remote_herdr_api(device)
        }
    }

    #[cfg(test)]
    pub(crate) fn delivery_observation(&self, actor: &Actor) -> Option<Observation> {
        self.delivery_observations
            .get(&actor.pane_id)
            .filter(|observation| observation.actor.same_identity(actor))
            .cloned()
    }

    /// Whether a bell may be typed into the recipient's pane now, and the
    /// observation the verdict was read from. The refusal says which fact
    /// held the letter, for the diagnostic log.
    pub(crate) fn delivery_bell_verdict(
        &self,
        recipient: &Actor,
        now: u64,
    ) -> Result<Observation, crate::delivery::doorbell::Hold> {
        use crate::delivery::doorbell::Hold;
        let current = self
            .delivery_observations
            .get(&recipient.pane_id)
            .filter(|observation| observation.actor.device_id == recipient.device_id)
            .ok_or(Hold::Absent)?;
        if !current.actor.same_identity(recipient) {
            return Err(Hold::Session);
        }
        crate::delivery::doorbell::judge(current, now)?;
        Ok(current.clone())
    }

    /// The final memory guard immediately before the off-lock pane write.
    /// The refusal names what moved since the verdict, for the diagnostic
    /// log.
    pub(crate) fn delivery_bell_current(
        &self,
        id: &str,
        observed: &Observation,
        reserved: Option<u8>,
    ) -> Result<(), crate::delivery::doorbell::Hold> {
        use crate::delivery::doorbell::Hold;
        let current = self
            .delivery_observations
            .get(&observed.actor.pane_id)
            .ok_or(Hold::Absent)?;
        if !current.actor.same_identity(&observed.actor) {
            return Err(Hold::Session);
        }
        if current.status != observed.status
            || current.state_change_seq != observed.state_change_seq
        {
            return Err(Hold::Moved);
        }
        if current.last_input_at_unix_ms != observed.last_input_at_unix_ms {
            return Err(Hold::Input);
        }
        let now = unix_milliseconds();
        crate::delivery::doorbell::judge(current, now)?;
        let letter_current = self.delivery_ledger.as_ref().is_ok_and(|ledger| {
            ledger.letters.iter().any(|letter| {
                letter.id == id
                    && letter.recipient.same_identity(&observed.actor)
                    && letter.state == crate::delivery::ledger::State::Pending
                    && match reserved {
                        Some(attempt) => (1..=3).contains(&attempt) && letter.attempts() == attempt,
                        None => letter.attempts() < 3,
                    }
                    && now.saturating_sub(letter.created_at_unix_ms)
                        < crate::delivery::DELIVERY_EXPIRY_MS
            })
        });
        letter_current.then_some(()).ok_or(Hold::Letter)
    }

    pub(crate) fn delivery_watch_work(&mut self) -> Vec<crate::delivery::worker::WatchWork> {
        let Ok(ledger) = self.delivery_state() else {
            return Vec::new();
        };
        ledger
            .watches
            .iter()
            .map(|watch| {
                let current = self.delivery_observations.get(&watch.target.pane_id);
                let observation = current
                    .filter(|observation| {
                        watch.target.require_native_identity().is_ok()
                            && observation.actor.require_native_identity().is_ok()
                            && observation.actor.same_identity(&watch.target)
                    })
                    .cloned();
                // Acquisition or temporary loss of a native reference does
                // not prove an execution ended. Preserve the original binding
                // and clocks, and withhold unproven status/file metadata.
                let proven_absence_or_replacement = match current {
                    None => true,
                    Some(observation) => matches!(
                        (&watch.target.session, &observation.actor.session),
                        (Some(original), Some(current)) if original != current
                    ),
                };
                let gone = proven_absence_or_replacement
                    && self.delivery_connected.contains(&watch.target.device_id)
                    && !self.delivery_overflow.contains(&watch.target.device_id);
                let source = crate::delivery::worker::ActivitySource {
                    link: if self.node == watch.target.device_id {
                        Some(Arc::clone(&self.own_node))
                    } else {
                        observation
                            .is_some()
                            .then(|| self.node_link(&watch.target.device_id).ok())
                            .flatten()
                    },
                };
                crate::delivery::worker::WatchWork {
                    id: watch.id.clone(),
                    observation,
                    gone,
                    status: watch.last_status.clone(),
                    state_change_seq: watch.last_state_change_seq,
                    status_changed_at_unix_ms: watch.status_changed_at_unix_ms,
                    source,
                    inactivity_ms: watch
                        .parent
                        .pane_id
                        .strip_prefix(crate::delivery::FACTORY_PREFIX)
                        .and_then(|id| self.factory_recipients.get(id).copied()),
                }
            })
            .collect()
    }
}

impl Runtime {
    pub(crate) fn coordination_fork_context(
        &self,
        pane: &str,
    ) -> Result<(Client, Authority, Actor), String> {
        let actor = self
            .delivery_observations
            .get(pane)
            .ok_or("parent_unavailable")?
            .actor
            .clone();
        let context = self
            .workspace_control_query(&actor.device_id, pane, Query::Info)
            .map_err(|_| "parent_unavailable")?
            .context;
        Ok((
            self.delivery_client.clone().ok_or("delivery_unavailable")?,
            Authority {
                caller: pane.into(),
                context,
            },
            actor,
        ))
    }
    pub(crate) fn coordination_context(&self, device: &str) -> Result<CoordinationContext, String> {
        let on_node = self.node == device;
        let (connector, scope, machine) = if on_node {
            let live = self.live.as_ref().ok_or("herdr_unavailable")?;
            (
                live.api_connector.clone(),
                live.socket_path
                    .to_str()
                    .ok_or("host_scope_unavailable")?
                    .to_owned(),
                self.node.to_string(),
            )
        } else {
            (
                self.remote_herdr_api(device).ok_or("device_unavailable")?,
                device.to_owned(),
                self.device_machine_ids
                    .get(device)
                    .cloned()
                    .ok_or("machine_identity_unavailable")?,
            )
        };
        Ok(CoordinationContext {
            connector,
            host_scope: scope,
            machine,
            codex: self.codex_daemon(device),
            on_node,
        })
    }
}

/// What a coordination command needs of the machine it acts on.
pub(crate) struct CoordinationContext {
    pub(crate) connector: Arc<dyn hide_herdr_client::ApiConnector>,
    pub(crate) host_scope: String,
    pub(crate) machine: String,
    pub(crate) codex: crate::codex_launch::CodexDaemon,
    /// The machine is the core's own node, whose Herdr the core reaches directly.
    pub(crate) on_node: bool,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::delivery::worker::Worker;
    use crate::handle::ChangeNotifier;
    use crate::model::{CoreOptions, SCHEMA_VERSION};
    use crate::sidebar::SessionSnapshotPayload;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// The state a retired coordinator must leave exactly as its owner left it.
    pub(crate) fn coordinator_memory(runtime: &Runtime) -> serde_json::Value {
        let panes = runtime
            .delivery_panes
            .iter()
            .map(|(device, read)| {
                let panes = read
                    .panes
                    .as_ref()
                    .map(|panes| panes.iter().collect::<std::collections::BTreeSet<_>>());
                (
                    device,
                    json!({"scope": read.host_scope, "floor": read.floor, "panes": panes}),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let observations = runtime
            .delivery_observations
            .iter()
            .map(|(pane, observed)| {
                (
                    pane,
                    json!({
                        "actor": observed.actor, "raw_pane": observed.raw_pane_id,
                        "status": observed.status, "sequence": observed.state_change_seq,
                        "changed": observed.status_changed_at_unix_ms,
                        "input": observed.last_input_at_unix_ms,
                        "submit": observed.last_submit_at_unix_ms,
                        "working": observed.entered_working_at_unix_ms,
                        "scope": observed.host_scope, "turn": format!("{:?}", observed.turn),
                        "session": observed.session.as_ref().map(|session| json!({
                            "kind": session.reference_kind, "value": session.reference_value,
                            "cwd": session.cwd,
                        })),
                    }),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let gone = runtime
            .registrations_gone
            .iter()
            .map(|(id, reason)| (id, reason.reason()))
            .collect::<std::collections::BTreeMap<_, _>>();
        json!({
            "panes": panes, "observations": observations, "gone": gone,
            "overflow": runtime.delivery_overflow.iter().collect::<std::collections::BTreeSet<_>>(),
            "connected": runtime.delivery_connected.iter().collect::<std::collections::BTreeSet<_>>(),
            "remote": runtime.snapshot.status.remote,
            "raw_sessions": runtime.device_raw_sessions.iter().collect::<std::collections::BTreeMap<_, _>>(),
            "generations": runtime.remote_connection_generations.iter().collect::<std::collections::BTreeMap<_, _>>(),
        })
    }

    pub(crate) fn coordinator_bootstrap(
        runtime: &mut Runtime,
        device: &str,
        connector: &Arc<dyn hide_herdr_client::ApiConnector>,
    ) {
        let control = runtime
            .remote_controls
            .get(device)
            .expect("installed owner");
        assert!(Arc::ptr_eq(&control.api_connector(), connector));
        let status = runtime
            .snapshot
            .status
            .remote
            .iter_mut()
            .find(|status| status.target_id == device)
            .expect("registered device status");
        status.state = "not_connected".into();
        status.message = None;
        assert!(status.session.is_none());
    }

    pub(crate) fn fixture(
        root: &std::path::Path,
    ) -> (Arc<Mutex<Runtime>>, Actor, Observation, PathBuf) {
        let state = root.join("state");
        hide_platform::fs::private::create_dir_all(&state).unwrap();
        let options: CoreOptions = serde_json::from_value(json!({
            "schema_version":SCHEMA_VERSION,"node_id":"test-node","home":root,"herdr_socket_path":null,
            "app_state_path":state.join("app.json"),
            "workspace_views_path":root.join("views.json"),
        }))
        .unwrap();
        let mut runtime = Runtime::new(
            options,
            crate::environment::EnvironmentReport {
                statuses: Vec::new(),
                home_path: Some(root.to_owned()),
                codex_home: None,
            },
            std::sync::Arc::new(hide_node::Local::new(Some(root.to_owned()))),
        );
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"recipient-session"},
        ]})).unwrap();
        runtime.observe_delivery(crate::node::TEST_NODE, &payload, None, None);
        runtime.snapshot.status.herdr.state = "connected".into();
        let panes = ["sender", "recipient"]
            .into_iter()
            .map(|id| crate::model::PaneSnapshot {
                id: id.into(),
                herdr_label: None,
                terminal_title: None,
                cwd: "/checkouts/fixture".into(),
                status_code: crate::model::AgentStatusCode::Attached,
                requires_close_confirmation: false,
                requires_close_status_check: false,
                identity_label: None,
                activity_at_unix_ms: None,
                fork: Default::default(),
                ports: Vec::new(),
                servers: Vec::new(),
                children: None,
                lineage_path: Vec::new(),
                sleep: None,
                sleep_action: None,
            })
            .collect();
        runtime.snapshot.navigator.workspaces = vec![crate::model::WorkspaceSnapshot {
            home_issues: Default::default(),
            tasks: Default::default(),
            pull_requests: Vec::new(),
            id: "workspace".into(),
            label: "Fixture".into(),
            path: "/checkouts/fixture".into(),
            remote_target_id: None,
            expanded: true,
            device_id: crate::node::TEST_NODE.into(),
            repo_name: "fixture".into(),
            is_git: false,
            default_branch: None,
            branches: Vec::new(),
            registered: true,
            temporary: false,
            session_workspace_ids: Vec::new(),
            last_activity_unix_ms: None,
            pinned: false,
            is_home: false,
            checkouts: vec![crate::model::CheckoutSnapshot {
                id: "checkout".into(),
                workspace_id: "workspace".into(),
                path: "/checkouts/fixture".into(),
                exists: true,
                has_panes: true,
                tabs: vec![crate::model::TabSnapshot {
                    naming: Default::default(),
                    agent: None,
                    id: Some("tab".into()),
                    workspace_id: Some("workspace".into()),
                    checkout_id: Some("checkout".into()),
                    label: None,
                    empty: false,
                    delegated: false,
                    panes,
                }],
                ..Default::default()
            }],
            inactive_checkouts: Default::default(),
            removal: Default::default(),
            disk: Default::default(),
            cleanup: None,
        }];
        let actor = Actor {
            pane_id: "sender".into(),
            name: "sender".into(),
            kind: "codex".into(),
            device_id: crate::node::TEST_NODE.into(),
            session: Some("sender-session".into()),
        };
        let target = runtime
            .delivery_observation(&Actor {
                pane_id: "recipient".into(),
                name: "recipient".into(),
                kind: "codex".into(),
                device_id: crate::node::TEST_NODE.into(),
                session: Some("recipient-session".into()),
            })
            .unwrap();
        (
            Arc::new(Mutex::new(runtime)),
            actor,
            target,
            hide_kit::layout::delivery_ledger(&state),
        )
    }

    pub(crate) fn authority(actor: &Actor) -> Authority {
        Authority {
            caller: actor.pane_id.clone(),
            context: Context {
                device_id: crate::node::TEST_NODE.into(),
                workspace_id: "workspace".into(),
                checkout_id: "checkout".into(),
                checkout_path: "/checkouts/fixture".into(),
            },
        }
    }

    /// Puts the fixture's recipient at rest in native session `native`, idle
    /// with no key and no change of status for long past the quiet period,
    /// and makes `herdr` the Herdr this machine's panes are reached through.
    pub(crate) fn recipient_at_rest(
        runtime: &mut Runtime,
        native: &str,
        herdr: &crate::fake_herdr::FakeHerdr,
    ) -> Observation {
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"idle","state_change_seq":2,
             "lineage_session":crate::wire::session_digest(native)},
        ]})).unwrap();
        runtime.observe_delivery(crate::node::TEST_NODE, &payload, None, None);
        let observation = runtime.delivery_observations.get_mut("recipient").unwrap();
        read_nothing_waits(observation);
        observation.status_changed_at_unix_ms = 0;
        observation.last_input_at_unix_ms = 0;
        observation.last_submit_at_unix_ms = 0;
        observation.entered_working_at_unix_ms = 0;
        let observation = observation.clone();
        runtime.live = Some(crate::live::LiveContext {
            socket_path: herdr.socket_path().to_path_buf(),
            herdr_bin: None,
            runtime: std::sync::Weak::new(),
            notifier: ChangeNotifier::noop(),
            api_connector: Arc::new(herdr.connector()),
            node: Arc::new(hide_node::Local::of_process()),
        });
        observation
    }

    fn key_event(pane: &str, bytes: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "schema_version": SCHEMA_VERSION, "kind": "key",
            "payload": {"pane_id": pane, "bytes_base64": crate::live::encode_base64(bytes)},
        }))
        .unwrap()
    }

    fn observe_recipient_status(runtime: &mut Runtime, status: &str, sequence: u64) {
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":status,"state_change_seq":sequence,"lineage_session":"recipient-session"},
        ]})).unwrap();
        runtime.observe_delivery(crate::node::TEST_NODE, &payload, None, None);
        read_nothing_waits(runtime.delivery_observations.get_mut("recipient").unwrap());
    }

    /// The recipient's session read says nothing waits for the operator in
    /// its current state, as the label overlay answers once it has read it;
    /// what the read says is tested on its own
    /// (`the_session_read_decides_the_turn_for_the_current_state_only`).
    pub(crate) fn read_nothing_waits(observation: &mut Observation) {
        observation.turn = Turn::Read(hide_session::turns::Waiting::Nothing);
    }

    /// What the recipient's session read says, as a doorbell test sets it.
    pub(crate) fn recipient_turn(runtime: &mut Runtime, turn: Turn) {
        runtime
            .delivery_observations
            .get_mut("recipient")
            .unwrap()
            .turn = turn;
    }

    /// The three facts the bell reads, per pane, optionally set to zero so a
    /// test sees which of them an event wrote.
    fn clocks(runtime: &mut Runtime, pane: &str, reset: bool) -> (u64, u64, u64) {
        let observation = runtime.delivery_observations.get_mut(pane).unwrap();
        let read = (
            observation.last_input_at_unix_ms,
            observation.last_submit_at_unix_ms,
            observation.entered_working_at_unix_ms,
        );
        if reset {
            observation.last_input_at_unix_ms = 0;
            observation.last_submit_at_unix_ms = 0;
            observation.entered_working_at_unix_ms = 0;
        }
        read
    }

    /// Which clocks an event wrote since the last call, as (key, submit, work).
    fn written(runtime: &mut Runtime) -> (bool, bool, bool) {
        let (input, submit, working) = clocks(runtime, "recipient", true);
        (input > 0, submit > 0, working > 0)
    }

    #[test]
    fn no_hide_key_ends_a_draft_not_even_the_enter_that_opens_a_picker() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        observe_recipient_status(&mut guard, "idle", 2);
        clocks(&mut guard, "recipient", true);
        clocks(&mut guard, "sender", true);

        // `/model` and Enter: the picker is open and Herdr reads the pane
        // `done`, so the bell must keep holding on a draft.
        for keys in [&b"/model"[..], b"\x1b", b"\x1b\r", b"\r"] {
            guard.dispatch_json(&key_event("recipient", keys));
            assert_eq!(written(&mut guard), (true, false, false), "{keys:?}");
        }
        let observation = guard.delivery_observations.get_mut("recipient").unwrap();
        observation.last_input_at_unix_ms = unix_milliseconds().saturating_sub(120_000);
        observation.status_changed_at_unix_ms = unix_milliseconds().saturating_sub(120_000);
        let held = guard
            .delivery_observations
            .get("recipient")
            .map(|observation| crate::delivery::doorbell::judge(observation, unix_milliseconds()));
        assert_eq!(held, Some(Err(crate::delivery::doorbell::Hold::Draft)));
        assert_eq!(
            clocks(&mut guard, "sender", false),
            (0, 0, 0),
            "another pane's clocks do not move"
        );

        // The phone's reply is a hide key and a submit.
        observe_recipient_status(&mut guard, "idle", 4);
        clocks(&mut guard, "recipient", true);
        guard.dispatch_json(
            &serde_json::to_vec(&json!({
                "schema_version": SCHEMA_VERSION, "kind": "pane_input_submitted",
                "payload": {"pane_id": "recipient"},
            }))
            .unwrap(),
        );
        assert_eq!(written(&mut guard), (true, true, false));
    }

    #[test]
    fn a_phone_write_is_input_and_only_a_reply_is_a_submit() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        observe_recipient_status(&mut guard, "idle", 2);
        clocks(&mut guard, "recipient", true);
        guard.dispatch_json(
            &serde_json::to_vec(&json!({
                "schema_version": SCHEMA_VERSION, "kind": "pane_input_sent",
                "payload": {"pane_id": "recipient"},
            }))
            .unwrap(),
        );
        assert_eq!(written(&mut guard), (true, false, false));
    }

    #[test]
    fn entering_work_is_stamped_once_and_staying_in_it_or_leaving_it_is_not() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        observe_recipient_status(&mut guard, "idle", 2);
        clocks(&mut guard, "recipient", true);
        observe_recipient_status(&mut guard, "working", 3);
        assert_eq!(written(&mut guard), (false, false, true));
        observe_recipient_status(&mut guard, "working", 4);
        assert_eq!(written(&mut guard), (false, false, false));
        observe_recipient_status(&mut guard, "blocked", 5);
        assert_eq!(written(&mut guard), (false, false, false));
        // Answering the menu puts the agent back to work: a new entry.
        observe_recipient_status(&mut guard, "working", 6);
        assert_eq!(written(&mut guard), (false, false, true));
        observe_recipient_status(&mut guard, "idle", 7);
        assert_eq!(written(&mut guard), (false, false, false));
    }

    /// The overlay a label worker would publish for a Codex pane whose
    /// session read, under Herdr state `seq`, found a finished plan turn.
    fn plan_overlay(pane: &str, native: &str, seq: u64) -> LabelOverlay {
        use hide_session::turns::TurnMark;
        let turn = Some("turn-1".to_owned());
        read_overlay(
            pane,
            native,
            seq,
            &[
                TurnMark::Started {
                    turn: turn.clone(),
                    mode: hide_session::turns::TurnMode::Plan,
                },
                TurnMark::Plan { turn: turn.clone() },
                TurnMark::Completed { turn },
            ],
        )
    }

    /// The overlay for a Codex pane whose session read, under Herdr state
    /// `seq`, found these records.
    fn read_overlay(
        pane: &str,
        native: &str,
        seq: u64,
        marks: &[hide_session::turns::TurnMark],
    ) -> LabelOverlay {
        let mut turns = hide_session::turns::TurnTracker::default();
        for (offset, mark) in marks.iter().enumerate() {
            turns.fold(offset as u64 * 10, mark);
        }
        let record = crate::labels::store::PaneRecord {
            owner: hide_session::label_reference_token("codex", "id", native),
            turns: Some(turns),
            turns_seq: Some(seq),
            ..Default::default()
        };
        LabelOverlay::of_records([(&pane.to_owned(), &record)], true, false)
    }

    /// PRD codex-plan-approval-hold D-04, D-06, D-07: the bell's turn fact is
    /// the overlay's read for the agent's current state and session; another
    /// state, another session or no overlay is not known for an agent whose
    /// read reports turns, and an agent whose read reports none is unchanged.
    #[test]
    fn the_session_read_decides_the_turn_for_the_current_state_only() {
        use crate::delivery::doorbell::Turn;
        use hide_session::turns::Waiting;
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        let overlay = plan_overlay("recipient", "recipient-native", 2);
        let observe = |guard: &mut Runtime, agent: &str, seq: u64, native: &str, labels| {
            let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
                {"id":"recipient","pane_id":"recipient","agent":agent,"agent_status":"done",
                 "state_change_seq":seq,"lineage_session":"recipient-session",
                 "agent_session":{"kind":"id","value":native}},
            ]}))
            .unwrap();
            guard.observe_delivery(crate::node::TEST_NODE, &payload, None, labels);
            guard.delivery_observations["recipient"].turn
        };
        assert_eq!(
            observe(&mut guard, "codex", 2, "recipient-native", Some(&overlay)),
            Turn::Read(Waiting::PlanApproval)
        );
        assert_eq!(
            observe(&mut guard, "codex", 3, "recipient-native", Some(&overlay)),
            Turn::Unread,
            "a newer state than the read"
        );
        assert_eq!(
            observe(&mut guard, "codex", 2, "another-native", Some(&overlay)),
            Turn::Unread,
            "another session in the pane"
        );
        assert_eq!(
            observe(&mut guard, "codex", 2, "recipient-native", None),
            Turn::Unread
        );
        assert_eq!(
            observe(&mut guard, "claude", 2, "recipient-native", None),
            Turn::NotReported
        );
        assert_eq!(
            observe(&mut guard, " Codex", 2, "recipient-native", Some(&overlay)),
            Turn::Unread,
            "the kind as the bell target reads it"
        );
    }

    /// B5: Herdr can read Codex done before its session file records the end
    /// of the turn. Only a plan-mode turn can end waiting for approval, so an
    /// ordinary turn still running in the file rings as before; an unfinished
    /// plan-mode turn is not known and holds until the next state is read.
    #[test]
    fn a_turn_not_yet_ended_in_the_file_rings_unless_it_runs_in_plan_mode() {
        use crate::delivery::doorbell::Hold;
        use hide_session::turns::{TurnMark, TurnMode};
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, target, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        let verdict = |guard: &mut Runtime, mode: TurnMode| {
            let started = TurnMark::Started {
                turn: Some("turn-1".to_owned()),
                mode,
            };
            let overlay = read_overlay("recipient", "recipient-native", 2, &[started]);
            let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
                {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"done",
                 "state_change_seq":2,"lineage_session":"recipient-session",
                 "agent_session":{"kind":"id","value":"recipient-native"}},
            ]}))
            .unwrap();
            guard.observe_delivery(crate::node::TEST_NODE, &payload, None, Some(&overlay));
            let long_ago = unix_milliseconds().saturating_sub(10 * 60_000);
            let observation = guard.delivery_observations.get_mut("recipient").unwrap();
            observation.last_input_at_unix_ms = long_ago;
            observation.last_submit_at_unix_ms = long_ago;
            observation.status_changed_at_unix_ms = long_ago;
            guard
                .delivery_bell_verdict(&target.actor, unix_milliseconds())
                .err()
        };
        assert_eq!(
            verdict(&mut guard, TurnMode::Other),
            None,
            "a default-mode turn rings"
        );
        assert_eq!(
            verdict(&mut guard, TurnMode::Plan),
            Some(Hold::SessionUnread)
        );
        assert_eq!(
            verdict(&mut guard, TurnMode::Unknown),
            Some(Hold::SessionUnread)
        );
    }

    #[test]
    fn the_bell_verdict_names_a_replaced_session_and_an_absent_pane() {
        use crate::delivery::doorbell::Hold;
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, target, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        observe_recipient_status(&mut guard, "idle", 2);
        let long_ago = unix_milliseconds().saturating_sub(10 * 60_000);
        for observation in guard.delivery_observations.values_mut() {
            observation.last_input_at_unix_ms = long_ago;
            observation.last_submit_at_unix_ms = long_ago;
            observation.status_changed_at_unix_ms = long_ago;
        }
        let now = unix_milliseconds();
        assert!(guard.delivery_bell_verdict(&target.actor, now).is_ok());
        assert_eq!(
            guard.delivery_bell_verdict(&target.actor, long_ago).err(),
            Some(Hold::Quiet)
        );
        let replaced: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"idle","state_change_seq":2,"lineage_session":"replacement-session"},
        ]})).unwrap();
        guard.observe_delivery(crate::node::TEST_NODE, &replaced, None, None);
        assert_eq!(
            guard.delivery_bell_verdict(&target.actor, now).err(),
            Some(Hold::Session)
        );
        let empty: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[]})).unwrap();
        guard.observe_delivery(crate::node::TEST_NODE, &empty, None, None);
        assert_eq!(
            guard.delivery_bell_verdict(&target.actor, now).err(),
            Some(Hold::Absent)
        );
    }

    /// `hide agent show here` answers who the caller is, so only a
    /// pane-bound credential can ask, as for every delivery command, and a
    /// pane-bound one whose hint names another pane is refused. What it
    /// answers is the caller's own record and never another pane's, with no
    /// renderer connected.
    #[test]
    fn only_the_attested_pane_asks_who_it_is() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, target, path) = fixture(root.path());
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        runtime.lock().unwrap().install_delivery_client(client);
        let context = authority(&target.actor).context;
        let show = |id: &str| Command::Agents {
            command: crate::coordination::Command::Show { id: id.into() },
        };
        let checkout =
            crate::workspace_control::checkout_caller_id(&"a".repeat(32), "/checkouts/fixture");
        let ask = |caller: &str, hint: Option<&str>, id: &str| {
            let prepared = runtime.lock().unwrap().prepare_delivery(
                crate::node::TEST_NODE,
                caller,
                &context,
                hint,
                show(id),
            )?;
            prepared.run(Duration::from_secs(5))
        };
        assert_eq!(
            ask(&checkout, Some("recipient"), "here").err().as_deref(),
            Some("agent_pane_required")
        );
        assert_eq!(
            ask("sender", Some("recipient"), "here").err().as_deref(),
            Some("caller_identity_conflict")
        );
        assert_eq!(
            ask("recipient", None, "here").err().as_deref(),
            Some("participant_unavailable")
        );
        {
            let mut guard = runtime.lock().unwrap();
            let actor = guard.delivery_observations["recipient"].actor.clone();
            let mut ledger = (*guard.delivery_state().unwrap()).clone();
            ledger.agents.push(crate::coordination::AgentRecord {
                id: "agent-7".into(),
                name: "recipient".into(),
                machine: crate::node::TEST_NODE.into(),
                host_scope: "fixture-scope".into(),
                native_machine: "fixture-machine".into(),
                session: "recipient-session".into(),
                instance: "terminal-recipient".into(),
                pane: "recipient".into(),
                parent: None,
                origin: None,
                project: None,
                actor,
                ended: false,
            });
            guard.delivery_ledger = Ok(Arc::new(ledger));
        }
        let own = ask("recipient", Some("recipient"), "here").unwrap();
        assert_eq!(
            (&own["id"], &own["pane"]),
            (&json!("agent-7"), &json!("recipient"))
        );
        // Another pane of the same checkout is not that participant.
        assert_eq!(
            ask("sender", None, "here").err().as_deref(),
            Some("participant_unavailable")
        );
        // Another pane shows the agent by name; a checkout-bound credential
        // shows none, since every agent command needs a pane-bound caller.
        assert_eq!(ask("sender", None, "agent-7").unwrap()["id"], "agent-7");
        assert_eq!(
            ask(&checkout, Some("sender"), "agent-7").err().as_deref(),
            Some("agent_pane_required")
        );
        drop(worker);
    }

    #[test]
    fn only_a_prompt_hook_of_the_panes_own_session_is_a_submission() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, target, path) = fixture(root.path());
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        let mut guard = runtime.lock().unwrap();
        guard.install_delivery_client(client);
        let own = crate::wire::session_digest("hook-session");
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"idle","state_change_seq":2,"lineage_session":own},
        ]})).unwrap();
        guard.observe_delivery(crate::node::TEST_NODE, &payload, None, None);
        let context = authority(&target.actor).context;
        clocks(&mut guard, "recipient", true);
        guard
            .prepare_delivery(
                crate::node::TEST_NODE,
                "recipient",
                &context,
                None,
                Command::Inbox,
            )
            .unwrap();
        assert_eq!(written(&mut guard), (false, false, false));
        for (bell, session, submitted) in [
            (false, Some("hook-session"), true),
            (true, Some("hook-session"), true),
            // Any process in the pane can run the hook command; a session
            // that is not the pane's own, or none, clears no draft.
            (false, Some("another-session"), false),
            (true, None, false),
        ] {
            guard
                .prepare_delivery(
                    crate::node::TEST_NODE,
                    "recipient",
                    &context,
                    None,
                    Command::Pull {
                        bell,
                        session: session.map(str::to_owned),
                    },
                )
                .unwrap();
            assert_eq!(
                written(&mut guard),
                (false, submitted, false),
                "bell {bell} session {session:?}"
            );
        }
        let pull = |session: String| Command::Pull {
            bell: false,
            session: Some(session),
        };
        // An id past the key bound is refused before it is hashed under the lock.
        assert_eq!(
            guard
                .prepare_delivery(
                    crate::node::TEST_NODE,
                    "recipient",
                    &context,
                    None,
                    pull("x".repeat(257))
                )
                .err()
                .as_deref(),
            Some("session_invalid")
        );
        // A hook that runs while the pane works is a queued prompt being taken
        // up; it must not clear a draft typed after the turn began.
        guard
            .delivery_observations
            .get_mut("recipient")
            .unwrap()
            .status = "working".into();
        guard
            .prepare_delivery(
                crate::node::TEST_NODE,
                "recipient",
                &context,
                None,
                pull("hook-session".into()),
            )
            .unwrap();
        assert_eq!(written(&mut guard), (false, false, false));
        drop(guard);
        drop(worker);
    }

    #[test]
    fn a_pane_reaches_a_factory_only_while_it_exists_and_only_the_factory_sends_as_it() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, path) = fixture(root.path());
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        let mut guard = runtime.lock().unwrap();
        guard.install_delivery_client(client);
        let context = guard
            .workspace_control_query(crate::node::TEST_NODE, "sender", Query::Info)
            .unwrap()
            .context;
        let send = |intent: &str| Command::Send {
            target: "factory:f-1".into(),
            intent: intent.into(),
            body: "done".into(),
            kind: "report".into(),
        };
        assert_eq!(
            guard
                .prepare_delivery(
                    crate::node::TEST_NODE,
                    "sender",
                    &context,
                    None,
                    send("early")
                )
                .err()
                .as_deref(),
            Some("target_unavailable"),
            "no Factory f-1 exists yet"
        );
        assert_eq!(
            guard.factory_delivery("f-1").err().as_deref(),
            Some("factory_unavailable")
        );
        guard.set_factory_recipients([("f-1".to_owned(), 30 * 60_000)].into());
        let prepared = guard
            .prepare_delivery(
                crate::node::TEST_NODE,
                "sender",
                &context,
                None,
                send("once"),
            )
            .unwrap();
        drop(guard);
        let result = prepared.run(Duration::from_secs(5)).unwrap();
        assert_eq!(result["recipient"]["pane_id"], "factory:f-1");
        assert_eq!(result["recipient"]["kind"], "factory");

        // The Factory answers as itself; a pane cannot borrow its authority.
        let guard = runtime.lock().unwrap();
        assert!(
            !guard.factory_authority_current(
                "sender",
                &Actor::factory("f-1", crate::node::TEST_NODE)
            )
        );
        assert!(guard.factory_authority_current(
            "factory:f-1",
            &Actor::factory("f-1", crate::node::TEST_NODE)
        ));
        let forged = Actor {
            session: Some("forged".into()),
            ..Actor::factory("f-1", crate::node::TEST_NODE)
        };
        assert!(!guard.factory_recipient_current(&forged));
        let prepared = guard
            .factory_prepare(
                "f-1",
                Some("sender"),
                Command::Send {
                    target: "sender".into(),
                    intent: "factory-answer".into(),
                    body: "noted".into(),
                    kind: "report".into(),
                },
            )
            .unwrap();
        drop(guard);
        let result = prepared.run(Duration::from_secs(5)).unwrap();
        assert_eq!(result["sender"]["pane_id"], "factory:f-1");
        let stored = crate::delivery::ledger::load(&path).unwrap();
        assert_eq!(stored.letters.len(), 2);
        drop(worker);
    }

    #[test]
    fn remote_delivery_uses_attested_pane_and_native_kind_without_a_remote_store() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, path) = fixture(root.path());
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        let caller = "remote:device:pane:sender";
        let context;
        {
            let mut guard = runtime.lock().unwrap();
            guard.install_delivery_client(client);
            let mut remote = guard.snapshot.navigator.workspaces[0].clone();
            remote.device_id = "device".into();
            remote.id = "remote-workspace".into();
            remote.remote_target_id = Some("device".into());
            for tab in &mut remote.checkouts[0].tabs {
                for pane in &mut tab.panes {
                    pane.id = format!("remote:device:pane:{}", pane.id);
                }
            }
            guard.snapshot.navigator.workspaces.push(remote);
            guard
                .snapshot
                .status
                .remote
                .push(crate::model::RemoteStatusSnapshot {
                    target_id: "device".into(),
                    state: "connected".into(),
                    message: None,
                    herdr_version: None,
                    session: None,
                    files: crate::model::RemoteFileListSnapshot::idle(),
                    catalog: Default::default(),
                });
            let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[{"id":"remote-sender","pane_id":"sender","agent":"claude","agent_status":"working","state_change_seq":1,"lineage_session":"remote-session"}]})).unwrap();
            guard.observe_delivery("device", &payload, Some("fixture-device"), None);
            context = guard
                .workspace_control_query("device", caller, Query::Info)
                .unwrap()
                .context;
            assert!(
                guard
                    .prepare_delivery(
                        "device",
                        caller,
                        &context,
                        Some("recipient"),
                        Command::Inbox
                    )
                    .is_err()
            );
            assert!(
                guard
                    .prepare_delivery(
                        "device",
                        caller,
                        &context,
                        Some("remote:other:pane:sender"),
                        Command::Inbox
                    )
                    .is_err()
            );
        }
        let prepared = runtime
            .lock()
            .unwrap()
            .prepare_delivery(
                "device",
                caller,
                &context,
                Some("sender"),
                Command::Send {
                    target: "recipient".into(),
                    intent: "remote-once".into(),
                    body: "done".into(),
                    kind: "report".into(),
                },
            )
            .unwrap();
        let result = prepared.run(Duration::from_secs(5)).unwrap();
        assert_eq!(result["sender"]["kind"], "claude");
        assert_eq!(result["sender"]["device_id"], "device");
        assert_eq!(result["sender"]["pane_id"], caller);
        let stored = crate::delivery::ledger::load(&path).unwrap();
        assert_eq!(stored.letters.len(), 1);
        assert_eq!(
            stored.letters[0].sender.session.as_deref(),
            Some("remote-session")
        );
        drop(worker);
    }

    #[test]
    fn a_checkout_bound_caller_is_refused_delivery_and_agent_commands_with_or_without_a_hint() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, actor, _, path) = fixture(root.path());
        let before = std::fs::read(&path).unwrap();
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        let caller = crate::workspace_control::checkout_caller_id("cap", "/checkouts/fixture");
        let context = authority(&actor).context;
        let commands = [
            Command::Send {
                target: "recipient".into(),
                intent: "checkout-send".into(),
                body: "hello".into(),
                kind: "request".into(),
            },
            Command::Inbox,
            Command::WatchStart {
                target: "recipient".into(),
                observer: None,
                actor: None,
            },
            Command::Agents {
                command: crate::coordination::Command::List,
            },
        ];
        let mut guard = runtime.lock().unwrap();
        guard.install_delivery_client(client);
        for hint in [None, Some("sender"), Some("recipient")] {
            for command in commands.clone() {
                assert_eq!(
                    guard
                        .prepare_delivery(
                            crate::node::TEST_NODE,
                            &caller,
                            &context,
                            hint,
                            command.clone()
                        )
                        .err()
                        .as_deref(),
                    Some("agent_pane_required"),
                    "hint {hint:?} command {command:?}"
                );
            }
        }
        // The pane-bound caller is unchanged: its own pane, and a hint that
        // names another pane is a conflict.
        assert_eq!(
            guard
                .prepare_delivery(
                    crate::node::TEST_NODE,
                    "sender",
                    &context,
                    Some("recipient"),
                    Command::Inbox
                )
                .err()
                .as_deref(),
            Some("caller_identity_conflict")
        );
        let prepared = guard
            .prepare_delivery(
                crate::node::TEST_NODE,
                "sender",
                &context,
                Some("sender"),
                commands[0].clone(),
            )
            .unwrap();
        drop(guard);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let result = prepared.run(Duration::from_secs(5)).unwrap();
        assert_eq!(result["sender"]["pane_id"], "sender");
        drop(worker);
    }

    #[test]
    fn prepared_command_refuses_a_changed_pane_context_without_saving() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, actor, _, path) = fixture(root.path());
        let before = std::fs::read(&path).unwrap();
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        let context = authority(&actor).context;
        let prepared = {
            let mut guard = runtime.lock().unwrap();
            guard.install_delivery_client(client);
            guard
                .prepare_delivery(
                    crate::node::TEST_NODE,
                    &actor.pane_id,
                    &context,
                    None,
                    Command::Send {
                        target: "recipient".into(),
                        intent: "must-refuse".into(),
                        body: "private".into(),
                        kind: "request".into(),
                    },
                )
                .unwrap()
        };
        runtime.lock().unwrap().snapshot.navigator.workspaces[0].checkouts[0].path =
            "/checkouts/moved".into();
        assert_eq!(
            prepared.run(Duration::from_secs(5)).unwrap_err(),
            "caller_context_changed"
        );
        assert!(
            runtime
                .lock()
                .unwrap()
                .delivery_state()
                .unwrap()
                .letters
                .is_empty()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(worker);
    }

    /// Two agents in panes `lead` and `child` on this machine's Herdr at
    /// `scope`, each registered, with `lead` watching `child`.
    fn registered_pair(runtime: &mut Runtime) -> Ledger {
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"lead","pane_id":"lead","agent":"claude","agent_status":"idle","state_change_seq":1,
             "lineage_session":crate::wire::session_digest("lead-native")},
            {"id":"child","pane_id":"child","agent":"codex","agent_status":"working","state_change_seq":1,
             "lineage_session":crate::wire::session_digest("child-native")},
        ]}))
        .unwrap();
        runtime.observe_delivery(crate::node::TEST_NODE, &payload, Some("scope"), None);
        let mut ledger = (*runtime.delivery_state().unwrap()).clone();
        for (id, pane) in [("agent-1", "lead"), ("agent-2", "child")] {
            ledger.agents.push(crate::coordination::AgentRecord {
                id: id.into(),
                name: pane.into(),
                machine: crate::node::TEST_NODE.into(),
                host_scope: "scope".into(),
                native_machine: "fixture-machine".into(),
                session: format!("{pane}-native"),
                instance: format!("terminal-{pane}"),
                pane: pane.into(),
                parent: (pane == "child").then(|| "agent-1".into()),
                origin: None,
                project: None,
                actor: runtime.delivery_observations[pane].actor.clone(),
                ended: false,
            });
        }
        ledger.next_id = 3;
        let lead = ledger.agents[0].actor.clone();
        let child = ledger.agents[1].actor.clone();
        crate::delivery::watch::start(&mut ledger, &lead, &child, 1).unwrap();
        ledger.validate().unwrap();
        runtime.delivery_ledger = Ok(Arc::new(ledger.clone()));
        ledger
    }

    fn only_lead() -> SessionSnapshotPayload {
        serde_json::from_value(json!({"agents":[
            {"id":"lead","pane_id":"lead","agent":"claude","agent_status":"idle","state_change_seq":1,
             "lineage_session":crate::wire::session_digest("lead-native")},
        ]}))
        .unwrap()
    }

    /// Herdr closing a registered agent's pane ends its registration and the
    /// watches on it in the durable ledger, and frees its name; the agent
    /// whose pane is still open stays registered.
    #[test]
    fn herdr_closing_a_registered_pane_ends_the_registration_and_its_watches() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, path) = fixture(root.path());
        let (worker, client) = Worker::spawn(
            Arc::downgrade(&runtime),
            ChangeNotifier::noop(),
            path.clone(),
        )
        .unwrap();
        registered_pair(&mut runtime.lock().unwrap());
        runtime.lock().unwrap().observe_delivery(
            crate::node::TEST_NODE,
            &only_lead(),
            Some("scope"),
            None,
        );
        // Any transaction commits the pending ends with it.
        client
            .submit(
                crate::delivery::worker::Effect::HumanClaim,
                Duration::from_secs(5),
            )
            .unwrap();
        let persisted = crate::delivery::ledger::load(&path).unwrap();
        let ended = |ledger: &Ledger| {
            ledger
                .agents
                .iter()
                .map(|record| (record.id.clone(), record.ended))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ended(&persisted),
            [("agent-1".to_owned(), false), ("agent-2".to_owned(), true)]
        );
        assert!(persisted.watches.is_empty());
        let guard = runtime.lock().unwrap();
        assert_eq!(*guard.delivery_state().unwrap(), persisted);
        assert!(guard.delivery_registrations_gone().is_empty());
        assert!(crate::coordination::resolve_actor(&persisted, "child").is_none());
        drop(guard);
        drop(worker);
    }

    /// A pane read ends only this Herdr's registrations it can vouch for:
    /// none made after its snapshot was asked for, none on another Herdr or
    /// device, and never a Factory's code-owned one, all of whose panes the
    /// snapshot lacks; a registration made before it whose pane it lacks
    /// does end.
    #[test]
    fn a_pane_read_never_ends_a_registration_it_cannot_vouch_for() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _, _, _) = fixture(root.path());
        let mut guard = runtime.lock().unwrap();
        let mut ledger = registered_pair(&mut guard);
        let child = ledger.agents[1].clone();
        let variant = |id: u64, pane: &str| {
            let mut record = child.clone();
            record.id = format!("agent-{id}");
            record.name = pane.into();
            record.pane = pane.into();
            record.actor.pane_id = pane.into();
            record.parent = None;
            record
        };
        let next = ledger.next_id;
        let mut elsewhere = variant(next, "elsewhere");
        elsewhere.host_scope = "another-herdr".into();
        let mut device = variant(next + 1, "device-pane");
        device.machine = "device".into();
        device.actor.device_id = "device".into();
        device.actor.pane_id = "remote:device:pane:device-pane".into();
        let factory = Actor::factory("f1", crate::node::TEST_NODE);
        let mut owned = variant(next + 2, &factory.pane_id);
        owned.actor = factory;
        ledger.agents.extend([elsewhere, device, owned]);
        ledger.next_id = next + 3;
        guard.delivery_ledger = Ok(Arc::new(ledger.clone()));
        // A reconnect asks for a fresh snapshot, and an agent registers
        // while it is out.
        guard.begin_delivery_pane_read(crate::node::TEST_NODE);
        let late = variant(ledger.next_id, "late");
        ledger.agents.push(late);
        ledger.next_id += 1;
        guard.delivery_ledger = Ok(Arc::new(ledger));
        // The snapshot lists only the lead.
        guard.observe_delivery(crate::node::TEST_NODE, &only_lead(), Some("scope"), None);
        assert_eq!(
            guard
                .delivery_registrations_gone()
                .into_iter()
                .map(|(id, reason)| (id, reason.reason()))
                .collect::<Vec<_>>(),
            [("agent-2".to_owned(), "pane_absent")]
        );
    }
}
