//! Pane-owned local attachment intents. Transport completion is never provider acceptance.
use serde::{Deserialize, Serialize};

pub const PER_PANE_LIMIT: usize = 4;
pub const TOTAL_LIMIT: usize = 16;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Shelf {
    pub pane_id: String,
    pub items: Vec<Attachment>,
    pub notice: Option<String>,
    pub following_bottom: Option<bool>,
    pub viewport_message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub state: State,
    pub message: String,
    pub provider: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Loading,
    Ready,
    Queued,
    HandoffUnconfirmed,
    Failed,
    Dismissed,
}

#[derive(Debug, Deserialize)]
pub struct Intent {
    pub pane_id: String,
    pub id: String,
    pub action: Action,
    pub name: Option<String>,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Stage,
    Prepared,
    Remove,
    Send,
    ReturnToPrompt,
}

pub fn paste(path: &str) -> Result<Vec<u8>, &'static str> {
    if !path.starts_with('/')
        || path
            .chars()
            .any(|c| c.is_control() || c == '"' || c == '\'')
        || path.len() > 4096
    {
        return Err("The staged image path cannot be safely pasted.");
    }
    Ok(format!("\x1b[200~\"{path}\"\x1b[201~").into_bytes())
}

#[cfg(test)]
mod tests {
    #[test]
    fn image_handoff_is_one_bracketed_path_without_prompt_submission() {
        assert_eq!(
            super::paste("/tmp/한글 image.png").unwrap(),
            b"\x1b[200~\"/tmp/\xed\x95\x9c\xea\xb8\x80 image.png\"\x1b[201~"
        );
        for unsafe_path in [
            "relative.png",
            "/tmp/a\n.png",
            "/tmp/a\x1b.png",
            "/tmp/a\".png",
        ] {
            assert!(super::paste(unsafe_path).is_err());
        }
    }
}

/// The TUI adapter only chooses a documented, empirically verified input shape.
/// It makes no claim about provider acceptance, mode, submission or transcript.
pub fn provider_delivery(kind: Option<&str>, path: &str) -> Result<Vec<u8>, &'static str> {
    match kind {
        Some("claude" | "codex") => paste(path),
        _ => Err("No verified image composer handoff is available for this provider."),
    }
}
