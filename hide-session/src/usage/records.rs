use serde::{Deserialize, Serialize};

use super::{Event, Tokens, key};
use crate::Agent;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Context {
    session: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    origin: Option<String>,
}

// Serde skips every unknown field with IgnoredAny. In particular none of these
// envelopes can deserialize a conversation body or a tool argument.
#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type")]
    kind: String,
    timestamp: Option<String>,
    id: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    cwd: Option<String>,
    message: Option<Message>,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    role: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    forked_from_id: Option<String>,
    info: Option<Info>,
}

#[derive(Deserialize)]
struct Info {
    total_token_usage: Option<Usage>,
    last_token_usage: Option<Usage>,
}

#[derive(Default, Deserialize, Serialize)]
struct Usage {
    #[serde(alias = "input_tokens")]
    input: Option<u64>,
    #[serde(alias = "output_tokens")]
    output: Option<u64>,
    #[serde(
        alias = "cache_read_input_tokens",
        alias = "cached_input_tokens",
        alias = "cacheRead"
    )]
    cache_read: Option<u64>,
    #[serde(
        alias = "cache_creation_input_tokens",
        alias = "cache_write_input_tokens",
        alias = "cacheWrite"
    )]
    cache_write: Option<u64>,
    cache_creation: Option<CacheCreation>,
    #[serde(alias = "reasoning_output_tokens")]
    reasoning: Option<u64>,
    cost: Option<Cost>,
}

#[derive(Deserialize, Serialize)]
struct CacheCreation {
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Deserialize, Serialize)]
struct Cost {
    total: Option<f64>,
}

fn bounded(value: Option<String>, limit: usize) -> Result<Option<String>, &'static str> {
    match value {
        Some(value)
            if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) =>
        {
            Err("metadata_invalid")
        }
        value => Ok(value),
    }
}

fn required(value: Option<String>, limit: usize) -> Result<String, &'static str> {
    bounded(value, limit)?.ok_or("identity_missing")
}

fn tokens(usage: &Usage, includes_cache: bool) -> Result<Tokens, &'static str> {
    let input = usage.input.ok_or("tokens_missing")?;
    let cache_read = usage.cache_read.unwrap_or(0);
    let cache_write = usage.cache_write.unwrap_or(0);
    let input = if includes_cache {
        input
            .checked_sub(cache_read)
            .and_then(|n| n.checked_sub(cache_write))
            .ok_or("tokens_inconsistent")?
    } else {
        input
    };
    let output = usage.output.ok_or("tokens_missing")?;
    let cache_write_1h = usage
        .cache_creation
        .as_ref()
        .and_then(|c| c.ephemeral_1h_input_tokens)
        .unwrap_or(0);
    if cache_write_1h > cache_write
        || usage.reasoning.is_some_and(|n| n > output)
        || input
            .checked_add(output)
            .and_then(|n| n.checked_add(cache_read))
            .and_then(|n| n.checked_add(cache_write))
            .is_none()
    {
        return Err("tokens_inconsistent");
    }
    Ok(Tokens {
        input,
        output,
        cache_read,
        cache_write,
        cache_write_1h,
        reasoning: usage.reasoning,
    })
}

fn recorded_cost(usage: &Usage) -> Result<Option<u64>, &'static str> {
    match usage.cost.as_ref().and_then(|c| c.total) {
        Some(cost) if !cost.is_finite() || cost < 0. || cost * 1_000_000. >= u64::MAX as f64 => {
            Err("cost_invalid")
        }
        // Providers sometimes record zero when no billable price was known.
        Some(cost) if cost > 0. => Ok(Some((cost * 1_000_000.).round() as u64)),
        _ => Ok(None),
    }
}

pub(super) fn parse(
    agent: Agent,
    bytes: &[u8],
    context: &mut Context,
) -> Result<Option<Event>, &'static str> {
    let record: Record = serde_json::from_slice(bytes).map_err(|_| "record_invalid")?;
    let at = || {
        record
            .timestamp
            .as_deref()
            .and_then(crate::parse_rfc3339)
            .and_then(|s| s.checked_mul(1000))
            .ok_or("timestamp_invalid")
    };
    match agent {
        Agent::Claude => {
            if record.kind != "assistant" {
                return Ok(None);
            }
            let Some(message) = record.message else {
                return Err("usage_missing");
            };
            if message.model.as_deref() == Some("<synthetic>") {
                return Ok(None);
            }
            let Some(usage) = message.usage else {
                return Err("usage_missing");
            };
            let id = required(message.id, 128)?;
            let request = bounded(record.request_id, 128)?;
            let session = required(record.session_id, 128)?;
            let at_ms = record
                .timestamp
                .as_deref()
                .and_then(crate::parse_rfc3339)
                .and_then(|s| s.checked_mul(1000))
                .ok_or("timestamp_invalid")?;
            Ok(Some(Event {
                key: key(&[&id, request.as_deref().unwrap_or("")]),
                at_ms,
                model: required(message.model, 128)?,
                tokens: tokens(&usage, false)?,
                recorded_cost_micro_usd: None,
                session,
                cwd: bounded(record.cwd, 4096)?,
                provider: None,
            }))
        }
        Agent::Pi | Agent::Omp => {
            if record.kind == "session" {
                context.session = Some(required(record.id, 128)?);
                context.cwd = bounded(record.cwd, 4096)?;
                return Ok(None);
            }
            if record.kind != "message" {
                return Ok(None);
            }
            let Some(message) = record.message else {
                return Err("record_invalid");
            };
            if message.role.as_deref() != Some("assistant") {
                return Ok(None);
            }
            let usage = message.usage.ok_or("usage_missing")?;
            let session = context.session.clone().ok_or("identity_missing")?;
            let id = required(record.id, 128)?;
            let at_ms = record
                .timestamp
                .as_deref()
                .and_then(crate::parse_rfc3339)
                .and_then(|s| s.checked_mul(1000))
                .ok_or("timestamp_invalid")?;
            Ok(Some(Event {
                key: key(&[&session, &id]),
                at_ms,
                model: required(message.model, 128)?,
                tokens: tokens(&usage, false)?,
                recorded_cost_micro_usd: recorded_cost(&usage)?,
                session,
                cwd: context.cwd.clone(),
                provider: bounded(message.provider, 128)?,
            }))
        }
        Agent::Codex => {
            if record.kind == "session_meta" {
                let payload = record.payload.ok_or("record_invalid")?;
                context.session = Some(required(payload.id, 128)?);
                context.origin = bounded(payload.forked_from_id, 128)?;
                context.cwd = bounded(payload.cwd, 4096)?;
                return Ok(None);
            }
            if record.kind == "turn_context" {
                context.model = Some(required(record.payload.and_then(|p| p.model), 128)?);
                return Ok(None);
            }
            if record.kind != "event_msg" {
                return Ok(None);
            }
            let payload = record.payload.ok_or("record_invalid")?;
            if payload.kind.as_deref() != Some("token_count") {
                return Ok(None);
            }
            // A rate-limit-only observation has no token event.
            let Some(info) = payload.info else {
                return Ok(None);
            };
            let usage = info.last_token_usage.ok_or("usage_missing")?;
            let total = info.total_token_usage.ok_or("dedup_missing")?;
            let session = context.session.clone().ok_or("identity_missing")?;
            let model = context.model.clone().ok_or("model_missing")?;
            let total = tokens(&total, true)?;
            let fingerprint = serde_json::to_string(&total).map_err(|_| "dedup_invalid")?;
            // Native origin plus cumulative counters makes a replay of an
            // archived/copied prefix converge without adding its last usage.
            let origin = context.origin.as_deref().unwrap_or(&session);
            let at_ms = at()?;
            Ok(Some(Event {
                key: key(&[origin, &model, &fingerprint]),
                at_ms,
                model,
                tokens: tokens(&usage, true)?,
                recorded_cost_micro_usd: None,
                session,
                cwd: context.cwd.clone(),
                provider: None,
            }))
        }
        _ => Err("unsupported"),
    }
}
