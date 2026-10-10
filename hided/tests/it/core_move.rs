//! Moving the core to another machine (PRD core-host-node-move): candidate
//! hided on both machines, the pinned Herdr on each, and a real SSH
//! connection that only the source opens.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::support::core_move::{ALIAS, Fixture, SOURCE_NODE, TARGET_NODE};
use crate::support::remote_delivery::{wait_for, wait_within};

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

/// A source killed while its core stopped for a move, before anything was
/// sent, finds the move in its journal at its next start and starts its
/// core again on its untouched folder; the target was never touched.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_source_killed_while_its_core_stops_starts_it_again() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        let target_before = listing(&fixture.target.state)?;
        // A move's journal, from a move that stopped at its staging.
        let planted = fixture.source.state.join("move-staging");
        std::fs::write(&planted, b"")?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let mut journal = fixture.journal_until("rolled_back")?;
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        std::fs::remove_file(&planted)?;
        fixture.kill_source()?;
        // What a kill while the core stops leaves: the journal at stopping,
        // written before the stop, and nothing staged or sent.
        journal["phase"] = json!({"phase": "stopping"});
        write_record(&fixture.source.state.join("core-move.json"), &journal)?;
        fixture.start_source()?;
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "stop_core", "{journal}");
        fixture.logged(&fixture.source, "move.resumed")?;
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        fixture.device_ready()?;
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(
            listing(&fixture.target.state)? == target_before,
            "the target changed"
        );
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

/// The new core refuses this machine's first link (here it cannot record
/// the commit while another change holds its record): this machine runs no
/// core until that machine's pending core is stopped and its copy taken
/// back, then starts its own unchanged.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_refused_first_link_rolls_back() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        wait_for("the copy placed on the target", || {
            Ok(fixture
                .target
                .record("core-handover.json")?
                .filter(|record| record["state"]["state"] == "pending"))
        })?;
        let held = fixture.hold_target_handover()?;
        let refused = fixture.logged(&fixture.target, "attach.refused")?;
        ensure!(
            refused["node"] == SOURCE_NODE && refused["reason"] == "move_unavailable",
            "{refused}"
        );
        fixture.logged(&fixture.source, "abort.failed")?;
        // Never two cores: the target's core may still take a link, so
        // this machine runs none.
        ensure!(fixture.role()? == "moving");
        let pending = fixture
            .target_core()?
            .context("the target's pending core")?;
        drop(held);
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "reattach", "{journal}");
        ensure!(
            journal["phase"]["cause"]["kind"] == "link_refused",
            "{journal}"
        );
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(!hide_platform::process::is_alive(pending));
        ensure!(fixture.target_core()?.is_none());
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.source.state.join("core-placement.json").exists());
        ensure!(!fixture.target.state.join("core-state.json").exists());
        ensure!(fixture.target.record("core-handover.json")?.is_none());
        Ok(())
    })();
    finish(fixture, journey)
}

/// This machine cannot stage its copy after its core stopped: nothing
/// reached the target, and the core starts again on its untouched folder.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_staging_failure_restarts_the_old_core() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        let target_before = listing(&fixture.target.state)?;
        // This machine cannot make its staging folder.
        std::fs::write(fixture.source.state.join("move-staging"), b"")?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "copy", "{journal}");
        ensure!(journal["phase"]["cause"]["kind"] == "staging", "{journal}");
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.source.state.join("core-placement.json").exists());
        ensure!(
            listing(&fixture.target.state)? == target_before,
            "the target changed"
        );
        Ok(())
    })();
    finish(fixture, journey)
}

/// The connection drops partway through the upload: the core starts again
/// unchanged, and the retry keeps the move's intent and sends again only
/// what the target does not already hold whole.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn an_upload_cut_midway_resumes_by_digest() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        // Armed once the core is stopping, so only the move's own upload
        // counts: the manifest, then the copy's files in name order. The
        // cut comes as the fourth of them opens, three already whole.
        fixture.journal_until("stopping")?;
        fixture.ssh.cut_sftp_at_open(5);
        let journal = wait_within(
            "the cut move rolled back",
            std::time::Duration::from_secs(60),
            || {
                Ok(fixture
                    .source
                    .record("core-move.json")?
                    .filter(|journal| journal["phase"]["phase"] == "rolled_back"))
            },
        )?;
        ensure!(journal["phase"]["failed"] == "copy", "{journal}");
        let intent = journal["intent"].as_str().context("intent")?.to_owned();
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.target.state.join("core-state.json").exists());
        ensure!(fixture.target.record("core-handover.json")?.is_none());
        let incoming = fixture.target.state.join("move-incoming").join(&intent);
        ensure!(incoming.is_dir(), "nothing of the copy reached the target");
        let whole: Vec<String> = listing(&incoming)?
            .into_iter()
            .filter(|name| !name.ends_with(".part"))
            .collect();
        ensure!(whole.len() == 3, "the cut came elsewhere: {whole:?}");

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
        let sent = records(&fixture, "copy.sent")?;
        ensure!(
            sent.len() == 1,
            "the cut try reported a whole copy: {sent:?}"
        );
        // The source's core ran between the tries, so its state store is
        // sent again; the two small files before the cut are held.
        let held = sent[0]["held"].as_u64().context("held")?;
        let uploaded = sent[0]["uploaded"].as_array().context("uploaded")?;
        ensure!(
            held >= 1 && held as usize + uploaded.len() == 7,
            "the retry sent the copy again: {sent:?}"
        );
        ensure!(fixture.target_core()?.is_some(), "no core on the target");
        Ok(())
    })();
    finish(fixture, journey)
}

/// The target refuses to place the copy, here because its folder gained
/// brain state of its own after the checks: that state stays its own and
/// in place, the copy waits in `move-incoming` for a retry, and this
/// machine's core starts again unchanged.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_place_failure_leaves_no_brain_state_on_the_target() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        // The place waits for the record's lock, so the folder changes
        // after the checks and before the place reads it.
        let held = fixture.hold_target_handover()?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let journal = fixture.journal_until("placed")?;
        let intent = journal["intent"].as_str().context("intent")?.to_owned();
        let own = fixture.target.state.join("labels.json");
        std::fs::write(&own, b"{\"own\":true}")?;
        drop(held);
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "copy", "{journal}");
        ensure!(
            journal["phase"]["cause"]["kind"] == "refused"
                && journal["phase"]["cause"]["step"] == "place",
            "{journal}"
        );
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(
            std::fs::read(&own)? == b"{\"own\":true}",
            "the target's own state changed"
        );
        ensure!(!fixture.target.state.join("core-state.json").exists());
        ensure!(!fixture.target.state.join("node.json").exists());
        ensure!(fixture.target.record("core-handover.json")?.is_none());
        ensure!(fixture.target_core()?.is_none());
        let incoming = fixture.target.state.join("move-incoming").join(&intent);
        ensure!(
            incoming.join("node.json").is_file(),
            "the copy was not kept"
        );
        ensure!(
            std::fs::read(incoming.join("labels.json"))? != b"{\"own\":true}",
            "the target's own state went into the copy"
        );
        Ok(())
    })();
    finish(fixture, journey)
}

/// The target's core cannot be confirmed stopped when the move is undone
/// (here its record cannot be read; a login item that cannot be removed
/// fails the same step): this machine waits and runs no core, and rolls
/// back once that machine answers that its core is gone.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_target_core_that_cannot_be_stopped_is_waited_for() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        wait_for("the copy placed on the target", || {
            Ok(fixture
                .target
                .record("core-handover.json")?
                .filter(|record| record["state"]["state"] == "pending"))
        })?;
        // The first link is refused, as in `a_refused_first_link_rolls_back`,
        // so the move is undone with the target's core running.
        let held = fixture.hold_target_handover()?;
        fixture.logged(&fixture.source, "abort.failed")?;
        let pending = fixture
            .target_core()?
            .context("the target's pending core")?;
        let record = fixture.target.state.join("hided.json");
        let readable = std::fs::metadata(&record)?.permissions();
        std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o000))?;
        drop(held);
        wait_for("the stop refused", || {
            Ok(records(&fixture, "abort.failed")?
                .into_iter()
                .find(|record| {
                    record["failure"]["reason"]
                        .as_str()
                        .is_some_and(|reason| reason.contains("the core's state could not be read"))
                }))
        })?;
        ensure!(fixture.role()? == "moving", "this machine started a core");
        ensure!(hide_platform::process::is_alive(pending));
        std::fs::set_permissions(&record, readable)?;
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "reattach", "{journal}");
        wait_for("the source's core again", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(!hide_platform::process::is_alive(pending));
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.target.state.join("core-state.json").exists());
        Ok(())
    })();
    finish(fixture, journey)
}

/// B6: from the node's window the core comes back to its machine; the
/// window keeps its address and shows what it showed before the core left,
/// and the other machine is a device again with no core of its own.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn the_core_moves_back_and_the_window_shows_what_it_showed_before() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        let window = fixture.window();
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let forward = fixture.journal_until("done")?;
        wait_for("the projects through the new core", || {
            Ok((fixture.projects()?.len() == 2).then_some(()))
        })?;
        ensure!(fixture.role()? == "node");

        fixture.event("core_move", json!({"action": "back"}))?;
        let journal = wait_for("the move back done", || {
            let journal = fixture.source.record("core-move.json")?;
            if let Some(journal) = &journal
                && journal["direction"] == "back"
                && journal["phase"]["phase"] == "rolled_back"
            {
                bail!("the move back rolled back: {journal}");
            }
            Ok(journal.filter(|journal| {
                journal["direction"] == "back" && journal["phase"]["phase"] == "done"
            }))
        })?;
        ensure!(journal["intent"] != forward["intent"], "{journal}");
        let intent = journal["intent"].as_str().context("intent")?;
        wait_for("this machine's core", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        let state = fixture.source.daemon()?.context("source state")?;
        ensure!(
            (state["port"].as_u64(), state["token"].as_str())
                == (Some(u64::from(window.0)), Some(window.1.as_str())),
            "the window's address changed: {state}"
        );
        ensure!(!fixture.source.state.join("core-placement.json").exists());
        ensure!(fixture.source.state.join("core-state.json").is_file());

        // The other machine runs no core and keeps its last state aside.
        ensure!(
            fixture.target_core()?.is_none(),
            "the target still runs a core"
        );
        ensure!(!fixture.target.state.join("core-state.json").exists());
        let moved_out = fixture.target.state.join("moved-out").join(intent);
        ensure!(
            moved_out.join("core-state.json").is_file(),
            "nothing set aside there"
        );
        let handover = fixture
            .target
            .record("core-handover.json")?
            .context("handover")?;
        ensure!(handover["state"]["state"] == "retired", "{handover}");

        fixture.device_ready()?;
        let after = wait_for("the projects of both machines again", || {
            let now = visible(&fixture)?;
            Ok((now["projects"].as_array().map(Vec::len) == Some(2)).then_some(now))
        })?;
        ensure!(after == before, "before: {before}\nafter: {after}");
        Ok(())
    })();
    finish(fixture, journey)
}

/// Moves the core to the target and waits until the window shows both
/// machines' projects through it.
fn moved_forward(fixture: &Fixture) -> Result<Value> {
    fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
    let journal = fixture.journal_until("done")?;
    wait_for("the projects through the new core", || {
        Ok((fixture.projects()?.len() == 2).then_some(()))
    })?;
    Ok(journal)
}

/// The source's move-back journal once it reaches `phase`.
fn back_journal_until(fixture: &Fixture, phase: &str) -> Result<Value> {
    wait_for(&format!("the move back at {phase}"), || {
        Ok(fixture
            .source
            .record("core-move.json")?
            .filter(|journal| journal["direction"] == "back" && journal["phase"]["phase"] == phase))
    })
}

/// A move back whose check fails changes nothing: the window stays a node
/// of the core, which keeps running.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_failing_move_back_check_changes_nothing() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        moved_forward(&fixture)?;
        let core = fixture.target_core()?.context("the target's core")?;
        // This machine holds brain state of its own again.
        std::fs::write(fixture.source.state.join("labels.json"), b"{}")?;
        let target_before = listing(&fixture.target.state)?;
        fixture.event("core_move", json!({"action": "back"}))?;
        let failed = wait_for("the move back's checks", || {
            Ok(records(&fixture, "checks.failed")?
                .into_iter()
                .find(|record| record["direction"] == "back"))
        })?;
        ensure!(failed["checks"] == json!(["own_state"]), "{failed}");
        ensure!(fixture.role()? == "node");
        ensure!(
            fixture.target_core()? == Some(core),
            "the target's core changed"
        );
        ensure!(
            listing(&fixture.target.state)? == target_before,
            "the target changed"
        );
        let journal = fixture
            .source
            .record("core-move.json")?
            .context("journal")?;
        ensure!(journal["direction"] == "forward", "{journal}");
        Ok(())
    })();
    finish(fixture, journey)
}

/// The copy cannot be pulled after the core's machine stopped its core:
/// that core starts again on its untouched folder and the window is its
/// node again.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_failed_pull_restarts_the_source_unchanged() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        moved_forward(&fixture)?;
        let before = fixture.projects()?;
        let core = fixture.target_core()?.context("the target's core")?;
        let state_before = std::fs::read(fixture.target.state.join("core-state.json"))?;
        // This machine cannot make its staging folder.
        let staging = fixture.source.state.join("move-staging");
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::write(&staging, b"")?;
        fixture.event("core_move", json!({"action": "back"}))?;
        let journal = back_journal_until(&fixture, "rolled_back")?;
        ensure!(journal["phase"]["failed"] == "copy", "{journal}");
        ensure!(fixture.role()? == "node");
        let restarted = fixture.target_core()?.context("no core on the target")?;
        ensure!(restarted != core, "the target's core never stopped");
        ensure!(fixture.target.record("core-handover.json")?.is_none());
        ensure!(
            !fixture
                .target
                .state
                .join("move-staging")
                .join(journal["intent"].as_str().context("intent")?)
                .exists()
        );
        ensure!(fixture.source.state.join("core-placement.json").is_file());
        // The core started on the folder it stopped with.
        let state_after = std::fs::read(fixture.target.state.join("core-state.json"))?;
        let parsed = |bytes: &[u8]| -> Result<Value> {
            let mut value: Value = serde_json::from_slice(bytes)?;
            value["pane_terminal_sizes"] = Value::Null;
            Ok(value["workspace_registrations"].take())
        };
        ensure!(
            parsed(&state_after)? == parsed(&state_before)?,
            "the target's projects changed"
        );
        let after = wait_for("the projects through the restarted core", || {
            let projects = fixture.projects()?;
            Ok((projects.len() == 2).then_some(projects))
        })?;
        ensure!(after == before, "{after:?}");
        Ok(())
    })();
    finish(fixture, journey)
}

/// The core's machine cannot be reached after it stopped its core: this
/// machine starts no core of its own, waits, and when that machine answers
/// again its core starts on its folder and the window is its node.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_source_unreachable_after_its_stop_is_waited_for() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        moved_forward(&fixture)?;
        let released = fixture.target_core()?.context("the target's core")?;
        fixture.event("core_move", json!({"action": "back"}))?;
        wait_for("the target's core released", || {
            let bytes =
                std::fs::read(fixture.target.state.join("Logs/core.jsonl")).unwrap_or_default();
            Ok(String::from_utf8_lossy(&bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|record| record["kind"] == "release.checked" && record["refused"].is_null())
                .then_some(()))
        })?;
        fixture.ssh.online(false)?;
        fixture.kill_source()?;
        let phase = fixture
            .source
            .record("core-move.json")?
            .context("journal")?["phase"]["phase"]
            .clone();
        ensure!(
            matches!(
                phase.as_str(),
                Some("releasing" | "released" | "placed_here")
            ),
            "the move back got past its commit before the cut: {phase}"
        );
        // The released core ends on its own, whatever reaches its machine.
        wait_for("the released core ended", || {
            Ok((!hide_platform::process::is_alive(released)).then_some(()))
        })?;
        fixture.start_source()?;
        fixture.logged(&fixture.source, "resume.failed")?;
        ensure!(fixture.role()? == "moving");
        ensure!(
            fixture.target_core()?.is_none(),
            "the target started a core while cut off"
        );
        ensure!(!fixture.source.state.join("core-state.json").exists());
        fixture.ssh.online(true)?;
        back_journal_until(&fixture, "rolled_back")?;
        wait_for("the window a node again", || {
            Ok((fixture.role()? == "node").then_some(()))
        })?;
        ensure!(fixture.target_core()?.is_some(), "no core on the target");
        wait_for("the projects through the restarted core", || {
            Ok((fixture.projects()?.len() == 2).then_some(()))
        })?;
        Ok(())
    })();
    finish(fixture, journey)
}

/// This machine was killed after the core's machine retired its core,
/// before it recorded the move back done: at its next start it finishes
/// the move and runs the core. The retired machine's hided, started again
/// as its login item would, ends at once and successfully, so keep-alive
/// does not restart it, and starts no core.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_driver_killed_after_the_retirement_finishes_the_move_back() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        moved_forward(&fixture)?;
        let placement = fixture
            .source
            .record("core-placement.json")?
            .context("placement")?;
        fixture.event("core_move", json!({"action": "back"}))?;
        let mut journal = back_journal_until(&fixture, "done")?;
        wait_for("this machine's core", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        fixture.kill_source()?;
        // What a kill between the retirement and its record leaves: the
        // journal at retiring and the placement still naming the core's
        // machine.
        journal["phase"] = json!({"phase": "retiring"});
        write_record(&fixture.source.state.join("core-move.json"), &journal)?;
        write_record(
            &fixture.source.state.join("core-placement.json"),
            &placement,
        )?;
        fixture.start_source()?;
        back_journal_until(&fixture, "done")?;
        fixture.logged(&fixture.source, "move.resumed")?;
        wait_for("this machine's core", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        ensure!(!fixture.source.state.join("core-placement.json").exists());
        ensure!(fixture.target_core()?.is_none());

        let ended = fixture.start_target_hided()?;
        ensure!(ended.success(), "the retired machine's hided: {ended}");
        fixture.logged(&fixture.target, "start.held")?;
        ensure!(fixture.target_core()?.is_none(), "a retired core started");
        ensure!(!fixture.target.state.join("core-state.json").exists());

        fixture.device_ready()?;
        let after = wait_for("the projects of both machines again", || {
            let now = visible(&fixture)?;
            Ok((now["projects"].as_array().map(Vec::len) == Some(2)).then_some(now))
        })?;
        ensure!(after == before, "before: {before}\nafter: {after}");
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
