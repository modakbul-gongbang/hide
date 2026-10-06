//! Memory-only admission and observations. Disk, process and helper work is
//! owned by delivery workers, never by the owner-thread caller.

use std::collections::HashSet;
use std::sync::Arc;

use crate::delivery::ledger::Ledger;
use crate::delivery::worker::{Authority, Client, Prepared, PreparedHuman};
use crate::delivery::{Actor, Command};
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
}

impl Runtime {
    pub(crate) fn observe_delivery(
        &mut self,
        device: &str,
        payload: &SessionSnapshotPayload,
        host_scope: Option<&str>,
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
            let pane_id = if device == "local" {
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
            let session_kind = match kind {
                "codex" => Some(hide_session::Agent::Codex),
                "claude" | "claude-code" | "claude_code" => Some(hide_session::Agent::Claude),
                _ => None,
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
            if device == "local" || pane.starts_with(&format!("remote:{device}:pane:")) {
                pane.to_owned()
            } else {
                format!("remote:{device}:pane:{pane}")
            }
        };
        let actor_pane = match Caller::parse(caller) {
            Caller::Pane(pane) => {
                if hint.is_some_and(|hint| qualify(hint) != pane) {
                    return Err("caller_identity_conflict".into());
                }
                pane.to_owned()
            }
            Caller::Checkout { .. } => qualify(hint.ok_or("agent_pane_required")?),
        };
        let actor_context = self
            .workspace_control_query(device, &actor_pane, Query::Info)
            .map_err(|_| "agent_pane_required")?
            .context;
        if actor_context != context {
            return Err("caller_identity_conflict".into());
        }
        let actor = self
            .delivery_observations
            .get(&actor_pane)
            .ok_or("agent_pane_required")?
            .actor
            .clone();
        actor.require_native_identity()?;
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
        self.delivery_ledger = Ok(ledger);
        changed
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
        if device == "local" {
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
    pub(crate) fn delivery_bell_current(
        &self,
        id: &str,
        observed: &Observation,
        reserved: Option<u8>,
    ) -> bool {
        let Some(current) = self.delivery_observations.get(&observed.actor.pane_id) else {
            return false;
        };
        current.actor.same_identity(&observed.actor)
            && current.status == observed.status
            && current.state_change_seq == observed.state_change_seq
            && current.last_input_at_unix_ms == observed.last_input_at_unix_ms
            && crate::delivery::doorbell::judge(current, unix_milliseconds()).is_ok()
            && self.delivery_ledger.as_ref().is_ok_and(|ledger| {
                ledger.letters.iter().any(|letter| {
                    letter.id == id
                        && letter.recipient.same_identity(&observed.actor)
                        && letter.state == crate::delivery::ledger::State::Pending
                        && match reserved {
                            Some(attempt) => {
                                (1..=3).contains(&attempt) && letter.attempts() == attempt
                            }
                            None => letter.attempts() < 3,
                        }
                        && unix_milliseconds().saturating_sub(letter.created_at_unix_ms)
                            < crate::delivery::DELIVERY_EXPIRY_MS
                })
            })
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
                let channel = if watch.target.device_id != "local" && observation.is_some() {
                    self.device_channel(&watch.target.device_id).ok()
                } else {
                    None
                };
                crate::delivery::worker::WatchWork {
                    id: watch.id.clone(),
                    observation,
                    gone,
                    status: watch.last_status.clone(),
                    state_change_seq: watch.last_state_change_seq,
                    status_changed_at_unix_ms: watch.status_changed_at_unix_ms,
                    home: self.home_path.clone(),
                    channel,
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
    pub(crate) fn coordination_context(
        &self,
        device: &str,
    ) -> Result<
        (
            Arc<dyn hide_herdr_client::ApiConnector>,
            String,
            String,
            crate::codex_launch::CodexDaemon,
        ),
        String,
    > {
        let (connector, scope, machine) = if device == "local" {
            let live = self.live.as_ref().ok_or("herdr_unavailable")?;
            (
                live.api_connector.clone(),
                live.socket_path
                    .to_str()
                    .ok_or("host_scope_unavailable")?
                    .to_owned(),
                self.local_machine_id
                    .clone()
                    .ok_or("machine_identity_unavailable")?,
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
        Ok((connector, scope, machine, self.codex_daemon(device)))
    }
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

    pub(crate) fn fixture(
        root: &std::path::Path,
    ) -> (Arc<Mutex<Runtime>>, Actor, Observation, PathBuf) {
        let state = root.join("state");
        hide_platform::fs::private::create_dir_all(&state).unwrap();
        let options: CoreOptions = serde_json::from_value(json!({
            "schema_version":SCHEMA_VERSION,"home":root,"herdr_socket_path":null,
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
        );
        let payload: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[
            {"id":"sender","pane_id":"sender","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"sender-session"},
            {"id":"recipient","pane_id":"recipient","agent":"codex","agent_status":"working","state_change_seq":1,"lineage_session":"recipient-session"},
        ]})).unwrap();
        runtime.observe_delivery("local", &payload, None);
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
            device_id: "local".into(),
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
            device_id: "local".into(),
            session: Some("sender-session".into()),
        };
        let target = runtime
            .delivery_observation(&Actor {
                pane_id: "recipient".into(),
                name: "recipient".into(),
                kind: "codex".into(),
                device_id: "local".into(),
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
                device_id: "local".into(),
                workspace_id: "workspace".into(),
                checkout_id: "checkout".into(),
                checkout_path: "/checkouts/fixture".into(),
            },
        }
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
        runtime.observe_delivery("local", &payload, None);
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
        guard.observe_delivery("local", &replaced, None);
        assert_eq!(
            guard.delivery_bell_verdict(&target.actor, now).err(),
            Some(Hold::Session)
        );
        let empty: SessionSnapshotPayload = serde_json::from_value(json!({"agents":[]})).unwrap();
        guard.observe_delivery("local", &empty, None);
        assert_eq!(
            guard.delivery_bell_verdict(&target.actor, now).err(),
            Some(Hold::Absent)
        );
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
        guard.observe_delivery("local", &payload, None);
        let context = authority(&target.actor).context;
        clocks(&mut guard, "recipient", true);
        guard
            .prepare_delivery("local", "recipient", &context, None, Command::Inbox)
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
                    "local",
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
                .prepare_delivery("local", "recipient", &context, None, pull("x".repeat(257)))
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
                "local",
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
            guard.observe_delivery("device", &payload, Some("fixture-device"));
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
    fn prepared_command_refuses_changed_pane_or_checkout_capability_context_without_saving() {
        for checkout_bound in [false, true] {
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
            let caller = if checkout_bound {
                crate::workspace_control::checkout_caller_id("cap", "/checkouts/fixture/sub")
            } else {
                actor.pane_id.clone()
            };
            let prepared = {
                let mut guard = runtime.lock().unwrap();
                guard.install_delivery_client(client);
                guard
                    .prepare_delivery(
                        "local",
                        &caller,
                        &context,
                        checkout_bound.then_some(actor.pane_id.as_str()),
                        Command::Send {
                            target: "recipient".into(),
                            intent: "must-refuse".into(),
                            body: "private".into(),
                            kind: "request".into(),
                        },
                    )
                    .unwrap()
            };
            {
                let mut guard = runtime.lock().unwrap();
                let checkouts = &mut guard.snapshot.navigator.workspaces[0].checkouts;
                if checkout_bound {
                    let mut narrower = checkouts[0].clone();
                    narrower.id = "nested".into();
                    narrower.path = "/checkouts/fixture/sub".into();
                    narrower.tabs.clear();
                    narrower.has_panes = false;
                    checkouts.push(narrower);
                } else {
                    checkouts[0].path = "/checkouts/moved".into();
                }
            }
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
    }
}
