use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::model::ProviderUsageSnapshot;

pub const WEEKLY_WINDOW_MINUTES: u64 = 10_080;

const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const CODEX_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const CODEX_CANDIDATE_LIMIT: usize = 32;

pub struct ProviderUsageReader {
    home: Option<PathBuf>,
    last_read: Option<Instant>,
    cached: Vec<ProviderUsageSnapshot>,
}

impl ProviderUsageReader {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            home,
            last_read: None,
            cached: ProviderUsageSnapshot::initial_rows(),
        }
    }

    /// Returns a fresh projection only when the refresh window lapses. File
    /// discovery and JSON parsing stay outside the runtime mutex in the live
    /// poller, and the one-second session tick reuses this cached answer.
    pub fn read_if_due(&mut self) -> Option<Vec<ProviderUsageSnapshot>> {
        if self
            .last_read
            .is_some_and(|last_read| last_read.elapsed() < REFRESH_INTERVAL)
        {
            return None;
        }
        self.last_read = Some(Instant::now());
        self.cached = read_provider_usage(self.home.as_deref());
        Some(self.cached.clone())
    }
}

fn read_provider_usage(home: Option<&Path>) -> Vec<ProviderUsageSnapshot> {
    let checked_at = unix_milliseconds();
    let Some(home) = home else {
        return vec![
            ProviderUsageSnapshot::unavailable(
                "claude",
                "Claude Code",
                "Home directory is unavailable; weekly usage cannot be read",
                checked_at,
            ),
            ProviderUsageSnapshot::unavailable(
                "codex",
                "Codex",
                "Home directory is unavailable; weekly usage cannot be read",
                checked_at,
            ),
        ];
    };

    vec![
        read_claude_usage(&home.join(".claude/.usage-cache.json"), checked_at),
        read_codex_usage(&home.join(".codex/sessions"), checked_at),
    ]
}

fn read_claude_usage(path: &Path, checked_at: u64) -> ProviderUsageSnapshot {
    let value = match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => value,
            Err(error) => {
                return ProviderUsageSnapshot::unavailable(
                    "claude",
                    "Claude Code",
                    format!("Claude Code weekly usage cache is malformed: {error}"),
                    checked_at,
                );
            }
        },
        Err(error) => {
            return ProviderUsageSnapshot::unavailable(
                "claude",
                "Claude Code",
                format!("Claude Code weekly usage cache is unavailable: {error}"),
                checked_at,
            );
        }
    };

    let Some(used_percent) = value.get("1w").and_then(Value::as_f64) else {
        return ProviderUsageSnapshot::unavailable(
            "claude",
            "Claude Code",
            "Claude Code weekly usage cache has no 1w value",
            checked_at,
        );
    };
    available_snapshot(
        "claude",
        "Claude Code",
        used_percent,
        value.get("1w_resets_at").and_then(Value::as_u64),
        checked_at,
    )
}

fn read_codex_usage(sessions_root: &Path, checked_at: u64) -> ProviderUsageSnapshot {
    let candidates = match newest_codex_session_files(sessions_root) {
        Ok(candidates) => candidates,
        Err(error) => {
            return ProviderUsageSnapshot::unavailable(
                "codex",
                "Codex",
                format!("Codex weekly usage sessions are unavailable: {error}"),
                checked_at,
            );
        }
    };

    for path in candidates {
        let Ok(tail) = read_tail(&path, CODEX_TAIL_BYTES) else {
            continue;
        };
        if let Some((used_percent, resets_at)) = parse_latest_codex_weekly_usage(&tail) {
            return available_snapshot("codex", "Codex", used_percent, resets_at, checked_at);
        }
    }

    ProviderUsageSnapshot::unavailable(
        "codex",
        "Codex",
        "No Codex 1w rate-limit event was found in recent sessions",
        checked_at,
    )
}

fn available_snapshot(
    provider: &str,
    label: &str,
    used_percent: f64,
    resets_at_unix_seconds: Option<u64>,
    checked_at: u64,
) -> ProviderUsageSnapshot {
    if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
        return ProviderUsageSnapshot::unavailable(
            provider,
            label,
            format!("{label} weekly usage percentage is outside 0 through 100"),
            checked_at,
        );
    }
    if resets_at_unix_seconds.is_some_and(|reset| reset <= checked_at / 1_000) {
        return ProviderUsageSnapshot::unavailable(
            provider,
            label,
            format!("{label} weekly usage expired at its last reset"),
            checked_at,
        );
    }
    ProviderUsageSnapshot {
        provider: provider.to_owned(),
        label: label.to_owned(),
        window_minutes: WEEKLY_WINDOW_MINUTES,
        state: "available".to_owned(),
        used_percent: Some(used_percent),
        resets_at_unix_seconds,
        message: None,
        last_checked_at_unix_ms: Some(checked_at),
    }
}

fn parse_latest_codex_weekly_usage(contents: &str) -> Option<(f64, Option<u64>)> {
    contents.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<Value>(line).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("event_msg")
            || value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
        {
            return None;
        }
        let limits = value.pointer("/payload/rate_limits")?;
        ["primary", "secondary"].into_iter().find_map(|name| {
            let window = limits.get(name)?;
            if window.get("window_minutes").and_then(Value::as_u64) != Some(WEEKLY_WINDOW_MINUTES) {
                return None;
            }
            Some((
                window.get("used_percent").and_then(Value::as_f64)?,
                Some(window.get("resets_at").and_then(Value::as_u64)?),
            ))
        })
    })
}

fn newest_codex_session_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut level = root.to_path_buf();
    for _ in 0..3 {
        let children = numeric_child_directories(&level)?;
        let Some(next) = children.into_iter().max() else {
            return Err(format!(
                "no dated session directory under {}",
                level.display()
            ));
        };
        level.push(next);
    }

    let entries = fs::read_dir(&level)
        .map_err(|error| format!("could not read {}: {error}", level.display()))?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")).then(
                || {
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    Some((modified, path))
                },
            )?
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    candidates.truncate(CODEX_CANDIDATE_LIMIT);
    Ok(candidates.into_iter().map(|(_, path)| path).collect())
}

fn numeric_child_directories(path: &Path) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.chars()
                .all(|character| character.is_ascii_digit())
                .then_some(name)
        })
        .collect())
}

fn read_tail(path: &Path, maximum_bytes: u64) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("could not stat {}: {error}", path.display()))?
        .len();
    let start = length.saturating_sub(maximum_bytes);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("could not seek {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut contents = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0
        && let Some(first_newline) = contents.find('\n')
    {
        contents.drain(..=first_newline);
    }
    Ok(contents)
}

fn unix_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_cache_projects_only_the_weekly_window() {
        let root = temporary_directory("claude-cache");
        let path = root.join("usage.json");
        fs::write(
            &path,
            br#"{"ts":10,"5h":72,"1w":43.4,"1w_resets_at":4102444800}"#,
        )
        .unwrap();

        let usage = read_claude_usage(&path, 1_000);

        assert_eq!(usage.provider, "claude");
        assert_eq!(usage.window_minutes, WEEKLY_WINDOW_MINUTES);
        assert_eq!(usage.used_percent, Some(43.4));
        assert_eq!(usage.resets_at_unix_seconds, Some(4_102_444_800));
        assert_eq!(usage.state, "available");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_or_expired_claude_values_are_observable_failures() {
        let root = temporary_directory("claude-invalid");
        let path = root.join("usage.json");
        fs::write(&path, br#"{"1w":101,"1w_resets_at":4102444800}"#).unwrap();
        let outside_range = read_claude_usage(&path, 1_000);
        assert_eq!(outside_range.state, "unavailable");
        assert!(
            outside_range
                .message
                .unwrap()
                .contains("outside 0 through 100")
        );

        fs::write(&path, br#"{"1w":40,"1w_resets_at":1}"#).unwrap();
        let expired = read_claude_usage(&path, 2_000);
        assert_eq!(expired.state, "unavailable");
        assert!(expired.message.unwrap().contains("expired"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_parser_chooses_the_exact_weekly_window_and_latest_event() {
        let contents = concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":12.0,\"window_minutes\":10080,\"resets_at\":4102444800}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":90.0,\"window_minutes\":300,\"resets_at\":4102444800},\"secondary\":{\"used_percent\":58.0,\"window_minutes\":10080,\"resets_at\":4102444801}}}}\n",
        );

        assert_eq!(
            parse_latest_codex_weekly_usage(contents),
            Some((58.0, Some(4_102_444_801)))
        );
    }

    #[test]
    fn codex_discovery_uses_the_newest_dated_directory_and_file() {
        let root = temporary_directory("codex-discovery");
        let older = root.join("2026/08/31");
        let newer = root.join("2026/09/01");
        fs::create_dir_all(&older).unwrap();
        fs::create_dir_all(&newer).unwrap();
        fs::write(older.join("old.jsonl"), "old").unwrap();
        fs::write(newer.join("new.jsonl"), "new").unwrap();

        let candidates = newest_codex_session_files(&root).unwrap();

        assert_eq!(candidates, vec![newer.join("new.jsonl")]);
        fs::remove_dir_all(root).unwrap();
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hide-{label}-{}-{}",
            std::process::id(),
            unix_milliseconds()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
