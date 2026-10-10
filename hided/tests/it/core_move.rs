//! Moving the core to another machine (PRD core-host-node-move): candidate
//! hided on both machines, the pinned Herdr on each, and a real SSH
//! connection that only the source opens.
#![cfg(unix)]

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::support::core_move::{ALIAS, Fixture, SOURCE_NODE, TARGET_NODE};
use crate::support::remote_delivery::wait_for;

/// B4: the move stops the source's core, places its brain state on the
/// target and starts the core there, and the source's window, at the same
/// address, becomes a node of that core and shows the same projects of the
/// same machines.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn the_core_moves_to_the_device_and_the_window_follows_it() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = fixture.projects()?;
        ensure!(
            before
                == vec![
                    (
                        SOURCE_NODE.to_owned(),
                        fixture.source.project().display().to_string()
                    ),
                    (
                        ALIAS.to_owned(),
                        fixture.target.project().display().to_string()
                    ),
                ],
            "the projects before the move: {before:?}"
        );
        let window = fixture.window();
        let instance = fixture.health()?["instance"].as_u64().context("instance")?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let journal = fixture.journal_until("done")?;
        let intent = journal["intent"].as_str().context("intent")?.to_owned();

        // The window kept its address, and a node role answers there now.
        let state = fixture.source.daemon()?.context("source state")?;
        ensure!(
            (state["port"].as_u64(), state["token"].as_str())
                == (Some(u64::from(window.0)), Some(window.1.as_str())),
            "the window's address changed: {state}"
        );
        ensure!(
            fixture.health()?["instance"].as_u64() > Some(instance),
            "no new role is mounted"
        );
        let placement = fixture
            .source
            .record("core-placement.json")?
            .context("placement")?;
        ensure!(placement["node"] == TARGET_NODE, "{placement}");
        ensure!(placement.get("move_intent").is_none(), "{placement}");

        // The source's brain state is set aside, the target's core runs on
        // the copy and the move left no record there.
        let moved_out = fixture.source.state.join("moved-out").join(&intent);
        ensure!(
            moved_out.join("core-state.json").is_file(),
            "nothing set aside"
        );
        ensure!(!fixture.source.state.join("core-state.json").exists());
        ensure!(fixture.target_core()?.is_some(), "no core on the target");
        ensure!(fixture.target.record("core-handover.json")?.is_none());

        // The same projects of the same machines, each now named as the new
        // core names them.
        let after = wait_for("the projects through the new core", || {
            let projects = fixture.projects()?;
            Ok((projects.len() == 2).then_some(projects))
        })?;
        ensure!(
            after
                == vec![
                    (
                        SOURCE_NODE.to_owned(),
                        fixture.source.project().display().to_string()
                    ),
                    (
                        TARGET_NODE.to_owned(),
                        fixture.target.project().display().to_string()
                    ),
                ],
            "the projects after the move: {after:?}"
        );
        let registrations = fixture.snapshot()?;
        let devices: Vec<&Value> = registrations
            .pointer("/ui_state/device_registrations")
            .and_then(Value::as_array)
            .context("device registrations")?
            .iter()
            .collect();
        ensure!(
            devices.len() == 1 && devices[0]["id"] == SOURCE_NODE && devices[0]["inbound"] == true,
            "{devices:?}"
        );
        Ok(())
    })();
    finish(fixture, journey)
}

/// The source's window state the journeys compare: its projects and the
/// devices it registers.
fn visible(fixture: &Fixture) -> Result<Value> {
    let snapshot = fixture.snapshot()?;
    Ok(json!({
        "projects": fixture.projects()?,
        "devices": snapshot.pointer("/ui_state/device_registrations").cloned(),
    }))
}

/// A move whose check fails changes nothing on either machine: the source
/// keeps its core and its window, and the target keeps its folder as it was.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_failing_check_changes_nothing() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let planted = fixture.target.state.join("core-state.json");
        std::fs::write(&planted, b"{\"schema_version\":0}")?;
        let target_before = listing(&fixture.target.state)?;
        let before = visible(&fixture)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let failed = fixture.logged(&fixture.source, "checks.failed")?;
        ensure!(failed["checks"] == json!(["target_state"]), "{failed}");
        ensure!(fixture.role()? == "core");
        ensure!(visible(&fixture)? == before, "the window changed");
        for name in ["core-move.json", "core-placement.json", "move-staging"] {
            ensure!(
                !fixture.source.state.join(name).exists(),
                "the source has {name}"
            );
        }
        ensure!(
            listing(&fixture.target.state)? == target_before,
            "the target changed"
        );
        ensure!(std::fs::read(&planted)? == b"{\"schema_version\":0}");
        Ok(())
    })();
    finish(fixture, journey)
}

/// The target's core cannot start after the source's core stopped: the
/// source waits while a core may still be starting there, then starts its
/// own core again unchanged; the retry keeps the move's intent and sends
/// nothing the target already holds.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_target_core_that_cannot_start_is_rolled_back_and_the_retry_reuses_the_copy() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        // Another process holds the target folder's instance lock, so the
        // core the move starts there exits at once.
        let lock = hided::state_file::acquire_lock(&fixture.target.state)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let failed = fixture.logged(&fixture.source, "abort.failed")?;
        let intent = failed["intent"].as_str().context("intent")?.to_owned();
        // Never two cores: while the target's folder may still start one,
        // the source runs none.
        ensure!(fixture.role()? == "moving");
        ensure!(fixture.target_core()?.is_none());
        drop(lock);
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "start_target", "{journal}");
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.source.state.join("core-placement.json").exists());
        ensure!(fixture.source.state.join("core-state.json").is_file());
        // The target holds no brain state, only the copy for a retry.
        ensure!(!fixture.target.state.join("core-state.json").exists());
        ensure!(fixture.target.record("core-handover.json")?.is_none());
        let incoming = fixture.target.state.join("move-incoming").join(&intent);
        ensure!(incoming.join("core-state.json").is_file(), "no copy kept");

        fixture.device_ready()?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        wait_for("the retry started", || {
            Ok((records(&fixture, "move.started")?.len() == 2).then_some(()))
        })?;
        let journal = fixture.journal_until("done")?;
        ensure!(
            journal["intent"] == intent.as_str(),
            "the retry took a new intent"
        );
        let sent: Vec<Value> = records(&fixture, "copy.sent")?;
        ensure!(sent.len() == 2, "{sent:?}");
        // The source's core ran between the two tries, so a store it wrote
        // since is sent again; the rest the target already held.
        let held = sent[1]["held"].as_u64().context("held")?;
        let uploaded = sent[1]["uploaded"].as_array().context("uploaded")?;
        ensure!(
            held >= 5 && held as usize + uploaded.len() == 7,
            "the retry sent the copy again: {sent:?}"
        );
        ensure!(fixture.target_core()?.is_some(), "no core on the target");
        ensure!(!incoming.exists(), "the copy stayed after the move");
        Ok(())
    })();
    finish(fixture, journey)
}

/// A source killed while its target's core starts finds the move in its
/// journal at its next start and undoes it before it runs anything.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_source_killed_mid_move_rolls_back_on_its_next_start() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        let lock = hided::state_file::acquire_lock(&fixture.target.state)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        fixture.logged(&fixture.source, "abort.failed")?;
        fixture.kill_source()?;
        drop(lock);
        fixture.start_source()?;
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["cause"]["kind"] == "local", "{journal}");
        ensure!(journal["phase"]["failed"] == "start_target", "{journal}");
        fixture.logged(&fixture.source, "move.resumed")?;
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.target.state.join("core-state.json").exists());
        ensure!(fixture.target_core()?.is_none());
        Ok(())
    })();
    finish(fixture, journey)
}

/// The new core took the link but the source never heard it: at its next
/// start the source reads the target's handover and goes forward.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_lost_link_answer_after_the_target_took_the_move_goes_forward() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let mut journal = fixture.journal_until("done")?;
        let after = wait_for("the projects through the new core", || {
            let projects = fixture.projects()?;
            Ok((projects.len() == 2).then_some(projects))
        })?;
        fixture.kill_source()?;
        // The state a lost answer leaves: the source recorded the link as
        // sent, the target recorded the move as active.
        let intent = journal["intent"].as_str().context("intent")?.to_owned();
        journal["phase"] = json!({"phase": "attach_sent"});
        write_record(&fixture.source.state.join("core-move.json"), &journal)?;
        let mut placement = fixture
            .source
            .record("core-placement.json")?
            .context("placement")?;
        placement["move_intent"] = json!(intent);
        write_record(
            &fixture.source.state.join("core-placement.json"),
            &placement,
        )?;
        write_record(
            &fixture.target.state.join("core-handover.json"),
            &json!({"version": 1, "intent": intent, "source": SOURCE_NODE, "target": TARGET_NODE, "state": {"state": "active"}}),
        )?;
        fixture.start_source()?;
        fixture.journal_until("done")?;
        ensure!(fixture.role()? == "node");
        ensure!(fixture.target.record("core-handover.json")?.is_none());
        let placement = fixture
            .source
            .record("core-placement.json")?
            .context("placement")?;
        ensure!(placement.get("move_intent").is_none(), "{placement}");
        let projects = wait_for("the projects through the new core", || {
            let projects = fixture.projects()?;
            Ok((projects.len() == 2).then_some(projects))
        })?;
        ensure!(projects == after, "{projects:?}");
        Ok(())
    })();
    finish(fixture, journey)
}

fn listing(dir: &std::path::Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_>>()?;
    names.retain(|name| name != "Logs");
    names.sort();
    Ok(names)
}

fn records(fixture: &Fixture, kind: &str) -> Result<Vec<Value>> {
    let bytes = std::fs::read(fixture.source.state.join("Logs/core.jsonl"))?;
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["kind"] == kind)
        .collect())
}

/// Writes a record as the product does: owner-only, replaced whole.
fn write_record(path: &std::path::Path, value: &Value) -> Result<()> {
    hide_platform::fs::atomic::write_file(
        path,
        &serde_json::to_vec_pretty(value)?,
        hide_platform::fs::Access::Private,
    )?;
    Ok(())
}

fn finish(mut fixture: Fixture, journey: Result<()>) -> Result<()> {
    if let Err(error) = journey {
        let source = fixture.log_tail(&fixture.source);
        let target = fixture.log_tail(&fixture.target);
        return Err(error.context(format!(
            "source log:\n{source}\ntarget log:\n{target}\nrun kept at {}",
            fixture.root.display()
        )));
    }
    fixture.remove_run_dir()
}
