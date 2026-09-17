use crate::ProviderId;

/// One structured log line. Identifiers and classes only: never the prompt,
/// the input, the generated text, a token, or a provider thread id.
#[derive(Clone, Debug, PartialEq)]
pub struct AiLogEvent {
    pub event: &'static str,
    pub request_id: Option<String>,
    pub feature_id: Option<&'static str>,
    pub provider: Option<ProviderId>,
    pub outcome_class: Option<&'static str>,
    pub duration_ms: Option<u64>,
    pub input_chars: Option<usize>,
    pub output_tokens: Option<u64>,
    pub attempt: Option<u8>,
    pub schema_version: Option<&'static str>,
    /// The resident app-server's pid, and the process measurement taken after
    /// the turn. Present on codex requests; `descendants`/`rss_bytes` are
    /// `None` when the platform cannot measure, and the detail says
    /// `measurement=unavailable` rather than logging a zero.
    pub app_server_pid: Option<u32>,
    pub descendants: Option<usize>,
    pub rss_bytes: Option<u64>,
    /// Short machine-readable detail such as `from=codex;to=claude`.
    pub detail: Option<String>,
}

impl AiLogEvent {
    pub fn new(event: &'static str) -> Self {
        Self {
            event,
            request_id: None,
            feature_id: None,
            provider: None,
            outcome_class: None,
            duration_ms: None,
            input_chars: None,
            output_tokens: None,
            attempt: None,
            schema_version: None,
            app_server_pid: None,
            descendants: None,
            rss_bytes: None,
            detail: None,
        }
    }
}

pub trait AiLogSink: Send + Sync {
    fn log(&self, event: AiLogEvent);
}

pub struct NoopLogSink;

impl AiLogSink for NoopLogSink {
    fn log(&self, _: AiLogEvent) {}
}
