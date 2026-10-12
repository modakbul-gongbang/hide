//! The start against the pinned Herdr itself, in zsh and fish panes whose
//! startup file asks a one-key question, as oh-my-zsh's update prompt does
//! with `read -k 1`. In the incident this replays, the start typed into that
//! question: its first letter answered it and the shell ran the rest.
//!
//! It needs the pinned binary and the shells, so it is ignored by default;
//! run it with `--ignored` and the binaries named in `HIDE_E2E_HERDR_BIN`,
//! `HIDE_TEST_ZSH_BIN` and `HIDE_TEST_FISH_BIN`. The server it starts is
//! private: its own socket, session, config and home, none of the operator's.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use hide_herdr_client::LocalSocketConnector;
use hide_host::shell_integration::UserShell;

use super::*;

/// A shell Herdr runs in its panes, and the user's own startup file for it:
/// it asks the question, and `claude` records the arguments it was started
/// with and becomes a long-running process of that name, which Herdr reads
/// as the agent (`~/bin/claude` runs as `claude`).
struct Fixture {
    variable: &'static str,
    rc: &'static str,
    text: &'static str,
}

const ZSH: Fixture = Fixture {
    variable: "HIDE_TEST_ZSH_BIN",
    rc: ".zshrc",
    text: r#"PS1='fixture %# '
claude() { print -r -- "${PWD:t} $*" >> ~/claude-args; exec -a claude ~/bin/claude 600 }
read -k 1 'answer?Update the framework now? [Y/n] '
print -r -- "${PWD:t} answered [$answer]" >> ~/answers
"#,
};

const FISH: Fixture = Fixture {
    variable: "HIDE_TEST_FISH_BIN",
    // Herdr gives its panes its own XDG_CONFIG_HOME.
    rc: "xdg-config/fish/config.fish",
    text: r#"function fish_prompt; echo -n 'fixture> '; end
function claude
    echo (basename $PWD)" $argv" >> ~/claude-args
    exec /bin/bash -c 'exec -a claude "$HOME/bin/claude" 600'
end
read -n 1 -P 'Update the framework now? [Y/n] ' answer
echo (basename $PWD)" answered [$answer]" >> ~/answers
"#,
};

const QUESTION: &str = "Update the framework now? [Y/n]";

/// The pane's node: it writes the integration for the fixture's shell.
struct Node {
    home: PathBuf,
    shell: PathBuf,
}

impl NodeLink for Node {
    fn call(
        &self,
        call: Call,
        _timeout: Duration,
    ) -> Result<crate::node_access::LinkAnswer, crate::node_access::LinkError> {
        let Call::ShellIntegration = call else {
            panic!("unexpected {call:?}");
        };
        let user = UserShell {
            shell: Some(self.shell.display().to_string()),
            ..Default::default()
        };
        let env = hide_host::shell_integration::install(&self.home, &user).unwrap();
        Ok(crate::node_access::LinkAnswer::Parsed(
            serde_json::to_value(env).unwrap(),
        ))
    }
}

fn pinned_version() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../contracts/herdr-bundle.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    manifest["version"].as_str().unwrap().to_owned()
}

struct PrivateHerdr {
    bin: PathBuf,
    shell: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    server: Option<Child>,
    // Dropped last: it holds every path above.
    _root: tempfile::TempDir,
}

impl PrivateHerdr {
    fn start(bin: PathBuf, fixture: &Fixture) -> Self {
        let shell = PathBuf::from(
            std::env::var_os(fixture.variable)
                .unwrap_or_else(|| panic!("{} names the shell", fixture.variable)),
        );
        let version = Command::new(&bin).arg("--version").output().unwrap().stdout;
        let version = String::from_utf8(version).unwrap();
        assert_eq!(
            version.split_whitespace().last(),
            Some(pinned_version().as_str()),
            "{} is not the pinned Herdr",
            bin.display()
        );
        // A Unix socket path is limited to about a hundred bytes.
        let root = tempfile::Builder::new()
            .prefix("hq")
            .tempdir_in("/tmp")
            .unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        let rc = home.join(fixture.rc);
        std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
        std::fs::write(rc, fixture.text).unwrap();
        // Linux names a process after the file it runs, so `sleep` is copied
        // under the agent's name there. macOS kills a copy of a system binary
        // run from elsewhere, and names the process by its first argument.
        let sleep = which("sleep");
        if cfg!(target_os = "macos") {
            std::os::unix::fs::symlink(&sleep, home.join("bin/claude")).unwrap();
        } else {
            std::fs::copy(&sleep, home.join("bin/claude")).unwrap();
        }
        let config = root.path().join("herdr-config.toml");
        std::fs::write(
            &config,
            "[update]\nversion_check = false\nmanifest_check = false\n",
        )
        .unwrap();
        let socket = root.path().join("h.sock");
        let log = std::fs::File::create(root.path().join("server.log")).unwrap();
        let mut herdr = Self {
            server: None,
            bin,
            shell,
            home,
            socket,
            config,
            _root: root,
        };
        herdr.server = Some(
            herdr
                .command()
                .arg("server")
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .expect("herdr server starts"),
        );
        herdr.wait_for(Duration::from_secs(60), "the server answers ping", || {
            hide_herdr_client::request_with_timeout(
                &herdr.socket,
                "ping",
                serde_json::json!({}),
                Duration::from_secs(2),
            )
            .is_ok()
        });
        herdr
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.bin);
        command.env_clear();
        command
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("SHELL", &self.shell)
            .env("TERM", "xterm-256color")
            .env("XDG_CONFIG_HOME", self.home.join("xdg-config"))
            .env("XDG_STATE_HOME", self.home.join("xdg-state"))
            .env("HERDR_SESSION", "hide-startup-question")
            .env("HERDR_SOCKET_PATH", &self.socket)
            .env("HERDR_CONFIG_PATH", &self.config)
            .env("HERDR_DISABLE_SOUND", "1");
        command
    }

    fn connector(&self) -> LocalSocketConnector {
        LocalSocketConnector::new(&self.socket)
    }

    fn request(&self, method: &str, params: Value) -> Value {
        request_with_connector(&self.connector(), method, params, READ_TIMEOUT)
            .unwrap_or_else(|error| panic!("{method}: {error}"))
    }

    /// A workspace in `folder` whose pane runs the shell with `env`.
    fn pane(&self, folder: &str, env: &std::collections::BTreeMap<String, String>) -> String {
        let cwd = self.home.join(folder);
        std::fs::create_dir_all(&cwd).unwrap();
        let params = wire::workspace_create_with_env_params(
            cwd.to_str().unwrap(),
            folder,
            false,
            env.clone(),
        )
        .unwrap();
        wire::created_workspace(self.request("workspace.create", params))
            .unwrap()
            .2
    }

    fn screen(&self, pane: &str) -> String {
        let params = wire::pane_read_params(pane, "visible", 40).unwrap();
        wire::pane_text(self.request("pane.read", params))
            .unwrap()
            .text
    }

    fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.home.join(name)).unwrap_or_default()
    }

    #[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
    fn wait_for(&self, within: Duration, what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + within;
        while !done() {
            assert!(
                Instant::now() < deadline,
                "{what}; server log:\n{}",
                std::fs::read_to_string(self.socket.with_file_name("server.log"))
                    .unwrap_or_default()
            );
            thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for PrivateHerdr {
    fn drop(&mut self) {
        let _ = self.command().args(["server", "stop"]).output();
        if let Some(server) = self.server.as_mut() {
            let _ = server.kill();
            let _ = server.wait();
        }
    }
}

fn which(program: &str) -> PathBuf {
    ["/bin", "/usr/bin"]
        .iter()
        .map(|folder| Path::new(folder).join(program))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("{program} is not installed"))
}

fn start(
    herdr: &PrivateHerdr,
    node: &dyn NodeLink,
    pane: &str,
    shell: ShellStart,
    name: &str,
    wait: Duration,
) -> Result<Value, StartError> {
    let params = wire::agent_start_params(
        pane,
        name,
        "claude",
        vec!["--".into(), format!("promote {name}")],
        crate::codex_launch::CodexDaemon::Unknown,
    )
    .unwrap();
    start_within(
        &herdr.connector(),
        Some(node),
        name,
        pane,
        shell,
        params,
        Duration::from_secs(10),
        wait,
        Duration::ZERO,
        &|| Ok(()),
    )
}

/// The pane the agent Herdr knows by `name` runs in, once it has shown.
fn agent_pane(herdr: &PrivateHerdr, name: &str) -> Option<String> {
    let answer = request_with_connector(
        &herdr.connector(),
        "agent.get",
        wire::agent_target_params(name).unwrap(),
        READ_TIMEOUT,
    )
    .ok()?;
    let agent = &answer["agent"];
    // Herdr names the start's kind at once; the agent has shown only once
    // its launch is no longer pending.
    if agent["launch_pending"] == Value::Bool(true) {
        return None;
    }
    agent["agent"].as_str()?;
    agent["pane_id"].as_str().map(str::to_owned)
}

#[test]
#[ignore = "needs the pinned Herdr and zsh: set HIDE_E2E_HERDR_BIN and HIDE_TEST_ZSH_BIN and run with --ignored"]
fn a_zsh_startup_question_is_never_answered_by_a_start_and_each_pane_runs_its_own_prompt() {
    startup_question(&ZSH);
}

#[test]
#[ignore = "needs the pinned Herdr and fish: set HIDE_E2E_HERDR_BIN and HIDE_TEST_FISH_BIN and run with --ignored"]
fn a_fish_startup_question_is_never_answered_by_a_start_and_each_pane_runs_its_own_prompt() {
    startup_question(&FISH);
}

fn startup_question(fixture: &Fixture) {
    let bin = PathBuf::from(
        std::env::var_os("HIDE_E2E_HERDR_BIN").expect("HIDE_E2E_HERDR_BIN names the pinned herdr"),
    );
    let herdr = PrivateHerdr::start(bin, fixture);
    let node = Node {
        home: herdr.home.clone(),
        shell: herdr.shell.clone(),
    };
    let (env, shell) = shell_environment(Some(&node), "real-herdr");
    assert_eq!(shell, ShellStart::Integrated);

    // Two starts back to back, each in its own new pane, both shells held by
    // the question.
    let first = herdr.pane("m43", &env);
    let second = herdr.pane("m72", &env);
    for pane in [&first, &second] {
        herdr.wait_for(Duration::from_secs(20), "the question is asked", || {
            herdr.screen(pane).contains(QUESTION)
        });
    }
    for (pane, name) in [(&first, "m43"), (&second, "m72")] {
        let Err(StartError::ShellStarting(line)) = start(
            &herdr,
            &node,
            pane,
            ShellStart::Integrated,
            name,
            Duration::from_secs(2),
        ) else {
            panic!("a shell held by its question is not a start");
        };
        assert!(
            line.as_deref().is_some_and(|line| line.contains(QUESTION)),
            "{line:?}"
        );
    }
    assert_eq!(herdr.file("answers"), "", "no start answered the question");
    assert_eq!(herdr.file("claude-args"), "", "nothing was typed");

    // The incident: a start that only waits for the shell to hold the
    // terminal types into the question, whose one key is the command's first
    // letter, and the shell runs the rest.
    let plain = herdr.pane("plain", &Default::default());
    herdr.wait_for(Duration::from_secs(20), "the question is asked", || {
        herdr.screen(&plain).contains(QUESTION)
    });
    start(
        &herdr,
        &node,
        &plain,
        ShellStart::Plain,
        "plain",
        Duration::from_secs(2),
    )
    .expect("Herdr types the start");
    herdr.wait_for(Duration::from_secs(20), "the question is answered", || {
        herdr.file("answers").contains("plain answered [c]")
    });
    herdr.wait_for(Duration::from_secs(20), "the rest runs", || {
        herdr.screen(&plain).contains("laude")
    });
    assert_eq!(herdr.file("claude-args"), "", "no agent ran");

    // The operator answers each question; the same starts then launch, each
    // with its own prompt, under its own name, in its own pane.
    for pane in [&first, &second] {
        herdr.request(
            "pane.send_text",
            wire::pane_send_text_params(pane, "n").unwrap(),
        );
    }
    for (pane, name) in [(&first, "m43"), (&second, "m72")] {
        start(
            &herdr,
            &node,
            pane,
            ShellStart::Integrated,
            name,
            Duration::from_secs(20),
        )
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }
    for (pane, name) in [(&first, "m43"), (&second, "m72")] {
        herdr.wait_for(Duration::from_secs(20), "the agent shows", || {
            agent_pane(&herdr, name).as_deref() == Some(pane.as_str())
        });
    }
    let mut answers: Vec<_> = herdr
        .file("answers")
        .lines()
        .filter(|line| !line.starts_with("plain"))
        .map(str::to_owned)
        .collect();
    answers.sort();
    assert_eq!(answers, ["m43 answered [n]", "m72 answered [n]"]);
    let mut args: Vec<_> = herdr
        .file("claude-args")
        .lines()
        .map(str::to_owned)
        .collect();
    args.sort();
    assert_eq!(args, ["m43 -- promote m43", "m72 -- promote m72"]);
}
