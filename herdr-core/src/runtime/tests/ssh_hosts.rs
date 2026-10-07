//! Add device's host list through the core: the event starts one listing, a
//! second request joins it, and the answer names each alias's address and the
//! device that already holds it. `ssh` is a stand-in script that records what
//! it was asked; nothing connects anywhere.

use super::*;
use crate::executable_fixture::write_executable;
use crate::model::DeviceRegistration;

/// Answers `ssh -F <config> -G -- <alias>` the way `ssh -G` does, and logs the
/// call so a test can count what ran.
const STAND_IN: &str = r#"#!/bin/sh
dir=$(dirname "$0")
echo "$*" >> "$dir/calls"
case "$5" in
  broken) exit 255 ;;
  studio|studio-ip) printf 'user grab\nhostname 10.0.0.2\nport 22\n' ;;
  fresh|old-alias) printf 'user grab\nhostname 10.0.0.9\nport 2222\n' ;;
  *) printf 'user grab\nhostname %s.example\nport 22\n' "$5" ;;
esac
"#;

fn registration(id: &str, label: &str, alias: &str) -> DeviceRegistration {
    DeviceRegistration {
        id: id.to_owned(),
        label: label.to_owned(),
        ssh_alias: Some(alias.to_owned()),
        herdr_socket_path: None,
        host_consent: None,
    }
}

fn list_event() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ssh_hosts_list",
        "payload": {}
    }))
    .unwrap()
}

#[test]
fn the_host_list_names_each_alias_its_address_and_the_device_that_holds_it() {
    let mut runtime = super::devices::runtime_with_home();
    let home = runtime.home_path.clone().expect("fixture home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(
        home.join(".ssh/config"),
        "Host *\n  ServerAliveInterval 30\nHost studio studio-ip fresh broken\n",
    )
    .unwrap();
    let ssh = home.join("ssh");
    write_executable(&ssh, STAND_IN);
    runtime.devices = Arc::new(hide_node::ssh::Connector::new(None).with_ssh_program(ssh));
    // "Studio Mac" is added under `studio`; "Old" under an alias the config
    // no longer names that reaches the same address as `fresh`.
    runtime.snapshot.ui_state.device_registrations = vec![
        registration("studio", "Studio Mac", "studio"),
        registration("old", "Old", "old-alias"),
    ];
    let shared = Arc::new(Mutex::new(runtime));
    shared.lock().unwrap().install_worker_context(
        Arc::downgrade(&shared),
        crate::handle::ChangeNotifier::noop(),
    );

    assert_eq!(
        shared.lock().unwrap().snapshot().status.ssh_hosts.state,
        "idle"
    );
    assert!(shared.lock().unwrap().dispatch_json(&list_event()));
    assert_eq!(
        shared.lock().unwrap().snapshot().status.ssh_hosts.state,
        "loading"
    );
    // A request while one runs is the same listing, not a second one.
    assert!(!shared.lock().unwrap().dispatch_json(&list_event()));
    wait(&shared, "the host list", |runtime| {
        runtime.snapshot().status.ssh_hosts.state == "ready"
    });

    let listing = shared.lock().unwrap().snapshot().status.ssh_hosts.clone();
    let row = |alias: &str| {
        listing
            .hosts
            .iter()
            .find(|host| host.alias == alias)
            .unwrap_or_else(|| panic!("{alias} is listed: {listing:?}"))
            .clone()
    };
    assert_eq!(
        listing
            .hosts
            .iter()
            .map(|host| host.alias.as_str())
            .collect::<Vec<_>>(),
        ["studio", "studio-ip", "fresh", "broken"],
        "the wildcard Host is never listed"
    );
    assert_eq!(row("studio").address.as_deref(), Some("grab@10.0.0.2:22"));
    assert_eq!(row("studio").added_as.as_deref(), Some("Studio Mac"));
    // Another alias of the same machine reads as added too.
    assert_eq!(row("studio-ip").added_as.as_deref(), Some("Studio Mac"));
    assert_eq!(row("fresh").address.as_deref(), Some("grab@10.0.0.9:2222"));
    assert_eq!(row("fresh").added_as.as_deref(), Some("Old"));
    assert_eq!(row("broken").address, None);
    assert_eq!(row("broken").problem.as_deref(), Some("ssh_failed"));
    assert_eq!(row("broken").added_as, None);
    assert!(!listing.truncated);

    // One `ssh -G` per alias, with the account's own config named.
    let calls = std::fs::read_to_string(home.join("calls")).unwrap();
    let config = home.join(".ssh/config");
    let mut asked: Vec<&str> = calls.lines().collect();
    asked.sort_unstable();
    let expected = |alias: &str| format!("-F {} -G -- {alias}", config.display());
    let mut want = [
        expected("broken"),
        expected("fresh"),
        expected("old-alias"),
        expected("studio"),
        expected("studio-ip"),
    ];
    want.sort_unstable();
    assert_eq!(asked, want.iter().map(String::as_str).collect::<Vec<_>>());
}

#[test]
fn a_missing_config_or_program_lists_nothing_or_names_the_problem() {
    let mut runtime = super::devices::runtime_with_home();
    runtime.devices = Arc::new(
        hide_node::ssh::Connector::new(None).with_ssh_program(PathBuf::from("/nonexistent/ssh")),
    );
    let home = runtime.home_path.clone().expect("fixture home");
    let shared = Arc::new(Mutex::new(runtime));
    shared.lock().unwrap().install_worker_context(
        Arc::downgrade(&shared),
        crate::handle::ChangeNotifier::noop(),
    );
    // No ~/.ssh/config at all: ready, with no host to offer (B52).
    assert!(shared.lock().unwrap().dispatch_json(&list_event()));
    wait(&shared, "the empty list", |runtime| {
        runtime.snapshot().status.ssh_hosts.state == "ready"
    });
    assert!(
        shared
            .lock()
            .unwrap()
            .snapshot()
            .status
            .ssh_hosts
            .hosts
            .is_empty()
    );

    // A config whose ssh cannot start shows the host with the problem.
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(home.join(".ssh/config"), "Host lonely\n").unwrap();
    assert!(shared.lock().unwrap().dispatch_json(&list_event()));
    wait(&shared, "the host list", |runtime| {
        runtime.snapshot().status.ssh_hosts.hosts.len() == 1
    });
    let hosts = shared
        .lock()
        .unwrap()
        .snapshot()
        .status
        .ssh_hosts
        .hosts
        .clone();
    assert_eq!(hosts[0].alias, "lonely");
    assert_eq!(hosts[0].problem.as_deref(), Some("ssh_missing"));
}
