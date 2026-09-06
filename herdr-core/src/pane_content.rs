//! Content identity is separate from Herdr's pane/layout identity.
//!
//! Browser hosts resolve chromux profiles and advertise stable identifiers plus
//! the current loopback port. The port is live transport state, not persisted
//! browser identity; profile storage never crosses this boundary.
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneContent {
    #[default]
    Terminal,
    Browser {
        binding_id: String,
        profile: String,
        target_id: String,
        session: String,
        cdp_port: u16,
        owns_target: bool,
    },
    Unavailable {
        reason: String,
    },
}

impl PaneContent {
    /// Only explicit host metadata opts a pane into native browser rendering.
    /// Incomplete/unknown content must remain visibly unavailable, not silently
    /// become a terminal or attach to a different browser/profile.
    pub fn from_tokens(tokens: &BTreeMap<String, Value>, remote: bool) -> Self {
        let Some(kind) = tokens.get("hide_content") else {
            return Self::Terminal;
        };
        let unavailable = |reason: &str| Self::Unavailable {
            reason: reason.to_owned(),
        };
        if kind.as_str() != Some("browser-v1") {
            return unavailable("Unsupported Hide pane content version");
        }
        if remote {
            return unavailable(
                "This browser belongs to a remote host; local CDP attachment is disabled",
            );
        }
        if let Some(error) = tokens.get("hide_browser_error") {
            return unavailable(
                error
                    .as_str()
                    .filter(|message| {
                        !message.is_empty()
                            && message.len() <= 320
                            && !message.chars().any(char::is_control)
                    })
                    .unwrap_or("Browser host reported an invalid error"),
            );
        }
        let identifier = |key: &str| {
            tokens
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 80
                        && value.as_bytes()[0].is_ascii_alphanumeric()
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                })
                .map(str::to_owned)
        };
        let (Some(binding_id), Some(profile), Some(target_id), Some(session)) = (
            identifier("hide_browser_binding"),
            identifier("hide_browser_profile"),
            identifier("hide_browser_target"),
            identifier("hide_browser_session"),
        ) else {
            return unavailable("Browser host metadata is incomplete or invalid");
        };
        let Some(cdp_port) = tokens
            .get("hide_browser_cdp_port")
            .and_then(Value::as_str)
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0)
        else {
            return unavailable("Browser host CDP port is missing or invalid");
        };
        let owns_target = match tokens
            .get("hide_browser_owns_target")
            .and_then(Value::as_str)
        {
            Some("true") => true,
            Some("false") => false,
            _ => return unavailable("Browser host target ownership is missing or invalid"),
        };
        Self::Browser {
            binding_id,
            profile,
            target_id,
            session,
            cdp_port,
            owns_target,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_tokens() -> BTreeMap<String, Value> {
        serde_json::from_value(serde_json::json!({
            "hide_content": "browser-v1",
            "hide_browser_binding": "qa-login",
            "hide_browser_profile": "work.qa",
            "hide_browser_target": "ABC123",
            "hide_browser_session": "hide-qa-login",
            "hide_browser_cdp_port": "9300",
            "hide_browser_owns_target": "false"
        }))
        .unwrap()
    }

    #[test]
    fn browser_identity_survives_projection_without_copying_profile_storage() {
        let content = PaneContent::from_tokens(&browser_tokens(), false);
        let wire = serde_json::to_value(content).unwrap();
        assert_eq!(wire["kind"], "browser");
        assert_eq!(wire["profile"], "work.qa");
        assert_eq!(wire["target_id"], "ABC123");
        assert_eq!(wire["binding_id"], "qa-login");
    }

    #[test]
    fn ordinary_panes_remain_terminal_but_invalid_browser_hosts_do_not() {
        assert!(PaneContent::from_tokens(&BTreeMap::new(), false).is_terminal());
        let mut tokens = browser_tokens();
        tokens.remove("hide_browser_target");
        assert!(matches!(
            PaneContent::from_tokens(&tokens, false),
            PaneContent::Unavailable { .. }
        ));
        tokens.insert(
            "hide_browser_target".into(),
            Value::String("../another-profile".into()),
        );
        assert!(matches!(
            PaneContent::from_tokens(&tokens, false),
            PaneContent::Unavailable { .. }
        ));
        tokens.insert("hide_content".into(), Value::String("browser-v2".into()));
        assert!(matches!(
            PaneContent::from_tokens(&tokens, false),
            PaneContent::Unavailable { .. }
        ));
    }

    #[test]
    fn remote_browser_cannot_resolve_to_a_local_profile_with_the_same_name() {
        assert!(matches!(
            PaneContent::from_tokens(&browser_tokens(), true),
            PaneContent::Unavailable { .. }
        ));
    }

    #[test]
    fn host_failure_is_visible_until_a_successful_lease_clears_it() {
        let mut tokens = browser_tokens();
        tokens.insert(
            "hide_browser_error".into(),
            Value::String("Browser target is closed".into()),
        );
        assert_eq!(
            PaneContent::from_tokens(&tokens, false),
            PaneContent::Unavailable {
                reason: "Browser target is closed".into(),
            }
        );
        tokens.remove("hide_browser_error");
        assert!(matches!(
            PaneContent::from_tokens(&tokens, false),
            PaneContent::Browser { .. }
        ));
    }
}
