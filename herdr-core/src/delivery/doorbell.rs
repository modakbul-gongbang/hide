//! A deliberately conservative TUI adapter. Herdr has no atomic composer
//! guard: uncertainty holds a letter, and no code here clears or restores input.
//! ANSI styling is retained because a dim placeholder and a typed draft can
//! contain identical text. This boundary must be reverified against real TUIs.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use hide_herdr_client::{ApiConnector, request_small_response};
use serde_json::{Value, json};

use crate::runtime::Runtime;
use crate::runtime::delivery::Observation;
use crate::wire;

use super::ledger::State;
use super::worker::{Client, Effect, now};

const QUIET_MS: u64 = 30_000;
const SCREEN_LIMIT: usize = 64 * 1024;
const WORK_PER_PASS: usize = 8;
const RPC_TIMEOUT: Duration = Duration::from_millis(500);
const BELL: &str = "Hide has pending mail. Read hide inbox for the full letter if the prompt hook did not include it.";

#[derive(Clone, PartialEq, Eq)]
struct Episode {
    status: String,
    sequence: Option<u64>,
    input: u64,
    session: Option<String>,
}

impl From<&Observation> for Episode {
    fn from(value: &Observation) -> Self {
        Self {
            status: value.status.clone(),
            sequence: value.state_change_seq,
            input: value.last_input_at_unix_ms,
            session: value.actor.session.clone(),
        }
    }
}

pub(crate) fn eligible(observation: &Observation, now: u64) -> bool {
    observation.actor.device_id == "local"
        && matches!(observation.status.as_str(), "idle" | "done")
        && observation.state_change_seq.is_some()
        && now.saturating_sub(observation.last_input_at_unix_ms) >= QUIET_MS
        && matches!(
            observation.actor.kind.as_str(),
            "codex" | "claude" | "claude-code" | "claude_code"
        )
}

pub(crate) fn run(runtime: Weak<Mutex<Runtime>>, client: Client, stop: Arc<AtomicBool>) {
    // Both maps are bounded by the <=1024 pending local letters in this pass.
    // A refusal is retried only after state/input changes, never by screen diff.
    let mut tried = HashMap::<String, Episode>::new();
    let mut rung = HashMap::<String, Episode>::new();
    while !stop.load(Ordering::Acquire) {
        let Some(owner) = runtime.upgrade() else {
            break;
        };
        let context = owner
            .lock()
            .ok()
            .and_then(|guard| guard.delivery_bell_context());
        if let Some((ledger, connector)) = context {
            let mut targets = HashSet::new();
            let pending: Vec<_> = ledger
                .letters
                .iter()
                .filter(|letter| {
                    letter.state == State::Pending
                        && letter.recipient.device_id == "local"
                        && letter.bell_errors < 3
                        && now().saturating_sub(letter.created_at_unix_ms)
                            < super::DELIVERY_EXPIRY_MS
                })
                .filter(|letter| targets.insert(letter.recipient.pane_id.clone()))
                .collect();
            let ids: HashSet<_> = pending.iter().map(|letter| letter.id.as_str()).collect();
            tried.retain(|id, _| ids.contains(id.as_str()));
            rung.retain(|pane, _| targets.contains(pane));
            let mut count = 0;
            for letter in pending {
                if stop.load(Ordering::Acquire) || count >= WORK_PER_PASS {
                    break;
                }
                let observed = owner
                    .lock()
                    .ok()
                    .and_then(|guard| guard.delivery_observation(&letter.recipient));
                let Some(observed) = observed.filter(|value| eligible(value, now())) else {
                    continue;
                };
                let episode = Episode::from(&observed);
                if tried.get(&letter.id) == Some(&episode)
                    || rung.get(&letter.recipient.pane_id) == Some(&episode)
                {
                    continue;
                }
                count += 1;
                tried.insert(letter.id.clone(), episode.clone());
                match deliver(&owner, connector.as_ref(), &letter.id, &observed, &stop) {
                    Ok(true) => {
                        rung.insert(letter.recipient.pane_id.clone(), episode);
                        record(&client, &letter.id, &observed, true);
                    }
                    Ok(false) => {}
                    Err(code) => {
                        crate::diagnostic!(json!({"component":"delivery","kind":"doorbell.failed",
                            "letter_id":letter.id,"pane_id":letter.recipient.pane_id,"code":code}));
                        record(&client, &letter.id, &observed, false);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn record(client: &Client, id: &str, observed: &Observation, sent: bool) {
    if let Err(code) = client.submit(
        Effect::Bell {
            id: id.to_owned(),
            recipient: observed.actor.clone(),
            sent,
        },
        Duration::from_secs(5),
    ) {
        crate::diagnostic!(
            json!({"component":"delivery","kind":"doorbell.transaction_failed",
            "letter_id":id,"code":code})
        );
    }
}

fn rpc(connector: &dyn ApiConnector, method: &str, params: Value) -> Result<Value, &'static str> {
    request_small_response(connector, method, params, RPC_TIMEOUT).map_err(|_| "herdr_call_failed")
}

fn native_matches(
    connector: &dyn ApiConnector,
    observed: &Observation,
) -> Result<bool, &'static str> {
    let parameters =
        wire::agent_target_params(&observed.raw_pane_id).map_err(|_| "herdr_parameters")?;
    let agent = wire::delivery_agent(rpc(connector, "agent.get", parameters)?)
        .map_err(|_| "herdr_format")?;
    Ok(agent.ready
        && agent.pane_id == observed.raw_pane_id
        && agent.name == observed.actor.name
        && agent.kind.as_ref() == Some(&observed.actor.kind)
        && agent.session == observed.actor.session
        && agent.status == observed.status
        && Some(agent.state_change_seq) == observed.state_change_seq)
}

/// All delivery pane writes go through this function. The last memory check
/// cannot eliminate an external socket/TUI input race (the approved D-18 limit).
fn deliver(
    owner: &Mutex<Runtime>,
    connector: &dyn ApiConnector,
    id: &str,
    observed: &Observation,
    stop: &AtomicBool,
) -> Result<bool, &'static str> {
    if !native_matches(connector, observed)? {
        return Ok(false);
    }
    let parameters =
        wire::delivery_screen_params(&observed.raw_pane_id).map_err(|_| "herdr_parameters")?;
    let screen =
        wire::pane_text(rpc(connector, "pane.read", parameters)?).map_err(|_| "herdr_format")?;
    if screen.truncated || !empty_composer(&observed.actor.kind, &screen.text) {
        return Ok(false);
    }
    if !native_matches(connector, observed)? {
        return Ok(false);
    }
    if stop.load(Ordering::Acquire)
        || !owner
            .lock()
            .map_err(|_| "runtime_unavailable")?
            .delivery_bell_current(id, observed)
    {
        return Ok(false);
    }
    let parameters =
        wire::delivery_input_params(&observed.raw_pane_id, BELL).map_err(|_| "herdr_parameters")?;
    rpc(connector, "pane.send_input", parameters)?;
    // Arrival is not intake. Only a flushed prompt-hook confirmation clears it.
    Ok(true)
}

#[derive(Clone, Copy)]
struct Cell {
    value: char,
    placeholder: bool,
}

fn styled_lines(screen: &str) -> Option<Vec<Vec<Cell>>> {
    if screen.len() > SCREEN_LIMIT {
        return None;
    }
    let mut lines = vec![Vec::new()];
    let mut chars = screen.chars().peekable();
    let mut dim = false;
    let mut gray = false;
    while let Some(value) = chars.next() {
        match value {
            '\n' => lines.push(Vec::new()),
            '\r' => {}
            '\x1b' => {
                if chars.next() != Some('[') {
                    return None;
                }
                let mut codes = String::new();
                loop {
                    match chars.next()? {
                        'm' => break,
                        value if value.is_ascii_digit() || value == ';' => {
                            if codes.len() >= 64 {
                                return None;
                            }
                            codes.push(value);
                        }
                        _ => return None,
                    }
                }
                let values: Vec<u16> = if codes.is_empty() {
                    vec![0]
                } else {
                    codes
                        .split(';')
                        .map(str::parse)
                        .collect::<Result<_, _>>()
                        .ok()?
                };
                let mut values = values.into_iter();
                while let Some(code) = values.next() {
                    match code {
                        0 => {
                            dim = false;
                            gray = false;
                        }
                        2 => dim = true,
                        22 => dim = false,
                        90 => gray = true,
                        30..=37 | 39 | 91..=97 => gray = false,
                        38 | 48 => {
                            let mode = values.next()?;
                            let is_gray = match mode {
                                5 => matches!(values.next()?, 240..=249),
                                2 => {
                                    let (r, g, b) =
                                        (values.next()?, values.next()?, values.next()?);
                                    r == g && g == b && (80..=180).contains(&r)
                                }
                                _ => return None,
                            };
                            if code == 38 {
                                gray = is_gray;
                            }
                        }
                        _ => {}
                    }
                }
            }
            value if value.is_control() => return None,
            value => lines.last_mut()?.push(Cell {
                value,
                placeholder: dim || gray,
            }),
        }
    }
    Some(lines)
}

/// A positive prompt row plus the runtime's footer is required. A multiline
/// draft, slash picker, unstyled placeholder or unknown footer refuses input.
fn empty_composer(kind: &str, screen: &str) -> bool {
    let Some(lines) = styled_lines(screen) else {
        return false;
    };
    let glyph = if kind == "codex" { '›' } else { '❯' };
    let Some(index) = lines.iter().rposition(|line| {
        line.iter()
            .find(|cell| !cell.value.is_whitespace())
            .is_some_and(|cell| cell.value == glyph)
    }) else {
        return false;
    };
    let prompt = &lines[index];
    let Some(glyph_index) = prompt.iter().position(|cell| cell.value == glyph) else {
        return false;
    };
    if prompt[glyph_index + 1..]
        .iter()
        .any(|cell| !cell.value.is_whitespace() && !cell.placeholder)
    {
        return false;
    }
    let mut footer = false;
    for line in &lines[index + 1..] {
        let text: String = line.iter().map(|cell| cell.value).collect();
        let text = text.trim();
        if text.is_empty() || text.chars().all(|value| matches!(value, '─' | '━' | ' ')) {
            continue;
        }
        let known = if kind == "codex" {
            text.starts_with('?') && text.contains("for shortcuts")
                || text.contains("% left") && text.contains('·')
        } else {
            text.contains("shift+tab") && (text.contains("permissions") || text.contains("mode"))
                || text == "? for shortcuts"
        };
        if !known {
            return false;
        }
        footer = true;
    }
    footer
}

#[cfg(test)]
mod tests {
    use super::empty_composer;

    #[test]
    fn composer_refuses_draft_menu_and_unknown_layout() {
        // D-20: identical placeholder text must remain a draft without dim.
        assert!(empty_composer(
            "claude",
            "❯ \x1b[2mTry a task\x1b[0m\n? for shortcuts"
        ));
        assert!(!empty_composer("claude", "❯ Try a task\n? for shortcuts"));
        assert!(empty_composer("codex", "› \n? for shortcuts"));
        assert!(!empty_composer(
            "codex",
            "› meaningful draft\n? for shortcuts"
        ));
        assert!(!empty_composer(
            "codex",
            "› \nsecond draft line\n? for shortcuts"
        ));
        assert!(!empty_composer(
            "codex",
            "› /model\nSelect a model\n? for shortcuts"
        ));
        assert!(!empty_composer("codex", "› \nEnter to select"));
        assert!(!empty_composer("codex", "› "));
    }
}
