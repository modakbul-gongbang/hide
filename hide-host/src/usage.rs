//! The machine side of the toolbar's Weekly Usage rows
//! (`hide_node_link::usage`): the Codex login in `auth.json`, the weekly
//! window the newest Codex session recorded, and `claude -p /usage` run with
//! the operator's own login.

use std::path::{Path, PathBuf};

use hide_ai::{CancelToken, ClaudeCliBackend, ClaudeConfig};
use hide_node_link::usage::{
    CodexCredentials, CodexWeeklyUsage, CredentialsAnswer, CredentialsRefusal, UsageText,
    WEEKLY_WINDOW_MINUTES,
};
use hide_session::{newest_codex_session_files, parse_rfc3339, read_tail};
use serde_json::Value;

const CODEX_TAIL_BYTES: u64 = 2 * 1024 * 1024;
/// The name the CLI is looked up by on `PATH`.
const CLAUDE_BINARY: &str = hide_agent_adapter::AgentId::ClaudeCode
    .adapter()
    .executables[0];

/// The token and account in `<codex_home>/auth.json`.
pub fn codex_credentials(codex_home: &Path) -> CredentialsAnswer {
    let refused = |refusal| CredentialsAnswer::Refused { refusal };
    let Some(value) = std::fs::read(codex_home.join("auth.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return refused(CredentialsRefusal::Missing);
    };
    let token = |name: &str| {
        value
            .get("tokens")?
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
    };
    match (token("access_token"), token("account_id")) {
        (Some(access_token), Some(account_id)) => CredentialsAnswer::Found {
            credentials: CodexCredentials {
                access_token,
                account_id,
            },
        },
        _ => refused(CredentialsRefusal::Schema),
    }
}

/// The weekly window the newest Codex session under `codex_home` recorded.
pub fn codex_session_usage(codex_home: &Path) -> Option<CodexWeeklyUsage> {
    let candidates = newest_codex_session_files(&codex_home.join("sessions")).ok()?;
    candidates.into_iter().find_map(|path| {
        let tail = read_tail(&path, CODEX_TAIL_BYTES).ok()?;
        parse_latest_codex_weekly_usage(&tail)
    })
}

fn parse_latest_codex_weekly_usage(contents: &str) -> Option<CodexWeeklyUsage> {
    contents.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<Value>(line).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("event_msg")
            || value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
        {
            return None;
        }
        let limits = value.pointer("/payload/rate_limits")?;
        let weekly = ["primary", "secondary"].into_iter().find_map(|name| {
            let window = limits.get(name)?;
            (window.get("window_minutes").and_then(Value::as_u64) == Some(WEEKLY_WINDOW_MINUTES))
                .then_some(window)
        })?;
        let source_at = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339)
            .map(|seconds| seconds.saturating_mul(1_000));
        Some(CodexWeeklyUsage {
            used_percent: weekly.get("used_percent").and_then(Value::as_f64)?,
            resets_at_unix_seconds: weekly.get("resets_at").and_then(Value::as_u64)?,
            source_at_unix_ms: source_at,
        })
    })
}

/// Runs `claude -p /usage` in `cwd` and answers what it printed. The run
/// reports through `report` at least once a second; a report answered
/// with false cancels it, which kills the child.
pub fn claude_usage_text(cwd: &Path, report: &mut dyn FnMut() -> bool) -> UsageText {
    let backend = ClaudeCliBackend::new(ClaudeConfig {
        binary: PathBuf::from(CLAUDE_BINARY),
        cwd: cwd.to_path_buf(),
        ..ClaudeConfig::default()
    });
    let cancel = CancelToken::new();
    let running = cancel.clone();
    let result = crate::reporting::run_reporting(
        "hide-node-claude-usage",
        move || backend.usage_text(&running),
        &|| cancel.cancel(),
        report,
    )
    .unwrap_or_else(|reason| Err(hide_ai::UsageError::Failed(reason)));
    match result {
        Ok(text) => UsageText::Text { text },
        Err(error) => UsageText::Failed { error },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_parser_handles_utc_fraction_and_offsets() {
        assert_eq!(parse_rfc3339("2030-01-01T00:00:00Z"), Some(1_893_456_000));
        assert_eq!(
            parse_rfc3339("2030-01-01T09:00:00.123+09:00"),
            Some(1_893_456_000)
        );
        assert_eq!(parse_rfc3339("2026-02-29T00:00:00Z"), None);
    }

    #[test]
    fn codex_parser_chooses_the_exact_weekly_window_and_latest_event() {
        let contents = concat!(
            "{\"timestamp\":\"2026-09-14T12:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":12.0,\"window_minutes\":10080,\"resets_at\":4102444800}}}}\n",
            "{\"timestamp\":\"2026-09-14T12:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":90.0,\"window_minutes\":300,\"resets_at\":4102444800},\"secondary\":{\"used_percent\":58.0,\"window_minutes\":10080,\"resets_at\":4102444801}}}}\n",
        );
        assert_eq!(
            parse_latest_codex_weekly_usage(contents),
            Some(CodexWeeklyUsage {
                used_percent: 58.0,
                resets_at_unix_seconds: 4_102_444_801,
                source_at_unix_ms: Some(1_789_387_260_000),
            })
        );
    }

    #[test]
    fn credentials_are_read_from_auth_json_and_a_missing_token_is_a_schema_refusal() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            codex_credentials(home.path()),
            CredentialsAnswer::Refused {
                refusal: CredentialsRefusal::Missing
            }
        );
        std::fs::write(
            home.path().join("auth.json"),
            r#"{"tokens":{"access_token":"t","account_id":" "}}"#,
        )
        .unwrap();
        assert_eq!(
            codex_credentials(home.path()),
            CredentialsAnswer::Refused {
                refusal: CredentialsRefusal::Schema
            }
        );
        std::fs::write(
            home.path().join("auth.json"),
            r#"{"tokens":{"access_token":"t","account_id":"a"}}"#,
        )
        .unwrap();
        assert_eq!(
            codex_credentials(home.path()),
            CredentialsAnswer::Found {
                credentials: CodexCredentials {
                    access_token: "t".into(),
                    account_id: "a".into()
                }
            }
        );
    }
}
