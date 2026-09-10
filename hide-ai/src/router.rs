use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::log::{AiLogEvent, AiLogSink};
use crate::schema;
use crate::{
    AiBackend, AiError, AiRequest, AiResult, Availability, CancelToken, ModelCatalog, ProviderId,
};

/// Retry and selection policy. The defaults carry the label plugin's proven
/// constants forward: exponential backoff capped at four attempts for a
/// refusal that may clear on its own, two for one settled by the input.
/// A request whose completion is unknown is never attempted again.
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

/// Which provider answers now, and why the selected one is not.
///
/// The router already names the answering provider in each `AiResult`; what
/// a consumer could not ask before is the standing question, "who answers
/// right now and why is it not my choice". This is that answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderState {
    /// The provider the configured priority puts first: the user's choice.
    pub selected: ProviderId,
    /// The provider a request made now would run on, or `None` when every
    /// provider is degraded. That state is reachable, so it is reported
    /// rather than filled in with a provider that cannot answer.
    pub active: Option<ProviderId>,
    /// Why `selected` stepped aside; absent when it did not.
    pub degraded: Option<Degraded>,
}

/// Why one provider is not answering, and until when when that is known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Degraded {
    pub provider: ProviderId,
    pub reason: Availability,
    /// What remains of an account-wide cooldown. `None` when the reason has
    /// no end the provider told us about, such as a missing login.
    pub until: Option<Duration>,
}

/// What selection decided about one provider.
///
/// `Parked` exists so a provider under an account-wide cooldown is never
/// asked anything: the cooldown already answers, and probing a provider that
/// cannot answer for hours is the poke that sticky failover exists to stop.
#[derive(Clone, Debug)]
enum Selection {
    Ready,
    Parked(Duration),
    Blocked(Availability),
}

impl Selection {
    fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// The availability a caller is told about, including the cooldown that
    /// was never a provider answer.
    fn availability(&self) -> Availability {
        match self {
            Self::Ready => Availability::Ready,
            Self::Parked(remaining) => Availability::Unavailable {
                reason: format!("usage_limited;retry_after_s={}", remaining.as_secs()),
            },
            Self::Blocked(state) => state.clone(),
        }
    }

    /// Short machine-readable reason for a log line.
    fn detail(&self) -> String {
        match self {
            Self::Ready => "ready".to_owned(),
            Self::Parked(remaining) => {
                format!("usage_limited;retry_after_s={}", remaining.as_secs())
            }
            Self::Blocked(
                state @ (Availability::Unavailable { reason }
                | Availability::Unsupported { reason }),
            ) => format!("{}:{reason}", state.class()),
            Self::Blocked(state) => state.class().to_owned(),
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
    done: Mutex<Option<Result<AiResult, AiError>>>,
    changed: Condvar,
}

/// Owns one in-flight entry for its leader. Settling it wakes every joiner
/// and frees the key; dropping it unsettled, which only a panic does, settles
/// it with an explicit error so no joiner waits on a leader that is gone.
struct FlightGuard<'a> {
    router: &'a AiRouter,
    key: DedupKey,
    flight: Arc<InFlight>,
    settled: bool,
}

impl FlightGuard<'_> {
    fn settle(&mut self, result: Result<AiResult, AiError>) {
        self.settled = true;
        {
            let mut done = self.flight.done.lock().unwrap_or_else(|e| e.into_inner());
            *done = Some(result);
        }
        self.flight.changed.notify_all();
        self.router.lock().in_flight.remove(&self.key);
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.settle(Err(AiError::Internal("leader_panicked".to_owned())));
        }
    }
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
    /// The selected provider a degradation event has already been logged
    /// for, so entering and leaving failover are each announced once rather
    /// than on every request.
    announced_degraded: Option<ProviderId>,
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
                announced_degraded: None,
                rollup: Rollup {
                    day: utc_day(),
                    counts: HashMap::new(),
                },
            }),
        }
    }

    /// Current availability of every registered provider, refreshed when the
    /// cached answer is older than the configured window. A provider parked
    /// by an account-wide cooldown reports that park instead of being asked.
    pub fn availability(&self) -> Vec<(ProviderId, Availability)> {
        self.select()
            .into_iter()
            .map(|(backend, selection)| (backend.id(), selection.availability()))
            .collect()
    }

    /// Which provider answers a request made now, and why the selected one
    /// stepped aside. `None` when the router has no registered provider,
    /// which is a wiring mistake rather than a runtime state.
    ///
    /// This is a query: it never logs and never runs a request. It may
    /// refresh a stale availability answer, which is what makes the return
    /// to the selected provider visible once its cooldown has expired.
    pub fn provider_state(&self) -> Option<ProviderState> {
        let selection = self.select();
        let (first, chosen) = selection.first()?;
        let selected = first.id();
        Some(ProviderState {
            selected,
            active: active_provider(&selection),
            degraded: degraded_of(selected, chosen),
        })
    }

    /// What each registered provider offers, in the configured priority
    /// order. This asks the provider rather than the router's cache, because
    /// a model list is not part of a selection decision and has no window of
    /// its own; the caller decides how often to ask.
    pub fn models(&self) -> Vec<(ProviderId, ModelCatalog)> {
        self.ordered_backends()
            .into_iter()
            .map(|backend| (backend.id(), backend.models()))
            .collect()
    }

    /// Forces the next selection to ask each provider again, for a caller
    /// that knows something changed (a login completed).
    pub fn invalidate_availability(&self) {
        self.lock().availability.clear();
    }

    /// Runs the request on the selected provider and returns the validated
    /// answer with the provider that produced it. Every failure is typed;
    /// see [`AiError`] for which ones the router retries or moves.
    pub fn execute(&self, request: &AiRequest, cancel: &CancelToken) -> Result<AiResult, AiError> {
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
        let mut guard = FlightGuard {
            router: self,
            key,
            flight,
            settled: false,
        };
        let result = self.execute_selected(request, cancel);
        guard.settle(result.clone());
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

    fn join(&self, flight: &InFlight, cancel: &CancelToken) -> Result<AiResult, AiError> {
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
    ) -> Result<AiResult, AiError> {
        let started = Instant::now();
        // Availability is read once per request, and a parked provider is
        // not read at all.
        let selection = self.select();
        self.announce_degradation(request, &selection);
        let connected: Vec<&Arc<dyn AiBackend>> = selection
            .iter()
            .filter(|(_, state)| state.is_ready())
            .map(|(backend, _)| backend)
            .collect();
        if connected.is_empty() {
            // A parked provider is the more useful answer than "no provider":
            // it says the account is waiting, which is what a caller decides
            // its own retry on, and it names the wait.
            let parked = selection.iter().find_map(|(backend, state)| match state {
                Selection::Parked(remaining) => Some((backend.id(), *remaining)),
                _ => None,
            });
            let (provider, error) = match parked {
                Some((provider, remaining)) => (
                    Some(provider),
                    AiError::UsageLimited {
                        retry_after: Some(remaining),
                    },
                ),
                None => (
                    None,
                    AiError::NoProvider(
                        selection
                            .iter()
                            .map(|(backend, state)| (backend.id(), state.availability()))
                            .collect(),
                    ),
                ),
            };
            self.finish(request, provider, &Err(error.clone()), started, 0);
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
                    let result = AiResult { provider, value };
                    self.finish(
                        request,
                        Some(provider),
                        &Ok(result.clone()),
                        started,
                        attempt,
                    );
                    return Ok(result);
                }
                Err((error, attempt)) => {
                    self.finish(
                        request,
                        Some(provider),
                        &Err(error.clone()),
                        started,
                        attempt,
                    );
                    // Only a refusal moves on: the provider never took the
                    // request, so another provider repeats nothing.
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
                        AiError::Transient(_) => self.config.max_transient_attempts,
                        AiError::InvalidOutput(_) => self.config.max_invalid_output_attempts,
                        // Submitted, fate unknown: a second attempt could be
                        // a second completion.
                        AiError::Timeout | AiError::CompletionUnknown(_) => 0,
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
                        AiError::Cancelled
                        | AiError::Unsupported(_)
                        | AiError::NoProvider(_)
                        | AiError::Internal(_) => 0,
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

    /// Decides every provider's standing in priority order without running
    /// anything. A provider under a live cooldown is skipped rather than
    /// probed: that skip is what makes a failover sticky, because the reason
    /// it stepped aside outlives the request that discovered it.
    fn select(&self) -> Vec<(Arc<dyn AiBackend>, Selection)> {
        self.ordered_backends()
            .into_iter()
            .map(|backend| {
                let selection = match self.cooldown_remaining(backend.id()) {
                    Some(remaining) => Selection::Parked(remaining),
                    None => match self.cached_availability(backend.as_ref()) {
                        state if state.is_ready() => Selection::Ready,
                        state => Selection::Blocked(state),
                    },
                };
                (backend, selection)
            })
            .collect()
    }

    /// Logs the two edges of a failover.
    ///
    /// `ai.fallback` only exists when one request tries two providers. Once
    /// the failover is sticky the parked provider is never tried, so without
    /// these events the log would show the provider silently change and
    /// silently change back.
    fn announce_degradation(
        &self,
        request: &AiRequest,
        selection: &[(Arc<dyn AiBackend>, Selection)],
    ) {
        let Some((backend, chosen)) = selection.first() else {
            return;
        };
        let selected = backend.id();
        let announced = self.lock().announced_degraded;
        match (announced, chosen.is_ready()) {
            (None, false) => {
                self.lock().announced_degraded = Some(selected);
                let active = active_provider(selection);
                let reason = chosen.detail();
                self.log(request, |event| {
                    event.event = "ai.provider.degraded";
                    event.provider = active;
                    event.outcome_class = Some(chosen.availability().class());
                    event.detail = Some(format!(
                        "selected={selected};reason={reason};active={}",
                        active.map_or("none".to_owned(), |provider| provider.to_string())
                    ));
                });
            }
            (Some(_), true) => {
                self.lock().announced_degraded = None;
                self.log(request, |event| {
                    event.event = "ai.provider.recovered";
                    event.provider = Some(selected);
                    event.outcome_class = Some("ready");
                    event.detail = Some(format!("selected={selected}"));
                });
            }
            _ => {}
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
            // The park is over. Drop the cached answer with it so the next
            // question is asked of the provider once, rather than answered
            // from a cache filled while it could not answer at all.
            state.availability.remove(&provider);
            return None;
        }
        Some(until - now)
    }

    fn finish(
        &self,
        request: &AiRequest,
        provider: Option<ProviderId>,
        result: &Result<AiResult, AiError>,
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

/// The highest-priority provider that can answer now, if any.
fn active_provider(selection: &[(Arc<dyn AiBackend>, Selection)]) -> Option<ProviderId> {
    selection
        .iter()
        .find(|(_, state)| state.is_ready())
        .map(|(backend, _)| backend.id())
}

fn degraded_of(selected: ProviderId, chosen: &Selection) -> Option<Degraded> {
    match chosen {
        Selection::Ready => None,
        Selection::Parked(remaining) => Some(Degraded {
            provider: selected,
            reason: chosen.availability(),
            until: Some(*remaining),
        }),
        Selection::Blocked(reason) => Some(Degraded {
            provider: selected,
            reason: reason.clone(),
            until: None,
        }),
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

    /// Scripted provider: answers from a queue of outcomes and counts both
    /// the requests and the availability probes it was asked for.
    struct Scripted {
        id: ProviderId,
        availability: Mutex<Availability>,
        probes: AtomicUsize,
        outcomes: Mutex<VecDeque<Result<Value, AiError>>>,
        calls: AtomicUsize,
        delay: Duration,
    }

    use std::collections::VecDeque;

    impl Scripted {
        fn new(id: ProviderId, outcomes: Vec<Result<Value, AiError>>) -> Arc<Self> {
            Arc::new(Self {
                id,
                availability: Mutex::new(Availability::Ready),
                probes: AtomicUsize::new(0),
                outcomes: Mutex::new(outcomes.into()),
                calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
            })
        }

        fn with_availability(id: ProviderId, availability: Availability) -> Arc<Self> {
            Arc::new(Self {
                id,
                availability: Mutex::new(availability),
                probes: AtomicUsize::new(0),
                outcomes: Mutex::new(VecDeque::new()),
                calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn probes(&self) -> usize {
            self.probes.load(Ordering::SeqCst)
        }

        fn set_availability(&self, state: Availability) {
            *self.availability.lock().unwrap() = state;
        }

        fn queue(&self, outcomes: Vec<Result<Value, AiError>>) {
            self.outcomes.lock().unwrap().extend(outcomes);
        }
    }

    impl AiBackend for Scripted {
        fn id(&self) -> ProviderId {
            self.id
        }
        fn availability(&self) -> Availability {
            self.probes.fetch_add(1, Ordering::SeqCst);
            self.availability.lock().unwrap().clone()
        }
        fn models(&self) -> ModelCatalog {
            ModelCatalog::Offered(vec![format!("{}-model", self.id)])
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
        let result = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(result.value["summary"], "a");
        assert_eq!(result.provider, ProviderId::Codex);
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

    /// A submitted request whose fate is unknown is spent: one call, no
    /// second attempt on the same provider, no attempt on another.
    #[test]
    fn a_completion_unknown_outcome_is_final_and_never_runs_elsewhere() {
        for error in [
            AiError::Timeout,
            AiError::CompletionUnknown("app_server_exited:signal".to_owned()),
        ] {
            let sink = Arc::new(Recorder::default());
            let codex = Scripted::new(ProviderId::Codex, vec![Err(error.clone()), ok("again")]);
            let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
            let router = make_router(vec![codex.clone(), claude.clone()], sink.clone());
            assert_eq!(
                router.execute(&request("p1", "x"), &CancelToken::new()),
                Err(error.clone()),
                "{error:?}"
            );
            assert_eq!(codex.calls(), 1, "{error:?}");
            assert_eq!(claude.calls(), 0, "{error:?}");
            assert!(sink.events("ai.fallback").is_empty());
            let finished = sink.events("ai.request.finished");
            assert_eq!(finished.len(), 1);
            assert_eq!(finished[0].outcome_class, Some(error.class()));
            // The provider is not marked unavailable: the next intent is new.
            assert_eq!(
                router
                    .execute(&request("p2", "y"), &CancelToken::new())
                    .unwrap()
                    .value["summary"],
                "again"
            );
        }
    }

    /// A refusal that may clear is retried on the same provider only.
    #[test]
    fn a_transient_refusal_retries_the_same_provider_and_never_runs_elsewhere() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![
                Err(AiError::Transient("rpc_error:thread/start:-1".to_owned())),
                Err(AiError::Transient(
                    "control_timeout:thread/start".to_owned(),
                )),
                ok("third"),
            ],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![codex.clone(), claude.clone()], sink.clone());
        let result = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(result.value["summary"], "third");
        assert_eq!(codex.calls(), 3);
        assert_eq!(claude.calls(), 0);
        assert!(sink.events("ai.fallback").is_empty());

        let exhausted = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::Transient("busy".to_owned())); 4],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![exhausted.clone(), claude.clone()], sink);
        assert!(matches!(
            router.execute(&request("p2", "x"), &CancelToken::new()),
            Err(AiError::Transient(_))
        ));
        assert_eq!(exhausted.calls(), 4);
        assert_eq!(claude.calls(), 0);
    }

    /// Whoever answers is named in the result, so a fallback can never be
    /// mistaken for the first provider's answer.
    #[test]
    fn a_result_names_the_provider_that_answered_after_a_fallback() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::with_availability(ProviderId::Codex, Availability::NeedsLogin);
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_router(vec![codex.clone(), claude.clone()], sink.clone());
        let result = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(result.provider, ProviderId::Claude);
        assert_eq!(result.value["summary"], "claude");
        let finished = sink.events("ai.request.finished");
        assert_eq!(finished[0].provider, Some(ProviderId::Claude));
    }

    /// A leader that panics must not strand its joiners or its key.
    #[test]
    fn a_panicking_leader_settles_every_joiner_with_an_error_and_frees_the_key() {
        struct Panicking {
            started: Arc<(Mutex<bool>, Condvar)>,
            release: Arc<(Mutex<bool>, Condvar)>,
            calls: AtomicUsize,
        }
        impl AiBackend for Panicking {
            fn id(&self) -> ProviderId {
                ProviderId::Codex
            }
            fn availability(&self) -> Availability {
                Availability::Ready
            }
            fn models(&self) -> ModelCatalog {
                ModelCatalog::Offered(Vec::new())
            }
            fn execute(&self, _: &AiRequest, _: &CancelToken) -> Result<AiResponse, AiError> {
                let calls = self.calls.fetch_add(1, Ordering::SeqCst);
                if calls > 0 {
                    return Ok(AiResponse {
                        value: json!({"summary": "fresh"}),
                        usage: crate::AiUsage::default(),
                    });
                }
                signal(&self.started);
                wait_for(&self.release);
                panic!("provider bug");
            }
        }
        fn signal(pair: &(Mutex<bool>, Condvar)) {
            *pair.0.lock().unwrap() = true;
            pair.1.notify_all();
        }
        fn wait_for(pair: &(Mutex<bool>, Condvar)) {
            let mut flag = pair.0.lock().unwrap();
            while !*flag {
                flag = pair.1.wait(flag).unwrap();
            }
        }
        let backend = Arc::new(Panicking {
            started: Arc::new((Mutex::new(false), Condvar::new())),
            release: Arc::new((Mutex::new(false), Condvar::new())),
            calls: AtomicUsize::new(0),
        });
        let sink = Arc::new(Recorder::default());
        let router = Arc::new(make_router(vec![backend.clone()], sink));
        let leader = {
            let router = Arc::clone(&router);
            std::thread::spawn(move || router.execute(&request("same", "x"), &CancelToken::new()))
        };
        wait_for(&backend.started);
        let joiner = {
            let router = Arc::clone(&router);
            std::thread::spawn(move || router.execute(&request("same", "x"), &CancelToken::new()))
        };
        // Give the joiner time to attach to the in-flight entry, then let
        // the leader blow up.
        std::thread::sleep(Duration::from_millis(100));
        signal(&backend.release);
        assert!(leader.join().is_err(), "the leader's panic propagates");
        assert_eq!(
            joiner.join().unwrap(),
            Err(AiError::Internal("leader_panicked".to_owned()))
        );
        assert!(router.lock().in_flight.is_empty());
        // The key is free: the same intent starts a new call.
        assert_eq!(
            router
                .execute(&request("same", "x"), &CancelToken::new())
                .unwrap()
                .value["summary"],
            "fresh"
        );
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
        let result = router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(result.value["summary"], "claude");
        assert_eq!(result.provider, ProviderId::Claude);
        let fallback = sink.events("ai.fallback");
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].detail.as_deref(), Some("from=codex;to=claude"));
        // The failed provider's state is remembered, so the next request
        // goes straight to the fallback without a second failure.
        let result = router
            .execute(&request("p2", "y"), &CancelToken::new())
            .unwrap();
        assert_eq!(result.value["summary"], "late");
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
                .unwrap()
                .value["summary"],
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
                .unwrap()
                .value["summary"],
            "logged in"
        );
    }

    #[test]
    fn concurrent_duplicates_share_one_provider_call() {
        let sink = Arc::new(Recorder::default());
        let codex = Arc::new(Scripted {
            id: ProviderId::Codex,
            availability: Mutex::new(Availability::Ready),
            probes: AtomicUsize::new(0),
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
            assert_eq!(handle.join().unwrap().value["summary"], "shared");
        }
        assert_eq!(codex.calls(), 1);
        assert_eq!(sink.events("ai.request.joined").len(), 2);
        // A different input for the same subject is a new intent.
        router
            .execute(&request("same", "other input"), &CancelToken::new())
            .unwrap();
        assert_eq!(codex.calls(), 2);
    }

    /// The availability cache is deliberately switched off here, so what
    /// keeps a parked provider from being asked is the cooldown itself and
    /// not a cache window that happens to be open.
    fn make_sticky_router(backends: Vec<Arc<dyn AiBackend>>, sink: Arc<Recorder>) -> AiRouter {
        let config = RouterConfig {
            availability_ttl: Duration::ZERO,
            backoff_base: Duration::from_millis(1),
            ..RouterConfig::default()
        };
        AiRouter::with_sleep(backends, config, sink, Box::new(|_| {}))
    }

    /// A consumer has to be able to ask the standing question rather than
    /// infer it from the last result it happened to see.
    #[test]
    fn provider_state_names_the_active_provider_and_why_the_selected_one_stepped_aside() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::with_availability(ProviderId::Codex, Availability::Ready);
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_sticky_router(vec![codex.clone(), claude.clone()], sink);
        assert_eq!(
            router.provider_state().unwrap(),
            ProviderState {
                selected: ProviderId::Codex,
                active: Some(ProviderId::Codex),
                degraded: None,
            }
        );
        codex.set_availability(Availability::NeedsLogin);
        assert_eq!(
            router.provider_state().unwrap(),
            ProviderState {
                selected: ProviderId::Codex,
                active: Some(ProviderId::Claude),
                degraded: Some(Degraded {
                    provider: ProviderId::Codex,
                    reason: Availability::NeedsLogin,
                    until: None,
                }),
            }
        );
        // A query answers; it does not run the request.
        assert_eq!(claude.calls(), 0);
    }

    /// A router with nothing registered has no selected provider, and says so
    /// instead of naming one.
    #[test]
    fn a_router_without_providers_has_no_provider_state() {
        let router = make_router(Vec::new(), Arc::new(Recorder::default()));
        assert_eq!(router.provider_state(), None);
    }

    /// Sticky means the reason outlives the request that found it: the parked
    /// provider is not asked for an answer, and is not even asked whether it
    /// is logged in, until its cooldown expires.
    #[test]
    fn a_usage_limit_is_sticky_and_stops_asking_the_parked_provider_anything() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::UsageLimited {
                retry_after: Some(Duration::from_secs(600)),
            })],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_sticky_router(vec![codex.clone(), claude.clone()], sink.clone());
        assert_eq!(
            router
                .execute(&request("p1", "x"), &CancelToken::new())
                .unwrap()
                .provider,
            ProviderId::Claude
        );
        let probes_after_park = codex.probes();
        for subject in ["p2", "p3", "p4"] {
            assert_eq!(
                router
                    .execute(&request(subject, subject), &CancelToken::new())
                    .unwrap()
                    .provider,
                ProviderId::Claude
            );
        }
        // One request spent on the parked provider, and nothing since.
        assert_eq!(codex.calls(), 1);
        assert_eq!(codex.probes(), probes_after_park, "the park was re-probed");

        let state = router.provider_state().unwrap();
        assert_eq!(state.selected, ProviderId::Codex);
        assert_eq!(state.active, Some(ProviderId::Claude));
        let degraded = state.degraded.expect("the reason stays queryable");
        assert_eq!(degraded.provider, ProviderId::Codex);
        assert!(degraded.until.expect("a cooldown ends") > Duration::from_secs(500));
        assert_eq!(degraded.reason.class(), "unavailable");
        assert_eq!(codex.probes(), probes_after_park);
    }

    /// When the reason ends the router asks the selected provider once and
    /// goes back to it; nobody has to tell it the limit reset.
    #[test]
    fn an_expired_cooldown_re_reads_availability_once_and_returns_to_the_selected_provider() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::UsageLimited {
                retry_after: Some(Duration::from_millis(200)),
            })],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude")]);
        let router = make_sticky_router(vec![codex.clone(), claude.clone()], sink.clone());
        router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        let parked_probes = codex.probes();
        std::thread::sleep(Duration::from_millis(250));
        codex.queue(vec![ok("codex is back")]);

        let state = router.provider_state().unwrap();
        assert_eq!(state.active, Some(ProviderId::Codex));
        assert_eq!(state.degraded, None);
        assert_eq!(
            codex.probes(),
            parked_probes + 1,
            "the return asks the provider exactly once"
        );
        assert_eq!(
            router
                .execute(&request("p2", "y"), &CancelToken::new())
                .unwrap()
                .value["summary"],
            "codex is back"
        );
    }

    /// Both providers down is reachable now that both can answer, so it is a
    /// reported state: no provider is claimed to be active, and the caller is
    /// told the wait it can act on rather than that nothing is installed.
    #[test]
    fn every_provider_degraded_is_reported_and_claims_no_active_provider() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::with_availability(ProviderId::Codex, Availability::NeedsLogin);
        let claude = Scripted::new(
            ProviderId::Claude,
            vec![Err(AiError::UsageLimited {
                retry_after: Some(Duration::from_secs(600)),
            })],
        );
        let router = make_sticky_router(vec![codex.clone(), claude.clone()], sink.clone());
        assert!(matches!(
            router.execute(&request("p1", "x"), &CancelToken::new()),
            Err(AiError::UsageLimited { .. })
        ));

        let state = router.provider_state().unwrap();
        assert_eq!(state.selected, ProviderId::Codex);
        assert_eq!(state.active, None, "no provider can answer");
        assert_eq!(
            state.degraded,
            Some(Degraded {
                provider: ProviderId::Codex,
                reason: Availability::NeedsLogin,
                until: None,
            })
        );
        // Every provider's own reason is still readable next to it.
        let availability = router.availability();
        assert_eq!(availability[0].1, Availability::NeedsLogin);
        match &availability[1].1 {
            Availability::Unavailable { reason } => {
                assert!(reason.starts_with("usage_limited"), "{reason}");
            }
            other => panic!("{other:?}"),
        }

        // The parked wait is the answer, not "no provider": a caller decides
        // its own retry on it.
        match router.execute(&request("p2", "y"), &CancelToken::new()) {
            Err(AiError::UsageLimited { retry_after }) => {
                assert!(retry_after.expect("the wait is named") > Duration::from_secs(500));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(claude.calls(), 1, "the parked provider was asked again");
    }

    /// `ai.fallback` is per-request and a sticky failover has no second
    /// provider to try, so entering and leaving get their own events.
    #[test]
    fn entering_and_leaving_failover_each_leave_their_own_event() {
        let sink = Arc::new(Recorder::default());
        let codex = Scripted::new(
            ProviderId::Codex,
            vec![Err(AiError::ProviderUnavailable("gone".to_owned()))],
        );
        let claude = Scripted::new(ProviderId::Claude, vec![ok("claude"), ok("claude again")]);
        // The availability cache is the mechanism here: an outage discovered
        // during a request is what keeps the next one off that provider.
        let router = make_router(vec![codex.clone(), claude.clone()], sink.clone());

        // The first request discovers the outage mid-flight, so it is the
        // per-request fallback line that records it.
        router
            .execute(&request("p1", "x"), &CancelToken::new())
            .unwrap();
        assert_eq!(sink.events("ai.fallback").len(), 1);
        assert!(sink.events("ai.provider.degraded").is_empty());

        // The next request never tries codex, so without this event the log
        // would show the provider change with no reason.
        router
            .execute(&request("p2", "y"), &CancelToken::new())
            .unwrap();
        let degraded = sink.events("ai.provider.degraded");
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].provider, Some(ProviderId::Claude));
        let detail = degraded[0].detail.clone().expect("a reason");
        assert!(detail.contains("selected=codex"), "{detail}");
        assert!(detail.contains("active=claude"), "{detail}");
        assert!(detail.contains("gone"), "{detail}");
        assert_eq!(sink.events("ai.fallback").len(), 1, "no second fallback");

        // Announced once, not once per request.
        router
            .execute(&request("p3", "z"), &CancelToken::new())
            .unwrap();
        assert_eq!(sink.events("ai.provider.degraded").len(), 1);
        assert!(sink.events("ai.provider.recovered").is_empty());

        codex.queue(vec![ok("codex is back")]);
        router.invalidate_availability();
        assert_eq!(
            router
                .execute(&request("p4", "w"), &CancelToken::new())
                .unwrap()
                .value["summary"],
            "codex is back"
        );
        let recovered = sink.events("ai.provider.recovered");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].provider, Some(ProviderId::Codex));
        assert_eq!(recovered[0].outcome_class, Some("ready"));
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
