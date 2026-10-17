use super::*;

/// An absolute folder on this system: `/work/<name>`, or `C:\work\<name>`.
fn folder(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\work\{name}"))
    } else {
        PathBuf::from(format!("/work/{name}"))
    }
}

fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini/antigravity-cli")).unwrap();
    home
}

fn none() -> Tools<'static> {
    Tools::none()
}

/// A repository's main checkout and one linked worktree of it, in the files
/// Git writes for them.
fn linked_worktree(root: &Path) -> (PathBuf, PathBuf) {
    let main = root.join("repo");
    let linked = root.join("repo-task");
    std::fs::create_dir_all(main.join(".git/worktrees/task")).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/task").display()),
    )
    .unwrap();
    std::fs::write(
        main.join(".git/worktrees/task/HEAD"),
        "ref: refs/heads/task\n",
    )
    .unwrap();
    std::fs::write(main.join(".git/worktrees/task/commondir"), "../..\n").unwrap();
    std::fs::write(
        main.join(".git/worktrees/task/gitdir"),
        format!("{}\n", linked.join(".git").display()),
    )
    .unwrap();
    (main, linked)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn hide_takes_out_only_the_agy_trust_it_added_and_keeps_the_rest_byte_for_byte() {
    let home = home();
    let settings = Agent::Antigravity.settings_path(home.path());
    let (mine, tree) = (folder("me"), folder("tree-1"));
    let original = format!(
        "{{\n  \"theme\": \"dark\",\n  \"trustedWorkspaces\": [{}]\n}}\n",
        serde_json::to_string(mine.to_str().unwrap()).unwrap()
    );
    std::fs::write(&settings, &original).unwrap();
    trust(Agent::Antigravity, home.path(), &tree, none()).unwrap();
    trust(Agent::Antigravity, home.path(), &tree, none()).unwrap();
    trust(Agent::Antigravity, home.path(), &mine, none()).unwrap();
    assert_eq!(
        read_json(&settings)["trustedWorkspaces"],
        serde_json::json!([mine.to_str().unwrap(), tree.to_str().unwrap()])
    );
    // The operator's own folder is never Hide's to take out.
    forget(Agent::Antigravity, home.path(), &mine, none()).unwrap();
    forget(Agent::Antigravity, home.path(), &tree, none()).unwrap();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
}

#[test]
fn switching_an_agent_off_takes_out_every_folder_hide_added_for_it_alone() {
    let home = home();
    let settings = Agent::Antigravity.settings_path(home.path());
    trust(Agent::Antigravity, home.path(), &folder("a"), none()).unwrap();
    trust(Agent::Antigravity, home.path(), &folder("b"), none()).unwrap();
    trust(Agent::ClaudeCode, home.path(), &folder("a"), none()).unwrap();
    assert_eq!(
        forget_all(Agent::Antigravity, home.path(), none()).unwrap(),
        2
    );
    assert_eq!(
        read_json(&settings)["trustedWorkspaces"],
        serde_json::json!([])
    );
    assert!(
        read_record(Agent::Antigravity, home.path())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        read_record(Agent::ClaudeCode, home.path()).unwrap().len(),
        1
    );
}

#[test]
fn removing_where_hide_never_trusted_a_folder_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    for agent in Agent::ALL {
        assert_eq!(forget(agent, home.path(), &folder("a"), none()).unwrap(), 0);
        assert_eq!(forget_all(agent, home.path(), none()).unwrap(), 0);
        assert!(!agent.settings_path(home.path()).exists());
    }
    assert!(!home.path().join(".hide").exists());
}

#[test]
fn settings_hide_cannot_read_stay_untouched_and_the_trust_fails() {
    let home = home();
    for agent in [Agent::Antigravity, Agent::ClaudeCode] {
        let settings = agent.settings_path(home.path());
        std::fs::write(&settings, "{ not json").unwrap();
        assert!(matches!(
            trust(agent, home.path(), &folder("a"), none()),
            Err(InstallFailure::Unparsable { .. })
        ));
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ not json");
        assert!(read_record(agent, home.path()).unwrap().is_empty());
        assert!(trust(agent, home.path(), Path::new("relative"), none()).is_err());
    }
}

#[test]
fn only_a_git_worktree_below_home_is_trusted_as_a_worktree() {
    let home = home();
    let tree = home.path().join("repo-tree");
    std::fs::create_dir_all(tree.join(".git")).unwrap();
    let plain = home.path().join("downloads");
    std::fs::create_dir_all(&plain).unwrap();

    for agent in [Agent::Antigravity, Agent::ClaudeCode] {
        for refused in [
            plain.clone(),
            home.path().to_path_buf(),
            home.path().parent().unwrap().to_path_buf(),
            tree.join("..").join("repo-tree"),
            home.path().join(".").join("repo-tree"),
            tree.join("."),
            home.path().join("missing"),
        ] {
            assert!(
                trust_worktree(agent, home.path(), &refused, none()).is_err(),
                "{} {}",
                agent.label(),
                refused.display()
            );
        }
        assert!(read_record(agent, home.path()).unwrap().is_empty());
    }
    assert!(!Agent::ClaudeCode.settings_path(home.path()).exists());

    trust_worktree(Agent::Antigravity, home.path(), &tree, none()).unwrap();
    let resolved = hide_platform::fs::identity::canonical(&tree).unwrap();
    assert_eq!(
        read_record(Agent::Antigravity, home.path()).unwrap(),
        [resolved.to_str().unwrap()],
        "the entry is the folder's resolved path"
    );
    assert_eq!(
        forget(Agent::Antigravity, home.path(), &tree, none()).unwrap(),
        1,
        "undone by its own spelling"
    );

    // Named through a link, the entry is still the worktree itself, so
    // pointing the link elsewhere afterwards trusts nothing new.
    #[cfg(unix)]
    {
        let link = home.path().join("tree-link");
        std::os::unix::fs::symlink(&tree, &link).unwrap();
        trust_worktree(Agent::Antigravity, home.path(), &link, none()).unwrap();
        assert_eq!(
            read_record(Agent::Antigravity, home.path()).unwrap(),
            [resolved.to_str().unwrap()]
        );
        assert_eq!(
            forget(Agent::Antigravity, home.path(), &link, none()).unwrap(),
            1
        );
    }
}

#[cfg(unix)]
#[test]
fn a_trust_whose_settings_write_fails_leaves_no_record_of_it() {
    use std::os::unix::fs::PermissionsExt;
    let home = home();
    let settings = Agent::Antigravity.settings_path(home.path());
    std::fs::write(&settings, "{}").unwrap();
    let folder_of_settings = settings.parent().unwrap();
    std::fs::set_permissions(folder_of_settings, std::fs::Permissions::from_mode(0o555)).unwrap();

    let trusted = trust(Agent::Antigravity, home.path(), &folder("tree-1"), none());

    std::fs::set_permissions(folder_of_settings, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(trusted.is_err());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}");
    assert!(
        read_record(Agent::Antigravity, home.path())
            .unwrap()
            .is_empty()
    );
    // So a trust the operator gives agy later stays theirs.
    assert_eq!(
        forget(Agent::Antigravity, home.path(), &folder("tree-1"), none()).unwrap(),
        0
    );
}

#[test]
fn claude_trusts_the_worktree_itself_and_hide_takes_out_only_its_own_flag() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".claude.json");
    let main = folder("repo");
    let kept = folder("kept");
    let tree = folder("repo-task");
    let key = |path: &Path| Agent::ClaudeCode.spelling(path).unwrap();
    // Claude's own file: other projects, an entry for the worktree Claude
    // filled before (trust off, other fields of its own), unknown keys.
    let original = format!(
        "{{\n  \"numStartups\": 3,\n  \"projects\": {{\n    {}: {{\n      \"hasTrustDialogAccepted\": true\n    }},\n    {}: {{\n      \"allowedTools\": [],\n      \"hasTrustDialogAccepted\": false\n    }}\n  }},\n  \"userID\": \"x\"\n}}\n",
        serde_json::to_string(&key(&main)).unwrap(),
        serde_json::to_string(&key(&kept)).unwrap(),
    );
    std::fs::write(&config, &original).unwrap();

    trust(Agent::ClaudeCode, home.path(), &tree, none()).unwrap();
    trust(Agent::ClaudeCode, home.path(), &kept, none()).unwrap();
    let written = read_json(&config);
    assert_eq!(
        written["projects"][key(&tree)],
        serde_json::json!({"hasTrustDialogAccepted": true})
    );
    assert_eq!(
        written["projects"][key(&kept)],
        serde_json::json!({"allowedTools": [], "hasTrustDialogAccepted": true})
    );
    // The repository's main checkout is the operator's and stays as it was.
    assert_eq!(
        written["projects"][key(&main)],
        serde_json::json!({"hasTrustDialogAccepted": true})
    );
    // A folder Claude already trusts stays the operator's.
    trust(Agent::ClaudeCode, home.path(), &main, none()).unwrap();
    assert_eq!(
        read_record(Agent::ClaudeCode, home.path()).unwrap(),
        [key(&tree), key(&kept)]
    );

    assert_eq!(
        forget(Agent::ClaudeCode, home.path(), &main, none()).unwrap(),
        0
    );
    assert_eq!(
        forget_all(Agent::ClaudeCode, home.path(), none()).unwrap(),
        2
    );
    let restored = read_json(&config);
    assert!(restored["projects"].get(key(&tree)).is_none());
    assert_eq!(
        restored["projects"][key(&kept)],
        serde_json::json!({"allowedTools": []}),
        "only the flag Hide set goes; the fields Claude wrote stay"
    );
    assert_eq!(
        restored["projects"][key(&main)],
        serde_json::json!({"hasTrustDialogAccepted": true})
    );
    assert_eq!(restored["userID"], "x");
}

#[test]
fn claude_s_file_is_made_when_claude_has_none_and_its_older_file_is_followed() {
    let home = tempfile::tempdir().unwrap();
    trust(Agent::ClaudeCode, home.path(), &folder("tree"), none()).unwrap();
    assert_eq!(
        read_json(&home.path().join(".claude.json"))["projects"]
            [Agent::ClaudeCode.spelling(&folder("tree")).unwrap()]["hasTrustDialogAccepted"],
        true
    );

    // Claude Code reads `~/.claude/.config.json` instead while it has content.
    let older = tempfile::tempdir().unwrap();
    let legacy = older.path().join(".claude/.config.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "{}").unwrap();
    assert_eq!(Agent::ClaudeCode.settings_path(older.path()), legacy);
    trust(Agent::ClaudeCode, older.path(), &folder("tree"), none()).unwrap();
    assert!(read_json(&legacy)["projects"].is_object());
    assert!(!older.path().join(".claude.json").exists());
}

#[test]
fn claude_s_own_lock_is_waited_for_and_released() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".claude.json");
    std::fs::write(&config, "{}").unwrap();
    let lock = home.path().join(".claude.json.lock");
    // A Claude Code saving its config holds the lock folder.
    std::fs::create_dir(&lock).unwrap();
    let refused = trust(Agent::ClaudeCode, home.path(), &folder("tree"), none()).unwrap_err();
    assert!(
        refused.message().contains("held its config lock"),
        "{}",
        refused.message()
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");
    assert!(
        read_record(Agent::ClaudeCode, home.path())
            .unwrap()
            .is_empty()
    );
    assert!(
        lock.is_dir(),
        "a lock Hide did not take is not Hide's to remove"
    );

    std::fs::remove_dir(&lock).unwrap();
    trust(Agent::ClaudeCode, home.path(), &folder("tree"), none()).unwrap();
    assert!(!lock.exists(), "Hide releases the lock it took");
}

#[cfg(unix)]
#[test]
fn a_claude_lock_left_by_a_process_that_ended_is_taken_over() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".claude.json"), "{}").unwrap();
    let lock = home.path().join(".claude.json.lock");
    std::fs::create_dir(&lock).unwrap();
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::File::open(&lock)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(long_ago))
        .unwrap();
    trust(Agent::ClaudeCode, home.path(), &folder("tree"), none()).unwrap();
    assert!(!lock.exists());
}

#[test]
fn each_agent_spells_a_folder_as_it_compares_one() {
    let path = folder("Repo-Task");
    let native = path.to_str().unwrap().to_owned();
    assert_eq!(Agent::Antigravity.spelling(&path).unwrap(), native);
    if cfg!(windows) {
        assert_eq!(
            Agent::ClaudeCode.spelling(&path).unwrap(),
            "C:/work/Repo-Task"
        );
        assert_eq!(Agent::Codex.spelling(&path).unwrap(), r"c:\work\repo-task");
    } else {
        assert_eq!(Agent::ClaudeCode.spelling(&path).unwrap(), native);
        assert_eq!(Agent::Codex.spelling(&path).unwrap(), native);
    }
}

#[test]
fn a_linked_worktree_names_its_repository_s_main_checkout() {
    let root = tempfile::tempdir().unwrap();
    let (main, linked) = linked_worktree(root.path());
    assert_eq!(
        main_checkout(&linked),
        Some(hide_platform::fs::identity::canonical(&main).unwrap())
    );
    assert_eq!(
        main_checkout(&main),
        Some(hide_platform::fs::identity::canonical(&main).unwrap())
    );
}

#[test]
fn codex_trust_needs_a_codex_to_record_it_through() {
    let root = tempfile::tempdir().unwrap();
    let (_, linked) = linked_worktree(root.path());
    let refused = trust_worktree(Agent::Codex, root.path(), &linked, none()).unwrap_err();
    assert!(
        refused.message().contains("no codex program"),
        "{}",
        refused.message()
    );
    assert!(read_record(Agent::Codex, root.path()).unwrap().is_empty());
}

#[test]
fn only_claude_codex_and_agy_starts_record_a_trust() {
    use hide_agent_adapter::LaunchDialect;
    for dialect in [
        LaunchDialect::Claude,
        LaunchDialect::Codex,
        LaunchDialect::Antigravity,
    ] {
        assert!(Agent::of(dialect).is_some(), "{dialect:?}");
    }
    for dialect in [
        LaunchDialect::Grok,
        LaunchDialect::OpenCode,
        LaunchDialect::Pi,
        LaunchDialect::Omp,
        LaunchDialect::Cursor,
    ] {
        assert!(Agent::of(dialect).is_none(), "{dialect:?}");
    }
}
