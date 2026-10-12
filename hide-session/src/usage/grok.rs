//! Grok's PromptUsage includes its children. The native summary session_kind
//! prefix is the same discriminator Grok uses for hidden subagent sessions.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use super::records::{bounded, required};
use super::{Event, Refusal, Tokens, checked_file, key};
use crate::Agent;

#[derive(Deserialize)]
pub(super) struct Summary {
    info: Identity,
    session_kind: Option<String>,
    #[serde(skip)]
    pub read_bytes: u64,
}

#[derive(Deserialize)]
struct Identity {
    id: String,
    cwd: Option<String>,
}

impl Summary {
    pub fn is_child(&self) -> bool {
        self.session_kind
            .as_ref()
            .is_some_and(|k| k.starts_with("subagent"))
    }
}

pub(super) fn summary(home: &Path, path: &Path) -> Result<Summary, Refusal> {
    let path = path
        .parent()
        .ok_or(Refusal::OutsideRoot)?
        .join("summary.json");
    let file = checked_file(home, Agent::Grok, &path)?;
    if file.metadata().map_err(|_| Refusal::Unreadable)?.len() > 64 * 1024 {
        return Err(Refusal::Unreadable);
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::Unreadable)?;
    if bytes.len() > 64 * 1024 {
        return Err(Refusal::Unreadable);
    }
    let mut summary: Summary = serde_json::from_slice(&bytes).map_err(|_| Refusal::Unreadable)?;
    required(Some(summary.info.id.clone()), 128).map_err(|_| Refusal::Unreadable)?;
    bounded(summary.info.cwd.clone(), 4096).map_err(|_| Refusal::Unreadable)?;
    bounded(summary.session_kind.clone(), 128).map_err(|_| Refusal::Unreadable)?;
    summary.read_bytes = bytes.len() as u64;
    Ok(summary)
}

#[derive(Deserialize)]
struct Envelope {
    timestamp: u64,
    params: Params,
}
#[derive(Deserialize)]
struct Params {
    update: Update,
}
#[derive(Deserialize)]
struct Update {
    #[serde(rename = "sessionUpdate")]
    kind: String,
    prompt_id: Option<String>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    #[serde(default)]
    usage_is_incomplete: bool,
    #[serde(default)]
    cost_is_partial: bool,
    model_usage: BTreeMap<String, ModelUsage>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cached_read_tokens: u64,
    #[serde(default)]
    cache_creation_tokens: u64,
    reasoning_tokens: Option<u64>,
    cost_usd_ticks: Option<u64>,
    #[serde(default)]
    cost_is_partial: bool,
}

pub(super) fn parse(bytes: &[u8], summary: &Summary) -> Result<Vec<Event>, &'static str> {
    let record: Envelope = serde_json::from_slice(bytes).map_err(|_| "record_invalid")?;
    if record.params.update.kind != "turn_completed" {
        return Ok(Vec::new());
    }
    let prompt = required(record.params.update.prompt_id, 128)?;
    let usage = record.params.update.usage.ok_or("usage_missing")?;
    if usage.model_usage.is_empty() {
        return Err("model_missing");
    }
    if usage.model_usage.len() > 128 {
        return Err("model_capacity");
    }
    usage
        .model_usage
        .into_iter()
        .map(|(model, u)| {
            let model = required(Some(model), 128)?;
            let input = u
                .input_tokens
                .checked_sub(u.cached_read_tokens)
                .and_then(|n| n.checked_sub(u.cache_creation_tokens))
                .ok_or("tokens_inconsistent")?;
            if u.reasoning_tokens.is_some_and(|n| n > u.output_tokens)
                || u.input_tokens.checked_add(u.output_tokens).is_none()
            {
                return Err("tokens_inconsistent");
            }
            let incomplete =
                usage.usage_is_incomplete || usage.cost_is_partial || u.cost_is_partial;
            // A tick is 10^-10 USD, a microdollar is 10^4 ticks.
            let cost = if incomplete {
                None
            } else {
                u.cost_usd_ticks
                    .filter(|n| *n > 0)
                    .map(|n| n / 10_000 + u64::from(n % 10_000 >= 5000))
            };
            Ok(Event {
                key: key(&[&prompt, &model]),
                at_ms: record.timestamp,
                model,
                session: summary.info.id.clone(),
                cwd: summary.info.cwd.clone(),
                provider: None,
                incomplete,
                recorded_cost_micro_usd: cost,
                tokens: Tokens {
                    input,
                    output: u.output_tokens,
                    cache_read: u.cached_read_tokens,
                    cache_write: u.cache_creation_tokens,
                    cache_write_1h: 0,
                    reasoning: u.reasoning_tokens,
                },
            })
        })
        .collect()
}
