//! Wires the provider layer into this plugin: which backends exist and how
//! the router's events reach the plugin log. Prompt and schema stay with the
//! feature in `context_label`; nothing here knows what is being asked.

use crate::{StatePaths, append_log};
use hide_ai::{
    AiBackend, AiLogEvent, AiLogSink, AiRouter, AiSettings, Availability, ClaudeCliBackend,
    ClaudeConfig, CodexAppServerBackend, CodexConfig, ProviderId,
};
use std::path::Path;
use std::sync::Arc;

/// Every provider the plugin can route to, in the order the operator's saved
/// choice puts them.
pub fn router(settings: &AiSettings, paths: &StatePaths) -> Arc<AiRouter> {
    build(all_backends(settings), settings, paths)
}

/// One provider only, for a verification that must not fall back.
pub fn router_for(
    provider: ProviderId,
    settings: &AiSettings,
    paths: &StatePaths,
) -> Arc<AiRouter> {
    let backends = all_backends(settings)
        .into_iter()
        .filter(|backend| backend.id() == provider)
        .collect();
    build(backends, settings, paths)
}

/// The operator's saved choice, from the file `hide-ai` owns, and the reason
/// it could not be read when it could not.
///
/// This function writes nothing to the log, because the watcher calls it on
/// every scan: a line written here would repeat every `POLL_INTERVAL` for as
/// long as the file stays broken, and this plugin does not rotate its log. It
/// hands the reason back instead, and the caller decides when a reason is new.
pub fn settings(home: &Path) -> (AiSettings, Option<String>) {
    match hide_ai::settings::load(home) {
        Ok(settings) => (settings, None),
        Err(error) => (AiSettings::default(), Some(error.to_string())),
    }
}

/// The saved choice for a command that reads it once and exits.
///
/// A file that cannot be read is not taken as the defaults in silence; here
/// the reason can go straight to the log, because the process ends after it.
pub fn settings_once(home: &Path, paths: &StatePaths) -> AiSettings {
    let (settings, failure) = settings(home);
    if let Some(reason) = failure {
        let _ = append_log(paths, "ai_settings_unreadable", None, Some(&reason));
    }
    settings
}

/// `provider=codex;model=gpt-5.6-luna`: what the startup log records about the
/// choice in force.
pub fn settings_detail(settings: &AiSettings) -> String {
    format!(
        "provider={};model={}",
        settings.provider,
        settings.model(settings.provider)
    )
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

fn all_backends(settings: &AiSettings) -> Vec<Arc<dyn AiBackend>> {
    vec![
        Arc::new(CodexAppServerBackend::new(CodexConfig {
            model: settings.model(ProviderId::Codex).to_owned(),
            ..CodexConfig::default()
        })),
        Arc::new(ClaudeCliBackend::new(ClaudeConfig {
            model: settings.model(ProviderId::Claude).to_owned(),
            ..ClaudeConfig::default()
        })),
    ]
}

fn build(
    backends: Vec<Arc<dyn AiBackend>>,
    settings: &AiSettings,
    paths: &StatePaths,
) -> Arc<AiRouter> {
    Arc::new(AiRouter::new(
        backends,
        // Only the priority moves with the choice; every retry, cooldown and
        // stickiness constant is still the router's own.
        settings.router_config(),
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
