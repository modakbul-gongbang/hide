//! `hide memory show|search|add|forget|undo` (PRD project-memory-v2 B7,
//! B8, B14-B16, B27, D-03, D-11).
//!
//! hided names the caller from its pane capability; the core checks the
//! caller under its lock and hands back a [`PreparedMemory`] holding what the
//! command needs, and the command itself (store reads, the Project's identity,
//! an `add`'s provider call) runs on hided's blocking task, off the lock.
//! A connected device's Memory is its own (D-22), so only a caller on this
//! machine is answered.

use crate::model::MemoryNoticeSnapshot;
use crate::node::NodeId;
use crate::node_access::NodeLink;
use crate::runtime::RuntimeWorkerContext;
use crate::runtime::memory_analysis::{
    AnalysisFailure, BatchSource, WeeklyUsage, admit_call, batch_identity, report_memory_change,
    write_cards,
};
use hide_ai::{AiError, AiRouter, AiSettings, CancelToken};
use hide_memory::{CandidateOutcome, MemoryItem, MemoryLifecycle, MemoryStore, card_name};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most characters `hide memory add` takes (B8).
pub const ADD_LIMIT_CHARS: usize = hide_memory::MEMORY_BODY_LIMIT_CHARS;
/// The most bytes a search's words may take; the matcher reads only the
/// first words anyway.
pub const SEARCH_LIMIT_BYTES: usize = 1024;
/// How long an `add` may spend asking the provider, kept inside the minute
/// B8 gives the whole command so the store is never written after the caller
/// stopped waiting.
pub const ADD_PROVIDER_DEADLINE: Duration = Duration::from_secs(50);
/// How long the caller waits for an `add` (B8).
pub const ADD_TIMEOUT: Duration = Duration::from_secs(60);
/// How long before the caller stops waiting an `add` stops writing, for its
/// answer to get back.
const ADD_ANSWER_MARGIN: Duration = Duration::from_secs(5);

/// The provider and session `add` writes its batch under: one per Project
/// and text, so the same text added again is the same batch and never a
/// second card. Once every card of that batch is gone (forgotten, undone,
/// or none was kept), the same text starts the next generation's batch.
const ADD_PROVIDER: &str = "hide";
const ADD_SESSION: &str = "memory-add";
/// The most generations one text is looked up through.
const ADD_GENERATIONS: u32 = 32;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum MemoryCommand {
    Show {
        number: u64,
    },
    Search {
        words: String,
    },
    Add {
        text: String,
    },
    /// The operator asked the agent to drop a card; Undo brings it back.
    Forget {
        number: u64,
    },
    /// Steps card `number` back one change.
    Undo {
        number: u64,
    },
}

impl MemoryCommand {
    /// Whether the command's own values are ones a parser could have made.
    pub fn valid(&self) -> bool {
        match self {
            Self::Show { number } | Self::Forget { number } | Self::Undo { number } => *number > 0,
            Self::Search { words } => !words.trim().is_empty() && words.len() <= SEARCH_LIMIT_BYTES,
            Self::Add { text } => {
                !text.trim().is_empty() && text.chars().count() <= ADD_LIMIT_CHARS
            }
        }
    }
}

/// The number a card name such as `m12` (or a bare `12`) gives.
pub fn parse_card(value: &str) -> Option<u64> {
    hide_memory::parse_card_name(value).filter(|number| *number > 0)
}

/// What one command needs, taken under the core's lock.
pub struct PreparedMemory {
    pub(crate) command: MemoryCommand,
    pub(crate) node: NodeId,
    pub(crate) sessions_node: Arc<dyn NodeLink>,
    pub(crate) database: PathBuf,
    pub(crate) checkout_path: String,
    /// The calling agent's Memory provider and session, when the caller is
    /// an agent pane Herdr knows the session of.
    pub(crate) reader: Option<(String, String)>,
    /// Hide AI's settings while it is on.
    pub(crate) settings: Option<AiSettings>,
    /// Where a change is reported, so the screen reads Memory again.
    pub(crate) context: Option<RuntimeWorkerContext>,
}

impl PreparedMemory {
    /// Runs the command: the answer's `result`, or a refusal code
    /// [`next_action`] explains.
    pub fn run(self) -> Result<Value, String> {
        let node = Arc::clone(&self.sessions_node);
        self.run_with(|settings| crate::ai::memory_add_router(&node, settings))
    }

    fn run_with(self, router: impl FnOnce(&AiSettings) -> AiRouter) -> Result<Value, String> {
        let deadline = Instant::now() + ADD_TIMEOUT - ADD_ANSWER_MARGIN;
        if !MemoryStore::exists(&self.database) {
            return Err("memory_off".into());
        }
        let identity = crate::runtime::memory::project_identity(
            self.sessions_node.as_ref(),
            &self.node,
            &self.checkout_path,
        )
        .map_err(|message| {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "command.project_unavailable",
                "message": message,
            }));
            "memory_project_unavailable".to_owned()
        })?;
        let mut store = MemoryStore::open(&self.database).map_err(|error| {
            crate::diagnostic!(json!({
                "component": "memory",
                "kind": "command.store_unavailable",
                "message": error.to_string(),
            }));
            "memory_unavailable".to_owned()
        })?;
        let enabled = match store.project_state(&identity.id) {
            Ok(state) => state.enabled,
            Err(hide_memory::MemoryError::ProjectMissing) => false,
            Err(error) => return Err(store_failure(&identity.id, &error)),
        };
        if !enabled {
            return Err("memory_off".into());
        }
        let now = crate::runtime::memory::now_ms();
        match self.command.clone() {
            MemoryCommand::Show { number } => {
                let reader = self
                    .reader
                    .as_ref()
                    .map(|(provider, session)| (provider.as_str(), session.as_str()));
                let item = store
                    .show(&identity.id, number, reader, now)
                    .map_err(|error| store_failure(&identity.id, &error))?
                    .ok_or("memory_card_not_found")?;
                if item.lifecycle == MemoryLifecycle::Tombstoned {
                    return Err("memory_card_forgotten".into());
                }
                self.report(&identity.id, None);
                Ok(card(&item))
            }
            MemoryCommand::Forget { number } => {
                let item = store
                    .item_by_number(&identity.id, number)
                    .map_err(|error| store_failure(&identity.id, &error))?
                    .ok_or("memory_card_not_found")?;
                if item.lifecycle == MemoryLifecycle::Tombstoned {
                    return Err("memory_card_forgotten".into());
                }
                let batch_id = store
                    .forget(&identity.id, &item.id)
                    .map_err(|error| store_failure(&identity.id, &error))?;
                self.report(
                    &identity.id,
                    Some(MemoryNoticeSnapshot {
                        message: format!("Forgot {}", card_name(number)),
                        undo_batch_id: Some(batch_id),
                    }),
                );
                Ok(json!({"card": card_name(number), "outcome": "forgotten"}))
            }
            MemoryCommand::Undo { number } => {
                let item = store
                    .undo_card(&identity.id, number)
                    .map_err(|error| store_failure(&identity.id, &error))?
                    .ok_or("memory_nothing_to_undo")?;
                self.report(&identity.id, None);
                Ok(json!({
                    "card": card_name(number),
                    "outcome": if item.lifecycle == MemoryLifecycle::Tombstoned {
                        "removed"
                    } else {
                        "restored"
                    },
                }))
            }
            MemoryCommand::Search { words } => {
                let items = store
                    .search(&identity.id, &words, hide_memory::SEARCH_RESULT_LIMIT)
                    .map_err(|error| store_failure(&identity.id, &error))?;
                Ok(json!({
                    "cards": items.iter().map(|item| json!({
                        "card": card_name(item.number),
                        "title": item.title,
                        "kind": item.kind.as_str(),
                    })).collect::<Vec<_>>(),
                }))
            }
            MemoryCommand::Add { text } => {
                let router = router(self.settings.as_ref().ok_or("memory_ai_off")?);
                let group = [json!({
                    "offset": 0,
                    "role": "assistant",
                    "kind": "agent_note",
                    "text": text.trim(),
                })];
                let serialized = serde_json::to_string(&group).map_err(|error| {
                    crate::diagnostic!(json!({
                        "component": "memory", "kind": "command.add_unencodable",
                        "message": error.to_string(),
                    }));
                    "memory_unavailable".to_owned()
                })?;
                let mut generation = 0;
                let session_id = loop {
                    if generation == ADD_GENERATIONS {
                        crate::diagnostic!(json!({
                            "component": "memory", "kind": "command.add_generations_spent",
                            "project_id": identity.id,
                        }));
                        return Err("memory_unavailable".into());
                    }
                    let session_id = match generation {
                        0 => ADD_SESSION.to_owned(),
                        n => format!("{ADD_SESSION}-{n}"),
                    };
                    let source = self.add_source(&identity.id, &session_id, deadline);
                    let (_, batch_id) = batch_identity(&source, &serialized);
                    if !store
                        .has_batch(&batch_id)
                        .map_err(|error| store_failure(&identity.id, &error))?
                    {
                        break session_id;
                    }
                    let live = live_cards(&store, &identity.id, &batch_id)?;
                    if !live.is_empty() {
                        return Ok(json!({
                            "outcomes": live.into_iter().map(|number| json!({
                                "outcome": "already_added",
                                "card": card_name(number),
                            })).collect::<Vec<_>>(),
                            "batch_id": batch_id,
                        }));
                    }
                    generation += 1;
                };
                let written = write_cards(
                    &mut store,
                    self.sessions_node.as_ref(),
                    &router,
                    &self.add_source(&identity.id, &session_id, deadline),
                    &group,
                    &CancelToken::new(),
                    &mut |store| admit_call(store, WeeklyUsage::Unknown, false),
                )
                .map_err(|failure| add_failure(&identity.id, failure))?;
                let kept = written
                    .summary
                    .outcomes
                    .iter()
                    .filter_map(|outcome| match outcome {
                        CandidateOutcome::Created { number }
                        | CandidateOutcome::Conflict { number, .. } => Some(card_name(*number)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.report(
                    &identity.id,
                    (!kept.is_empty()).then(|| MemoryNoticeSnapshot {
                        message: format!("Added {}", kept.join(" ")),
                        undo_batch_id: Some(written.batch_id.clone()),
                    }),
                );
                Ok(json!({
                    "outcomes": written.summary.outcomes.iter().map(outcome).collect::<Vec<_>>(),
                    "batch_id": written.batch_id,
                }))
            }
        }
    }

    fn add_source<'a>(
        &'a self,
        project_id: &'a str,
        session_id: &'a str,
        deadline: Instant,
    ) -> BatchSource<'a> {
        BatchSource {
            project_id,
            provider: ADD_PROVIDER,
            session_id,
            checkout_path: &self.checkout_path,
            deadline: Some(deadline),
        }
    }

    fn report(&self, project_id: &str, notice: Option<MemoryNoticeSnapshot>) {
        if let Some(context) = &self.context {
            report_memory_change(context, project_id, notice);
        }
    }
}

/// The cards of a written batch that are still in use, active or waiting for
/// review.
fn live_cards(store: &MemoryStore, project_id: &str, batch_id: &str) -> Result<Vec<u64>, String> {
    let mut live = Vec::new();
    for number in store
        .batch_card_numbers(batch_id)
        .map_err(|error| store_failure(project_id, &error))?
    {
        let item = store
            .item_by_number(project_id, number)
            .map_err(|error| store_failure(project_id, &error))?;
        if item.is_some_and(|item| {
            matches!(
                item.lifecycle,
                MemoryLifecycle::Active | MemoryLifecycle::Conflicting
            )
        }) {
            live.push(number);
        }
    }
    Ok(live)
}

fn card(item: &MemoryItem) -> Value {
    json!({
        "card": card_name(item.number),
        "title": item.title,
        "kind": item.kind.as_str(),
        "state": match item.lifecycle {
            MemoryLifecycle::Conflicting => "needs_review",
            lifecycle => lifecycle.as_str(),
        },
        "body": item.body,
        "sources": item.source_count,
        "operator_sources": item.human_source_count,
        "conflicts_with": item.conflict_number.map(card_name),
        "cited_paths": item.cited_paths,
    })
}

fn outcome(outcome: &CandidateOutcome) -> Value {
    match outcome {
        CandidateOutcome::Created { number } => {
            json!({"outcome": "added", "card": card_name(*number)})
        }
        CandidateOutcome::Merged { number } => {
            json!({"outcome": "same_as", "card": card_name(*number)})
        }
        CandidateOutcome::Superseded { number } => {
            json!({"outcome": "replaced", "card": card_name(*number)})
        }
        CandidateOutcome::Conflict { number, existing } => json!({
            "outcome": "needs_review",
            "card": card_name(*number),
            "conflicts_with": card_name(*existing),
        }),
        CandidateOutcome::Discarded => json!({"outcome": "not_kept"}),
        CandidateOutcome::SecretRejected => json!({"outcome": "secret"}),
        CandidateOutcome::CapacityRejected => json!({"outcome": "full"}),
    }
}

fn store_failure(project_id: &str, error: &hide_memory::MemoryError) -> String {
    crate::diagnostic!(json!({
        "component": "memory",
        "kind": "command.store_failed",
        "project_id": project_id,
        "message": error.to_string(),
    }));
    "memory_unavailable".into()
}

/// A failed `add` wrote nothing: the batch is written only after the
/// provider answered and the answer was checked.
fn add_failure(project_id: &str, failure: AnalysisFailure) -> String {
    let code = match &failure {
        AnalysisFailure::Provider(AiError::NotAuthenticated) => "memory_ai_sign_in",
        AnalysisFailure::Provider(AiError::Timeout) => "memory_add_timeout",
        AnalysisFailure::Provider(AiError::UsageLimited { .. }) => "memory_ai_usage_limited",
        AnalysisFailure::Provider(_) => "memory_ai_failed",
        AnalysisFailure::Deferred(_) => "memory_daily_cap",
        AnalysisFailure::Cancelled => "memory_add_timeout",
        AnalysisFailure::Local(_) => "memory_unavailable",
    };
    crate::diagnostic!(json!({
        "component": "memory",
        "kind": "command.add_failed",
        "project_id": project_id,
        "reason": code,
        "message": match &failure {
            AnalysisFailure::Provider(error) => error.to_string(),
            AnalysisFailure::Local(message) => message.clone(),
            AnalysisFailure::Deferred(reason) => (*reason).to_owned(),
            AnalysisFailure::Cancelled => "cancelled".to_owned(),
        },
    }));
    code.into()
}

/// What the caller can do about a refusal.
pub fn next_action(code: &str) -> &'static str {
    match code {
        "memory_device_unsupported" => {
            "Run hide memory on the machine that runs Hide; a connected device has no Project Memory"
        }
        "memory_off" => "Turn Memory on for this Project in Hide's Project Sessions view",
        "memory_ai_off" => "Turn Hide AI on and choose an agent in Hide's Settings",
        "memory_ai_sign_in" => "Sign in to the agent Hide AI uses, then retry",
        "memory_ai_usage_limited" => "Retry after the agent's usage limit resets",
        "memory_daily_cap" => "Retry tomorrow; this machine reached its daily Memory calls",
        "memory_add_timeout" => "Nothing was saved; retry with shorter text",
        "memory_card_not_found" => "Find the card with hide memory search <words>",
        "memory_card_forgotten" => {
            "The card was forgotten; find another with hide memory search <words>"
        }
        "memory_text_too_long" => "Shorten the text to 4,000 characters and retry",
        "agent_pane_required" | "caller_context_changed" => {
            "Run the command from the agent's own pane and retry"
        }
        _ => "Retry; the reason is in Hide's diagnostic log",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hide_ai::{
        AiBackend, AiRequest, AiResponse, Availability, ModelCatalog, NoopLogSink, ProviderId,
        RouterConfig,
    };
    use hide_memory::{AnalysisBatch, Candidate, CandidateKind, CandidateRelation};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Answers each request with the next scripted value and counts calls.
    struct Scripted {
        answers: Mutex<Vec<Value>>,
        calls: AtomicUsize,
    }

    impl AiBackend for Scripted {
        fn id(&self) -> ProviderId {
            ProviderId::CLAUDE
        }
        fn availability(&self) -> Availability {
            Availability::Ready
        }
        fn models(&self) -> ModelCatalog {
            ModelCatalog::Offered(vec!["fixture".to_owned()])
        }
        fn execute(&self, _: &AiRequest, _: &CancelToken) -> Result<AiResponse, AiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let value = self.answers.lock().unwrap().pop();
            value
                .map(|value| AiResponse {
                    value,
                    usage: Default::default(),
                })
                .ok_or_else(|| AiError::Transient("no scripted answer".to_owned()))
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        root: String,
        database: PathBuf,
        project_id: String,
        node: Arc<dyn NodeLink>,
        backend: Arc<Scripted>,
    }

    impl Fixture {
        /// A Project with Memory on and card m1, which cites `src/lib.rs`.
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            std::fs::create_dir_all(root.join("src")).unwrap();
            let root = std::fs::canonicalize(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let node: Arc<dyn NodeLink> =
                Arc::new(hide_node::Local::new(Some(temp.path().join("home"))));
            let identity = crate::runtime::memory::project_identity(
                node.as_ref(),
                &crate::node::test_node(),
                &root,
            )
            .unwrap();
            let database = temp.path().join("memory.sqlite3");
            let mut store = MemoryStore::open(&database).unwrap();
            store
                .ensure_project(&identity.id, &identity.root, crate::node::TEST_NODE)
                .unwrap();
            store.set_enabled(&identity.id, true, true).unwrap();
            store
                .apply_candidates(
                    &AnalysisBatch {
                        id: "seed".to_owned(),
                        project_id: identity.id.clone(),
                        provider: "codex".to_owned(),
                        analysis_provider: "codex".to_owned(),
                        session_id: "seed".to_owned(),
                        content_hash: "seed".to_owned(),
                        created_at_unix_ms: 1,
                    },
                    &[Candidate {
                        text: "Run tests with nextest before a merge".to_owned(),
                        title: "Tests run with nextest".to_owned(),
                        aliases: vec!["테스트".to_owned()],
                        kind: CandidateKind::Rule,
                        confidence: 0.9,
                        salience: 0.9,
                        source_offsets: vec![1],
                        human_offsets: vec![1],
                        cited_paths: vec!["src/lib.rs".to_owned()],
                        relation: CandidateRelation::New,
                    }],
                )
                .unwrap();
            Self {
                _temp: temp,
                root,
                database,
                project_id: identity.id,
                node,
                backend: Arc::new(Scripted {
                    answers: Mutex::new(Vec::new()),
                    calls: AtomicUsize::new(0),
                }),
            }
        }

        fn run(&self, command: MemoryCommand, ai: bool) -> Result<Value, String> {
            let backend: Arc<dyn AiBackend> = self.backend.clone();
            PreparedMemory {
                command,
                node: crate::node::test_node(),
                sessions_node: Arc::clone(&self.node),
                database: self.database.clone(),
                checkout_path: self.root.clone(),
                reader: Some(("claude".to_owned(), "caller-session".to_owned())),
                settings: ai.then(AiSettings::default),
                context: None,
            }
            .run_with(|_| {
                AiRouter::new(
                    vec![backend],
                    RouterConfig {
                        priority: vec![ProviderId::CLAUDE],
                        max_transient_attempts: 1,
                        ..RouterConfig::default()
                    },
                    Arc::new(NoopLogSink),
                )
            })
        }

        fn store(&self) -> MemoryStore {
            MemoryStore::open(&self.database).unwrap()
        }

        fn calls(&self) -> usize {
            self.backend.calls.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn show_answers_the_card_with_its_files_and_counts_the_read_for_the_callers_session() {
        // B14, B27, D-18.
        let fixture = Fixture::new();

        let card = fixture
            .run(MemoryCommand::Show { number: 1 }, false)
            .unwrap();

        assert_eq!(card["card"], "m1");
        assert_eq!(card["body"], "Run tests with nextest before a merge");
        assert_eq!(card["state"], "active");
        assert_eq!(card["kind"], "rule");
        assert_eq!(card["sources"], 1);
        assert_eq!(card["cited_paths"], json!(["src/lib.rs"]));
        let item = fixture
            .store()
            .item_by_number(&fixture.project_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!((item.pull_count, item.provided_session_count), (1, 1));

        assert_eq!(
            fixture.run(MemoryCommand::Show { number: 9 }, false),
            Err("memory_card_not_found".to_owned())
        );
        let id = item.id;
        fixture.store().forget(&fixture.project_id, &id).unwrap();
        assert_eq!(
            fixture.run(MemoryCommand::Show { number: 1 }, false),
            Err("memory_card_forgotten".to_owned())
        );
    }

    #[test]
    fn search_lists_matching_cards_and_answers_none_without_failing() {
        // B15.
        let fixture = Fixture::new();
        let search = |words: &str| {
            fixture
                .run(
                    MemoryCommand::Search {
                        words: words.to_owned(),
                    },
                    false,
                )
                .unwrap()
        };

        assert_eq!(
            search("테스트를 돌려줘")["cards"],
            json!([{"card": "m1", "title": "Tests run with nextest", "kind": "rule"}])
        );
        assert_eq!(search("오늘 점심 메뉴")["cards"], json!([]));
    }

    #[test]
    fn add_writes_a_card_once_and_the_same_text_again_adds_nothing() {
        // B7, B8, D-11.
        let fixture = Fixture::new();
        fixture.backend.answers.lock().unwrap().push(json!({
            "candidates": [{
                "text": "Lint runs before tests.",
                "title": "Lint before tests",
                "aliases": ["lint"],
                "kind": "rule",
                "confidence": 0.8,
                "salience": 0.7,
                "source_offsets": [0],
                "cited_paths": [],
                "relation": "new",
                "target_id": null,
            }],
            "alias_backfill": [],
        }));
        let add = || {
            fixture.run(
                MemoryCommand::Add {
                    text: "Lint runs before tests".to_owned(),
                },
                true,
            )
        };

        let added = add().unwrap();
        assert_eq!(
            added["outcomes"],
            json!([{"outcome": "added", "card": "m2"}])
        );
        let card = fixture
            .store()
            .item_by_number(&fixture.project_id, 2)
            .unwrap()
            .unwrap();
        assert_eq!(
            card.human_source_count, 0,
            "add is never the operator's word"
        );

        assert_eq!(
            add().unwrap()["outcomes"],
            json!([{"outcome": "already_added", "card": "m2"}])
        );
        assert_eq!(fixture.calls(), 1);
        assert!(
            fixture
                .store()
                .item_by_number(&fixture.project_id, 3)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn forget_drops_a_card_undo_brings_it_back_and_a_forgotten_text_can_be_added_again() {
        // letter-4413: the operator tells the agent to forget a card.
        let fixture = Fixture::new();
        let run = |command| fixture.run(command, true);

        assert_eq!(
            run(MemoryCommand::Forget { number: 1 }).unwrap(),
            json!({"card": "m1", "outcome": "forgotten"})
        );
        assert_eq!(
            run(MemoryCommand::Forget { number: 1 }),
            Err("memory_card_forgotten".to_owned())
        );
        assert_eq!(
            run(MemoryCommand::Undo { number: 1 }).unwrap(),
            json!({"card": "m1", "outcome": "restored"})
        );
        assert_eq!(
            run(MemoryCommand::Show { number: 1 }).unwrap()["card"],
            "m1"
        );
        assert_eq!(
            run(MemoryCommand::Forget { number: 7 }),
            Err("memory_card_not_found".to_owned())
        );

        let answer = |title: &str| {
            json!({
                "candidates": [{
                    "text": "Lint runs before tests.",
                    "title": title,
                    "aliases": ["lint"],
                    "kind": "rule",
                    "confidence": 0.8,
                    "salience": 0.7,
                    "source_offsets": [0],
                    "cited_paths": [],
                    "relation": "new",
                    "target_id": null,
                }],
                "alias_backfill": [],
            })
        };
        let add = || {
            run(MemoryCommand::Add {
                text: "Lint runs before tests".to_owned(),
            })
        };
        fixture
            .backend
            .answers
            .lock()
            .unwrap()
            .push(answer("Lint first"));
        assert_eq!(
            add().unwrap()["outcomes"],
            json!([{"outcome": "added", "card": "m2"}])
        );
        run(MemoryCommand::Forget { number: 2 }).unwrap();

        // Every card of the first add is gone, so the same text is a new add.
        fixture
            .backend
            .answers
            .lock()
            .unwrap()
            .push(answer("Lint again"));
        assert_eq!(
            add().unwrap()["outcomes"],
            json!([{"outcome": "added", "card": "m3"}])
        );
        assert_eq!(
            add().unwrap()["outcomes"],
            json!([{"outcome": "already_added", "card": "m3"}])
        );
        assert_eq!(fixture.calls(), 2);

        // Undoing an add removes the card it made, and then there is nothing left.
        assert_eq!(
            run(MemoryCommand::Undo { number: 3 }).unwrap(),
            json!({"card": "m3", "outcome": "removed"})
        );
        assert_eq!(
            run(MemoryCommand::Undo { number: 3 }),
            Err("memory_nothing_to_undo".to_owned())
        );
    }

    #[test]
    fn a_failed_or_refused_add_writes_nothing() {
        // B8, B16.
        let fixture = Fixture::new();
        let add = |ai| {
            fixture.run(
                MemoryCommand::Add {
                    text: "Lint runs before tests".to_owned(),
                },
                ai,
            )
        };

        assert_eq!(add(false), Err("memory_ai_off".to_owned()));
        // The provider has no answer: nothing is written.
        assert_eq!(add(true), Err("memory_ai_failed".to_owned()));
        assert!(
            fixture
                .store()
                .item_by_number(&fixture.project_id, 2)
                .unwrap()
                .is_none()
        );

        fixture
            .store()
            .set_enabled(&fixture.project_id, false, true)
            .unwrap();
        for command in [
            MemoryCommand::Show { number: 1 },
            MemoryCommand::Search {
                words: "nextest".to_owned(),
            },
            MemoryCommand::Add {
                text: "Lint runs before tests".to_owned(),
            },
        ] {
            assert_eq!(fixture.run(command, true), Err("memory_off".to_owned()));
        }
        assert_eq!(fixture.calls(), 1);
    }

    #[test]
    fn a_parser_made_command_is_valid_and_an_oversized_add_is_not() {
        assert!(
            MemoryCommand::Add {
                text: "가".repeat(ADD_LIMIT_CHARS)
            }
            .valid()
        );
        assert!(
            !MemoryCommand::Add {
                text: "가".repeat(ADD_LIMIT_CHARS + 1)
            }
            .valid()
        );
        assert!(!MemoryCommand::Show { number: 0 }.valid());
        assert!(!MemoryCommand::Forget { number: 0 }.valid());
        assert_eq!(parse_card("m12"), Some(12));
        assert_eq!(parse_card("m0"), None);
    }
}
