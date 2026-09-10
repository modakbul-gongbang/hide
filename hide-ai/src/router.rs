use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::log::{AiLogEvent, AiLogSink};
use crate::schema;
use crate::{AiBackend, AiError, AiRequest, Availability, CancelToken, ProviderId};

/// Retry and selection policy. The defaults carry the label plugin's proven
/// constants forward: exponential backoff capped at four attempts for a
/// failure that may clear on its own, two for one settled by the input.
#[derive(Clone, Debug)]
pub struct RouterConfig {
    /// Order used when more than one provider is connected.
    pub priority: Vec<ProviderId>,
    pub availability_ttl: Duration,
    pub backoff_base: Duration,
    pub max_transient_attempts: u8,
    pub max_invalid_output_attempts: u8,
    /// Cooldown applied when a provider reports a usage limit without a
    /// reset time.
    pub default_cooldown: Duration,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            priority: vec![ProviderId::Codex, ProviderId::Claude],
            availability_ttl: Duration::from_secs(30),
            backoff_base: Duration::from_secs(5),
            max_transient_attempts: 4,
            max_invalid_output_attempts: 2,
            default_cooldown: Duration::from_secs(600),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DedupKey {
    feature_id: &'static str,
    subject_id: String,
    input_hash: u64,
}

struct InFlight {
    done: Mutex<Option<Result<Value, AiError>>>,
    changed: Condvar,
}

#[derive(Default)]
struct Rollup {
    day: u64,
    counts: HashMap<(ProviderId, &'static str), u64>,
}

struct State {
    in_flight: HashMap<DedupKey, Arc<InFlight>>,
    cooldown_until: HashMap<ProviderId, Instant>,
    availability: HashMap<ProviderId, (Availability, Instant)>,
    rollup: Rollup,
}

/// Selects a provider, suppresses duplicate intents, applies the retry and
/// fallback policy, and re-validates every answer against the feature schema.
pub struct AiRouter {
    backends: Vec<Arc<dyn AiBackend>>,
    config: RouterConfig,
    sink: Arc<dyn AiLogSink>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
    state: Mutex<State>,
}

impl AiRouter {
    pub fn new(
        backends: Vec<Arc<dyn AiBackend>>,
        config: RouterConfig,
        sink: Arc<dyn AiLogSink>,
    ) -> Self {
        Self::with_sleep(backends, config, sink, Box::new(std::thread::sleep))
    }

    /// Tests inject a sleeper so backoff is observable without waiting it out.
    pub fn with_sleep(
        backends: Vec<Arc<dyn AiBackend>>,
        config: RouterConfig,
        sink: Arc<dyn AiLogSink>,
        sleep: Box<dyn Fn(Duration) + Send + Sync>,
    ) -> Self {
        Self {
            backends,
            config,
            sink,
            sleep,
            state: Mutex::new(State {
                in_flight: HashMap::new(),
                cooldown_until: HashMap::new(),
                availability: HashMap::new(),
                rollup: Rollup {
                    day: utc_day(),
                    counts: HashMap::new(),
                },
            }),
        }
    }

    /// Current availability of every registered provider, refreshed when the
    /// cached answer is older than the configured window.
    pub fn availability(&self) -> Vec<(ProviderId, Availability)> {
        self.backends
            .iter()
            .map(|backend| (backend.id(), self.cached_availability(backend.as_ref())))
            .collect()
    }

    /// Forces the next selection to ask each provider again, for a caller
    /// that knows something changed (a login completed).
    pub fn invalidate_availability(&self) {
        self.lock().availability.clear();
    }

    pub fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<Value, AiError> {
        let key = DedupKey {
            feature_id: request.feature_id,
            subject_id: request.subject_id.clone(),
            input_hash: input_hash(request),
        };
        let (flight, leader) = {
            let mut state = self.lock();
            match state.in_flight.get(&key) {
                Some(existing) => (Arc::clone(existing), false),
                None => {
                    let flight = Arc::new(InFlight {
                        done: Mutex::new(None),
                        changed: Condvar::new(),
                    });
                    state.in_flight.insert(key.clone(), Arc::clone(&flight));
                    (flight, true)
                }
            }
        };
        if !leader {
            self.log(request, |event| {
                event.event = "ai.request.joined";
            });
            return self.join(&flight, cancel);
        }
        let result = self.execute_selected(request, cancel);
        {
            let mut done = flight.done.lock().unwrap_or_else(|e| e.into_inner());
            *done = Some(result.clone());
        }
        flight.changed.notify_all();
        self.lock().in_flight.remove(&key);
        result
    }

    /// Emits the daily rollup for the day that just ended, if any. Called on
    /// every completed request and available to a caller at shutdown.
    pub fn flush_rollup(&self) {
        let today = utc_day();
        let previous = {
            let mut state = self.lock();
            if state.rollup.day == today && !state.rollup.counts.is_empty() {
                // The day is still open; nothing to close.
                return;
            }
            std::mem::replace(
                &mut state.rollup,
                Rollup {
                    day: today,
                    counts: HashMap::new(),
                },
            )
        };
        if previous.counts.is_empty() {
            return;
        }
        let mut parts: Vec<String> = previous
            .counts
            .iter()
            .map(|((provider, class), count)| format!("{provider}.{class}={count}"))
            .collect();
        parts.sort();
        let mut event = AiLogEvent::new("ai.daily_rollup");
        event.detail = Some(format!("day={};{}", previous.day, parts.join(";")));
        self.sink.log(event);
    }

    fn join(&self, flight: &InFlight, cancel: &CancelToken) -> Result<Value, AiError> {
        let mut done = flight.done.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(result) = done.as_ref() {
                return result.clone();
            }
            if cancel.is_cancelled() {
                return Err(AiError::Cancelled);
            }
            let (guard, _) = flight
                .changed
                .wait_timeout(done, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner());
            done = guard;
        }
    }

    fn execute_selected(
        &self,
        request: &AiRequest,
        cancel: &CancelToken,
    ) -> Result<Value, AiError> {
        let started = Instant::now();
        let ordered = self.ordered_backends();
        let states: Vec<(ProviderId, Availability)> = ordered
            .iter()
            .map(|backend| (backend.id(), self.cached_availability(backend.as_ref())))
            .collect();
        let connected: Vec<&Arc<dyn AiBackend>> = ordered
            .iter()
            .zip(states.iter())
            .filter(|(_, (_, state))| state.is_ready())
            .map(|(backend, _)| backend)
            .collect();
        if connected.is_empty() {
            let error = AiError::NoProvider(states);
            self.finish(request, None, &Err(error.clone()), started, 0);
            return Err(error);
        }

        let mut last_error = None;
        let mut previous: Option<ProviderId> = None;
        for backend in connected {
            let provider = backend.id();
            if let Some(from) = previous {
                self.log(request, |event| {
                    event.event = "ai.fallback";
                    event.provider = Some(provider);
                    event.detail = Some(format!("from={from};to={provider}"));
                });
            }
            let outcome = self.execute_on(backend.as_ref(), request, cancel);
            match outcome {
                Ok((value, attempt)) => {
                    self.finish(
                        request,
                        Some(provider),
                        &Ok(value.clone()),
                        started,
                        attempt,
                    );
                    return Ok(value);
                }
                Err((error, attempt)) => {
                    self.finish(
                        request,
                        Some(provider),
                        &Err(error.clone()),
                        started,
                        attempt,
                    );
                    let eligible = matches!(
                        error,
                        AiError::ProviderUnavailable(_)
                            | AiError::NotAuthenticated
                            | AiError::UsageLimited { .. }
                            | AiError::Unsupported(_)
                    );
                    if !eligible || cancel.is_cancelled() {
                        return Err(error);
                    }
                    previous = Some(provider);
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or(AiError::NoProvider(Vec::new())))
    }

    /// Runs the retry policy on one provider. The returned attempt number is
    /// the last one made.
    fn execute_on(
        &self,
        backend: &dyn AiBackend,
        request: &AiRequest,
        cancel: &CancelToken,
    ) -> Result<(Value, u8), (AiError, u8)> {
        let provider = backend.id();
        if let Some(remaining) = self.cooldown_remaining(provider) {
            return Err((
                AiError::UsageLimited {
                    retry_after: Some(remaining),
                },
                0,
            ));
        }
        let mut attempt: u8 = 0;
        loop {
            attempt = attempt.saturating_add(1);
            if cancel.is_cancelled() {
                return Err((AiError::Cancelled, attempt));
            }
            let result = backend.execute(request, cancel).and_then(|response| {
                schema::validate(&request.output_schema, &response.value)?;
                Ok(response)
            });
            match result {
                Ok(response) => {
                    self.log(request, |event| {
                        event.event = "ai.attempt";
                        event.provider = Some(provider);
                        event.outcome_class = Some("ok");
                        event.attempt = Some(attempt);
                        event.output_tokens = response.usage.output_tokens;
                    });
                    return Ok((response.value, attempt));
                }
                Err(error) => {
                    self.log(request, |event| {
                        event.event = "ai.attempt";
                        event.provider = Some(provider);
                        event.outcome_class = Some(error.class());
                        event.attempt = Some(attempt);
                    });
                    let limit = match &error {
                        AiError::Transient(_) | AiError::Timeout => {
                            self.config.max_transient_attempts
                        }
                        AiError::InvalidOutput(_) => self.config.max_invalid_output_attempts,
                        AiError::UsageLimited { retry_after } => {
                            let wait = retry_after.unwrap_or(self.config.default_cooldown);
                            self.lock()
                                .cooldown_until
                                .insert(provider, Instant::now() + wait);
                            0
                        }
                        AiError::NotAuthenticated => {
                            self.lock()
                                .availability
                                .insert(provider, (Availability::NeedsLogin, Instant::now()));
                            0
                        }
                        AiError::ProviderUnavailable(reason) => {
                            self.lock().availability.insert(
                                provider,
                                (
                                    Availability::Unavailable {
                                        reason: reason.clone(),
                                    },
                                    Instant::now(),
                                ),
                            );
                            0
                        }
                        AiError::Cancelled | AiError::Unsupported(_) | AiError::NoProvider(_) => 0,
                    };
                    if attempt >= limit {
                        return Err((error, attempt));
                    }
                    let backoff = self.config.backoff_base * 4u32.pow(u32::from(attempt - 1));
                    (self.sleep)(backoff);
                }
            }
        }
    }

    fn ordered_backends(&self) -> Vec<Arc<dyn AiBackend>> {
        let mut ordered: Vec<Arc<dyn AiBackend>> = Vec::new();
        for provider in &self.config.priority {
            if let Some(backend) = self.backends.iter().find(|b| b.id() == *provider) {
                ordered.push(Arc::clone(backend));
            }
        }
        for backend in &self.backends {
            if !ordered.iter().any(|b| b.id() == backend.id()) {
                ordered.push(Arc::clone(backend));
            }
        }
        ordered
    }

    fn cached_availability(&self, backend: &dyn AiBackend) -> Availability {
        let provider = backend.id();
        if let Some((state, checked)) = self.lock().availability.get(&provider)
            && checked.elapsed() < self.config.availability_ttl
        {
            return state.clone();
        }
        let state = backend.availability();
        self.lock()
            .availability
            .insert(provider, (state.clone(), Instant::now()));
        state
    }

    fn cooldown_remaining(&self, provider: ProviderId) -> Option<Duration> {
        let mut state = self.lock();
        let until = *state.cooldown_until.get(&provider)?;
        let now = Instant::now();
        if until <= now {
            state.cooldown_until.remove(&provider);
            return None;
        }
        Some(until - now)
    }

    fn finish(
        &self,
        request: &AiRequest,
        provider: Option<ProviderId>,
        result: &Result<Value, AiError>,
        started: Instant,
        attempt: u8,
    ) {
        let class = match result {
            Ok(_) => "ok",
            Err(error) => error.class(),
        };
        self.log(request, |event| {
            event.event = "ai.request.finished";
            event.provider = provider;
            event.outcome_class = Some(class);
            event.duration_ms = Some(started.elapsed().as_millis() as u64);
            event.attempt = Some(attempt);
            if let Err(error) = result {
                event.detail = Some(error.to_string());
            }
        });
        if let Some(provider) = provider {
            let mut state = self.lock();
            *state.rollup.counts.entry((provider, class)).or_insert(0) += 1;
        }
        self.flush_rollup();
    }

    fn log(&self, request: &AiRequest, fill: impl FnOnce(&mut AiLogEvent)) {
        let mut event = AiLogEvent::new("ai.event");
        event.request_id = Some(request.request_id.0.clone());
        event.feature_id = Some(request.feature_id);
        event.input_chars = Some(request.input.chars().count());
        event.schema_version = Some(request.schema_version);
        fill(&mut event);
        self.sink.log(event);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn input_hash(request: &AiRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    request.system.hash(&mut hasher);
    request.input.hash(&mut hasher);
    request.output_schema.to_string().hash(&mut hasher);
    hasher.finish()
}

fn utc_day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiResponse, RequestId};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Scripted provider: answers from a queue of outcomes and counts calls.
    struct Scripted {
        id: ProviderId,
        availability: Availability,
        outcomes: Mutex<VecDeque<Result<Value, AiError>>>,
        calls: AtomicUsize,
        delay: Duration,
    }

    use std::collections::VecDeque;

    impl Scripted {
        fn new(id: ProviderId, outcomes: Vec<Result<Value, AiError>>) -> Arc<Self> {
            Arc::new(Self {
                id,
                availability: Availability::Ready,
                outcomes: Mutex::new(outcomes.into()),
                calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
            })
        }

        fn with_availability(id: ProviderId, availability: Availability) -> Arc<Self> {
            Arc::new(Self {
                id,
                availability,
                outcomes: Mutex::new(VecDeque::new()),
                calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl AiBackend for Scripted {
        fn id(&self) -> ProviderId {
            self.id
        }
        fn availability(&self) -> Availability {
            self.availability.clone()
        }
        fn execute(&self, _: &AiRequest, _: &CancelToken) -> Result<AiResponse, AiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            let next = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(json!({"summary": "late"})));
            next.map(|value| AiResponse {
                value,
                usage: crate::AiUsage::default(),
            })
        }
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<AiLogEvent>>);

    impl AiLogSink for Recorder {
        fn log(&self, event: AiLogEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    impl Recorder {
        fn events(&self, name: &str) -> Vec<AiLogEvent> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.event == name)
                .cloned()
                .collect()
        }
    }

    fn schema() -> Value {
        json!({"type": "object", "required": ["summary"], "properties": {"summary": {"type": "string"}}})
    }

    fn request(subject: &str, input: &str) -> AiRequest {
        AiRequest {
            feature_id: "test_feature",
            request_id: RequestId(format!("r-{subject}")),
            subject_id: subject.to_owned(),
            system: "sys".to_owned(),
            input: input.to_owned(),
            output_schema: schema(),
            max_output_tokens: 64,
            deadline: Duration::from_secs(5),
            schema_version: "test.v1",
        }
    }

    fn make_router(backends: Vec<Arc<dyn AiBackend>>, sink: Arc<Recorder>) -> AiRouter {
        let config = RouterConfig {
            backoff_base: Duration::from_millis(1),
            ..RouterConfig::default()
        };
        AiRouter::with_sleep(backends, config, sink, Box::new(|_| {}))
    }

    fn ok(summary: &str) -> Result<Value, AiError> {
        Ok(json!({"summary": summary}))
    }

    #[test]
    fn a_single_connected_provider_is_used_and_its_answer_validated() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(ProviderId::Codex, vec![ok("a")]);
        let router = make_router(vec![codex.clone()], sink.clone());
        let value = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(value["summary"], "a");
        let finished = sink.events("ai.request.finished");
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].provider, Some(ProviderId::Codex));
        assert_eq!(finished[0].outcome_class, Some("ok"));
        assert_eq!(finished[0].request_id.as_deref(), Some("r-p1"));
        assert!(finished[0].detail.is_none());
    }

    #[test]
    fn a_schema_mismatch_is_invalid_output_after_two_attempts_and_never_falls_back() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![
                Ok(json!({"other": 1})),
                Ok(json!({"summary": 5})),
                ok("never"),
            ],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![codex.clone(), claude.clone()], sink);
        let error = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(error, AiError::InvalidOutput(_)), "{error:?}");
        assert_eq!(codex.calls(), 2);
        assert_eq!(claude.calls(), 0);
    }

    #[test]
    fn a_timeout_retries_the_same_provider_and_never_runs_elsewhere() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::Timeout), Err(AiError::Timeout), ok("third")],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![codex.clone(), claude.clone()], sink.clone());
        let value = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(value["summary"], "third");
        assert_eq!(codex.calls(), 3);
        assert_eq!(claude.calls(), 0);
        assert!(sink.events("ai.fallback").is_empty());

        let exhausted = Scripted::new(ProviderId::Codex, vec![Err(AiError::Timeout); 4]);
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![exhausted.clone(), claude.clone()], sink);
        assert_eq!(
            router.execute(&request("p2", "x"), &CancelToken::new()),
            Err(AiError::Timeout)
        );
        assert_eq!(exhausted.calls(), 4);
        assert_eq!(claude.calls(), 0);
    }

    #[test]
    fn provider_unavailable_falls_back_in_priority_order_with_a_logged_event() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::ProviderUnavailable("gone".to_owned()))],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![codex.clone(), claude.clone()], sink.clone());
        let value = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(value["summary"], "claude");
        let fallback = sink.events("ai.fallback");
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].detail.as_deref(), Some("from=codex;to=claude"));
        // The failed provider's state is remembered, so the next request
        // goes straight to the fallback without a second failure.
        let value = router
            .execute(&request("p2", "y"), &CancelToken::new())
            .unwrap();
        assert_eq!(value["summary"], "late");
        assert_eq!(codex.calls(), 1);
    }

    #[test]
    fn an_unsupported_provider_is_reported_and_never_selected() {
        let sink = Arc::new(Recorder::default());
        let claude = Scripted::with_availability(
            ProviderId::Claude,
            Availability::Unsupported {
                reason: "gap".to_owned(),
            },
        );
        let codex = Scripted::with_availability(ProviderId::Codex, Availability::NeedsLogin);
        let router = make_router(vec![codex.clone(), claude.clone()], sink);
        let error = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap_err();
        match error {
            AiError::NoProvider(states) => {
                assert_eq!(states[0], (ProviderId::Codex, Availability::NeedsLogin));
                assert_eq!(states[1].0, ProviderId::Claude);
                assert_eq!(states[1].1.class(), "unsupported");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(claude.calls(), 0);
    }

    #[test]
    fn a_usage_limit_parks_the_provider_account_wide_until_it_resets() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![
                Err(AiError::UsageLimited {
                    retry_after: Some(Duration::from_millis(200)),
                }),
                ok("after"),
            ],
        );
        let router = make_router(vec![codex.clone()], sink);
        let first = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(first, AiError::UsageLimited { .. }));
        // Another feature and subject on the same account is parked too,
        // without spending a call.
        let mut other = request("p2", "y");
        other.feature_id = "other_feature";
        let second = router.execute(&other, &CancelToken::new()).unwrap_err();
        assert!(matches!(
            second,
            AiError::UsageLimited {
                retry_after: Some(_)
            }
        ));
        assert_eq!(codex.calls(), 1);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(
            router
                .execute(&request("p3", "z"), &CancelToken::new())
                .unwrap()["summary"],
            "after"
        );
    }

    #[test]
    fn not_authenticated_is_not_retried_until_availability_changes() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::NotAuthenticated), ok("logged in")],
        );
        let router = make_router(vec![codex.clone()], sink);
        assert_eq!(
            router.execute(&request("p1", "x"), &CancelToken::new()),
            Err(AiError::NotAuthenticated)
        );
        let parked = router
            .execute(&request("p2", "y"), &CancelToken::new())
            .unwrap_err();
        assert!(matches!(parked, AiError::NoProvider(_)), "{parked:?}");
        assert_eq!(codex.calls(), 1);
        router.invalidate_availability();
        assert_eq!(
            router
                .execute(&request("p3", "z"), &CancelToken::new())
                .unwrap()["summary"],
            "logged in"
        );
    }

    #[test]
    fn concurrent_duplicates_share_one_provider_call() {
        let sink = Arc::new(Recorder::default());
        let codex = Arc::new(Scripted {
            id: ProviderId::Codex,
            availability: Availability::Ready,
            outcomes: Mutex::new(vec![ok("shared")].into()),
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(200),
        });
        let router = Arc::new(make_router(vec![codex.clone()], sink.clone()));
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let router = Arc::clone(&router);
                std::thread::spawn(move || {
                    router
                        .execute(&request("same", "same input"), &CancelToken::new())
                        .unwrap()
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap()["summary"], "shared");
        }
        assert_eq!(codex.calls(), 1);
        assert_eq!(sink.events("ai.request.joined").len(), 2);
        // A different input for the same subject is a new intent.
        router
            .execute(&request("same", "other input"), &CancelToken::new())
            .unwrap();
        assert_eq!(codex.calls(), 2);
    }

    #[test]
    fn log_events_carry_identifiers_and_classes_but_no_content() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(ProviderId::Codex, vec![ok("secret summary text")]);
        let router = make_router(vec![codex], sink.clone());
        let mut req = request("p1", "the user's private transcript");
        req.system = "confidential prompt".to_owned();
        router.execute(&req, &CancelToken::new()).unwrap();
        let all = sink.0.lock().unwrap();
        assert!(!all.is_empty());
        for event in all.iter() {
            let rendered = format!("{event:?}");
            assert!(!rendered.contains("private transcript"), "{rendered}");
            assert!(!rendered.contains("confidential"), "{rendered}");
            assert!(!rendered.contains("secret summary"), "{rendered}");
            assert_eq!(event.feature_id, Some("test_feature"));
            assert_eq!(event.schema_version, Some("test.v1"));
            assert_eq!(event.input_chars, Some(req.input.chars().count()));
        }
    }
}
