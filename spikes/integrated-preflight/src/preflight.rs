use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use serde_json::{Map, Value, json};

const TELEMETRY_SCHEMA: &str = "herdr.t1-preflight.telemetry.v1";
const RESERVED_FIELDS: &[&str] = &[
    "schema",
    "run_id",
    "seq",
    "monotonic_ns",
    "pid",
    "phase",
    "event",
];

#[derive(Clone, Debug)]
pub struct Contract {
    run_id: String,
    phase: String,
    events_path: PathBuf,
    scenario_path: Option<PathBuf>,
}

struct EventLog {
    run_id: String,
    phase: String,
    writer: BufWriter<File>,
    started_at: Instant,
    seq: u64,
    last_monotonic_ns: u64,
    completed: bool,
}

thread_local! {
    static EVENT_LOG: RefCell<Option<EventLog>> = const { RefCell::new(None) };
}

impl Contract {
    pub fn from_parts(
        run_id: Option<String>,
        phase: Option<String>,
        events_path: Option<PathBuf>,
        scenario_path: Option<PathBuf>,
    ) -> Result<Option<Self>> {
        if run_id.is_none() && phase.is_none() && events_path.is_none() && scenario_path.is_none() {
            return Ok(None);
        }
        let phase = phase.ok_or_else(|| {
            anyhow!("stage=preflight.contract cause=phase-and-events-must-be-provided-together")
        })?;
        let events_path = events_path.ok_or_else(|| {
            anyhow!("stage=preflight.contract cause=phase-and-events-must-be-provided-together")
        })?;
        if !matches!(
            phase.as_str(),
            "clean_closed" | "warm_closed" | "browser_included" | "relaunch_closed"
        ) {
            return Err(anyhow!(
                "stage=preflight.contract field=phase cause=invalid-value value={phase:?}"
            ));
        }
        if scenario_path.as_ref() == Some(&events_path) {
            return Err(anyhow!(
                "stage=preflight.contract cause=events-path-must-not-overwrite-scenario"
            ));
        }
        let scenario = scenario_path
            .as_ref()
            .map(|path| load_scenario(path))
            .transpose()?;
        let scenario_run_id = scenario
            .as_ref()
            .and_then(|value| value.get("run_id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let run_id = match (run_id, scenario_run_id) {
            (Some(argument), Some(scenario)) if argument != scenario => {
                return Err(anyhow!(
                    "stage=preflight.contract cause=run-id-mismatch argument={argument:?} scenario={scenario:?}"
                ));
            }
            (Some(argument), _) => argument,
            (None, Some(scenario)) => scenario,
            (None, None) => {
                return Err(anyhow!(
                    "stage=preflight.contract cause=run-id-or-scenario-required"
                ));
            }
        };
        if run_id.is_empty()
            || run_id.len() > 64
            || !run_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
        {
            return Err(anyhow!(
                "stage=preflight.contract field=run-id cause=invalid-value value={run_id:?}"
            ));
        }
        Ok(Some(Self {
            run_id,
            phase,
            events_path,
            scenario_path,
        }))
    }

    pub fn start(self) -> Result<()> {
        create_parent(&self.events_path)?;
        let writer = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&self.events_path)
            .with_context(|| {
                format!(
                    "stage=preflight.events.open path={} cause=must-not-preexist retryable=false",
                    self.events_path.display()
                )
            })?;
        EVENT_LOG.with(|slot| {
            if slot.borrow().is_some() {
                return Err(anyhow!(
                    "stage=preflight.events.init cause=already-initialized"
                ));
            }
            *slot.borrow_mut() = Some(EventLog {
                run_id: self.run_id,
                phase: self.phase,
                writer: BufWriter::new(writer),
                started_at: Instant::now(),
                seq: 0,
                last_monotonic_ns: 0,
                completed: false,
            });
            Ok(())
        })?;
        if let Some(path) = self.scenario_path {
            emit(
                "scenario.ready",
                json!({
                    "path": path,
                    "mode": "read-only",
                    "valid_json_object": true,
                }),
            );
        }
        Ok(())
    }
}

pub fn emit(event: &'static str, details: Value) {
    if let Err(error) = try_emit(event, details) {
        eprintln!("event=preflight.events.failed target={event} retryable=false error={error:#?}");
        // A dropped event must be observable in the stream the harness reads,
        // not only on stderr; the failure record's own keys are envelope-safe.
        let failure = json!({ "target": event, "cause": format!("{error:#}") });
        if let Err(error) = try_emit("telemetry.emit_failed", failure) {
            eprintln!(
                "event=preflight.events.failed target=telemetry.emit_failed retryable=false error={error:#?}"
            );
        }
    }
}

pub fn active() -> bool {
    EVENT_LOG.with(|slot| slot.borrow().is_some())
}

pub fn complete(browser_enabled: bool) {
    EVENT_LOG.with(|slot| {
        let should_emit = slot
            .borrow()
            .as_ref()
            .is_some_and(|log| !log.completed);
        if should_emit {
            if let Err(error) = try_emit(
                "phase.complete",
                json!({ "browser_enabled": browser_enabled }),
            ) {
                eprintln!(
                    "event=preflight.events.failed target=phase.complete retryable=false error={error:#?}"
                );
            }
            if let Some(log) = slot.borrow_mut().as_mut() {
                log.completed = true;
                if let Err(error) = log.writer.flush() {
                    eprintln!(
                        "event=preflight.events.flush.failed retryable=false error={error:?}"
                    );
                }
            }
        }
    });
}

pub fn monotonic_ns() -> u64 {
    EVENT_LOG.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|log| log.started_at.elapsed().as_nanos().min(u64::MAX as u128) as u64)
            .unwrap_or(0)
    })
}

pub fn validate_details<'details>(
    event: &str,
    details: &'details Value,
) -> Result<&'details Map<String, Value>> {
    let details = details.as_object().ok_or_else(|| {
        anyhow!("stage=preflight.events.serialize event={event} cause=details-must-be-object")
    })?;
    if let Some(field) = RESERVED_FIELDS
        .iter()
        .find(|field| details.contains_key(**field))
    {
        return Err(anyhow!(
            "stage=preflight.events.serialize event={event} cause=reserved-field field={field}"
        ));
    }
    Ok(details)
}

fn try_emit(event: &'static str, details: Value) -> Result<()> {
    EVENT_LOG.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(log) = slot.as_mut() else {
            return Ok(());
        };
        let details = validate_details(event, &details)?;
        log.seq += 1;
        let monotonic_ns = (log.started_at.elapsed().as_nanos().min(u64::MAX as u128) as u64)
            .max(log.last_monotonic_ns.saturating_add(1));
        log.last_monotonic_ns = monotonic_ns;
        let mut record = Map::new();
        record.insert("schema".to_owned(), Value::from(TELEMETRY_SCHEMA));
        record.insert("run_id".to_owned(), Value::from(log.run_id.clone()));
        record.insert("seq".to_owned(), Value::from(log.seq));
        record.insert("monotonic_ns".to_owned(), Value::from(monotonic_ns));
        record.insert("pid".to_owned(), Value::from(std::process::id()));
        record.insert("phase".to_owned(), Value::from(log.phase.clone()));
        record.insert("event".to_owned(), Value::from(event));
        record.extend(details.clone());
        serde_json::to_writer(&mut log.writer, &Value::Object(record))
            .context("stage=preflight.events.serialize")?;
        log.writer
            .write_all(b"\n")
            .context("stage=preflight.events.write")?;
        log.writer.flush().context("stage=preflight.events.flush")
    })
}

fn load_scenario(path: &Path) -> Result<Value> {
    let scenario_bytes = std::fs::read(path).with_context(|| {
        format!(
            "stage=preflight.scenario.read path={} retryable=false",
            path.display()
        )
    })?;
    let scenario: Value = serde_json::from_slice(&scenario_bytes).with_context(|| {
        format!(
            "stage=preflight.scenario.parse path={} retryable=false",
            path.display()
        )
    })?;
    if !scenario.is_object() {
        return Err(anyhow!(
            "stage=preflight.scenario.parse path={} cause=root-must-be-object",
            path.display()
        ));
    }
    Ok(scenario)
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "stage=preflight.events.mkdir path={} retryable=false",
                parent.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_is_optional_only_when_all_identity_arguments_are_absent() {
        assert!(
            Contract::from_parts(None, None, None, None)
                .unwrap()
                .is_none()
        );
        let error = Contract::from_parts(
            Some("test".to_owned()),
            Some("clean_closed".to_owned()),
            None,
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("provided-together"));
    }

    fn start_contract_for_test(name: &str) -> PathBuf {
        let events_path = std::env::temp_dir().join(format!(
            "herdr-preflight-{name}-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&events_path);
        Contract::from_parts(
            Some("test".to_owned()),
            Some("warm_closed".to_owned()),
            Some(events_path.clone()),
            None,
        )
        .unwrap()
        .unwrap()
        .start()
        .unwrap();
        events_path
    }

    fn read_events(path: &Path) -> Vec<Value> {
        let raw = std::fs::read_to_string(path).unwrap();
        let _ = std::fs::remove_file(path);
        raw.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn a_reserved_field_payload_is_dropped_and_the_drop_is_in_the_event_stream() {
        // EVENT_LOG is thread-local, and libtest runs each test on its own thread.
        let events_path = start_contract_for_test("reserved-field");
        emit(
            "input.action.armed",
            json!({ "action": "terminal.plain_key_control", "phase": "warm_closed" }),
        );
        let events = read_events(&events_path);
        assert!(
            events
                .iter()
                .all(|event| event["event"] != "input.action.armed")
        );
        let failure = events
            .iter()
            .find(|event| event["event"] == "telemetry.emit_failed")
            .expect("a dropped emit must be observable in the telemetry stream");
        assert_eq!(failure["target"], "input.action.armed");
        let cause = failure["cause"].as_str().unwrap();
        assert!(cause.contains("cause=reserved-field"));
        assert!(cause.contains("field=phase"));
    }

    #[test]
    fn a_valid_payload_after_a_dropped_one_keeps_the_sequence_contiguous() {
        let events_path = start_contract_for_test("sequence-after-drop");
        emit("broken.event", json!({ "seq": 7 }));
        emit("healthy.event", json!({ "value": 1 }));
        let events = read_events(&events_path);
        let sequences: Vec<u64> = events
            .iter()
            .map(|event| event["seq"].as_u64().unwrap())
            .collect();
        assert_eq!(sequences, vec![1, 2]);
        assert_eq!(events[0]["event"], "telemetry.emit_failed");
        assert_eq!(events[0]["target"], "broken.event");
        assert_eq!(events[1]["event"], "healthy.event");
    }

    #[test]
    fn contract_rejects_an_invalid_phase_before_opening_event_output() {
        let error = Contract::from_parts(
            Some("test".to_owned()),
            Some("unknown".to_owned()),
            Some(PathBuf::from("events.jsonl")),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid-value"));
    }
}
