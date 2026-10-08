use hide_platform::user_agents::{UserAgents, account_home};
use std::path::Path;

#[test]
fn login_agent_paths_belong_to_the_supplied_home() {
    let home = Path::new("isolated-home");
    assert_eq!(
        UserAgents::plist(home, "example.agent"),
        home.join("Library/LaunchAgents/example.agent.plist")
    );
    assert!(account_home().unwrap().is_absolute());
}

#[cfg(unix)]
#[test]
fn fixture_inspection_failure_does_not_attempt_a_bootout() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::AtomicBool;
    let directory = tempfile::tempdir().unwrap();
    let command = directory.path().join("launchctl-fixture");
    std::fs::write(
        &command,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$HOME/calls\"\nexit 5\n",
    )
    .unwrap();
    std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755)).unwrap();
    let boundary = UserAgents::fixture(command, "isolated-domain".into());
    assert!(
        boundary
            .unload("example.agent", directory.path(), &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("calls")).unwrap(),
        "print\n"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn systems_without_login_agents_need_no_command() {
    use std::sync::atomic::AtomicBool;
    let directory = tempfile::tempdir().unwrap();
    UserAgents::current()
        .unload("example.agent", directory.path(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}
