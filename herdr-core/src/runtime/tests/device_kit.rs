//! A device's install kit as the runtime drives it (PRD device-parity B13,
//! B17, B19, B21-B24): the connection pass, the operator's Reinstall, the
//! consent a device needs first, a call that fails, and removal.

use super::*;
use crate::host_access::{HostAnswer, HostCallError, HostChannel};
use hide_host::protocol::{Call, KitAction};
use hide_kit::{ComponentId, ComponentReport, ComponentState, KitReport};
use std::sync::{Arc, Mutex};

const DEVICE: &str = "studio";

/// A device helper that answers only kit calls: a set report, and a set
/// removal outcome for `remove`.
struct KitDevice {
    calls: Mutex<Vec<(KitAction, String, Option<String>)>>,
    answer: Mutex<Result<KitReport, String>>,
    closed: Mutex<Option<String>>,
    /// When set, an apply or status call waits here until the test lets it
    /// answer, as a slow device would.
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl KitDevice {
    fn answering(answer: Result<KitReport, String>) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            answer: Mutex::new(answer),
            closed: Mutex::new(None),
            gate: Mutex::new(None),
        })
    }

    /// A device whose next non-removal call answers only once the returned
    /// sender is used.
    fn held(answer: Result<KitReport, String>) -> (Arc<Self>, std::sync::mpsc::Sender<()>) {
        let (release, gate) = std::sync::mpsc::channel();
        let device = Self::answering(answer);
        *device.gate.lock().unwrap() = Some(gate);
        (device, release)
    }

    fn calls(&self) -> Vec<(KitAction, String, Option<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl HostChannel for KitDevice {
    fn call(&self, call: Call, _timeout: Duration) -> Result<HostAnswer, HostCallError> {
        let Call::Kit {
            action,
            cli_dir,
            herdr_socket,
        } = call
        else {
            return Err(HostCallError::NotConnected("kit calls only".to_owned()));
        };
        let removing = action == KitAction::Remove;
        self.calls
            .lock()
            .unwrap()
            .push((action, cli_dir, herdr_socket));
        if !removing && let Some(gate) = self.gate.lock().unwrap().take() {
            let _ = gate.recv();
        }
        if removing {
            let removed = hide_host::protocol::KitRemoved {
                kit: hide_kit::RemoveReport {
                    components: vec![(ComponentId::Cli, hide_kit::RemoveOutcome::Removed)],
                },
                helper_root: hide_kit::RemoveOutcome::Removed,
            };
            return Ok(HostAnswer::Parsed(serde_json::to_value(removed).unwrap()));
        }
        match &*self.answer.lock().unwrap() {
            Ok(report) => Ok(HostAnswer::Parsed(serde_json::to_value(report).unwrap())),
            Err(reason) => Err(HostCallError::Unknown(reason.clone())),
        }
    }

    fn closed_reason(&self) -> Option<String> {
        self.closed.lock().unwrap().clone()
    }

    fn close(&self, reason: &str) {
        *self.closed.lock().unwrap() = Some(reason.to_owned());
    }
}

fn report(states: &[(ComponentId, ComponentState)]) -> KitReport {
    KitReport {
        components: states
            .iter()
            .map(|(id, state)| ComponentReport {
                id: *id,
                state: *state,
                reason: None,
                location: Some(format!("/home/me/{}", id.code())),
                codex_daemon: None,
            })
            .collect(),
        labels_retirement: Default::default(),
        legacy_retirement: Default::default(),
    }
}

fn dispatch(shared: &Mutex<Runtime>, kind: &str, payload: serde_json::Value) {
    let event =
        serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
}

/// A registered device with the given consent, optionally connected to
/// `helper`, with the kit workers able to run.
fn device_runtime(
    consent: Option<crate::model::HostConsent>,
    helper: Option<Arc<KitDevice>>,
) -> Arc<Mutex<Runtime>> {
    let shared = Arc::new(Mutex::new(runtime()));
    dispatch(
        &shared,
        "register_device",
        serde_json::json!({ "id": DEVICE, "label": DEVICE, "ssh_alias": DEVICE }),
    );
    {
        let mut runtime = shared.lock().unwrap();
        let registration = runtime
            .snapshot
            .ui_state
            .device_registrations
            .iter_mut()
            .find(|registration| registration.id == DEVICE)
            .unwrap();
        registration.host_consent = consent;
        registration.herdr_socket_path = Some("~/.config/herdr/other.sock".to_owned());
        if let Some(helper) = helper {
            runtime.device_hosts.insert(
                DEVICE.to_owned(),
                hosts::DeviceHost {
                    phase: hosts::HostPhase::Ready {
                        host: helper,
                        platform: "macos aarch64".to_owned(),
                        helper_path: "/home/me/.local/share/hide/host-helper/0123456789abcdef/hide-host-helper".to_owned(),
                    },
                    generation: 1,
                },
            );
        }
        runtime.install_worker_context(
            Arc::downgrade(&shared),
            crate::handle::ChangeNotifier::noop(),
        );
        runtime.refresh_device_snapshots();
    }
    shared
}

fn granted(runtime: &Mutex<Runtime>) -> crate::model::HostConsent {
    runtime.lock().unwrap().new_host_consent()
}

fn kit(shared: &Mutex<Runtime>) -> crate::model::KitSnapshot {
    shared
        .lock()
        .unwrap()
        .snapshot()
        .navigator
        .devices
        .iter()
        .find(|device| device.id == DEVICE)
        .expect("device row")
        .kit
        .clone()
}

/// Waits for the device's kit worker to finish what is queued.
fn settle(shared: &Mutex<Runtime>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while shared.lock().unwrap().device_kit_running.contains(DEVICE) {
        assert!(
            Instant::now() < deadline,
            "the device kit worker never finished"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn with_consent(helper: Option<Arc<KitDevice>>) -> Arc<Mutex<Runtime>> {
    let probe = device_runtime(None, None);
    let consent = granted(&probe);
    device_runtime(Some(consent), helper)
}

/// B13: a connected device's kit is installed by the connection pass, with
/// the consent's command folder and the registration's Herdr socket, and its
/// row shows each part the way This Mac's does.
#[test]
fn a_connected_device_installs_its_kit_and_shows_each_part() {
    let helper = KitDevice::answering(Ok(report(&[
        (ComponentId::Cli, ComponentState::Installed),
        (ComponentId::ClaudeCodeHook, ComponentState::Installed),
        (ComponentId::CodexHook, ComponentState::Absent),
        (ComponentId::Hcoord, ComponentState::Failed),
    ])));
    let shared = with_consent(Some(Arc::clone(&helper)));

    shared
        .lock()
        .unwrap()
        .queue_device_kit(DEVICE, KitJob::Apply(hide_kit::Scope::automatic()));
    assert!(kit(&shared).busy, "the row says the install is running");
    settle(&shared);

    assert_eq!(
        helper.calls(),
        vec![(
            KitAction::Apply,
            crate::remote::host::DEFAULT_CLI_DIR.to_owned(),
            Some("~/.config/herdr/other.sock".to_owned()),
        )]
    );
    let kit = kit(&shared);
    assert!(!kit.busy);
    assert_eq!(kit.unavailable, None);
    assert_eq!(
        kit.components
            .iter()
            .map(|part| (part.id, part.state))
            .collect::<Vec<_>>(),
        vec![
            (ComponentId::Cli, ComponentState::Installed),
            (ComponentId::ClaudeCodeHook, ComponentState::Installed),
            (ComponentId::CodexHook, ComponentState::Absent),
            (ComponentId::Hcoord, ComponentState::Failed),
        ]
    );
    assert!(kit.offers_reinstall);
}

/// B8: Reinstall on a device's row sends only the parts that need it.
#[test]
fn reinstall_on_a_device_sends_only_the_parts_that_need_it() {
    let helper = KitDevice::answering(Ok(report(&[
        (ComponentId::Cli, ComponentState::Installed),
        (ComponentId::ClaudeCodeHook, ComponentState::Removed),
        (ComponentId::Hcoord, ComponentState::Outdated),
    ])));
    let shared = with_consent(Some(Arc::clone(&helper)));
    shared
        .lock()
        .unwrap()
        .queue_device_kit(DEVICE, KitJob::Status);
    settle(&shared);

    dispatch(
        &shared,
        "kit_reinstall",
        serde_json::json!({ "device_id": DEVICE }),
    );
    settle(&shared);
    let calls = helper.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].0, KitAction::Status);
    assert_eq!(
        calls[1].0,
        KitAction::Reinstall {
            components: vec![ComponentId::ClaudeCodeHook, ComponentId::Hcoord],
            turn_off: Vec::new(),
        }
    );
}

/// B21: a device registered without consent installs nothing, its row says
/// what would install the kit, and Reinstall is refused rather than queued.
#[test]
fn a_device_without_consent_installs_nothing_and_says_why() {
    let shared = device_runtime(None, None);
    let kit = kit(&shared);
    assert!(
        kit.unavailable
            .as_deref()
            .is_some_and(|reason| reason.contains("allow its helper")),
        "{kit:?}"
    );
    dispatch(
        &shared,
        "kit_reinstall",
        serde_json::json!({ "device_id": DEVICE }),
    );
    let runtime = shared.lock().unwrap();
    assert!(!runtime.device_kit_pending.contains_key(DEVICE));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("kit.unavailable")
    );
}

/// B17: a device whose platform this build does not carry shows why and
/// installs nothing.
#[test]
fn a_device_on_an_unsupported_platform_shows_why() {
    let shared = with_consent(None);
    {
        let mut runtime = shared.lock().unwrap();
        runtime.device_hosts.insert(
            DEVICE.to_owned(),
            hosts::DeviceHost {
                phase: hosts::HostPhase::Unsupported(
                    "This Hide build does not include the device helper for Linux x86_64"
                        .to_owned(),
                ),
                generation: 1,
            },
        );
        runtime.refresh_device_snapshots();
    }
    assert_eq!(
        kit(&shared).unavailable.as_deref(),
        Some("This Hide build does not include the device helper for Linux x86_64")
    );
}

/// D-13, B19: a device allowed under contract 2 for the same folders is
/// carried to the whole kit without asking when its connection starts, and
/// keeps the identity it was bound to; one allowed for other folders asks
/// again.
#[test]
fn a_contract_2_consent_is_carried_to_the_kit_on_its_next_connection() {
    let probe = device_runtime(None, None);
    let identity = crate::model::HostIdentity {
        user: "me".to_owned(),
        hostname: "studio.local".to_owned(),
        port: 22,
        host_key_sha256: "key".to_owned(),
    };
    let mut consent = granted(&probe);
    consent.contract = 2;
    consent.identity = Some(identity.clone());
    let shared = device_runtime(Some(consent.clone()), None);
    assert_eq!(
        shared.lock().unwrap().host_snapshot(DEVICE).consent,
        "granted"
    );
    shared.lock().unwrap().start_device_host(DEVICE);
    let carried = shared
        .lock()
        .unwrap()
        .snapshot
        .ui_state
        .device_registrations
        .iter()
        .find(|registration| registration.id == DEVICE)
        .and_then(|registration| registration.host_consent.clone())
        .unwrap();
    assert_eq!(carried.contract, crate::remote::host::HOST_CONSENT_CONTRACT);
    assert_eq!(carried.identity, Some(identity));

    let mut elsewhere = consent;
    elsewhere.helper_root = "~/elsewhere".to_owned();
    let shared = device_runtime(Some(elsewhere), None);
    shared.lock().unwrap().start_device_host(DEVICE);
    let runtime = shared.lock().unwrap();
    assert_eq!(runtime.host_snapshot(DEVICE).consent, "outdated");
    assert_eq!(
        runtime
            .snapshot
            .ui_state
            .device_registrations
            .iter()
            .find(|registration| registration.id == DEVICE)
            .and_then(|registration| registration.host_consent.as_ref())
            .map(|consent| consent.contract),
        Some(2)
    );
}

/// PRD hide-home-layout D-12, B20: a device allowed for the old default
/// helper root is not asked again; the connection installs at the new
/// default root and keeps the bound identity. A consent for another root,
/// or a daemon told another root, carries nothing.
#[test]
fn a_consent_for_the_old_default_root_moves_to_the_new_one_without_asking() {
    use hide_kit::layout::{HELPER_ROOT, LEGACY_HELPER_ROOT};
    let probe = device_runtime(None, None);
    let identity = crate::model::HostIdentity {
        user: "me".to_owned(),
        hostname: "studio.local".to_owned(),
        port: 22,
        host_key_sha256: "key".to_owned(),
    };
    let mut consent = granted(&probe);
    assert_eq!(consent.helper_root, HELPER_ROOT);
    consent.helper_root = LEGACY_HELPER_ROOT.to_owned();
    consent.identity = Some(identity.clone());
    let shared = device_runtime(Some(consent.clone()), None);
    {
        let runtime = shared.lock().unwrap();
        let snapshot = runtime.host_snapshot(DEVICE);
        assert_eq!(snapshot.consent, "granted");
        assert_eq!(snapshot.helper_root.as_deref(), Some(HELPER_ROOT));
    }
    let connecting = shared
        .lock()
        .unwrap()
        .carried_consent(DEVICE, consent.clone());
    assert_eq!(
        connecting.helper_root, HELPER_ROOT,
        "this connection installs at the new root"
    );
    assert_eq!(connecting.identity, Some(identity));
    let saved = shared
        .lock()
        .unwrap()
        .snapshot
        .ui_state
        .device_registrations
        .iter()
        .find(|registration| registration.id == DEVICE)
        .and_then(|registration| registration.host_consent.clone())
        .unwrap();
    assert_eq!(saved.helper_root, HELPER_ROOT);

    let mut elsewhere = consent.clone();
    elsewhere.helper_root = "~/elsewhere".to_owned();
    let shared = device_runtime(Some(elsewhere.clone()), None);
    let kept = shared.lock().unwrap().carried_consent(DEVICE, elsewhere);
    assert_eq!(kept.helper_root, "~/elsewhere");
    assert_eq!(
        shared.lock().unwrap().host_snapshot(DEVICE).consent,
        "outdated"
    );

    // An isolated daemon told its own root asks again for the old default.
    let shared = device_runtime(Some(consent.clone()), None);
    shared.lock().unwrap().host_helper_root = "~/.cache/hide-test/helper".to_owned();
    assert_eq!(
        shared.lock().unwrap().host_snapshot(DEVICE).consent,
        "outdated"
    );
    let kept = shared.lock().unwrap().carried_consent(DEVICE, consent);
    assert_eq!(kept.helper_root, LEGACY_HELPER_ROOT);
}

/// A kit call that fails on a device never read says why on its row, and
/// the row stops saying it is working.
#[test]
fn a_failed_kit_call_on_an_unread_device_says_why() {
    let helper = KitDevice::answering(Err("The device did not answer in time".to_owned()));
    let shared = with_consent(Some(helper));
    shared
        .lock()
        .unwrap()
        .queue_device_kit(DEVICE, KitJob::Apply(hide_kit::Scope::automatic()));
    settle(&shared);
    let kit = kit(&shared);
    assert!(!kit.busy);
    assert!(
        kit.unavailable
            .as_deref()
            .is_some_and(|reason| reason.contains("did not answer in time")),
        "{kit:?}"
    );
}

fn registered(shared: &Mutex<Runtime>) -> bool {
    shared
        .lock()
        .unwrap()
        .snapshot
        .ui_state
        .device_registrations
        .iter()
        .any(|registration| registration.id == DEVICE)
}

/// B23: removing a connected device takes Hide's kit off it on its own
/// helper connection, which closes once that is done.
#[test]
fn removing_a_connected_device_takes_its_kit_off_and_then_closes_the_helper() {
    let helper = KitDevice::answering(Ok(report(&[])));
    let shared = with_consent(Some(Arc::clone(&helper)));
    dispatch(
        &shared,
        "remove_device",
        serde_json::json!({ "device_id": DEVICE }),
    );
    assert!(!registered(&shared), "the registration goes at once");
    settle(&shared);

    assert_eq!(
        helper
            .calls()
            .into_iter()
            .map(|(action, ..)| action)
            .collect::<Vec<_>>(),
        vec![KitAction::Remove]
    );
    assert_eq!(helper.closed_reason().as_deref(), Some("device removed"));
    assert!(!shared.lock().unwrap().device_kit_removing(DEVICE));
}

/// Two registrations that reach one account on one machine (two Herdr
/// servers there) share its hooks, `hide` link and helper root, so removing
/// one leaves the kit for the other and only closes its own helper.
#[test]
fn removing_one_of_two_registrations_of_the_same_account_keeps_the_kit() {
    let helper = KitDevice::answering(Ok(report(&[])));
    let shared = with_consent(Some(Arc::clone(&helper)));
    {
        let mut runtime = shared.lock().unwrap();
        let registrations = &mut runtime.snapshot.ui_state.device_registrations;
        let device = registrations
            .iter_mut()
            .find(|registration| registration.id == DEVICE)
            .unwrap();
        device.host_consent.as_mut().unwrap().identity = Some(crate::model::HostIdentity {
            user: "me".to_owned(),
            hostname: "studio.local".to_owned(),
            port: 22,
            host_key_sha256: "SHA256:studio".to_owned(),
        });
        let mut twin = device.clone();
        twin.id = "studio-second-herdr".to_owned();
        twin.ssh_alias = Some("studio-by-address".to_owned());
        twin.host_consent
            .as_mut()
            .unwrap()
            .identity
            .as_mut()
            .unwrap()
            .hostname = "10.0.0.7".to_owned();
        twin.label = "Studio, second Herdr".to_owned();
        registrations.push(twin);
        runtime.refresh_device_snapshots();
    }
    // The removal confirmation reads this to say the kit stays.
    assert_eq!(
        kit(&shared).shares_account_with.as_deref(),
        Some("Studio, second Herdr")
    );
    dispatch(
        &shared,
        "remove_device",
        serde_json::json!({ "device_id": DEVICE }),
    );
    settle(&shared);

    assert!(!registered(&shared));
    assert!(helper.calls().is_empty(), "{:?}", helper.calls());
    assert_eq!(helper.closed_reason().as_deref(), Some("device removed"));
    assert!(!shared.lock().unwrap().device_kit_removing(DEVICE));
}

/// A device removed while its install call is still running on the device:
/// the removal runs after that call, and the call's late answer brings no
/// kit state back, so the same id added again starts unread.
#[test]
fn an_install_answer_that_arrives_after_removal_brings_nothing_back() {
    let (helper, release) =
        KitDevice::held(Ok(report(&[(ComponentId::Cli, ComponentState::Installed)])));
    let shared = with_consent(Some(Arc::clone(&helper)));
    shared
        .lock()
        .unwrap()
        .queue_device_kit(DEVICE, KitJob::Apply(hide_kit::Scope::automatic()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while helper.calls().is_empty() {
        assert!(Instant::now() < deadline, "the install call never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    dispatch(
        &shared,
        "remove_device",
        serde_json::json!({ "device_id": DEVICE }),
    );
    release.send(()).unwrap();
    settle(&shared);

    assert_eq!(
        helper
            .calls()
            .into_iter()
            .map(|(action, ..)| action)
            .collect::<Vec<_>>(),
        vec![KitAction::Apply, KitAction::Remove]
    );
    let runtime = shared.lock().unwrap();
    assert!(!runtime.device_kit_removing(DEVICE));
    assert_eq!(
        runtime.kit_state(DEVICE),
        crate::model::KitSnapshot::default()
    );
}

/// B24: a device removed while its helper is not connected loses only its
/// registration; nothing is sent, and adding it again later installs the
/// whole kit on its next connection.
#[test]
fn removing_a_device_whose_helper_is_not_connected_sends_nothing() {
    let shared = with_consent(None);
    dispatch(
        &shared,
        "remove_device",
        serde_json::json!({ "device_id": DEVICE }),
    );
    assert!(!registered(&shared));
    let runtime = shared.lock().unwrap();
    assert!(!runtime.device_kit_removing(DEVICE));
    assert!(!runtime.device_kit_running.contains(DEVICE));
}

/// A device added again while its kit is still coming off waits to connect,
/// so the removal cannot delete what the new connection installs.
#[test]
fn a_device_added_again_during_its_removal_waits_to_connect() {
    let shared = with_consent(None);
    let mut runtime = shared.lock().unwrap();
    runtime.device_kit_removing.insert(DEVICE.to_owned());
    runtime.start_device_host(DEVICE);
    let host = runtime.host_snapshot(DEVICE);
    assert_eq!(host.state, "unavailable");
    assert!(
        host.message
            .as_deref()
            .is_some_and(|message| message.contains("still taking its kit off")),
        "{host:?}"
    );
}
