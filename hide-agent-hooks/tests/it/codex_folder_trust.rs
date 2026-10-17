//! Codex's trust of a worktree Hide made, recorded and taken out through a
//! stand-in `codex app-server` (`fixtures/fake-codex.py`), which keeps the
//! user layer's `projects` table in `fake-projects.json` and versions it, so
//! what a test reads back is what Codex would hold. `codex_binary` runs the
//! real Codex against the same calls.
//!
//! The stand-in is a shebang script, so these tests run where one runs.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use hide_agent_hooks::folder_trust::{self, Agent, Tools};
use serde_json::{Value, json};

struct Fixture {
    home: tempfile::TempDir,
    codex: PathBuf,
    stop: AtomicBool,
    main: PathBuf,
    tree: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        let codex = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.py");
        crate::stand_ins::ready(&codex);
        let (main, tree) = linked_worktree(home.path());
        Self {
            home,
            codex,
            stop: AtomicBool::new(false),
            main,
            tree,
        }
    }

    fn tools(&self) -> Tools<'_> {
        Tools {
            codex: Some(&self.codex),
            stop: &self.stop,
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.home.path().join(".codex").join(name)
    }

    fn mode(&self, mode: &str) {
        std::fs::write(self.file("fake-mode"), mode).unwrap();
    }

    fn projects(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.file("fake-projects.json")).unwrap()).unwrap()
    }

    fn set_projects(&self, projects: Value) {
        std::fs::write(self.file("fake-projects.json"), projects.to_string()).unwrap();
    }

    fn writes(&self) -> usize {
        std::fs::read_to_string(self.file("fake-calls.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| *line == "config/batchWrite")
            .count()
    }

    fn trust(&self) -> Result<(), String> {
        folder_trust::trust_worktree(Agent::Codex, self.home.path(), &self.tree, self.tools())
            .map_err(|failure| failure.message())
    }

    fn forget(&self) -> usize {
        folder_trust::forget(Agent::Codex, self.home.path(), &self.tree, self.tools()).unwrap()
    }
}

fn resolved(path: &Path) -> String {
    hide_platform::fs::identity::canonical(path)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
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

#[test]
fn codex_trusts_the_worktree_itself_and_hide_takes_out_only_that() {
    let fixture = Fixture::new();
    let other = json!({"/somewhere/else": {"trust_level": "trusted"}});
    fixture.set_projects(other.clone());

    fixture.trust().unwrap();
    fixture.trust().unwrap();
    let tree = resolved(&fixture.tree);
    let mut expected = other.clone();
    expected[&tree] = json!({"trust_level": "trusted"});
    assert_eq!(fixture.projects(), expected);
    assert_eq!(
        fixture.writes(),
        1,
        "a folder Codex trusts is not written again"
    );
    assert!(
        fixture.projects().get(resolved(&fixture.main)).is_none(),
        "the repository's main checkout is the operator's"
    );

    assert_eq!(fixture.forget(), 1);
    assert_eq!(fixture.projects(), other);
    assert_eq!(fixture.forget(), 0);
}

#[test]
fn a_folder_codex_already_trusts_stays_the_operator_s() {
    let fixture = Fixture::new();
    let tree = resolved(&fixture.tree);
    fixture.set_projects(json!({&tree: {"trust_level": "trusted"}}));
    fixture.trust().unwrap();
    assert_eq!(fixture.writes(), 0);
    assert_eq!(fixture.forget(), 0);
    assert_eq!(
        fixture.projects(),
        json!({&tree: {"trust_level": "trusted"}})
    );
}

#[test]
fn a_repository_the_operator_marked_untrusted_refuses_the_trust_and_says_what_to_do() {
    let fixture = Fixture::new();
    let main = resolved(&fixture.main);
    fixture.set_projects(json!({&main: {"trust_level": "untrusted"}}));
    let refused = fixture.trust().unwrap_err();
    assert!(
        refused.contains(&format!("Codex marks {main} untrusted"))
            && refused.contains("trust it in Codex, or start the agent there yourself"),
        "{refused}"
    );
    assert_eq!(fixture.writes(), 0);
    assert_eq!(
        fixture.projects(),
        json!({&main: {"trust_level": "untrusted"}})
    );

    // The worktree's own "untrusted" is the operator's no as well.
    let tree = resolved(&fixture.tree);
    fixture.set_projects(json!({&tree: {"trust_level": "untrusted"}}));
    assert!(fixture.trust().unwrap_err().contains("untrusted"));
    assert_eq!(fixture.writes(), 0);
    assert_eq!(fixture.forget(), 0, "nothing was recorded as Hide's");
}

#[test]
fn a_write_codex_does_not_apply_fails_and_leaves_no_record() {
    let fixture = Fixture::new();
    fixture.set_projects(json!({}));
    fixture.mode("overridden");
    let refused = fixture.trust().unwrap_err();
    assert!(refused.contains("managed Codex configuration"), "{refused}");
    fixture.mode("ok");
    // So a trust the operator gives later stays theirs.
    assert_eq!(fixture.forget(), 0);
}

#[test]
fn forgetting_keeps_what_else_the_folder_s_entry_holds() {
    let fixture = Fixture::new();
    fixture.set_projects(json!({}));
    fixture.trust().unwrap();
    let tree = resolved(&fixture.tree);
    let mut projects = fixture.projects();
    projects[&tree]["note"] = json!("the operator's");
    fixture.set_projects(projects);
    assert_eq!(fixture.forget(), 1);
    assert_eq!(
        fixture.projects(),
        json!({&tree: {"note": "the operator's"}})
    );
}

#[test]
fn a_codex_with_no_user_config_yet_is_given_one() {
    let fixture = Fixture::new();
    fixture.trust().unwrap();
    assert_eq!(
        fixture.projects(),
        json!({resolved(&fixture.tree): {"trust_level": "trusted"}})
    );
}
