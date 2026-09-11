//! What the background AI providers can do right now, read for the Settings
//! screen.
//!
//! Answering costs child processes: `codex app-server` is started and asked
//! for the account and its model list, and `claude auth status` is run. So
//! this reader is shaped like the project panel's readers rather than like a
//! projection: the session-sync coordinator drives it, `read_if_due` decides
//! whether anything runs, the work itself happens on
//! [`BackgroundRead`](crate::reader::BackgroundRead)'s worker thread, and the
//! runtime mutex is never held across any of it.
//!
//! It also reads nothing at all while nobody is looking. `observing` is false
//! unless the Background AI group is on screen, and an unobserved request
//! answers from no provider, so an idle Hide starts no provider process.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hide_ai::{
    AiBackend, AiRouter, AiSettings, Availability, ModelCatalog, NoopLogSink, ProviderId,
};

use crate::model::{BackgroundAiProviderSnapshot, BackgroundAiSnapshot};
use crate::reader::BackgroundRead;

/// How stale a provider answer may be while the group is on screen.
///
/// A login completes outside Hide, so the screen has to notice on its own;
/// thirty seconds is soon enough for someone who has just logged in and rare
/// enough that the child processes are not a background load. It matches the
/// router's own availability window, so the two never disagree for long.
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
/// The least time between one read finishing and a changed request starting
/// the next, so switching the model twice does not start two probes.
const SPACING: Duration = Duration::from_secs(2);

/// What to ask the providers, and the freshness key for asking again.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AiRequest {
    /// False unless the Background AI group is on screen. An unobserved
    /// request starts no provider process; see the module comment.
    pub observing: bool,
    /// The model each provider is configured with. Codex reports a model the
    /// account does not offer as an availability state, so a changed choice
    /// is a different question and is asked at once rather than waiting out
    /// the interval.
    pub models: BTreeMap<ProviderId, String>,
}

/// The router one screen reuses, and the models it was built for. Rebuilding
/// is what a changed model costs, because a backend is constructed with one.
type CachedRouter = Arc<Mutex<Option<(BTreeMap<ProviderId, String>, Arc<AiRouter>)>>>;

pub struct AiReader {
    inner: BackgroundRead<AiRequest, BackgroundAiSnapshot>,
}

impl AiReader {
    pub fn new() -> Self {
        // One router outlives the reads that use it, so a Codex app-server
        // child is started once for a screen rather than once per read. It is
        // rebuilt only when the configured models change, because the model
        // is what the backend was constructed with.
        let cached: CachedRouter = Arc::new(Mutex::new(None));
        Self {
            inner: BackgroundRead::new(REFRESH_INTERVAL, SPACING, move |request: &AiRequest| {
                let mut guard = cached.lock().unwrap_or_else(|error| error.into_inner());
                if !request.observing {
                    // Drop the router with the screen: its Codex child is a
                    // process, and nothing is reading its answers.
                    *guard = None;
                    return BackgroundAiSnapshot::unread();
                }
                let router = match guard.as_ref() {
                    Some((models, router)) if *models == request.models => Arc::clone(router),
                    _ => {
                        let router = Arc::new(AiRouter::new(
                            backends(&request.models),
                            AiSettings::default().router_config(),
                            Arc::new(NoopLogSink),
                        ));
                        *guard = Some((request.models.clone(), Arc::clone(&router)));
                        router
                    }
                };
                drop(guard);
                read(&router, &request.models)
            }),
        }
    }

    pub fn read_if_due(&mut self, request: AiRequest) -> Option<BackgroundAiSnapshot> {
        self.inner.poll(request)
    }
}

impl Default for AiReader {
    fn default() -> Self {
        Self::new()
    }
}

fn backends(models: &BTreeMap<ProviderId, String>) -> Vec<Arc<dyn AiBackend>> {
    let model_for = |provider: ProviderId| {
        models
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| hide_ai::settings::default_model(provider).to_owned())
    };
    vec![
        Arc::new(hide_ai::CodexAppServerBackend::new(hide_ai::CodexConfig {
            model: model_for(ProviderId::Codex),
            ..hide_ai::CodexConfig::default()
        })),
        Arc::new(hide_ai::ClaudeCliBackend::new(hide_ai::ClaudeConfig {
            model: model_for(ProviderId::Claude),
            ..hide_ai::ClaudeConfig::default()
        })),
    ]
}

fn read(router: &AiRouter, models: &BTreeMap<ProviderId, String>) -> BackgroundAiSnapshot {
    let availability = router.availability();
    let catalogs = router.models();
    let providers = hide_ai::PROVIDERS
        .iter()
        .map(|provider| {
            let state = availability
                .iter()
                .find(|(id, _)| id == provider)
                .map(|(_, state)| state.clone());
            let catalog = catalogs
                .iter()
                .find(|(id, _)| id == provider)
                .map(|(_, catalog)| catalog.clone());
            project(*provider, state.as_ref(), catalog.as_ref(), models)
        })
        .collect();
    BackgroundAiSnapshot {
        providers,
        ..BackgroundAiSnapshot::default()
    }
}

/// One provider row. The core writes the words beside the provider's name,
/// so no view builds a sentence out of a state class.
fn project(
    provider: ProviderId,
    state: Option<&Availability>,
    catalog: Option<&ModelCatalog>,
    models: &BTreeMap<ProviderId, String>,
) -> BackgroundAiProviderSnapshot {
    let (state_class, headline, message) = match state {
        None => (
            "unread",
            "Not checked yet".to_owned(),
            Some("The provider has not been asked yet".to_owned()),
        ),
        Some(Availability::Ready) => ("ready", "Signed in".to_owned(), None),
        Some(Availability::NeedsLogin) => (
            "needs_login",
            "Sign in required".to_owned(),
            Some(format!("Run `{provider} login` and check again")),
        ),
        Some(Availability::NotInstalled) => (
            "not_installed",
            "Not installed".to_owned(),
            Some("The command is not on the login shell's PATH".to_owned()),
        ),
        Some(Availability::Unavailable { reason }) => (
            "unavailable",
            "Cannot answer".to_owned(),
            Some(reason.clone()),
        ),
        Some(Availability::Unsupported { reason }) => (
            "unsupported",
            "Not supported".to_owned(),
            Some(reason.clone()),
        ),
    };
    BackgroundAiProviderSnapshot {
        id: provider.as_str().to_owned(),
        label: provider.label().to_owned(),
        state: state_class.to_owned(),
        headline,
        message,
        model: models
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| hide_ai::settings::default_model(provider).to_owned()),
        models: catalog
            .map(|catalog| catalog.offered().to_vec())
            .unwrap_or_default(),
        models_unavailable_reason: match catalog {
            None => Some("The provider has not been asked yet".to_owned()),
            Some(catalog) => catalog.unknown_reason().map(str::to_owned),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> BTreeMap<ProviderId, String> {
        AiSettings::default().models
    }

    #[test]
    fn an_unobserved_request_asks_no_provider_anything() {
        let mut reader = AiReader::new();
        let request = AiRequest {
            observing: false,
            models: models(),
        };
        let mut answer = None;
        for _ in 0..500 {
            if let Some(read) = reader.read_if_due(request.clone()) {
                answer = Some(read);
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let answer = answer.expect("the reader answers an unobserved request");
        assert_eq!(
            answer,
            BackgroundAiSnapshot::unread(),
            "nothing was asked, so every provider is unread"
        );
        assert!(
            answer
                .providers
                .iter()
                .all(|provider| provider.state == "unread"),
            "an unobserved read reports unread rather than a guess"
        );
    }

    #[test]
    fn a_provider_that_was_not_asked_is_unread_rather_than_unavailable() {
        let row = project(ProviderId::Codex, None, None, &models());
        assert_eq!(row.state, "unread");
        assert!(row.models.is_empty());
        assert!(row.models_unavailable_reason.is_some());
    }

    #[test]
    fn each_availability_state_carries_its_own_words_and_the_reason_it_has() {
        let models = models();
        let ready = project(ProviderId::Codex, Some(&Availability::Ready), None, &models);
        assert_eq!(ready.state, "ready");
        assert_eq!(ready.headline, "Signed in");
        assert_eq!(ready.message, None);

        let needs_login = project(
            ProviderId::Claude,
            Some(&Availability::NeedsLogin),
            None,
            &models,
        );
        assert_eq!(needs_login.state, "needs_login");
        assert_eq!(needs_login.headline, "Sign in required");

        let unavailable = project(
            ProviderId::Codex,
            Some(&Availability::Unavailable {
                reason: "model_not_offered:some-model".to_owned(),
            }),
            None,
            &models,
        );
        assert_eq!(unavailable.state, "unavailable");
        assert_eq!(
            unavailable.message.as_deref(),
            Some("model_not_offered:some-model"),
            "the provider layer's own reason reaches the screen"
        );
    }

    #[test]
    fn an_unknown_model_list_is_reported_as_unknown_rather_than_as_no_models() {
        let row = project(
            ProviderId::Codex,
            Some(&Availability::Ready),
            Some(&ModelCatalog::Unknown {
                reason: "codex_not_installed".to_owned(),
            }),
            &models(),
        );
        assert!(row.models.is_empty());
        assert_eq!(
            row.models_unavailable_reason.as_deref(),
            Some("codex_not_installed")
        );

        let offered = project(
            ProviderId::Claude,
            Some(&Availability::Ready),
            Some(&ModelCatalog::Offered(vec!["haiku".to_owned()])),
            &models(),
        );
        assert_eq!(offered.models, vec!["haiku".to_owned()]);
        assert_eq!(offered.models_unavailable_reason, None);
    }

    #[test]
    fn a_row_reports_the_model_the_provider_is_configured_with() {
        let mut models = models();
        models.insert(ProviderId::Claude, "sonnet".to_owned());
        assert_eq!(
            project(ProviderId::Claude, None, None, &models).model,
            "sonnet"
        );
        assert_eq!(
            project(ProviderId::Claude, None, None, &BTreeMap::new()).model,
            hide_ai::settings::default_model(ProviderId::Claude),
            "a provider with no configured model reports the backend's own default"
        );
    }
}
