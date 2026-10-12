//! The two checks a card gets after it was written (PRD memory-card-quality
//! D-07, D-11): a card whose evidence moved is read against the repository
//! again, and a card about to be offered for the repository's documents is
//! read against the documents agents already have.
//!
//! Both are provider calls on the analysis worker under the analysis call
//! cap and weekly deferral (D-08): a deferral leaves the card as it was, and
//! a card is asked about at most once a day (B10). Nothing here runs under
//! the runtime lock; the files come from the node, the operator's own
//! instruction files from the account this process runs as.

use std::collections::BTreeSet;
use std::path::PathBuf;

use hide_ai::{AiError, AiRouter, CancelToken};
use hide_memory::{
    DOCUMENT_EXCERPT_BYTES, DOCUMENT_LIMIT, Excerpt, FILE_EXCERPT_BYTES, GLOBAL_EXCERPT_BYTES,
    GLOBAL_LIMIT, MemoryStore, PROMOTION_CHECKS_PER_PASS, PendingCard, PromoteCheck,
    PromoteCheckCard, Promotion, PromotionState, REVERIFY_PER_PASS, Verification, cited_files,
    parse_promote_check, parse_verification, pointer_target, promote_check_request, verify_request,
    written_names,
};
use serde_json::json;

use super::memory::{digest, now_ms};
use super::memory_analysis::AnalysisFailure;
use super::memory_evidence::CheckoutFacts;
use crate::node_access::{NodeLink, read_bytes};

/// What a Project's checks changed.
#[derive(Default)]
pub(super) struct Checked {
    pub(super) changed: usize,
}

type Admit<'a> = &'a mut dyn FnMut(&MemoryStore) -> Result<(), AnalysisFailure>;

fn provider_failure(error: AiError) -> AnalysisFailure {
    match error {
        AiError::Cancelled => AnalysisFailure::Cancelled,
        error => AnalysisFailure::Provider(error),
    }
}

/// Reads at most `limit` bytes of a checkout's file through the node.
fn excerpt(node: &dyn NodeLink, root: &str, relative: &str, limit: usize) -> Option<Excerpt> {
    let range = read_bytes(node, root, relative, 0, limit as u64).ok()?;
    let bytes = range.bytes().ok()?;
    Some(Excerpt {
        path: relative.to_owned(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        truncated: range.total > bytes.len() as u64,
    })
}

/// Asks about the Project's unverified cards one by one, at most
/// [`REVERIFY_PER_PASS`] a pass, and applies each answer: a card that holds
/// goes back to the index with its evidence taken again, an outdated one gets
/// its corrected text as a new revision its Undo takes back, and one that no
/// longer applies is forgotten, which Undo takes back too (B7-B9).
#[allow(clippy::too_many_arguments)] // one Project's store, node, provider and the pass's limits
pub(super) fn reverify_pending(
    store: &mut MemoryStore,
    node: &dyn NodeLink,
    facts: &mut CheckoutFacts<'_>,
    router: &AiRouter,
    project_id: &str,
    root: &str,
    cancel: &CancelToken,
    admit: Admit<'_>,
) -> Result<Checked, AnalysisFailure> {
    let local = |error: String| AnalysisFailure::Local(error);
    let mut checked = Checked::default();
    let cards = store
        .pending_cards(project_id, now_ms(), REVERIFY_PER_PASS)
        .map_err(|error| local(error.to_string()))?;
    for card in cards {
        if cancel.is_cancelled() {
            return Err(AnalysisFailure::Cancelled);
        }
        // The deferral comes first, so a held-back card keeps its day.
        admit(store)?;
        if !store
            .begin_verification(&card.item_id, now_ms())
            .map_err(|error| local(error.to_string()))?
        {
            continue;
        }
        let answers = facts.facts(&card.evidence);
        let files = cited_files(&card.evidence)
            .into_iter()
            .filter_map(|path| excerpt(node, root, &path, FILE_EXCERPT_BYTES))
            .collect::<Vec<_>>();
        let request = verify_request(
            digest(&[
                "memory-verify",
                project_id,
                &card.item_id,
                &card.revision.to_string(),
                &now_ms().to_string(),
            ]),
            project_id,
            &card,
            &answers,
            &files,
        )
        .map_err(|error| local(error.to_string()))?;
        let answer = router.execute(&request, cancel).map_err(provider_failure)?;
        let verdict = parse_verification(&answer.value).map_err(|error| {
            AnalysisFailure::Provider(AiError::InvalidOutput(error.to_string()))
        })?;
        let applied = match verdict {
            Verification::Valid => {
                let evidence = facts.gather(&card.body, &card.cited_paths);
                store.confirm_card(&card.item_id, card.revision, &evidence, now_ms())
            }
            Verification::Outdated(correction) => {
                revise(store, facts, project_id, &card, correction, root, node)
            }
            Verification::Gone => store
                .forget(project_id, &card.item_id)
                .map(|_| true),
        }
        .map_err(|error| local(error.to_string()))?;
        if applied {
            checked.changed += 1;
        }
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": "verification.applied",
            "project_id": project_id,
            "card": card.number,
            "applied": applied,
        }));
    }
    Ok(checked)
}

fn revise(
    store: &mut MemoryStore,
    facts: &mut CheckoutFacts<'_>,
    project_id: &str,
    card: &PendingCard,
    correction: hide_memory::Correction,
    root: &str,
    node: &dyn NodeLink,
) -> Result<bool, hide_memory::MemoryError> {
    let cited = super::memory_analysis::paths_inside(node, root, &card.cited_paths);
    let cited = card
        .cited_paths
        .iter()
        .filter(|path| cited.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    let evidence = facts.gather(&correction.text, &cited);
    let title = if correction.title.is_empty() {
        card.title.as_str()
    } else {
        correction.title.as_str()
    };
    let aliases = if correction.aliases.is_empty() {
        card.aliases.as_slice()
    } else {
        correction.aliases.as_slice()
    };
    store.revise_card(
        project_id,
        &card.item_id,
        card.revision,
        &correction.text,
        title,
        aliases,
        &cited,
        &evidence,
        now_ms(),
    )
}

/// The documents agents read at the start of work here: `AGENTS.md`,
/// `CLAUDE.md` when it says something else, then the repository documents
/// they point to, at most [`DOCUMENT_LIMIT`] and each cut to a bound.
fn repository_documents(
    node: &dyn NodeLink,
    facts: &CheckoutFacts<'_>,
    root: &str,
) -> Vec<Excerpt> {
    let mut documents: Vec<Excerpt> = Vec::new();
    for name in ["AGENTS.md", "CLAUDE.md"] {
        if let Some(found) = excerpt(node, root, name, DOCUMENT_EXCERPT_BYTES)
            && !documents.iter().any(|known| known.text == found.text)
        {
            documents.push(found);
        }
    }
    let pointed = documents
        .iter()
        .flat_map(|document| written_names(&document.text).paths)
        .filter(|path| path.ends_with(".md"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for path in facts.existing(&pointed) {
        if documents.len() >= DOCUMENT_LIMIT {
            break;
        }
        if let Some(found) = excerpt(node, root, &path, DOCUMENT_EXCERPT_BYTES)
            && !documents.iter().any(|known| known.path == found.path)
        {
            documents.push(found);
        }
    }
    documents.truncate(DOCUMENT_LIMIT);
    documents
}

/// The operator's own instruction files, which agents read in every
/// repository. Read from the account this process runs as, the machine whose
/// Memory this is; a file that is not there is skipped, and a link to a file
/// already read is not read twice.
fn global_documents() -> Vec<Excerpt> {
    let Some(home) = std::env::var_os(crate::environment::HOME_KEY).map(PathBuf::from) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut documents = Vec::new();
    for relative in [
        ".claude/CLAUDE.md",
        ".codex/AGENTS.md",
        ".config/agents/AGENTS.md",
    ] {
        let path = home.join(relative);
        let Ok(real) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !seen.insert(real.clone()) || documents.len() >= GLOBAL_LIMIT {
            continue;
        }
        let Ok(file) = std::fs::File::open(&real) else {
            continue;
        };
        use std::io::Read;
        let mut bytes = Vec::new();
        if file
            .take(GLOBAL_EXCERPT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        let truncated = bytes.len() > GLOBAL_EXCERPT_BYTES;
        bytes.truncate(GLOBAL_EXCERPT_BYTES);
        documents.push(Excerpt {
            // The name an agent knows it by, never the account's folder.
            path: format!("~/{relative}"),
            text: String::from_utf8_lossy(&bytes).into_owned(),
            truncated,
        });
    }
    documents
}

/// Reads each card about to be suggested against the evidence now and the
/// documents agents already read, at most [`PROMOTION_CHECKS_PER_PASS`] a
/// pass (D-11). A card to propose is marked checked at its current text so
/// the tab asks about it; an outdated one is corrected like a re-verification
/// (B16); one a repository document already says becomes a pointer at that
/// file with no pull request (B15), and one the operator's global files
/// already say is forgotten, which Undo takes back.
#[allow(clippy::too_many_arguments)] // one Project's store, node, provider and the pass's limits
pub(super) fn check_before_suggestion(
    store: &mut MemoryStore,
    node: &dyn NodeLink,
    facts: &mut CheckoutFacts<'_>,
    router: &AiRouter,
    project_id: &str,
    root: &str,
    cancel: &CancelToken,
    admit: Admit<'_>,
) -> Result<Checked, AnalysisFailure> {
    let local = |error: String| AnalysisFailure::Local(error);
    let mut checked = Checked::default();
    let due = store
        .promotion_unchecked(project_id, now_ms(), PROMOTION_CHECKS_PER_PASS)
        .map_err(|error| local(error.to_string()))?;
    if due.is_empty() {
        return Ok(checked);
    }
    let repository = repository_documents(node, facts, root);
    let global = global_documents();
    let known = repository
        .iter()
        .chain(&global)
        .map(|document| document.path.clone())
        .collect::<Vec<_>>();
    for suggestion in due {
        if cancel.is_cancelled() {
            return Err(AnalysisFailure::Cancelled);
        }
        let Some(item) = store
            .item_by_number(project_id, suggestion.number)
            .map_err(|error| local(error.to_string()))?
        else {
            continue;
        };
        let detail = store
            .detail(project_id, &item.id)
            .map_err(|error| local(error.to_string()))?;
        let answers = facts.facts(&detail.evidence);
        admit(store)?;
        // The day's one question about the card, so a card the check keeps
        // correcting costs one call a day (B10).
        if !store
            .begin_verification(&item.id, now_ms())
            .map_err(|error| local(error.to_string()))?
        {
            continue;
        }
        let request = promote_check_request(
            digest(&[
                "memory-promote-check",
                project_id,
                &item.id,
                &item.revision.to_string(),
            ]),
            project_id,
            &PromoteCheckCard {
                number: item.number,
                kind: item.kind,
                title: &item.title,
                body: &item.body,
                evidence: &detail.evidence,
                facts: &answers,
            },
            &repository,
            &global,
        )
        .map_err(|error| local(error.to_string()))?;
        let answer = router.execute(&request, cancel).map_err(provider_failure)?;
        let verdict = parse_promote_check(&answer.value, &known).map_err(|error| {
            AnalysisFailure::Provider(AiError::InvalidOutput(error.to_string()))
        })?;
        match verdict {
            PromoteCheck::Propose => {
                store
                    .mark_promotion_checked(project_id, item.number, item.revision)
                    .map_err(|error| local(error.to_string()))?;
            }
            PromoteCheck::Outdated(correction) => {
                let pending = PendingCard {
                    item_id: item.id.clone(),
                    number: item.number,
                    revision: item.revision,
                    kind: item.kind,
                    title: item.title.clone(),
                    body: item.body.clone(),
                    aliases: item.aliases.clone(),
                    cited_paths: item.cited_paths.clone(),
                    evidence: detail.evidence.clone(),
                    outdated_reason: None,
                };
                if revise(store, facts, project_id, &pending, correction, root, node)
                    .map_err(|error| local(error.to_string()))?
                {
                    checked.changed += 1;
                }
            }
            PromoteCheck::InDocs { document } => {
                if pointer_target(&document) && !document.starts_with("~/") {
                    point_at(store, project_id, &item, &document)
                        .map_err(|error| local(error.to_string()))?;
                } else {
                    store
                        .forget(project_id, &item.id)
                        .map_err(|error| local(error.to_string()))?;
                }
                checked.changed += 1;
            }
        }
        crate::diagnostic!(json!({
            "component": "memory",
            "kind": "promotion.checked",
            "project_id": project_id,
            "card": item.number,
        }));
    }
    Ok(checked)
}

/// The card becomes a line pointing at the document that already says it:
/// the merged state without a pull request, left out of every prompt.
fn point_at(
    store: &mut MemoryStore,
    project_id: &str,
    item: &hide_memory::MemoryItem,
    document: &str,
) -> Result<(), hide_memory::MemoryError> {
    let attempt = store
        .promotions(project_id)?
        .into_iter()
        .find(|(number, _)| *number == item.number)
        .map_or(0, |(_, promotion)| promotion.attempt);
    let now = now_ms();
    store.set_promotion(
        project_id,
        item.number,
        &Promotion {
            state: PromotionState::Merged,
            revision: item.revision,
            attempt,
            agent_id: None,
            branch: None,
            pr_number: None,
            target_path: Some(document.to_owned()),
            started_at_unix_ms: now,
            updated_at_unix_ms: now,
        },
    )
}
