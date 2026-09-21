//! Bounded, fail-open Project Memory projection for agent hooks.

use crate::runtime::{AgentRuntime, HookEvent, hook_stdout_with_context};
use hide_memory::{
    HOOK_DEADLINE_MS, HOOK_INPUT_LIMIT_BYTES, Injection, InjectionOutcome, MemoryStore,
    RetrievalQuery,
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
    home.join("Library/Application Support/hide/project-memory.sqlite3")
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
    let base = || HookMemoryResult {
        stdout: crate::runtime::hook_stdout(runtime, event),
        outcome: HookMemoryOutcome::Unavailable,
    };
    if exceeded {
        return HookMemoryResult {
            outcome: HookMemoryOutcome::InputTooLarge,
            ..base()
        };
    }
    let Ok(input) = serde_json::from_slice::<HookInput>(bytes) else {
        return base();
    };
    let Some(cwd) = input.cwd else {
        return HookMemoryResult {
            outcome: HookMemoryOutcome::ProjectUnresolved,
            ..base()
        };
    };
    let Ok(project) = hide_project::resolve(&cwd, "local") else {
        return HookMemoryResult {
            outcome: HookMemoryOutcome::ProjectUnresolved,
            ..base()
        };
    };
    if Instant::now() >= deadline {
        return HookMemoryResult {
            outcome: HookMemoryOutcome::Deadline,
            ..base()
        };
    }
    let Ok(store) = MemoryStore::open_hook_read_only_with_deadline(&database_path(home), deadline)
    else {
        if Instant::now() >= deadline {
            return HookMemoryResult {
                outcome: HookMemoryOutcome::Deadline,
                ..base()
            };
        }
        return base();
    };
    let runtime_id = match runtime {
        AgentRuntime::Codex => "codex",
        AgentRuntime::ClaudeCode => "claude",
    };
    let session_id = input.session_id.unwrap_or_default();
    if session_id.is_empty() {
        return base();
    }
    let query = match event {
        HookEvent::SessionStart => RetrievalQuery::session_start(&project.id),
        HookEvent::UserPromptSubmit => {
            let mut text = input.prompt.unwrap_or_default();
            if let Ok(topics) = store.recent_session_topics(&project.id, runtime_id, &session_id) {
                for topic in topics {
                    text.push('\n');
                    text.push_str(&topic);
                }
            }
            let mut excluded =
                match store.session_start_receipt_ids(&project.id, runtime_id, &session_id) {
                    Ok(Some(items)) => items,
                    // Only the receipt records what SessionStart actually
                    // delivered. If the first prompt races receipt projection,
                    // omitting Memory for that prompt is the only read-only
                    // outcome that cannot repeat or falsely exclude an item.
                    Ok(None) | Err(_) => return base(),
                };
            excluded.sort_unstable();
            excluded.dedup();
            RetrievalQuery::prompt(&project.id, text, excluded)
                .with_path_context(canonical_path_context(&project, &cwd).to_string_lossy())
        }
        HookEvent::SubagentStart | HookEvent::SubagentStop | HookEvent::Stop => return base(),
    };
    if Instant::now() >= deadline {
        return HookMemoryResult {
            outcome: HookMemoryOutcome::Deadline,
            ..base()
        };
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let mut query = query;
    query.deadline = remaining;
    let Ok(injection) = store.retrieve(&query) else {
        if Instant::now() >= deadline {
            return HookMemoryResult {
                outcome: HookMemoryOutcome::Deadline,
                ..base()
            };
        }
        return base();
    };
    let item_key = injection
        .items
        .iter()
        .map(|(id, revision, _)| format!("{id}@{revision}"))
        .collect::<Vec<_>>()
        .join(",");
    let Ok(receipt_auth) = store.receipt_auth_tag(
        &project.id,
        runtime_id,
        &session_id,
        event.name(),
        &item_key,
    ) else {
        return base();
    };
    render(runtime, event, injection, &receipt_auth, deadline, base)
}

fn canonical_path_context(project: &hide_project::ProjectIdentity, cwd: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    canonical
        .strip_prefix(&project.checkout_root)
        .map(|relative| project.root.join(relative))
        .unwrap_or(canonical)
}

fn render(
    runtime: AgentRuntime,
    event: HookEvent,
    injection: Injection,
    receipt_auth: &str,
    deadline: Instant,
    base: impl Fn() -> HookMemoryResult,
) -> HookMemoryResult {
    if Instant::now() >= deadline {
        return HookMemoryResult {
            outcome: HookMemoryOutcome::Deadline,
            ..base()
        };
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
    let receipt = injection
        .items
        .iter()
        .map(|(id, revision, _)| format!("{id}@{revision}"))
        .collect::<Vec<_>>()
        .join(",");
    let receipt = format!(
        "<hide-memory-receipt event=\"{}\" count=\"{}\" items=\"{}\" auth=\"{}\" />\n",
        event.name(),
        injection.items.len(),
        receipt,
        receipt_auth,
    );
    let context = match injection.context() {
        Some(mut context) => {
            context.push_str(&receipt);
            context
        }
        None if event == HookEvent::SessionStart
            && injection.outcome == InjectionOutcome::Empty =>
        {
            receipt
        }
        None => {
            return HookMemoryResult {
                stdout: crate::runtime::hook_stdout(runtime, event),
                outcome,
            };
        }
    };
    HookMemoryResult {
        stdout: hook_stdout_with_context(runtime, event, Some(&context)),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_memory::{
        AnalysisBatch, Candidate, CandidateKind, CandidateRelation, Injection, InjectionOutcome,
        SessionSourceRecord,
    };
    use std::fs;
    use std::process::Command;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
            home.join("Library/Application Support/hide/project-memory.sqlite3")
        );
        // SAFETY: guarded by ENV_LOCK and restored before the test returns.
        unsafe {
            std::env::remove_var(MEMORY_DATABASE_ENV);
        }
    }

    #[test]
    fn project_path_terms_rank_matches_but_never_create_a_prompt_match() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project-memory");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        store
            .record_injection(
                &project.id,
                "codex",
                "unrelated-session",
                None,
                &Injection {
                    outcome: InjectionOutcome::Empty,
                    items: Vec::new(),
                    token_count: 0,
                },
            )
            .unwrap();
        store
            .apply_candidates(
                &AnalysisBatch {
                    id: "path-query-batch".into(),
                    project_id: project.id.clone(),
                    provider: "codex".into(),
                    analysis_provider: "codex".into(),
                    session_id: "source-session".into(),
                    content_hash: "path-query-hash".into(),
                    created_at_unix_ms: 1,
                },
                &[Candidate {
                    text: "Project Memory provider work uses a bounded queue".into(),
                    kind: CandidateKind::Rule,
                    confidence: 0.9,
                    salience: 0.9,
                    source_offsets: vec![1],
                    direct_human_source: true,
                    relation: CandidateRelation::New,
                }],
            )
            .unwrap();
        drop(store);

        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": project_root,
            "session_id": "unrelated-session",
            "prompt": "zz--no-match-unrelated-query"
        }))
        .unwrap();
        let result = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            &home,
        );

        assert_eq!(result.outcome, HookMemoryOutcome::Empty);
        assert!(result.stdout.is_none());
    }

    #[test]
    fn prompt_lookup_omits_only_items_from_the_actual_session_start_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        for index in 0..6 {
            store
                .apply_candidates(
                    &AnalysisBatch {
                        id: format!("b{index}"),
                        project_id: project.id.clone(),
                        provider: "codex".into(),
                        analysis_provider: "codex".into(),
                        session_id: "s".into(),
                        content_hash: format!("h{index}"),
                        created_at_unix_ms: index,
                    },
                    &[Candidate {
                        text: format!("Rule {index} about durable hooks"),
                        kind: CandidateKind::Rule,
                        confidence: 0.9,
                        salience: 1.0 - index as f64 / 10.0,
                        source_offsets: vec![index],
                        direct_human_source: true,
                        relation: CandidateRelation::New,
                    }],
                )
                .unwrap();
        }
        let session_start_item = store
            .list_memories(&project.id, "")
            .unwrap()
            .into_iter()
            .find(|memory| memory.body.starts_with("Rule 0 "))
            .unwrap();
        store
            .record_injection(
                &project.id,
                "codex",
                "fixture-session",
                None,
                &hide_memory::Injection {
                    outcome: hide_memory::InjectionOutcome::Provided,
                    items: vec![(
                        session_start_item.id,
                        session_start_item.revision,
                        session_start_item.body,
                    )],
                    token_count: 6,
                },
            )
            .unwrap();
        drop(store);
        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": project_root,
            "session_id": "fixture-session",
            "prompt": "durable hooks"
        }))
        .unwrap();
        let result = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            &home,
        );
        assert_eq!(result.outcome, HookMemoryOutcome::Provided { count: 3 });
        let output = result.stdout.unwrap();
        assert!(
            output.contains("Rule 1"),
            "unseen items remain eligible: {output}"
        );
        assert!(!output.contains("Rule 0"));
    }

    #[test]
    fn immediate_first_prompt_omits_memory_until_the_exact_receipt_is_projected() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        let project = hide_project::resolve(&project_root, "local").unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        for index in 0..8 {
            store
                .apply_candidates(
                    &AnalysisBatch {
                        id: format!("race-b{index}"),
                        project_id: project.id.clone(),
                        provider: "codex".into(),
                        analysis_provider: "codex".into(),
                        session_id: "source".into(),
                        content_hash: format!("race-h{index}"),
                        created_at_unix_ms: index,
                    },
                    &[Candidate {
                        text: format!("Race rule {index} about durable hooks"),
                        kind: CandidateKind::Rule,
                        confidence: 0.9,
                        salience: 1.0 - index as f64 / 10.0,
                        source_offsets: vec![index],
                        direct_human_source: true,
                        relation: CandidateRelation::New,
                    }],
                )
                .unwrap();
        }
        drop(store);
        let session_id = "immediate-session";
        let start_payload = serde_json::to_vec(&serde_json::json!({
            "cwd": project_root,
            "session_id": session_id
        }))
        .unwrap();
        let start = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::SessionStart,
            &start_payload,
            false,
            &home,
        );
        assert_eq!(start.outcome, HookMemoryOutcome::Provided { count: 5 });
        let start_output = start.stdout.unwrap();
        let started_rules = (0..8)
            .filter(|index| start_output.contains(&format!("Race rule {index} ")))
            .collect::<Vec<_>>();
        assert_eq!(started_rules.len(), 5);

        let mut store = MemoryStore::open(&path).unwrap();
        store
            .apply_candidates(
                &AnalysisBatch {
                    id: "race-new-batch".into(),
                    project_id: project.id.clone(),
                    provider: "codex".into(),
                    analysis_provider: "codex".into(),
                    session_id: "other-session".into(),
                    content_hash: "race-new-hash".into(),
                    created_at_unix_ms: 100,
                },
                &[Candidate {
                    text: "New higher-ranked durable hook rule".into(),
                    kind: CandidateKind::Rule,
                    confidence: 1.0,
                    salience: 1.0,
                    source_offsets: vec![100],
                    direct_human_source: true,
                    relation: CandidateRelation::New,
                }],
            )
            .unwrap();
        drop(store);

        let prompt_payload = serde_json::to_vec(&serde_json::json!({
            "cwd": project_root,
            "session_id": session_id,
            "prompt": "durable hooks"
        }))
        .unwrap();
        let prompt = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &prompt_payload,
            false,
            &home,
        );
        assert_eq!(prompt.outcome, HookMemoryOutcome::Unavailable);
        assert!(prompt.stdout.is_none());
        assert!(
            !home
                .join("Library/Application Support/hide/project-memory-receipts")
                .exists(),
            "the read-only hook must not create a receipt sidecar",
        );
    }

    #[test]
    fn linked_worktree_cwd_keeps_checkout_relative_path_relevance() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let main = temp.path().join("main");
        let linked = temp.path().join("linked");
        fs::create_dir_all(&home).unwrap();
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
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "init"])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    "linked",
                    linked.to_str().unwrap()
                ])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        let linked_cwd = linked.join("crates/memory/src");
        fs::create_dir_all(&linked_cwd).unwrap();

        let project = hide_project::resolve(&linked_cwd, "local").unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        store
            .record_injection(
                &project.id,
                "codex",
                "linked-session",
                None,
                &Injection {
                    outcome: InjectionOutcome::Empty,
                    items: Vec::new(),
                    token_count: 0,
                },
            )
            .unwrap();
        store
            .upsert_session_source(&SessionSourceRecord {
                id: "path-source".into(),
                project_id: project.id.clone(),
                provider: "codex".into(),
                locator: linked.join("session.jsonl").to_string_lossy().into_owned(),
                checkout_path: linked_cwd.to_string_lossy().into_owned(),
                started_at_unix_ms: Some(1),
                updated_at_unix_ms: 1,
                unavailable_reason: None,
            })
            .unwrap();
        store
            .apply_candidates(
                &AnalysisBatch {
                    id: "path-batch".into(),
                    project_id: project.id.clone(),
                    provider: "codex".into(),
                    analysis_provider: "codex".into(),
                    session_id: "path-source".into(),
                    content_hash: "path-hash".into(),
                    created_at_unix_ms: 1,
                },
                &[Candidate {
                    text: "Keep the local projection bounded".into(),
                    kind: CandidateKind::Rule,
                    confidence: 0.9,
                    salience: 0.9,
                    source_offsets: vec![1],
                    direct_human_source: true,
                    relation: CandidateRelation::New,
                }],
            )
            .unwrap();
        assert_eq!(project.checkout_root, fs::canonicalize(&linked).unwrap());
        let path_context = canonical_path_context(&project, &linked_cwd);
        assert_eq!(path_context, project.root.join("crates/memory/src"));
        assert_eq!(
            store
                .retrieve(
                    &RetrievalQuery::prompt(&project.id, "zz-no-literal-match", vec![])
                        .with_path_context(path_context.to_string_lossy()),
                )
                .unwrap()
                .outcome,
            InjectionOutcome::Provided,
        );
        drop(store);

        let payload = serde_json::to_vec(&serde_json::json!({
            "cwd": linked_cwd,
            "session_id": "linked-session",
            "prompt": "zz-no-literal-match"
        }))
        .unwrap();
        let result = project_memory_output(
            AgentRuntime::Codex,
            HookEvent::UserPromptSubmit,
            &payload,
            false,
            &home,
        );

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
    fn empty_session_start_emits_an_internal_receipt_without_a_zero_item_message() {
        for runtime in [AgentRuntime::ClaudeCode, AgentRuntime::Codex] {
            let result = render(
                runtime,
                HookEvent::SessionStart,
                Injection {
                    outcome: InjectionOutcome::Empty,
                    items: Vec::new(),
                    token_count: 0,
                },
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                Instant::now() + Duration::from_secs(1),
                || HookMemoryResult {
                    stdout: crate::runtime::hook_stdout(runtime, HookEvent::SessionStart),
                    outcome: HookMemoryOutcome::Unavailable,
                },
            );

            assert_eq!(result.outcome, HookMemoryOutcome::Empty);
            let output = result.stdout.unwrap();
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            let context = value["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap();
            assert!(
                context.contains(
                    "<hide-memory-receipt event=\"SessionStart\" count=\"0\" items=\"\" auth=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" />"
                )
            );
            assert!(!context.contains("Project Memory ready 0"));
            assert!(!context.contains("<hide-memory-context"));
        }
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
        let project = hide_project::resolve(&project_root, "local").unwrap();
        let path = database_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, "local")
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
        let result = project_memory_output(
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
