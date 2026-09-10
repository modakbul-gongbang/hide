use crate::{AiBackend, AiError, AiRequest, AiResponse, Availability, CancelToken, ProviderId};

/// Reason text is the record of the gap; keep it in step with
/// `agents/runs/hide-ai-provider-layer/CONTRACT-2026-09-10.md`.
pub const UNSUPPORTED_REASON: &str = "no structured-result contract: Herdr agent.prompt and agent.wait submit and complete a turn, but no Herdr method or Claude Code hook returns the answer for a request id in isolation from the user's conversation; headless and print modes are excluded by contract";

/// The Claude Code interactive path has a stable submit (`agent.prompt`) and
/// completion (`agent.wait`, `Stop` hook) contract and no stable structured
/// result or isolation contract, so it is modelled rather than driven: the
/// router sees it, reports it, and never selects it.
#[derive(Debug, Default)]
pub struct ClaudeInteractiveBackend;

impl ClaudeInteractiveBackend {
    pub fn new() -> Self {
        Self
    }
}

impl AiBackend for ClaudeInteractiveBackend {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn availability(&self) -> Availability {
        Availability::Unsupported {
            reason: UNSUPPORTED_REASON.to_owned(),
        }
    }

    fn execute(&self, _: &AiRequest, _: &CancelToken) -> Result<AiResponse, AiError> {
        Err(AiError::Unsupported(UNSUPPORTED_REASON.to_owned()))
    }
}
