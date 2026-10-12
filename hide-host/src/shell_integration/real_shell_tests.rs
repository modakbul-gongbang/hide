//! The integration run by the shells themselves: the user's own files are
//! read in the shell's order and see their own `ZDOTDIR`, and the shell
//! starts with `ZDOTDIR` and `XDG_DATA_DIRS` as the user had them, unset
//! included. They need the shells, named in `HIDE_TEST_ZSH_BIN` and
//! `HIDE_TEST_FISH_BIN`, so they are ignored by default; the rust lane's
//! shell step runs them with `--ignored`.

use std::io::Write;
use std::process::{Command, Stdio};

use super::*;

fn shell(variable: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} names the shell")),
    )
}

/// A shell started as Herdr starts a pane's, with `env` from the
/// integration, reading `commands` on its input. Answers what the user's
/// files and the commands wrote, then the mark the first prompt reported.
fn run(
    bin: &Path,
    args: &[&str],
    home: &Path,
    env: &BTreeMap<String, String>,
    commands: &str,
) -> (String, String) {
    let mark = home.join("mark.sh");
    std::fs::write(&mark, "#!/bin/sh\necho \"$@\" >> \"$HOME/mark\"\n").unwrap();
    std::fs::set_permissions(&mark, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "dumb")
        .env("HERDR_BIN_PATH", &mark)
        .env("HERDR_PANE_ID", "w1:p1")
        .envs(env)
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(commands.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let read = |name: &str| {
        let text = std::fs::read_to_string(home.join(name)).unwrap_or_default();
        let _ = std::fs::remove_file(home.join(name));
        text.replace(&home.display().to_string(), "~")
    };
    (read("log"), read("mark"))
}

const ZSH_AFTER: &str = "print -r -- \"after ${ZDOTDIR-unset} ${(t)ZDOTDIR} ${XDG_DATA_DIRS-unset} ${+HIDE_SHELL_ZDOTDIR} ${(t)from_zshenv} ${(t)from_zshrc}\" >> $HOME/log\nexit\n";
const MARK: &str = "pane report-metadata w1:p1 --source hide:shell --token hide_prompt=1\n";

#[test]
#[ignore = "needs zsh: set HIDE_TEST_ZSH_BIN and run with --ignored"]
fn zsh_reads_the_users_files_in_its_own_order_and_gives_their_environment_back() {
    let zsh = shell("HIDE_TEST_ZSH_BIN");
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    // Each of the user's files says it ran, with the ZDOTDIR it saw, and
    // declares something that must outlive it.
    let files = |folder: &Path| {
        std::fs::create_dir_all(folder).unwrap();
        for name in ["zshenv", "zprofile", "zshrc", "zlogin"] {
            std::fs::write(
                folder.join(format!(".{name}")),
                format!("print -r -- \".{name} ${{ZDOTDIR-unset}}\" >> $HOME/log\ntypeset -A from_{name}\nfrom_{name}[a]=1\n"),
            )
            .unwrap();
        }
    };
    files(&home);
    let user = |zdotdir: Option<&str>, data_dirs: Option<&str>| UserShell {
        shell: Some(zsh.display().to_string()),
        zdotdir: zdotdir.map(str::to_owned),
        data_dirs: data_dirs.map(str::to_owned),
    };
    let env = install(&home.join("hide"), &user(None, None)).unwrap();

    // A login shell, the pane's: all four, in zsh's order, then the mark.
    let (log, mark) = run(&zsh, &["-i", "-l"], &home, &env, ZSH_AFTER);
    assert_eq!(
        log,
        ".zshenv unset\n.zprofile unset\n.zshrc unset\n.zlogin unset\nafter unset  unset 0 association association\n"
    );
    assert_eq!(mark, MARK);

    // An interactive shell that is not a login shell reads two.
    let (log, mark) = run(&zsh, &["-i"], &home, &env, ZSH_AFTER);
    assert_eq!(
        log,
        ".zshenv unset\n.zshrc unset\nafter unset  unset 0 association association\n"
    );
    assert_eq!(mark, MARK);

    // A command reads only .zshenv, gets everything back, and marks nothing.
    let (log, mark) = run(
        &zsh,
        &["-c", ZSH_AFTER.trim_end_matches("exit\n")],
        &home,
        &env,
        "",
    );
    assert_eq!(log, ".zshenv unset\nafter unset  unset 0 association \n");
    assert_eq!(mark, "");

    // A user whose own ZDOTDIR and XDG_DATA_DIRS are set gets them back,
    // exported, and their files are read from their ZDOTDIR.
    let zdotdir = home.join("z");
    files(&zdotdir);
    let env = install(
        &home.join("hide"),
        &user(Some(&zdotdir.display().to_string()), Some("/opt/share")),
    )
    .unwrap();
    let (log, mark) = run(&zsh, &["-i", "-l"], &home, &env, ZSH_AFTER);
    assert_eq!(
        log,
        ".zshenv ~/z\n.zprofile ~/z\n.zshrc ~/z\n.zlogin ~/z\nafter ~/z scalar-export /opt/share 0 association association\n"
    );
    assert_eq!(mark, MARK);
}

const FISH_STATE: &str = "(set -q XDG_DATA_DIRS; and echo $XDG_DATA_DIRS; or echo unset)\" \"(set -q ZDOTDIR; and echo $ZDOTDIR; or echo unset)\" \"(set -q HIDE_SHELL_ZDOTDIR; and echo leaked; or echo clean)";

#[test]
#[ignore = "needs fish: set HIDE_TEST_FISH_BIN and run with --ignored"]
fn fish_gives_the_users_environment_back_before_their_own_configuration() {
    let fish = shell("HIDE_TEST_FISH_BIN");
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let config = home.join(".config/fish");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.fish"),
        format!("echo \"config \"{FISH_STATE} >> $HOME/log\n"),
    )
    .unwrap();
    let after = format!(
        "echo \"after \"{FISH_STATE}\" \"(env | string match -q 'XDG_DATA_DIRS=*'; and echo exported; or echo absent) >> $HOME/log\nexit\n"
    );
    let user = |zdotdir: Option<&str>, data_dirs: Option<&str>| UserShell {
        shell: Some(fish.display().to_string()),
        zdotdir: zdotdir.map(str::to_owned),
        data_dirs: data_dirs.map(str::to_owned),
    };

    // The answer is the same fish started without Hide: a system's own
    // vendor configuration runs after Hide's and must see what it would have
    // seen (Ubuntu's snapd.fish sets XDG_DATA_DIRS when it is unset).
    let without = |originals: &[(&str, &str)]| {
        let env = originals
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        run(&fish, &["-i"], &home, &env, &after).0
    };

    let env = install(&home.join("hide"), &user(None, None)).unwrap();
    let (log, _) = run(&fish, &["-i"], &home, &env, &after);
    assert_eq!(log, without(&[]));
    assert!(log.contains(" clean"), "{log}");

    let env = install(&home.join("hide"), &user(Some("/z"), Some("/opt/share"))).unwrap();
    let (log, _) = run(&fish, &["-i"], &home, &env, &after);
    assert_eq!(
        log,
        without(&[("ZDOTDIR", "/z"), ("XDG_DATA_DIRS", "/opt/share")])
    );
    assert!(log.starts_with("config /opt/share /z clean\n"), "{log}");
}
