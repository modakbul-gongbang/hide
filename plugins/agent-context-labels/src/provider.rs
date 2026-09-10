//! Wires the provider layer into this plugin: which backends exist and how
//! the router's events reach the plugin log. Prompt and schema stay with the
//! feature in `context_label`; nothing here knows what is being asked.

use crate::{StatePaths, append_log};
use hide_ai::{
    AiBackend, AiLogEvent, AiLogSink, AiRouter, Availability, ClaudeCliBackend, ClaudeConfig,
    CodexAppServerBackend, CodexConfig, ProviderId, RouterConfig,
};
use std::sync::Arc;

/// Every provider the plugin can route to, in the configured priority order.
pub fn router(paths: &StatePaths) -> Arc<AiRouter> {
    build(all_backends(), paths)
}

/// One provider only, for a verification that must not fall back.
pub fn router_for(provider: ProviderId, paths: &StatePaths) -> Arc<AiRouter> {
    let backends = all_backends()
        .into_iter()
        .filter(|backend| backend.id() == provider)
        .collect();
    build(backends, paths)
}

/// `codex=ready;claude=needs_login`: the shape the startup log and
/// the verification command both print. A reason is a diagnostic class from
/// the provider layer, never content.
pub fn availability_detail(states: &[(ProviderId, Availability)]) -> String {
    states
        .iter()
        .map(|(provider, state)| match state {
            Availability::Unavailable { reason } | Availability::Unsupported { reason } => {
                format!("{provider}={}:{reason}", state.class())
            }
            _ => format!("{provider}={}", state.class()),
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn all_backends() -> Vec<Arc<dyn AiBackend>> {
    vec![
        Arc::new(CodexAppServerBackend::new(CodexConfig::default())),
        Arc::new(ClaudeCliBackend::new(ClaudeConfig::default())),
    ]
}

fn build(backends: Vec<Arc<dyn AiBackend>>, paths: &StatePaths) -> Arc<AiRouter> {
    Arc::new(AiRouter::new(
        backends,
        RouterConfig::default(),
        Arc::new(EventLog {
            paths: paths.clone(),
        }),
    ))
}

/// Lands router events in `events.jsonl` beside the watcher's own. The
/// detail carries identifiers and classes only, matching the router's rule.
struct EventLog {
    paths: StatePaths,
}

impl AiLogSink for EventLog {
    fn log(&self, event: AiLogEvent) {
        let mut parts = Vec::new();
        if let Some(request_id) = &event.request_id {
            parts.push(format!("request_id={request_id}"));
        }
        if let Some(feature) = event.feature_id {
            parts.push(format!("feature={feature}"));
        }
        if let Some(provider) = event.provider {
            parts.push(format!("provider={provider}"));
        }
        if let Some(outcome) = event.outcome_class {
            parts.push(format!("outcome={outcome}"));
        }
        if let Some(attempt) = event.attempt {
            parts.push(format!("attempt={attempt}"));
        }
        if let Some(duration_ms) = event.duration_ms {
            parts.push(format!("duration_ms={duration_ms}"));
        }
        if let Some(input_chars) = event.input_chars {
            parts.push(format!("input_chars={input_chars}"));
        }
        if let Some(output_tokens) = event.output_tokens {
            parts.push(format!("output_tokens={output_tokens}"));
        }
        if let Some(schema_version) = event.schema_version {
            parts.push(format!("schema={schema_version}"));
        }
        if let Some(detail) = &event.detail {
            parts.push(detail.clone());
        }
        // The log is the diagnostic record, not the outcome: a line that
        // cannot be written must not fail the request it describes.
        let _ = append_log(&self.paths, event.event, None, Some(&parts.join(";")));
    }
}
