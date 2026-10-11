use hide_platform::user_agents::{UserAgents, account_home, last_exit_of};
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
    use std::sync::atomic::AtomicBool;
    let directory = tempfile::tempdir().unwrap();
    let command = directory.path().join("launchctl-fixture");
    crate::stand_ins::program(
        &command,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$HOME/calls\"\nexit 5\n",
    );
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

/// A bootout returns once its job has ended, which a job may take launchd's
/// whole exit allowance for: one past the bound of every other command
/// still unloads it.
#[cfg(unix)]
#[test]
fn a_job_that_takes_its_time_to_stop_is_still_unloaded() {
    use std::sync::atomic::AtomicBool;
    let directory = tempfile::tempdir().unwrap();
    let command = directory.path().join("launchctl-fixture");
    crate::stand_ins::program(
        &command,
        "#!/bin/sh\ncase \"$1\" in\n  print) [ -f \"$HOME/loaded\" ] && exit 0; exit 113 ;;\n  bootout) sleep 6; rm -f \"$HOME/loaded\" ;;\nesac\nexit 0\n",
    );
    std::fs::write(directory.path().join("loaded"), "").unwrap();
    let boundary = UserAgents::fixture(command, "isolated-domain".into());
    boundary
        .unload("example.agent", directory.path(), &AtomicBool::new(false))
        .unwrap();
    assert!(!directory.path().join("loaded").exists());
}

/// An install replaces what is loaded, then loads the written list; a
/// removal unloads before it deletes the list.
#[cfg(unix)]
#[test]
fn an_install_loads_the_written_list_and_a_removal_unloads_it_first() {
    use hide_platform::user_agents::LoginAgent;
    use std::sync::atomic::AtomicBool;
    let directory = tempfile::tempdir().unwrap();
    let command = directory.path().join("launchctl-fixture");
    // `print` answers loaded once the list is bootstrapped and until a
    // bootout.
    crate::stand_ins::program(
        &command,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/calls\"\ncase \"$1\" in\n  print) [ -f \"$HOME/loaded\" ] && exit 0; exit 113 ;;\n  bootstrap) touch \"$HOME/loaded\" ;;\n  bootout) rm -f \"$HOME/loaded\" ;;\nesac\nexit 0\n",
    );
    let boundary = UserAgents::fixture(command, "isolated-domain".into());
    let home = directory.path();
    let stop = AtomicBool::new(false);
    let agent = LoginAgent {
        label: "example.agent",
        program: Path::new("/bin/sleep"),
        arguments: &["600"],
        environment: &[("HOME", "/private/tmp/a&b")],
        log: &home.join("agent.log"),
    };
    boundary.install(&agent, home, &stop).unwrap();
    let list = UserAgents::plist(home, "example.agent");
    let written = std::fs::read_to_string(&list).unwrap();
    assert!(
        written.contains("<string>/private/tmp/a&amp;b</string>"),
        "{written}"
    );
    #[cfg(target_os = "macos")]
    {
        let lint = std::process::Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&list)
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
    }
    assert!(boundary.is_loaded("example.agent", home, &stop).unwrap());
    boundary.install(&agent, home, &stop).unwrap();
    boundary.remove("example.agent", home, &stop).unwrap();
    assert!(!list.exists());
    let calls = std::fs::read_to_string(home.join("calls")).unwrap();
    let bootstrap = format!("bootstrap isolated-domain {}", list.display());
    let changes: Vec<&str> = calls
        .lines()
        .filter(|call| !call.starts_with("print "))
        .collect();
    assert_eq!(
        changes,
        [
            bootstrap.as_str(),
            "bootout isolated-domain/example.agent",
            bootstrap.as_str(),
            "bootout isolated-domain/example.agent",
        ]
    );
}

/// The real launchd of a disposable macOS runner (PRD core-host-node-move
/// D-16): a login agent installed with this list starts, comes back after it
/// is killed, stays stopped after it exits successfully, and is gone with
/// its process when removed. It refuses to run anywhere but a hosted CI
/// runner, since `gui/<uid>` is the account's real session whatever HOME is.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "real launchd: only on a disposable macOS CI runner"]
fn a_login_agent_restarts_after_a_kill_never_after_success_and_ends_when_removed() {
    use hide_platform::user_agents::LoginAgent;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    assert!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
        "this test reaches the account's real launchd; it runs only on a hosted CI runner"
    );
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let program = home.join("agent");
    // Records each start; a TERM ends it successfully.
    crate::stand_ins::program(
        &program,
        "#!/bin/sh\ntrap 'exit 0' TERM\necho $$ >> \"$HOME/starts\"\nwhile :; do sleep 1; done\n",
    );
    let label = format!("dev.withhide.contract.{}", std::process::id());
    let home_value = home.to_string_lossy().into_owned();
    let environment = [("HOME", home_value.as_str())];
    let agent = LoginAgent {
        label: &label,
        program: &program,
        arguments: &[],
        environment: &environment,
        log: &home.join("agent.log"),
    };
    let agents = UserAgents::current();
    let stop = AtomicBool::new(false);
    let starts = || -> Vec<u32> {
        std::fs::read_to_string(home.join("starts"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect()
    };
    #[allow(clippy::disallowed_methods)] // a bounded poll of another process
    let wait = |what: &str, within: Duration, done: &dyn Fn() -> bool| {
        let deadline = Instant::now() + within;
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    let alive = |pid: u32| hide_platform::process::is_alive(pid);
    assert!(
        agents.session_present(home, &stop).unwrap(),
        "no GUI session"
    );
    agents.install(&agent, home, &stop).unwrap();
    let removed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait("the first start", Duration::from_secs(20), &|| {
            starts().len() == 1
        });
        let first = starts()[0];
        assert!(alive(first));
        hide_platform::process::kill_tree(first).unwrap();
        wait("the restart after a kill", Duration::from_secs(30), &|| {
            starts().len() == 2
        });
        let second = starts()[1];
        assert!(alive(second));
        hide_platform::process::terminate(second).unwrap();
        wait("the successful exit", Duration::from_secs(10), &|| {
            !alive(second)
        });
        #[allow(clippy::disallowed_methods)] // past launchd's ten-second throttle
        std::thread::sleep(Duration::from_secs(15));
        assert_eq!(starts().len(), 2, "launchd restarted a successful exit");
        agents.kickstart(&label, home, &stop).unwrap();
        wait("the start asked for", Duration::from_secs(20), &|| {
            starts().len() == 3
        });
        starts()[2]
    }));
    let removal = agents.remove(&label, home, &stop);
    let running = removed.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    removal.unwrap();
    assert!(
        !agents.is_loaded(&label, home, &stop).unwrap(),
        "still loaded"
    );
    wait("the removed agent's end", Duration::from_secs(10), &|| {
        !alive(running)
    });
    assert!(!UserAgents::plist(home, &label).exists());
}

/// A job's exit is read only once it is not running and has exited.
#[test]
fn a_job_s_exit_is_read_only_once_it_ran_and_stopped() {
    let job = |state: &str, code: &str| {
        format!(
            "gui/501/example.job = {{\n\tactive count = 0\n\tstate = {state}\n\truns = 1\n\tlast exit code = {code}\n}}\n"
        )
    };
    assert_eq!(last_exit_of(&job("not running", "3")).as_deref(), Some("3"));
    assert_eq!(last_exit_of(&job("running", "3")), None);
    assert_eq!(last_exit_of(&job("not running", "(never exited)")), None);
    assert_eq!(last_exit_of("gui/501/example.job = {\n}\n"), None);
}

/// A one-shot job is loaded from a list outside the login agents folder,
/// so no later login loads it, and launchd never restarts it.
#[cfg(unix)]
#[test]
fn a_one_shot_job_is_loaded_from_its_own_list_and_never_restarted() {
    use hide_platform::user_agents::LoginAgent;
    use std::sync::atomic::AtomicBool;
    let directory = tempfile::tempdir().unwrap();
    let command = directory.path().join("launchctl-fixture");
    crate::stand_ins::program(
        &command,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/calls\"\ncase \"$1\" in\n  print) [ -f \"$HOME/loaded\" ] && exit 0; exit 113 ;;\n  bootstrap) touch \"$HOME/loaded\" ;;\n  bootout) rm -f \"$HOME/loaded\" ;;\nesac\nexit 0\n",
    );
    let boundary = UserAgents::fixture(command, "isolated-domain".into());
    let home = directory.path();
    let stop = AtomicBool::new(false);
    let list = home.join("job.plist");
    let agent = LoginAgent {
        label: "example.job",
        program: Path::new("/usr/bin/true"),
        arguments: &[],
        environment: &[],
        log: &home.join("job.log"),
    };
    boundary.start_once(&agent, &list, home, &stop).unwrap();
    let written = std::fs::read_to_string(&list).unwrap();
    assert!(
        written.contains("<key>KeepAlive</key><false/>"),
        "{written}"
    );
    assert!(!written.contains("ThrottleInterval"), "{written}");
    #[cfg(target_os = "macos")]
    {
        let lint = std::process::Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&list)
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
    }
    assert!(!UserAgents::plist(home, "example.job").exists());
    boundary.unload("example.job", home, &stop).unwrap();
    let calls = std::fs::read_to_string(home.join("calls")).unwrap();
    let changes: Vec<&str> = calls
        .lines()
        .filter(|call| !call.starts_with("print "))
        .collect();
    assert_eq!(
        changes,
        [
            format!("bootstrap isolated-domain {}", list.display()).as_str(),
            "bootout isolated-domain/example.job",
        ]
    );
}

/// The real launchd of a disposable macOS runner (PRD core-host-node-move
/// B3): a one-shot job runs once in the GUI session, is not started again
/// after it fails, and is gone when unloaded. It refuses to run anywhere
/// but a hosted CI runner, like the login agent's test.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "real launchd: only on a disposable macOS CI runner"]
fn a_one_shot_job_runs_once_in_the_gui_session_even_when_it_fails() {
    use hide_platform::user_agents::LoginAgent;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    assert!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
        "this test reaches the account's real launchd; it runs only on a hosted CI runner"
    );
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let program = home.join("job");
    // Records each start and fails, which a login agent would restart.
    crate::stand_ins::program(&program, "#!/bin/sh\necho $$ >> \"$HOME/starts\"\nexit 3\n");
    let label = format!("dev.withhide.contract.once.{}", std::process::id());
    let home_value = home.to_string_lossy().into_owned();
    let environment = [("HOME", home_value.as_str())];
    let agent = LoginAgent {
        label: &label,
        program: &program,
        arguments: &[],
        environment: &environment,
        log: &home.join("job.log"),
    };
    let agents = UserAgents::current();
    let stop = AtomicBool::new(false);
    let starts = || {
        std::fs::read_to_string(home.join("starts"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    agents
        .start_once(&agent, &home.join("job.plist"), home, &stop)
        .unwrap();
    let ran = std::panic::catch_unwind(|| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while starts() == 0 {
            assert!(Instant::now() < deadline, "the job never ran");
            #[allow(clippy::disallowed_methods)] // a bounded poll of another process
            std::thread::sleep(Duration::from_millis(200));
        }
        #[allow(clippy::disallowed_methods)] // past launchd's ten-second throttle
        std::thread::sleep(Duration::from_secs(15));
        assert_eq!(starts(), 1, "launchd started a one-shot job again");
        assert_eq!(
            agents.last_exit(&label, home, &stop).unwrap().as_deref(),
            Some("3")
        );
    });
    let unloaded = agents.unload(&label, home, &stop);
    ran.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    unloaded.unwrap();
    assert!(!agents.is_loaded(&label, home, &stop).unwrap());
    assert!(!UserAgents::plist(home, &label).exists());
}
