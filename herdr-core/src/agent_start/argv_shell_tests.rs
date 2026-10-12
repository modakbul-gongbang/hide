//! A pane Hide opens with `layout.apply` running the shell's own argv, the
//! way VS Code starts its terminals, against the pinned Herdr itself: the
//! argv carries Hide's startup step, whose end marks the first prompt, and
//! the user's own startup files ask a one-key question first. Each shell is
//! its own test, named after it, and needs the shell's binary in its
//! variable (`HIDE_TEST_<SHELL>_BIN`) and the pinned Herdr in
//! `HIDE_E2E_HERDR_BIN`, so they are ignored by default.
//!
//! On Windows the questions live where the shells read them, the current
//! user's PowerShell profile and cmd's `AutoRun`, which belong to the
//! machine's account; those tests write them only with
//! `HIDE_TEST_WRITE_USER_SHELL_FILES` set, which only a CI runner sets, and
//! each question asks only in a pane whose env carries `HIDE_PROBE_QUESTION`.
//!
//! The server is private: its own socket, session, config and home.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use hide_herdr_client::LocalSocketConnector;
use serde_json::json;

use super::*;

const QUESTION: &str = "Update the framework now? [Y/n]";
/// The arguments of Hide's startup step's last command, the first prompt's
/// mark, after `herdr pane report-metadata <pane>`.
const MARK: &str = "--source hide:shell --token hide_prompt=1";

/// A shell started the argv way: its argv, the env it is given, and the
/// user's files that ask the question.
struct Launch {
    argv: Vec<String>,
    env: BTreeMap<String, String>,
}

struct PrivateHerdr {
    bin: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    log: PathBuf,
    server: Option<Child>,
    _root: tempfile::TempDir,
}

impl PrivateHerdr {
    fn new() -> Self {
        let bin = PathBuf::from(
            std::env::var_os("HIDE_E2E_HERDR_BIN")
                .expect("HIDE_E2E_HERDR_BIN names the pinned herdr"),
        );
        let root = if cfg!(unix) {
            // A Unix socket path is limited to about a hundred bytes.
            tempfile::Builder::new()
                .prefix("ha")
                .tempdir_in("/tmp")
                .unwrap()
        } else {
            tempfile::tempdir().unwrap()
        };
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        let config = root.path().join("herdr-config.toml");
        std::fs::write(
            &config,
            "[update]\nversion_check = false\nmanifest_check = false\n",
        )
        .unwrap();
        let mut herdr = Self {
            bin,
            socket: root.path().join("h.sock"),
            log: root.path().join("server.log"),
            home,
            config,
            server: None,
            _root: root,
        };
        herdr.start();
        herdr
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.bin);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy().to_uppercase();
            if key.starts_with("HERDR_") || key.starts_with("HIDE_") {
                command.env_remove(key);
            }
        }
        command
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("TERM", "xterm-256color")
            .env("XDG_CONFIG_HOME", self.home.join("xdg-config"))
            .env("XDG_STATE_HOME", self.home.join("xdg-state"))
            .env("HERDR_SESSION", "hide-argv-shell")
            .env("HERDR_SOCKET_PATH", &self.socket)
            .env("HERDR_CONFIG_PATH", &self.config)
            .env("HERDR_DISABLE_SOUND", "1");
        command
    }

    fn start(&mut self) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .unwrap();
        self.server = Some(
            self.command()
                .arg("server")
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .expect("herdr server starts"),
        );
        self.wait_until(
            Duration::from_secs(60),
            || {
                hide_herdr_client::request_with_timeout(
                    &self.socket,
                    "ping",
                    json!({}),
                    Duration::from_secs(2),
                )
                .is_ok()
            },
            || format!("the server answers ping; log:\n{}", self.server_log()),
        );
    }

    fn stop(&mut self) {
        let _ = self.command().args(["server", "stop"]).output();
        if let Some(mut server) = self.server.take() {
            self.wait_until(
                Duration::from_secs(30),
                || server.try_wait().is_ok_and(|status| status.is_some()),
                || "the server stops".into(),
            );
        }
    }

    fn server_log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn connector(&self) -> LocalSocketConnector {
        LocalSocketConnector::new(&self.socket)
    }

    fn request(&self, method: &str, params: Value) -> Value {
        request_with_connector(&self.connector(), method, params, READ_TIMEOUT)
            .unwrap_or_else(|error| panic!("{method}: {error}"))
    }

    fn screen(&self, pane: &str) -> String {
        let params = wire::pane_read_params(pane, "visible", 40).unwrap();
        wire::pane_text(self.request("pane.read", params))
            .unwrap()
            .text
    }

    fn processes(&self, pane: &str) -> Value {
        self.request("pane.process_info", json!({"pane_id": pane}))
    }

    fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.home.join(name))
            .unwrap_or_default()
            .replace("\r\n", "\n")
    }

    /// A new unfocused tab in `workspace` whose one pane runs `launch`.
    fn tab(&self, workspace: &str, label: &str, launch: &Launch) -> (String, String) {
        let cwd = self.home.join(label);
        std::fs::create_dir_all(&cwd).unwrap();
        let answer = self.request(
            "layout.apply",
            json!({"workspace_id": workspace, "tab_label": label, "focus": false,
                "root": {"type": "pane", "cwd": cwd, "env": launch.env, "command": launch.argv}}),
        );
        let layout = &answer["layout"];
        (
            layout["tab_id"].as_str().unwrap().to_owned(),
            layout["root"]["pane_id"].as_str().unwrap().to_owned(),
        )
    }

    fn wait_on(&self, pane: &str, within: Duration, what: &str, done: impl FnMut() -> bool) {
        self.wait_until(within, done, || {
            format!(
                "{what}; pane {pane} shows:\n{}\nprocesses: {}",
                self.screen(pane),
                self.processes(pane)
            )
        });
    }

    #[allow(clippy::disallowed_methods)] // a polling helper: it sleeps between observations of a state, bounded by a deadline
    fn wait_until(
        &self,
        within: Duration,
        mut done: impl FnMut() -> bool,
        failure: impl FnOnce() -> String,
    ) {
        let deadline = Instant::now() + within;
        while !done() {
            if Instant::now() >= deadline {
                panic!("{}", failure());
            }
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

fn shell_bin(variable: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| panic!("{variable} names the shell"))
}

/// The pane's PATH: the stand-in agent first.
fn pane_path(herdr: &PrivateHerdr) -> String {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(
        std::iter::once(herdr.home.join("bin")).chain(std::env::split_paths(&inherited)),
    )
    .unwrap()
    .into_string()
    .unwrap()
}

/// `claude` on unix: records the folder it started in and its arguments,
/// then runs as a long-lived process of that name, which Herdr reads as the
/// agent. Linux names a process after its file, macOS after its first
/// argument.
#[cfg(unix)]
fn unix_agent(herdr: &PrivateHerdr) {
    let agent = herdr.home.join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .unwrap();
    if cfg!(target_os = "macos") {
        std::os::unix::fs::symlink(sleep, agent.join("claude")).unwrap();
    } else {
        std::fs::copy(sleep, agent.join("claude")).unwrap();
    }
    let script = herdr.home.join("bin/claude");
    std::fs::write(
        &script,
        "#!/bin/bash\nprintf '%s\\n' \"${PWD##*/} $*\" >> \"$HOME/claude-args\"\nexec -a claude \"$HOME/agent/claude\" 600\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// Herdr starts Windows agents through PowerShell: `claude.ps1` is found as
/// a script, records its folder and arguments, and runs a copy of
/// `timeout.exe` named `claude.exe`, which Herdr reads as the agent.
#[cfg(windows)]
fn windows_agent(herdr: &PrivateHerdr) {
    let agent = herdr.home.join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::fs::copy(
        Path::new(&system).join(r"System32\timeout.exe"),
        agent.join("claude.exe"),
    )
    .unwrap();
    std::fs::write(
        herdr.home.join(r"bin\claude.ps1"),
        "Add-Content -Path \"$env:USERPROFILE\\claude-args\" -Value ((Split-Path -Leaf (Get-Location).Path) + ' ' + ($args -join ' '))\r\n& \"$env:USERPROFILE\\agent\\claude.exe\" /t 600 /nobreak\r\n",
    )
    .unwrap();
}

/// The mark as a POSIX shell command.
#[cfg(unix)]
fn posix_mark() -> String {
    format!(
        "\"$HERDR_BIN_PATH\" pane report-metadata \"$HERDR_PANE_ID\" {} >/dev/null 2>&1",
        MARK
    )
}

#[cfg(unix)]
fn login() -> bool {
    // Herdr's `auto` mode: login shells on macOS, not on Linux.
    cfg!(target_os = "macos")
}

#[cfg(unix)]
fn zsh(herdr: &PrivateHerdr) -> Launch {
    std::fs::write(
        herdr.home.join(".zshrc"),
        format!(
            "PS1='fixture %# '\nread -k 1 'answer?{QUESTION} '\nprint -r -- \"${{PWD:t}} answered [$answer]\" >> ~/answers\n"
        ),
    )
    .unwrap();
    let user = hide_host::shell_integration::UserShell {
        shell: Some(shell_bin("HIDE_TEST_ZSH_BIN")),
        ..Default::default()
    };
    let installed = hide_host::shell_integration::install(&herdr.home, &user).unwrap();
    let mut argv = vec![shell_bin("HIDE_TEST_ZSH_BIN")];
    if login() {
        argv.push("-l".into());
    }
    Launch {
        argv,
        env: [
            ("ZDOTDIR".to_owned(), installed["ZDOTDIR"].clone()),
            ("PATH".to_owned(), pane_path(herdr)),
        ]
        .into(),
    }
}

#[cfg(unix)]
fn bash(herdr: &PrivateHerdr) -> Launch {
    std::fs::write(
        herdr.home.join(".bashrc"),
        format!(
            "PS1='fixture$ '\nread -n 1 -p '{QUESTION} ' answer; echo\nprintf '%s\\n' \"${{PWD##*/}} answered [$answer]\" >> ~/answers\n"
        ),
    )
    .unwrap();
    std::fs::write(
        herdr.home.join(".bash_profile"),
        "[ -r ~/.bashrc ] && . ~/.bashrc\n",
    )
    .unwrap();
    // Hide's bash startup step: the user's files in bash's own order, as a
    // login shell or not, then the mark.
    let init = herdr.home.join("hide-init.bash");
    std::fs::write(
        &init,
        format!(
            "if [ -n \"${{HIDE_SHELL_LOGIN-}}\" ]; then\n  unset HIDE_SHELL_LOGIN\n  [ -r /etc/profile ] && . /etc/profile\n  if [ -r ~/.bash_profile ]; then . ~/.bash_profile\n  elif [ -r ~/.bash_login ]; then . ~/.bash_login\n  elif [ -r ~/.profile ]; then . ~/.profile\n  fi\nelse\n  [ -r ~/.bashrc ] && . ~/.bashrc\nfi\n{}\n",
            posix_mark()
        ),
    )
    .unwrap();
    let mut env: BTreeMap<String, String> = [("PATH".to_owned(), pane_path(herdr))].into();
    if login() {
        env.insert("HIDE_SHELL_LOGIN".into(), "1".into());
    }
    Launch {
        argv: vec![
            shell_bin("HIDE_TEST_BASH_BIN"),
            "--init-file".into(),
            init.display().to_string(),
        ],
        env,
    }
}

#[cfg(unix)]
fn fish(herdr: &PrivateHerdr) -> Launch {
    let config = herdr.home.join("xdg-config/fish");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.fish"),
        format!(
            "function fish_prompt; echo -n 'fixture> '; end\nread -n 1 -P '{QUESTION} ' answer\necho (basename $PWD)\" answered [$answer]\" >> ~/answers\n"
        ),
    )
    .unwrap();
    // fish generates completions from man pages in the background the first
    // time it reads input unless this folder exists.
    std::fs::create_dir_all(herdr.home.join(".local/share/fish/generated_completions")).unwrap();
    let mut argv = vec![shell_bin("HIDE_TEST_FISH_BIN")];
    if login() {
        argv.push("-l".into());
    }
    argv.extend([
        "-C".into(),
        format!(
            "$HERDR_BIN_PATH pane report-metadata $HERDR_PANE_ID {} >/dev/null 2>&1",
            MARK
        ),
    ]);
    Launch {
        argv,
        env: [("PATH".to_owned(), pane_path(herdr))].into(),
    }
}

#[cfg(windows)]
fn write_user_shell_files() {
    assert!(
        std::env::var_os("HIDE_TEST_WRITE_USER_SHELL_FILES").is_some(),
        "the Windows shells read their startup question from the account's own files; only a CI runner sets HIDE_TEST_WRITE_USER_SHELL_FILES"
    );
}

#[cfg(windows)]
fn powershell(herdr: &PrivateHerdr, variable: &str) -> Launch {
    write_user_shell_files();
    let bin = shell_bin(variable);
    let profile = Command::new(&bin)
        .args(["-NoProfile", "-Command", "$PROFILE.CurrentUserCurrentHost"])
        .output()
        .unwrap();
    let profile = PathBuf::from(String::from_utf8(profile.stdout).unwrap().trim());
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(
        &profile,
        format!(
            "if ($env:HIDE_PROBE_QUESTION) {{\r\n  Write-Host -NoNewline '{QUESTION} '\r\n  $key = [Console]::ReadKey($true)\r\n  Add-Content -Path \"$env:USERPROFILE\\answers\" -Value ((Split-Path -Leaf (Get-Location).Path) + ' answered [' + $key.KeyChar + ']')\r\n}}\r\n"
        ),
    )
    .unwrap();
    let init = herdr.home.join("hide-init.ps1");
    std::fs::write(
        &init,
        format!(
            "& $env:HERDR_BIN_PATH pane report-metadata $env:HERDR_PANE_ID {} | Out-Null\r\n",
            MARK
        ),
    )
    .unwrap();
    Launch {
        argv: vec![
            bin,
            "-NoExit".into(),
            "-Command".into(),
            format!(". '{}'", init.display()),
        ],
        env: [
            ("PATH".to_owned(), pane_path(herdr)),
            ("HIDE_PROBE_QUESTION".to_owned(), "1".to_owned()),
        ]
        .into(),
    }
}

#[cfg(windows)]
fn cmd(herdr: &PrivateHerdr) -> Launch {
    write_user_shell_files();
    // `if errorlevel` is read when it runs, after `choice`; the rest of the
    // line is expanded once, before it.
    let answer = format!(
        "if defined HIDE_PROBE_QUESTION (choice /c yn /n /m \"{QUESTION} \" & if errorlevel 2 (echo %CD% answered [n]>> \"%USERPROFILE%\\answers\") else (echo %CD% answered [y]>> \"%USERPROFILE%\\answers\"))"
    );
    let status = Command::new("reg")
        .args([
            "add",
            r"HKCU\Software\Microsoft\Command Processor",
            "/v",
            "AutoRun",
            "/t",
            "REG_SZ",
            "/d",
            &answer,
            "/f",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let init = herdr.home.join("hide-init.cmd");
    std::fs::write(
        &init,
        format!(
            "@\"%HERDR_BIN_PATH%\" pane report-metadata %HERDR_PANE_ID% {} >nul 2>&1\r\n",
            MARK
        ),
    )
    .unwrap();
    Launch {
        argv: vec![
            shell_bin("HIDE_TEST_CMD_BIN"),
            "/k".into(),
            init.display().to_string(),
        ],
        env: [
            ("PATH".to_owned(), pane_path(herdr)),
            ("HIDE_PROBE_QUESTION".to_owned(), "1".to_owned()),
        ]
        .into(),
    }
}

fn start(
    herdr: &PrivateHerdr,
    pane: &str,
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
        None,
        name,
        pane,
        ShellStart::Integrated,
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
    if agent["launch_pending"] == Value::Bool(true) {
        return None;
    }
    agent["agent"].as_str()?;
    agent["pane_id"].as_str().map(str::to_owned)
}

/// The scenario every shell runs: two tabs back to back, each held by the
/// question with nothing typed, then answered, each start then running its
/// own prompt under its own name; the operator's tab keeps the focus; after
/// a server restart the tabs keep their labels and run what Herdr restores.
fn argv_shell(make: impl FnOnce(&PrivateHerdr) -> Launch) {
    let mut herdr = PrivateHerdr::new();
    #[cfg(unix)]
    unix_agent(&herdr);
    #[cfg(windows)]
    windows_agent(&herdr);
    let launch = make(&herdr);
    let created = herdr.request(
        "workspace.create",
        json!({"cwd": herdr.home, "label": "argv", "focus": false}),
    );
    let workspace = created["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let operator_tab = created["tab"]["tab_id"].as_str().unwrap().to_owned();

    let first = herdr.tab(&workspace, "m43", &launch);
    let second = herdr.tab(&workspace, "m72", &launch);
    for (_, pane) in [&first, &second] {
        herdr.wait_on(
            pane,
            Duration::from_secs(30),
            "the question is asked",
            || herdr.screen(pane).contains(QUESTION),
        );
    }
    for ((_, pane), name) in [(&first, "m43"), (&second, "m72")] {
        let Err(StartError::ShellStarting(line)) =
            start(&herdr, pane, name, Duration::from_secs(2))
        else {
            panic!(
                "{name}: a shell held by its question is not a start; pane shows:\n{}\nprocesses: {}",
                herdr.screen(pane),
                herdr.processes(pane)
            );
        };
        assert!(
            line.as_deref().is_some_and(|line| line.contains(QUESTION)),
            "{line:?}"
        );
    }
    assert_eq!(herdr.file("answers"), "", "no start answered the question");
    assert_eq!(herdr.file("claude-args"), "", "nothing was typed");
    let workspace_now = herdr.request("workspace.get", json!({"workspace_id": workspace}));
    assert_eq!(
        workspace_now["workspace"]["active_tab_id"].as_str(),
        Some(operator_tab.as_str()),
        "an unfocused tab leaves the operator's tab active"
    );

    for (_, pane) in [&first, &second] {
        herdr.request(
            "pane.send_text",
            wire::pane_send_text_params(pane, "n").unwrap(),
        );
    }
    for ((_, pane), name) in [(&first, "m43"), (&second, "m72")] {
        start(&herdr, pane, name, Duration::from_secs(30))
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }
    for ((_, pane), name) in [(&first, "m43"), (&second, "m72")] {
        herdr.wait_on(pane, Duration::from_secs(30), "the agent shows", || {
            agent_pane(&herdr, name).as_deref() == Some(pane.as_str())
        });
    }
    let answers = herdr.file("answers");
    assert_eq!(
        answers.lines().count(),
        2,
        "each question answered once, by the operator: {answers}"
    );
    let mut args: Vec<_> = herdr
        .file("claude-args")
        .lines()
        .map(|line| line.replace("-- ", ""))
        .collect();
    args.sort();
    assert_eq!(args, ["m43 promote m43", "m72 promote m72"]);

    // A server restart: what comes back in the tabs.
    herdr.stop();
    herdr.start();
    let tabs = herdr.request("tab.list", json!({"workspace_id": workspace}));
    let labels: Vec<_> = tabs["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tab| tab["label"].as_str())
        .collect();
    assert!(labels.contains(&"m43") && labels.contains(&"m72"), "{tabs}");
    let panes = herdr.request("pane.list", json!({"workspace_id": workspace}));
    for pane in panes["panes"].as_array().unwrap() {
        let id = pane["pane_id"].as_str().unwrap();
        eprintln!(
            "restored {id} in {}: {}",
            pane["tab_id"],
            herdr.processes(id)
        );
    }
}

#[cfg(unix)]
#[test]
#[ignore = "needs the pinned Herdr and zsh: HIDE_E2E_HERDR_BIN, HIDE_TEST_ZSH_BIN"]
fn zsh_started_from_its_argv_marks_its_first_prompt_after_the_question() {
    argv_shell(zsh);
}

#[cfg(unix)]
#[test]
#[ignore = "needs the pinned Herdr and bash: HIDE_E2E_HERDR_BIN, HIDE_TEST_BASH_BIN"]
fn bash_started_from_its_argv_marks_its_first_prompt_after_the_question() {
    argv_shell(bash);
}

#[cfg(unix)]
#[test]
#[ignore = "needs the pinned Herdr and fish: HIDE_E2E_HERDR_BIN, HIDE_TEST_FISH_BIN"]
fn fish_started_from_its_argv_marks_its_first_prompt_after_the_question() {
    argv_shell(fish);
}

#[cfg(windows)]
#[test]
#[ignore = "needs the pinned Herdr and Windows PowerShell: HIDE_E2E_HERDR_BIN, HIDE_TEST_POWERSHELL_BIN"]
fn windows_powershell_started_from_its_argv_marks_its_first_prompt_after_the_question() {
    argv_shell(|herdr| powershell(herdr, "HIDE_TEST_POWERSHELL_BIN"));
}

#[cfg(windows)]
#[test]
#[ignore = "needs the pinned Herdr and PowerShell 7: HIDE_E2E_HERDR_BIN, HIDE_TEST_PWSH_BIN"]
fn pwsh_started_from_its_argv_marks_its_first_prompt_after_the_question() {
    argv_shell(|herdr| powershell(herdr, "HIDE_TEST_PWSH_BIN"));
}

#[cfg(windows)]
#[test]
#[ignore = "needs the pinned Herdr and cmd: HIDE_E2E_HERDR_BIN, HIDE_TEST_CMD_BIN"]
fn cmd_started_from_its_argv_marks_its_first_prompt_after_the_question() {
    argv_shell(cmd);
}
