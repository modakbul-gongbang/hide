//! What one agent hook gets from Project Memory (PRD core-host-node-move
//! D-12, B14): the core answers it from the store it owns, for the Project of
//! the checkout the hook's pane is attested to, so the store is read on the
//! core's machine whichever machine's agent asks. Bounded by the caller's
//! expiry and failing open: any missing part answers with no Memory.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Injection, InjectionOutcome, MemoryStore, RetrievalQuery};

/// The most of a prompt a hook sends the core to rank Memory by: its start,
/// which names the task, and few enough bytes that the request stays one
/// small message whichever machine the core runs on.
pub const PROMPT_LIMIT_BYTES: usize = 8 * 1024;

/// `prompt`'s first [`PROMPT_LIMIT_BYTES`] bytes at most, cut on a character
/// boundary.
pub fn prompt_prefix(prompt: &str) -> &str {
    prefix(prompt, PROMPT_LIMIT_BYTES)
}

/// `text`'s first `bytes` bytes at most, cut on a character boundary.
pub fn prefix(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The two events Memory answers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum HookEvent {
    SessionStart,
    UserPromptSubmit,
}

impl HookEvent {
    /// The event's name as receipts carry it.
    pub fn name(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
        }
    }
}

/// How a hook's Memory read ended, for the hook's own record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum HookOutcome {
    Provided { count: usize },
    Empty,
    Disabled,
    Unavailable,
    Deadline,
    ProjectUnresolved,
}

/// What one hook asks for.
pub struct HookRequest<'a> {
    /// The provider id Memory keys sessions and receipts by: `claude`,
    /// `codex` or `opencode`.
    pub runtime_id: &'a str,
    pub event: HookEvent,
    pub session_id: &'a str,
    pub prompt: Option<&'a str>,
    /// Where in the Project the agent works, in the Project's own root
    /// ([`path_context`]); ranks matching items higher.
    pub path_context: Option<&'a str>,
}

/// The Memory context for one hook, with its receipt, or none.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HookContext {
    pub context: Option<String>,
    #[serde(flatten)]
    pub outcome: HookOutcome,
}

impl HookContext {
    fn without(outcome: HookOutcome) -> Self {
        Self {
            context: None,
            outcome,
        }
    }
}

/// `cwd` spelled inside the Project's main root: a linked worktree's
/// checkout-relative path maps into the root every checkout of the Project
/// shares. `cwd` and the Project's roots are each spelled as the machine
/// holding the checkout canonicalizes them, which may not be this one, so
/// the mapping compares names and never asks this machine's filesystem.
pub fn path_context(project: &hide_project::ProjectIdentity, cwd: &str) -> String {
    let checkout = project.checkout_root.to_string_lossy();
    match cwd.strip_prefix(checkout.as_ref()) {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            format!("{}{rest}", project.root.to_string_lossy())
        }
        _ => cwd.to_owned(),
    }
}

/// The context `store_path`'s Memory gives `request` for `project`, or none
/// with why; `expired` bounds every step.
pub fn context_for<E>(
    store_path: &Path,
    project: &hide_project::ProjectIdentity,
    request: HookRequest<'_>,
    expired: E,
) -> HookContext
where
    E: Fn() -> bool + Clone + Send + 'static,
{
    let base = || HookContext::without(HookOutcome::Unavailable);
    let deadline = || HookContext::without(HookOutcome::Deadline);
    let Ok(store) = MemoryStore::open_hook_read_only_with_expiry(store_path, expired.clone())
    else {
        if expired() {
            return deadline();
        }
        return base();
    };
    if expired() {
        return deadline();
    }
    if request.session_id.is_empty() {
        return base();
    }
    let query = match request.event {
        HookEvent::SessionStart => RetrievalQuery::session_start(&project.id),
        HookEvent::UserPromptSubmit => {
            let mut text = request.prompt.unwrap_or_default().to_owned();
            if let Ok(topics) =
                store.recent_session_topics(&project.id, request.runtime_id, request.session_id)
            {
                for topic in topics {
                    text.push('\n');
                    text.push_str(&topic);
                }
            }
            let mut excluded = match store.session_start_receipt_ids(
                &project.id,
                request.runtime_id,
                request.session_id,
            ) {
                Ok(Some(items)) => items,
                // Only the receipt records what SessionStart actually
                // delivered. If the first prompt races receipt projection,
                // omitting Memory for that prompt is the only read-only
                // outcome that cannot repeat or falsely exclude an item.
                Ok(None) | Err(_) => return base(),
            };
            excluded.sort_unstable();
            excluded.dedup();
            let query = RetrievalQuery::prompt(&project.id, text, excluded);
            match request.path_context {
                Some(path) => query.with_path_context(path),
                None => query,
            }
        }
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
    let item_key = items_key(&injection);
    let Ok(receipt_auth) = store.receipt_auth_tag(
        &project.id,
        request.runtime_id,
        request.session_id,
        request.event.name(),
        &item_key,
    ) else {
        return base();
    };
    render(request.event, injection, &receipt_auth, &expired)
}

fn items_key(injection: &Injection) -> String {
    injection
        .items
        .iter()
        .map(|(id, revision, _)| format!("{id}@{revision}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn render(
    event: HookEvent,
    injection: Injection,
    receipt_auth: &str,
    expired: &impl Fn() -> bool,
) -> HookContext {
    if expired() {
        return HookContext::without(HookOutcome::Deadline);
    }
    let outcome = match injection.outcome {
        InjectionOutcome::Provided => HookOutcome::Provided {
            count: injection.items.len(),
        },
        InjectionOutcome::Empty => HookOutcome::Empty,
        InjectionOutcome::Disabled => HookOutcome::Disabled,
        InjectionOutcome::Deadline => HookOutcome::Deadline,
        InjectionOutcome::Unavailable | InjectionOutcome::Stale => HookOutcome::Unavailable,
    };
    let receipt = format!(
        "<hide-memory-receipt event=\"{}\" count=\"{}\" items=\"{}\" auth=\"{}\" />\n",
        event.name(),
        injection.items.len(),
        items_key(&injection),
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
        None => return HookContext::without(outcome),
    };
    HookContext {
        context: Some(context),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisBatch, Candidate, CandidateKind, CandidateRelation, SessionSourceRecord};
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    const NODE: &str = "node-a";

    /// A clock that moves only when the test moves it, ending at the store's
    /// own hook deadline.
    fn invocation_clock() -> (Arc<AtomicU64>, impl Fn() -> bool + Clone + Send + 'static) {
        let anchor = Instant::now();
        let end = anchor + Duration::from_millis(crate::HOOK_DEADLINE_MS);
        let elapsed = Arc::new(AtomicU64::new(0));
        let clock = Arc::clone(&elapsed);
        let expired = move || anchor + Duration::from_millis(clock.load(Ordering::SeqCst)) >= end;
        (elapsed, expired)
    }

    /// A store at `root` holding the enabled Project at `checkout`.
    fn enabled_store(root: &Path, checkout: &Path) -> (PathBuf, hide_project::ProjectIdentity) {
        let project = hide_project::resolve(checkout, NODE).unwrap();
        let path = crate::database_path(root);
        let store = MemoryStore::open(&path).unwrap();
        store
            .ensure_project(&project.id, &project.root, NODE)
            .unwrap();
        store.set_enabled(&project.id, true, true).unwrap();
        (path, project)
    }

    fn rule(
        project: &str,
        id: &str,
        text: &str,
        salience: f64,
        offset: u64,
    ) -> (AnalysisBatch, Candidate) {
        (
            AnalysisBatch {
                id: format!("{id}-batch"),
                project_id: project.to_owned(),
                provider: "codex".into(),
                analysis_provider: "codex".into(),
                session_id: "source-session".into(),
                content_hash: format!("{id}-hash"),
                created_at_unix_ms: offset,
            },
            Candidate {
                text: text.into(),
                kind: CandidateKind::Rule,
                confidence: 0.9,
                salience,
                source_offsets: vec![offset],
                direct_human_source: true,
                relation: CandidateRelation::New,
            },
        )
    }

    fn add_rule(path: &Path, project: &str, id: &str, text: &str, salience: f64, offset: u64) {
        let (batch, candidate) = rule(project, id, text, salience, offset);
        MemoryStore::open(path)
            .unwrap()
            .apply_candidates(&batch, &[candidate])
            .unwrap();
    }

    fn record_start(path: &Path, project: &str, session: &str, items: Vec<(String, u64, String)>) {
        MemoryStore::open(path)
            .unwrap()
            .record_injection(
                project,
                "codex",
                session,
                None,
                &Injection {
                    outcome: if items.is_empty() {
                        InjectionOutcome::Empty
                    } else {
                        InjectionOutcome::Provided
                    },
                    token_count: items.len() * 6,
                    items,
                },
            )
            .unwrap();
    }

    fn ask<'a>(event: HookEvent, session: &'a str, prompt: Option<&'a str>) -> HookRequest<'a> {
        HookRequest {
            runtime_id: "codex",
            event,
            session_id: session,
            prompt,
            path_context: None,
        }
    }

    fn frozen() -> impl Fn() -> bool + Clone + Send + 'static {
        invocation_clock().1
    }

    #[test]
    fn project_path_terms_rank_matches_but_never_create_a_prompt_match() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project-memory");
        fs::create_dir_all(&checkout).unwrap();
        let (path, project) = enabled_store(temp.path(), &checkout);
        record_start(&path, &project.id, "unrelated-session", Vec::new());
        add_rule(
            &path,
            &project.id,
            "path-query",
            "Project Memory provider work uses a bounded queue",
            0.9,
            1,
        );

        let cwd = checkout.canonicalize().unwrap();
        let path_context = path_context(&project, &cwd.to_string_lossy());
        let result = context_for(
            &path,
            &project,
            HookRequest {
                path_context: Some(&path_context),
                ..ask(
                    HookEvent::UserPromptSubmit,
                    "unrelated-session",
                    Some("zz--no-match-unrelated-query"),
                )
            },
            frozen(),
        );

        assert_eq!(result.outcome, HookOutcome::Empty);
        assert!(result.context.is_none());
    }

    #[test]
    fn prompt_lookup_omits_only_items_from_the_actual_session_start_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        fs::create_dir_all(&checkout).unwrap();
        let (path, project) = enabled_store(temp.path(), &checkout);
        for index in 0..6 {
            add_rule(
                &path,
                &project.id,
                &format!("b{index}"),
                &format!("Rule {index} about durable hooks"),
                1.0 - index as f64 / 10.0,
                index,
            );
        }
        let first = MemoryStore::open(&path)
            .unwrap()
            .list_memories(&project.id, "")
            .unwrap()
            .into_iter()
            .find(|memory| memory.body.starts_with("Rule 0 "))
            .unwrap();
        record_start(
            &path,
            &project.id,
            "fixture-session",
            vec![(first.id, first.revision, first.body)],
        );

        let result = context_for(
            &path,
            &project,
            ask(
                HookEvent::UserPromptSubmit,
                "fixture-session",
                Some("durable hooks"),
            ),
            frozen(),
        );

        assert_eq!(result.outcome, HookOutcome::Provided { count: 3 });
        let context = result.context.unwrap();
        assert!(
            context.contains("Rule 1"),
            "unseen items remain eligible: {context}"
        );
        assert!(!context.contains("Rule 0"));
    }

    #[test]
    fn an_immediate_first_prompt_omits_memory_until_the_exact_receipt_is_projected() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        fs::create_dir_all(&checkout).unwrap();
        let (path, project) = enabled_store(temp.path(), &checkout);
        for index in 0..8 {
            add_rule(
                &path,
                &project.id,
                &format!("race-b{index}"),
                &format!("Race rule {index} about durable hooks"),
                1.0 - index as f64 / 10.0,
                index,
            );
        }
        let session = "immediate-session";
        let start = context_for(
            &path,
            &project,
            ask(HookEvent::SessionStart, session, None),
            frozen(),
        );
        assert_eq!(start.outcome, HookOutcome::Provided { count: 5 });
        let start = start.context.unwrap();
        assert_eq!(
            (0..8)
                .filter(|index| start.contains(&format!("Race rule {index} ")))
                .count(),
            5
        );
        add_rule(
            &path,
            &project.id,
            "race-new",
            "New higher-ranked durable hook rule",
            1.0,
            100,
        );

        let prompt = context_for(
            &path,
            &project,
            ask(HookEvent::UserPromptSubmit, session, Some("durable hooks")),
            frozen(),
        );

        assert_eq!(prompt.outcome, HookOutcome::Unavailable);
        assert!(prompt.context.is_none());
        assert!(
            !temp.path().join("project-memory-receipts").exists(),
            "a read-only answer creates no receipt sidecar",
        );
    }

    #[test]
    fn a_linked_worktree_cwd_keeps_its_checkout_relative_path_relevance() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("main");
        let linked = temp.path().join("linked");
        fs::create_dir_all(&main).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&main)
                    .status()
                    .unwrap()
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        fs::write(main.join("README.md"), "fixture\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "commit.gpgsign=false", "commit", "-qm", "init"]);
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ]);
        let linked_cwd = linked.join("crates/memory/src");
        fs::create_dir_all(&linked_cwd).unwrap();

        // The pane's checkout names the Project; the cwd only ranks.
        let (path, project) = enabled_store(temp.path(), &linked);
        assert_eq!(project.checkout_root, fs::canonicalize(&linked).unwrap());
        record_start(&path, &project.id, "linked-session", Vec::new());
        let mut store = MemoryStore::open(&path).unwrap();
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
        let (mut batch, candidate) = rule(
            &project.id,
            "path",
            "Keep the local projection bounded",
            0.9,
            1,
        );
        batch.session_id = "path-source".into();
        store.apply_candidates(&batch, &[candidate]).unwrap();
        drop(store);

        let cwd = fs::canonicalize(&linked_cwd).unwrap();
        let path_context = path_context(&project, &cwd.to_string_lossy());
        assert_eq!(
            PathBuf::from(&path_context),
            project.root.join("crates/memory/src")
        );
        let result = context_for(
            &path,
            &project,
            HookRequest {
                path_context: Some(&path_context),
                ..ask(
                    HookEvent::UserPromptSubmit,
                    "linked-session",
                    Some("zz-no-literal-match"),
                )
            },
            frozen(),
        );

        assert_eq!(result.outcome, HookOutcome::Provided { count: 1 });
        assert!(
            result
                .context
                .unwrap()
                .contains("Keep the local projection bounded")
        );
    }

    #[test]
    fn a_cwd_maps_into_the_project_root_by_its_names_whatever_machine_spells_them() {
        let project = |root: &str, checkout: &str| hide_project::ProjectIdentity {
            id: "project:x".into(),
            root: PathBuf::from(root),
            checkout_root: PathBuf::from(checkout),
            device_id: NODE.into(),
            kind: hide_project::ProjectKind::Git,
        };
        let linked = project("/repo/main", "/repo/linked");
        assert_eq!(
            path_context(&linked, "/repo/linked/src/a"),
            "/repo/main/src/a"
        );
        assert_eq!(path_context(&linked, "/repo/linked"), "/repo/main");
        assert_eq!(
            path_context(&linked, "/repo/linked-other/src"),
            "/repo/linked-other/src"
        );
        assert_eq!(path_context(&linked, "/elsewhere"), "/elsewhere");
        // A Windows node's spelling, read on a core that is not Windows.
        let windows = project(r"\\?\C:\repo\main", r"\\?\C:\repo\linked");
        assert_eq!(
            path_context(&windows, r"\\?\C:\repo\linked\src"),
            r"\\?\C:\repo\main\src"
        );
    }

    #[test]
    fn an_empty_session_start_carries_its_receipt_without_a_zero_item_message() {
        let result = render(
            HookEvent::SessionStart,
            Injection {
                outcome: InjectionOutcome::Empty,
                items: Vec::new(),
                token_count: 0,
            },
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &frozen(),
        );

        assert_eq!(result.outcome, HookOutcome::Empty);
        assert_eq!(
            result.context.unwrap(),
            "<hide-memory-receipt event=\"SessionStart\" count=\"0\" items=\"\" auth=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" />\n"
        );
    }

    #[test]
    fn an_expired_ask_answers_deadline_before_opening_a_missing_store() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        fs::create_dir_all(&checkout).unwrap();
        let project = hide_project::resolve(&checkout, NODE).unwrap();
        let path = crate::database_path(temp.path());
        let (elapsed, expired) = invocation_clock();
        elapsed.fetch_add(100, Ordering::SeqCst);

        let result = context_for(
            &path,
            &project,
            ask(HookEvent::SessionStart, "s", None),
            expired,
        );

        assert_eq!(result.outcome, HookOutcome::Deadline);
        assert!(result.context.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn a_render_at_the_deadline_carries_neither_the_capsule_nor_its_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        fs::create_dir_all(&checkout).unwrap();
        let (path, project) = enabled_store(temp.path(), &checkout);
        let body = "Keep the invocation deadline through rendering";
        add_rule(&path, &project.id, "render-expiry", body, 0.9, 1);

        let (elapsed, expired) = invocation_clock();
        let reader = MemoryStore::open_hook_read_only_with_expiry(&path, expired.clone()).unwrap();
        elapsed.fetch_add(99, Ordering::SeqCst);
        let injection = reader
            .retrieve_with_expiry(&RetrievalQuery::session_start(&project.id), &expired)
            .unwrap();
        assert_eq!(injection.outcome, InjectionOutcome::Provided);
        assert_eq!(injection.items[0].2, body);
        let item_key = items_key(&injection);
        let auth = reader
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
                    &auth
                )
                .unwrap()
        );
        elapsed.fetch_add(1, Ordering::SeqCst);

        let result = render(HookEvent::SessionStart, injection, &auth, &expired);

        assert_eq!(result.outcome, HookOutcome::Deadline);
        assert_eq!(result.context, None);
    }

    #[test]
    fn a_missing_or_corrupt_store_answers_unavailable_for_both_events() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        fs::create_dir_all(&checkout).unwrap();
        let project = hide_project::resolve(&checkout, NODE).unwrap();
        let path = crate::database_path(temp.path());
        for event in [HookEvent::SessionStart, HookEvent::UserPromptSubmit] {
            let missing = context_for(
                &path,
                &project,
                ask(event, "s", Some("keep going")),
                frozen(),
            );
            assert_eq!(missing.outcome, HookOutcome::Unavailable, "{event:?}");
            assert!(missing.context.is_none());
        }
        fs::write(&path, b"not a sqlite database").unwrap();
        let corrupt = context_for(
            &path,
            &project,
            ask(HookEvent::UserPromptSubmit, "s", Some("keep going")),
            frozen(),
        );
        assert_eq!(corrupt.outcome, HookOutcome::Unavailable);
        assert!(corrupt.context.is_none());
    }

    #[test]
    fn a_locked_store_answers_unavailable_without_waiting_for_the_writer() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        fs::create_dir_all(&checkout).unwrap();
        let (path, project) = enabled_store(temp.path(), &checkout);

        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.pragma_update(None, "journal_mode", "DELETE").unwrap();
        lock.pragma_update(None, "locking_mode", "EXCLUSIVE")
            .unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let started = Instant::now();
        // The lock decides the outcome; the frozen clock never expires, and
        // the elapsed bound below catches a wait for the writer.
        let result = context_for(
            &path,
            &project,
            ask(HookEvent::UserPromptSubmit, "s", Some("keep going")),
            frozen(),
        );
        assert_eq!(result.outcome, HookOutcome::Unavailable);
        assert!(result.context.is_none());
        assert!(started.elapsed() < Duration::from_millis(crate::HOOK_DEADLINE_MS));
        lock.execute_batch("ROLLBACK").unwrap();
    }
}
