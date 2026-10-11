//! Bounded, fail-open Project Memory projection for agent hooks.

use crate::runtime::{AgentRuntime, HookEvent, hook_stdout_with_context};
use hide_memory::{
    HOOK_DEADLINE_MS, HOOK_INPUT_LIMIT_BYTES, Injection, InjectionKind, InjectionOutcome,
    MemoryStore, RetrievalQuery,
};
use serde::Deserialize;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MEMORY_DATABASE_ENV: &str = "HIDE_MEMORY_DATABASE_PATH";
pub const MEMORY_TESTING_ENV: &str = "HIDE_PROJECT_MEMORY_TESTING";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookMemoryOutcome {
    Provided { count: usize },
    Empty,
    Disabled,
    Unavailable,
    Deadline,
    InputTooLarge,
    ProjectUnresolved,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookMemoryResult {
    pub stdout: Option<String>,
    pub outcome: HookMemoryOutcome,
}

#[derive(Debug, Deserialize)]
struct HookInput {
    cwd: Option<PathBuf>,
    prompt: Option<String>,
    session_id: Option<String>,
}

/// Retains only the bounded stdin prefix and stops as soon as overflow is
/// observed. The installed binary additionally puts this read behind the
/// hook's absolute process deadline.
pub fn read_bounded(mut input: impl Read) -> io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(HOOK_INPUT_LIMIT_BYTES.min(16 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = HOOK_INPUT_LIMIT_BYTES.saturating_sub(retained.len());
        let keep = remaining.min(read);
        retained.extend_from_slice(&buffer[..keep]);
        exceeded |= keep < read;
        if exceeded {
            break;
        }
    }
    Ok((retained, exceeded))
}

pub fn database_path(home: &Path) -> PathBuf {
    let testing = std::env::var(MEMORY_TESTING_ENV).as_deref() == Ok("1");
    if testing
        && let Some(path) = std::env::var_os(MEMORY_DATABASE_ENV).filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    // The state folder a daemon on this home uses by default, which is where
    // the core writes the store (`hide_kit::layout::default_state_dir`; hide-kit
    // depends on this crate, and herdr-core's tests hold the two equal). A
    // relocated state folder is not followed: production ignores the process
    // environment here (see `production_hook_ignores_a_database_path_override`).
    hide_memory::database_path(&home.join(".hide").join("state"))
}

pub fn project_memory_output(
    runtime: AgentRuntime,
    event: HookEvent,
    bytes: &[u8],
    exceeded: bool,
    home: &Path,
) -> HookMemoryResult {
    project_memory_output_until(
        runtime,
        event,
        bytes,
        exceeded,
        home,
        Instant::now() + Duration::from_millis(HOOK_DEADLINE_MS),
    )
}

pub fn project_memory_output_until(
    runtime: AgentRuntime,
    event: HookEvent,
    bytes: &[u8],
    exceeded: bool,
    home: &Path,
    deadline: Instant,
) -> HookMemoryResult {
    let expired = move || Instant::now() >= deadline;
    project_memory_output_with_expiry(runtime, event, bytes, exceeded, home, expired)
}

// One per-invocation monotonic predicate covers open, retrieval and render.
// Its clones share the same absolute end; once expired, it stays expired.
fn project_memory_output_with_expiry<E>(
    runtime: AgentRuntime,
    event: HookEvent,
    bytes: &[u8],
    exceeded: bool,
    home: &Path,
    expired: E,
) -> HookMemoryResult
where
    E: Fn() -> bool + Clone + Send + 'static,
{
    let projected = if exceeded {
        MemoryContext::without(HookMemoryOutcome::InputTooLarge)
    } else {
        match serde_json::from_slice::<HookInput>(bytes) {
            Ok(input) => memory_context_with_expiry(
                MemoryRequest {
                    runtime_id: runtime.memory_id(),
                    event,
                    cwd: input.cwd,
                    prompt: input.prompt,
                    session_id: input.session_id,
                },
                home,
                expired,
            ),
            Err(_) => MemoryContext::without(HookMemoryOutcome::Unavailable),
        }
    };
    HookMemoryResult {
        stdout: match &projected.context {
            Some(context) => hook_stdout_with_context(runtime, event, Some(context)),
            None => crate::runtime::hook_stdout(runtime, event),
        },
        outcome: projected.outcome,
    }
}

/// What one hook asks Project Memory for, whatever runtime's payload it came in.
pub struct MemoryRequest<'a> {
    /// The provider id Memory keys sessions and receipts by: `claude`, `codex`
    /// or `opencode`.
    pub runtime_id: &'a str,
    /// `SessionStart` for a session's first context, `UserPromptSubmit` for a
    /// later prompt; any other event asks for nothing.
    pub event: HookEvent,
    pub cwd: Option<PathBuf>,
    pub prompt: Option<String>,
    pub session_id: Option<String>,
}

/// The Memory context for one hook, with its receipt, or none.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemoryContext {
    pub context: Option<String>,
    pub outcome: HookMemoryOutcome,
}

impl MemoryContext {
    fn without(outcome: HookMemoryOutcome) -> Self {
        Self {
            context: None,
            outcome,
        }
    }
}

/// [`MemoryContext`] for `request`, bounded by `deadline` and failing open.
pub fn memory_context_until(
    request: MemoryRequest<'_>,
    home: &Path,
    deadline: Instant,
) -> MemoryContext {
    memory_context_with_expiry(request, home, move || Instant::now() >= deadline)
}

fn memory_context_with_expiry<E>(
    request: MemoryRequest<'_>,
    home: &Path,
    expired: E,
) -> MemoryContext
where
    E: Fn() -> bool + Clone + Send + 'static,
{
    let MemoryRequest {
        runtime_id,
        event,
        cwd,
        prompt,
        session_id,
    } = request;
    let base = || MemoryContext::without(HookMemoryOutcome::Unavailable);
    let deadline = || MemoryContext::without(HookMemoryOutcome::Deadline);
    let Some(cwd) = cwd else {
        return MemoryContext::without(HookMemoryOutcome::ProjectUnresolved);
    };
    let Ok(store) =
        MemoryStore::open_hook_read_only_with_expiry(&database_path(home), expired.clone())
    else {
        if expired() {
            return deadline();
        }
        return base();
    };
    // A Project is keyed by the machine it lives on, the node id the core
    // stores it under (PRD core-host-node D-23). It is read only once a store
    // exists, so a machine without Project Memory pays no lookup.
    let Ok(node) = hide_platform::host::machine_id() else {
        return MemoryContext::without(HookMemoryOutcome::ProjectUnresolved);
    };
    let Ok(project) = hide_project::resolve(&cwd, &node) else {
        return MemoryContext::without(HookMemoryOutcome::ProjectUnresolved);
    };
    if expired() {
        return deadline();
    }
    let session_id = session_id.unwrap_or_default();
    if session_id.is_empty() {
        return base();
    }
    let query = match event {
        HookEvent::SessionStart => RetrievalQuery::session_start(&project.id),
        HookEvent::UserPromptSubmit => {
            let excluded = match store.session_given_ids(&project.id, runtime_id, &session_id) {
                Ok(Some(items)) => items,
                // A new session while the Project's receipts are full: nothing
                // could record what it is given, so it gets none (B26).
                Ok(None) | Err(_) => return base(),
            };
            RetrievalQuery::prompt(&project.id, prompt.unwrap_or_default(), excluded)
        }
        HookEvent::SubagentStart
        | HookEvent::SubagentStop
        | HookEvent::Stop
        | HookEvent::PreToolUse => return base(),
    };
    if expired() {
        return deadline();
    }
    let Ok(injection) = store.retrieve_with_expiry(&query, &expired) else {
        if expired() {
            return deadline();
        }
        return base();
    };
    // The index is not a delivery and leaves no receipt; only cards given in
    // full are recorded, so a session's prompts do not repeat them (B12).
    let receipt_auth = match injection.kind {
        InjectionKind::Index => None,
        InjectionKind::Bodies => match store.receipt_auth_tag(
            &project.id,
            runtime_id,
            &session_id,
            event.name(),
            &injection.item_key(),
        ) {
            Ok(tag) => Some(tag),
            Err(_) => return base(),
        },
    };
    render(event, injection, receipt_auth.as_deref(), &expired)
}

fn render(
    event: HookEvent,
    injection: Injection,
    receipt_auth: Option<&str>,
    expired: &impl Fn() -> bool,
) -> MemoryContext {
    if expired() {
        return MemoryContext::without(HookMemoryOutcome::Deadline);
    }
    let outcome = match injection.outcome {
        InjectionOutcome::Provided => HookMemoryOutcome::Provided {
            count: injection.items.len(),
        },
        InjectionOutcome::Empty => HookMemoryOutcome::Empty,
        InjectionOutcome::Disabled => HookMemoryOutcome::Disabled,
        InjectionOutcome::Deadline => HookMemoryOutcome::Deadline,
        InjectionOutcome::Unavailable | InjectionOutcome::Stale => HookMemoryOutcome::Unavailable,
    };
    let Some(mut context) = injection.context() else {
        return MemoryContext::without(outcome);
    };
    if let Some(auth) = receipt_auth {
        context.push_str(&format!(
            "<hide-memory-receipt event=\"{}\" count=\"{}\" items=\"{}\" auth=\"{}\" />\n",
            event.name(),
            injection.items.len(),
            injection.item_key(),
            auth,
        ));
    }
    MemoryContext {
        context: Some(context),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The node id the hook keys this machine's Projects by.
    fn node() -> String {
        hide_platform::host::machine_id().unwrap()
    }
    use hide_memory::{
        AnalysisBatch, Candidate, CandidateKind, CandidateRelation, Injection, InjectionOutcome,
    };
    use std::fs;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn invocation_clock() -> (Arc<AtomicU64>, impl Fn() -> bool + Clone + Send + 'static) {
        let anchor = Instant::now();
        let end = anchor + Duration::from_millis(HOOK_DEADLINE_MS);
        let elapsed = Arc::new(AtomicU64::new(0));
        let clock = Arc::clone(&elapsed);
        let expired = move || anchor + Duration::from_millis(clock.load(Ordering::SeqCst)) >= end;
        (elapsed, expired)
    }

    fn project_memory_output_with_frozen_time(
        runtime: AgentRuntime,
        event: HookEvent,
        bytes: &[u8],
        exceeded: bool,
        home: &Path,
    ) -> HookMemoryResult {
        let (_elapsed, expired) = invocation_clock();
        project_memory_output_with_expiry(runtime, event, bytes, exceeded, home, expired)
    }

    #[test]
    fn oversized_input_is_drained_but_never_parsed() {
        let bytes = vec![b'x'; HOOK_INPUT_LIMIT_BYTES + 7];
        let (retained, exceeded) = read_bounded(bytes.as_slice()).unwrap();
        assert_eq!(retained.len(), HOOK_INPUT_LIMIT_BYTES);
        assert!(exceeded);
    }

    #[test]
    fn production_hook_ignores_a_database_path_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = Path::new("/fixture/home");
        // SAFETY: this module serializes its environment-mutating tests.
        unsafe {
            std::env::set_var(MEMORY_DATABASE_ENV, "/tmp/attacker.sqlite3");
            std::env::remove_var(MEMORY_TESTING_ENV);
        }
        assert_eq!(
            database_path(home),
            home.join(".hide/state/project-memory.sqlite3")
        );
        // SAFETY: guarded by ENV_LOCK and restored before the test returns.
        unsafe {
            std::env::remove_var(MEMORY_DATABASE_ENV);
        }
    }

    fn rule(text: &str, salience: f64) -> Candidate {
        Candidate {
            text: text.into(),
            title: String::new(),
            aliases: Vec::new(),
            kind: CandidateKind::Rule,
            confidence: 0.9,
            salience,
            source_offsets: vec![1],
            human_offsets: vec![1],
            cited_paths: Vec::new(),
            relation: CandidateRelation::New,
        }
    }

    /// A home whose Memory store holds `cards` for the Project at `cwd`,
    /// with Memory on.
    fn memory_home(
        temp: &Path,
        cwd: &Path,
        cards: &[Candidate],
    ) -> (PathBuf, hide_project::ProjectIdentity, MemoryStore) {
        let home = temp.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(cwd).unwrap();
        let project = hide_project::resolve(cwd, &node()).unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, &node())
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        store
            .apply_candidates(
                &AnalysisBatch {
                    id: "fixture-batch".into(),
                    project_id: project.id.clone(),
                    provider: "codex".into(),
                    analysis_provider: "codex".into(),
                    session_id: "source-session".into(),
                    content_hash: "fixture-hash".into(),
                    created_at_unix_ms: 1,
                },
                cards,
            )
            .unwrap();
        (home, project, store)
    }

    fn prompt_hook(home: &Path, cwd: &Path, session_id: &str, prompt: &str) -> HookMemoryResult {
        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": cwd,
            "session_id": session_id,
            "prompt": prompt,
        }))
        .unwrap();
        project_memory_output_with_frozen_time(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            home,
        )
    }

    #[test]
    fn a_prompt_sharing_no_word_or_alias_gets_no_memory() {
        // B12
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project-memory");
        let (home, _project, store) = memory_home(
            temp.path(),
            &cwd,
            &[rule(
                "Project Memory provider work uses a bounded queue",
                0.9,
            )],
        );
        drop(store);

        let result = prompt_hook(&home, &cwd, "unrelated-session", "오늘 점심 메뉴 추천해줘");
        assert_eq!(result.outcome, HookMemoryOutcome::Empty);
        assert!(result.stdout.is_none());
        let result = prompt_hook(
            &home,
            &cwd,
            "unrelated-session",
            "the provider queue is stuck",
        );
        assert_eq!(result.outcome, HookMemoryOutcome::Provided { count: 1 });
    }

    #[test]
    fn a_prompt_omits_cards_the_session_was_already_given_in_full() {
        // B12: a card given at an earlier prompt is not given again.
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        let cards = (0..6)
            .map(|index| rule(&format!("Rule {index} about durable hooks"), 0.9))
            .collect::<Vec<_>>();
        let (home, project, mut store) = memory_home(temp.path(), &cwd, &cards);
        store
            .record_receipt(
                &project.id,
                "codex",
                "fixture-session",
                "event:9",
                "m1@1",
                2,
            )
            .unwrap();
        drop(store);

        let result = prompt_hook(&home, &cwd, "fixture-session", "durable hooks");
        assert_eq!(result.outcome, HookMemoryOutcome::Provided { count: 3 });
        let output = result.stdout.unwrap();
        assert!(!output.contains("Rule 0 "), "{output}");
        assert!(
            output.contains("m2@1"),
            "the receipt names cards by number: {output}"
        );
    }

    #[test]
    fn session_start_gives_the_index_without_a_receipt_and_the_first_prompt_gets_memory() {
        // B10, B13: the index is not a delivery, so it records nothing and
        // the first prompt does not wait for it.
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        let mut cards = (0..22)
            .map(|index| {
                rule(
                    &format!("Race rule {index} about durable hooks"),
                    1.0 - f64::from(index) / 100.0,
                )
            })
            .collect::<Vec<_>>();
        cards.push(Candidate {
            kind: CandidateKind::Fact,
            ..rule("A fact about durable hooks", 1.0)
        });
        let (home, _project, store) = memory_home(temp.path(), &cwd, &cards);
        drop(store);
        let start_payload = serde_json::to_vec(&serde_json::json!({
            "cwd": cwd,
            "session_id": "immediate-session"
        }))
        .unwrap();
        let start = project_memory_output_with_frozen_time(
            AgentRuntime::Codex,
            HookEvent::SessionStart,
            &start_payload,
            false,
            &home,
        );
        assert_eq!(start.outcome, HookMemoryOutcome::Provided { count: 20 });
        let start_output = start.stdout.unwrap();
        assert!(start_output.contains("hide memory show"));
        assert!(start_output.contains("m1 Race rule 0 about durable hooks"));
        assert!(!start_output.contains("A fact about"));
        assert!(!start_output.contains("Race rule 21 "));
        assert!(!start_output.contains("hide-memory-receipt"));

        let prompt = prompt_hook(&home, &cwd, "immediate-session", "durable hooks");
        assert_eq!(prompt.outcome, HookMemoryOutcome::Provided { count: 3 });
        assert!(prompt.stdout.unwrap().contains("hide-memory-receipt"));
    }

    #[test]
    fn a_linked_worktree_cwd_reaches_its_main_checkouts_memory() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("main");
        let linked = temp.path().join("linked");
        fs::create_dir_all(&main).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&main)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(main.join("README.md"), "fixture\n").unwrap();
        for args in [
            vec!["add", "."],
            vec!["-c", "commit.gpgsign=false", "commit", "-qm", "init"],
            vec![
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&main)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let linked_cwd = linked.join("crates/memory/src");
        let (home, project, store) = memory_home(
            temp.path(),
            &linked_cwd,
            &[rule("Keep the local projection bounded", 0.9)],
        );
        assert_eq!(project.checkout_root, fs::canonicalize(&linked).unwrap());
        drop(store);

        let result = prompt_hook(&home, &linked_cwd, "linked-session", "projection 정리");
        assert_eq!(result.outcome, HookMemoryOutcome::Provided { count: 1 });
        assert!(
            result
                .stdout
                .unwrap()
                .contains("Keep the local projection bounded")
        );
    }

    #[test]
    fn current_runtime_prompt_fixtures_parse_and_use_the_verified_context_envelope() {
        for (runtime, bytes) in [
            (
                AgentRuntime::ClaudeCode,
                include_bytes!("../tests/fixtures/claude-user-prompt-submit.json").as_slice(),
            ),
            (
                AgentRuntime::Codex,
                include_bytes!("../tests/fixtures/codex-user-prompt-submit.json").as_slice(),
            ),
        ] {
            let input: HookInput = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                input.cwd.as_deref(),
                Some(Path::new("/tmp/fixture-project"))
            );
            assert_eq!(
                input.prompt.as_deref(),
                Some("Inspect the project memory boundary")
            );
            let output = hook_stdout_with_context(
                runtime,
                HookEvent::UserPromptSubmit,
                Some("Project Memory:\n- Keep the boundary local.\n"),
            )
            .unwrap();
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(
                value["hookSpecificOutput"]["hookEventName"],
                "UserPromptSubmit"
            );
            assert_eq!(
                value["hookSpecificOutput"]["additionalContext"],
                "Project Memory:\n- Keep the boundary local.\n"
            );
        }
    }

    #[test]
    fn an_empty_session_start_adds_nothing_to_the_base_context() {
        // B10: no card for the index, no index.
        let (_elapsed, expired) = invocation_clock();
        let result = render(
            HookEvent::SessionStart,
            Injection {
                outcome: InjectionOutcome::Empty,
                kind: InjectionKind::Index,
                items: Vec::new(),
                token_count: 0,
            },
            None,
            &expired,
        );

        assert_eq!(result.outcome, HookMemoryOutcome::Empty);
        assert_eq!(result.context, None);
    }

    #[test]
    fn expired_hook_keeps_only_base_context_before_opening_a_missing_database() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": project_root,
            "session_id": "expired-session"
        }))
        .unwrap();
        assert!(!database_path(&home).exists());
        let (elapsed, expired) = invocation_clock();
        elapsed.fetch_add(100, Ordering::SeqCst);

        let result = project_memory_output_with_expiry(
            AgentRuntime::Codex,
            HookEvent::SessionStart,
            &payload,
            false,
            &home,
            expired,
        );

        assert_eq!(result.outcome, HookMemoryOutcome::Deadline);
        let output: serde_json::Value =
            serde_json::from_str(result.stdout.as_ref().unwrap()).unwrap();
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            crate::runtime::PURPOSE_CONTEXT
        );
        let real_expired = project_memory_output_until(
            AgentRuntime::Codex,
            HookEvent::SessionStart,
            &payload,
            false,
            &home,
            Instant::now(),
        );
        assert_eq!(real_expired, result);
    }

    #[test]
    fn render_omits_a_real_capsule_and_authenticated_receipt_at_the_invocation_end() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        let body = "Keep the invocation deadline through rendering";
        let (home, project, store) = memory_home(temp.path(), &cwd, &[rule(body, 0.9)]);
        drop(store);
        let path = database_path(&home);

        let (elapsed, expired) = invocation_clock();
        let reader = MemoryStore::open_hook_read_only_with_expiry(&path, expired.clone()).unwrap();
        elapsed.fetch_add(99, Ordering::SeqCst);
        let injection = reader
            .retrieve_with_expiry(&RetrievalQuery::session_start(&project.id), &expired)
            .unwrap();
        assert_eq!(injection.outcome, InjectionOutcome::Provided);
        assert_eq!(injection.items.len(), 1);
        assert_eq!(injection.items[0].text, body);
        let item_key = injection.item_key();
        let receipt_auth = reader
            .receipt_auth_tag(
                &project.id,
                "codex",
                "render-session",
                "SessionStart",
                &item_key,
            )
            .unwrap();
        assert!(
            reader
                .verify_receipt_auth(
                    &project.id,
                    "codex",
                    "render-session",
                    "SessionStart",
                    &item_key,
                    &receipt_auth,
                )
                .unwrap()
        );
        elapsed.fetch_add(1, Ordering::SeqCst);

        let result = render(
            HookEvent::SessionStart,
            injection,
            Some(&receipt_auth),
            &expired,
        );

        // An expired render carries no Memory and no receipt; the hook then
        // prints only its base context.
        assert_eq!(result.outcome, HookMemoryOutcome::Deadline);
        assert_eq!(result.context, None);
    }

    #[test]
    fn missing_and_corrupt_databases_fail_open_for_both_hook_events() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let payload =
            serde_json::to_vec(&serde_json::json!({"cwd":project_root,"prompt":"keep going"}))
                .unwrap();

        let missing_prompt = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            &home,
        );
        assert_eq!(missing_prompt.outcome, HookMemoryOutcome::Unavailable);
        assert!(missing_prompt.stdout.is_none());
        let missing_start = project_memory_output(
            AgentRuntime::ClaudeCode,
            HookEvent::SessionStart,
            &payload,
            false,
            &home,
        );
        assert_eq!(missing_start.outcome, HookMemoryOutcome::Unavailable);
        let start_envelope: serde_json::Value =
            serde_json::from_str(&missing_start.stdout.unwrap()).unwrap();
        assert_eq!(
            start_envelope["hookSpecificOutput"]["additionalContext"],
            crate::runtime::PURPOSE_CONTEXT
        );

        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"not a sqlite database").unwrap();
        let corrupt = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            &home,
        );
        assert_eq!(corrupt.outcome, HookMemoryOutcome::Unavailable);
        assert!(corrupt.stdout.is_none());
    }

    #[test]
    fn locked_database_fails_open_without_waiting_for_the_writer() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, &node()).unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, &node())
            .unwrap();
        drop(store);

        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.pragma_update(None, "journal_mode", "DELETE").unwrap();
        lock.pragma_update(None, "locking_mode", "EXCLUSIVE")
            .unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let payload =
            serde_json::to_vec(&serde_json::json!({"cwd":project_root,"prompt":"keep going"}))
                .unwrap();
        let started = Instant::now();
        // Lock refusal determines the outcome; invocation expiry is tested separately.
        // Keep the real elapsed-time bound below to catch waiting for the writer.
        let result = project_memory_output_with_frozen_time(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            &home,
        );
        assert_eq!(result.outcome, HookMemoryOutcome::Unavailable);
        assert!(result.stdout.is_none());
        assert!(started.elapsed() < Duration::from_millis(HOOK_DEADLINE_MS));
        lock.execute_batch("ROLLBACK").unwrap();
    }
}
