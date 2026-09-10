//! The operator's provider and model choice, and the file it lives in.
//!
//! Two processes read this: Hide writes it from Settings, and the plugin that
//! consumes the router reads it back. Both link this crate, so the path, the
//! schema and the priority rule live here once rather than being agreed twice.
//!
//! The file is a sibling of Hide's own `state.json`, not a plugin's
//! configuration directory: a path keyed to a plugin id would tie Hide to
//! whichever plugin happened to be first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::router::RouterConfig;
use crate::{AiError, Availability, ProviderId};

/// Every provider a choice can name, in the order they are offered.
///
/// The router's own default priority is this order; a choice reorders it
/// rather than replacing it, which is what keeps failover available.
pub const PROVIDERS: &[ProviderId] = &[ProviderId::Codex, ProviderId::Claude];

/// What the operator chose. A field that is absent from the file takes the
/// default rather than making the whole file unreadable, and a field this
/// version does not know is ignored, so an older Hide reads a newer file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AiSettings {
    #[serde(default = "default_provider")]
    pub provider: ProviderId,
    /// The model each provider is asked for. A provider with no entry is
    /// asked for its own `DEFAULT_MODEL`.
    #[serde(default)]
    pub models: BTreeMap<ProviderId, String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            models: PROVIDERS
                .iter()
                .map(|provider| (*provider, default_model(*provider).to_owned()))
                .collect(),
        }
    }
}

fn default_provider() -> ProviderId {
    ProviderId::Codex
}

/// The model a provider is asked for when the operator has not chosen one.
/// It is the backend's own constant, never a second list.
pub fn default_model(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => crate::codex::DEFAULT_MODEL,
        ProviderId::Claude => crate::claude::DEFAULT_MODEL,
    }
}

impl AiSettings {
    /// The model this provider will be asked for, whether or not the file
    /// named one.
    pub fn model(&self, provider: ProviderId) -> &str {
        self.models
            .get(&provider)
            .map(String::as_str)
            .unwrap_or_else(|| default_model(provider))
    }

    pub fn set_model(&mut self, provider: ProviderId, model: impl Into<String>) {
        self.models.insert(provider, model.into());
    }

    /// The router policy this choice produces: the chosen provider first,
    /// every other provider behind it in the offered order.
    ///
    /// Nothing else about the policy moves. A chosen provider that cannot
    /// answer still steps aside to the next one, and that move is still
    /// sticky, because the retry and cooldown constants are the defaults.
    pub fn router_config(&self) -> RouterConfig {
        let mut priority = vec![self.provider];
        priority.extend(
            PROVIDERS
                .iter()
                .copied()
                .filter(|provider| *provider != self.provider),
        );
        RouterConfig {
            priority,
            ..RouterConfig::default()
        }
    }

    /// The provider a first run should start on: the one that can answer when
    /// exactly one can, and the offered order otherwise.
    ///
    /// This is only consulted when no file exists. Once the operator has
    /// chosen, their choice stands even while it is degraded, because the
    /// router already reports the degradation and returns to the choice on
    /// its own.
    pub fn provider_for_first_run(availability: &[(ProviderId, Availability)]) -> ProviderId {
        let mut ready = availability
            .iter()
            .filter(|(_, state)| state.is_ready())
            .map(|(provider, _)| *provider);
        match (ready.next(), ready.next()) {
            (Some(only), None) => only,
            _ => default_provider(),
        }
    }
}

/// Where the settings file lives, given a home directory.
pub fn settings_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("hide")
        .join("ai.json")
}

/// Reads the operator's choice.
///
/// A file that is not there is not a failure: nobody has chosen yet, and the
/// defaults are the answer. Anything else is reported, because a file that
/// exists and cannot be read is a fact the caller has to state before it
/// falls back; see `AI_PROVIDERS.md`.
pub fn load(home: &Path) -> Result<AiSettings, AiError> {
    let path = settings_path(home);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AiSettings::default());
        }
        Err(error) => {
            return Err(AiError::Internal(format!(
                "ai_settings_unreadable:{}",
                error.kind()
            )));
        }
    };
    serde_json::from_slice(&bytes).map_err(|error| {
        AiError::InvalidOutput(format!("ai_settings_malformed:line={}", error.line()))
    })
}

/// Writes the operator's choice, creating the directory if it is not there.
///
/// The write goes to a temporary file in the same directory and is renamed
/// over the target, so a reader never sees a half-written file.
pub fn save(home: &Path, settings: &AiSettings) -> Result<(), AiError> {
    let path = settings_path(home);
    let parent = path
        .parent()
        .ok_or_else(|| AiError::Internal("ai_settings_path_without_parent".to_owned()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| AiError::Internal(format!("ai_settings_dir:{}", error.kind())))?;
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|_| AiError::Internal("ai_settings_not_encodable".to_owned()))?;
    let temporary = parent.join(format!("ai.json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, &bytes)
        .map_err(|error| AiError::Internal(format!("ai_settings_write:{}", error.kind())))?;
    std::fs::rename(&temporary, &path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        AiError::Internal(format!("ai_settings_rename:{}", error.kind()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hide-ai-settings-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("a temporary home is creatable");
        root
    }

    fn write_settings(home: &Path, body: &str) {
        let path = settings_path(home);
        std::fs::create_dir_all(path.parent().expect("the settings path has a parent"))
            .expect("the settings directory is creatable");
        std::fs::write(path, body).expect("the settings file is writable");
    }

    #[test]
    fn the_settings_file_sits_beside_hides_own_state() {
        let path = settings_path(Path::new("/Users/example"));
        assert_eq!(
            path,
            PathBuf::from("/Users/example/Library/Application Support/hide/ai.json")
        );
    }

    #[test]
    fn no_file_means_the_defaults_rather_than_a_failure() {
        let home = home("absent");
        let settings = load(&home).expect("a missing file is not a failure");
        assert_eq!(settings, AiSettings::default());
        assert_eq!(settings.provider, ProviderId::Codex);
        assert_eq!(
            settings.model(ProviderId::Codex),
            crate::codex::DEFAULT_MODEL
        );
        assert_eq!(
            settings.model(ProviderId::Claude),
            crate::claude::DEFAULT_MODEL
        );
        std::fs::remove_dir_all(home).expect("the temporary home is removable");
    }

    #[test]
    fn a_missing_field_takes_its_default_and_an_unknown_field_is_ignored() {
        let home = home("partial");
        write_settings(
            &home,
            r#"{"provider":"claude","spending_cap":{"weekly":10},"schema_version":9}"#,
        );
        let settings = load(&home).expect("an unknown field does not make the file unreadable");
        assert_eq!(settings.provider, ProviderId::Claude);
        assert_eq!(
            settings.model(ProviderId::Claude),
            crate::claude::DEFAULT_MODEL,
            "a provider the file named no model for takes the backend's own default"
        );
        std::fs::remove_dir_all(home).expect("the temporary home is removable");
    }

    #[test]
    fn malformed_json_is_reported_rather_than_read_as_the_defaults() {
        let home = home("malformed");
        write_settings(&home, "{\"provider\": ");
        let error = load(&home).expect_err("a file that cannot be parsed is a failure");
        assert_eq!(error.class(), "invalid_output");
        assert!(
            error.to_string().contains("ai_settings_malformed"),
            "the failure names what could not be read: {error}"
        );
        std::fs::remove_dir_all(home).expect("the temporary home is removable");
    }

    #[test]
    fn a_saved_choice_is_what_the_next_load_returns() {
        let home = home("roundtrip");
        let mut settings = AiSettings {
            provider: ProviderId::Claude,
            ..AiSettings::default()
        };
        settings.set_model(ProviderId::Claude, "sonnet");
        save(&home, &settings).expect("the settings file is writable");
        assert_eq!(load(&home).expect("the saved file is readable"), settings);
        assert!(
            !settings_path(&home)
                .parent()
                .expect("the settings path has a parent")
                .join(format!("ai.json.{}.tmp", std::process::id()))
                .exists(),
            "the temporary file does not outlive the rename"
        );
        std::fs::remove_dir_all(home).expect("the temporary home is removable");
    }

    #[test]
    fn the_chosen_provider_leads_the_priority_and_the_other_still_follows() {
        let settings = AiSettings {
            provider: ProviderId::Claude,
            ..AiSettings::default()
        };
        let config = settings.router_config();
        assert_eq!(
            config.priority,
            vec![ProviderId::Claude, ProviderId::Codex],
            "the choice leads and the fallback is still behind it"
        );
        let defaults = RouterConfig::default();
        assert_eq!(config.availability_ttl, defaults.availability_ttl);
        assert_eq!(config.backoff_base, defaults.backoff_base);
        assert_eq!(
            config.max_transient_attempts,
            defaults.max_transient_attempts
        );
        assert_eq!(
            config.max_invalid_output_attempts,
            defaults.max_invalid_output_attempts
        );
        assert_eq!(config.default_cooldown, defaults.default_cooldown);
    }

    #[test]
    fn the_default_choice_leaves_the_routers_own_order_alone() {
        assert_eq!(
            AiSettings::default().router_config().priority,
            RouterConfig::default().priority
        );
    }

    #[test]
    fn a_first_run_starts_on_the_only_provider_that_can_answer() {
        let one_ready = [
            (ProviderId::Codex, Availability::NeedsLogin),
            (ProviderId::Claude, Availability::Ready),
        ];
        assert_eq!(
            AiSettings::provider_for_first_run(&one_ready),
            ProviderId::Claude
        );

        let both_ready = [
            (ProviderId::Codex, Availability::Ready),
            (ProviderId::Claude, Availability::Ready),
        ];
        assert_eq!(
            AiSettings::provider_for_first_run(&both_ready),
            ProviderId::Codex
        );

        let neither = [
            (ProviderId::Codex, Availability::NotInstalled),
            (ProviderId::Claude, Availability::NeedsLogin),
        ];
        assert_eq!(
            AiSettings::provider_for_first_run(&neither),
            ProviderId::Codex
        );
    }
}
