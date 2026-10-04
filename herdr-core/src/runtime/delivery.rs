//! Memory-only admission and observations. Disk, process and helper work is
//! owned by delivery workers, never by the owner-thread caller.

use std::collections::HashSet;
use std::sync::Arc;

use crate::delivery::ledger::Ledger;
use crate::delivery::worker::{Authority, Client, Prepared};
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
    pub last_input_at_unix_ms: u64,
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
                last_input_at_unix_ms: previous.map(|old| old.last_input_at_unix_ms).unwrap_or(now),
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

    pub(crate) fn prepare_delivery(
        &self,
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
        if context != *expected || device != "local" {
            return Err("caller_context_changed".into());
        }
        let actor_pane = match Caller::parse(caller) {
            Caller::Pane(pane) => {
                if hint.is_some_and(|hint| hint != pane) {
                    return Err("caller_identity_conflict".into());
                }
                pane
            }
            Caller::Checkout { .. } => hint.ok_or("agent_pane_required")?,
        };
        let actor_context = self
            .workspace_control_query(device, actor_pane, Query::Info)
            .map_err(|_| "agent_pane_required")?
            .context;
        if actor_context != context {
            return Err("caller_identity_conflict".into());
        }
        let actor = self
            .delivery_observations
            .get(actor_pane)
            .ok_or("agent_pane_required")?
            .actor
            .clone();
        actor.require_native_identity()?;
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
            Command::Send { target, .. } | Command::WatchStart { target } => {
                let mut matches = self.delivery_observations.values().filter(|observation| {
                    observation.actor.pane_id == *target || observation.actor.name == *target
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
        if matches!(&command, Command::Send { .. }) && !repeated {
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

    pub(crate) fn delivery_bell_context(
        &self,
    ) -> Option<(Arc<Ledger>, Arc<dyn hide_herdr_client::ApiConnector>)> {
        Some((
            self.delivery_state().ok()?,
            self.live.as_ref()?.api_connector.clone(),
        ))
    }

    pub(crate) fn delivery_observation(&self, actor: &Actor) -> Option<Observation> {
        self.delivery_observations
            .get(&actor.pane_id)
            .filter(|observation| observation.actor.same_identity(actor))
            .cloned()
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
            && crate::delivery::doorbell::eligible(current, unix_milliseconds())
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
                status_label: "Attached".into(),
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
