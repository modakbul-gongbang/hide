//! Moving the core to another machine (PRD core-host-node-move): candidate
//! hided on both machines, the pinned Herdr on each, and a real SSH
//! connection that only the source opens.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::support::core_move::{ALIAS, Fixture, SOURCE_NODE, TARGET_NODE};
use crate::support::fake_tailscale::FakeTailscale;
use crate::support::remote_delivery::renderer::Renderer;
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
        // Settings of the target's own, never merged with the core's.
        let theirs = fixture.target.ai_settings();
        std::fs::create_dir_all(theirs.parent().context("settings folder")?)?;
        std::fs::write(&theirs, r#"{"provider":"codex"}"#)?;
        let target_before = listing(&fixture.target.state)?;
        let before = visible(&fixture)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let failed = fixture.logged(&fixture.source, "checks.failed")?;
        ensure!(failed["checks"] == json!(["target_state"]), "{failed}");
        let detail = failed["failed"][0]["detail"].as_str().unwrap_or_default();
        ensure!(
            detail.contains("core-state.json") && detail.contains("Hide AI settings"),
            "{failed}"
        );
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
        ensure!(std::fs::read_to_string(&theirs)? == r#"{"provider":"codex"}"#);
        Ok(())
    })();
    finish(fixture, journey)
}

/// B3: each step missing on the target is named with what its check found,
/// `gh` signed out, no desktop session, sleep on power and Hide AI's agent
/// signed out, and the move starts nothing on either machine.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn each_step_missing_on_the_target_is_named_and_nothing_changes() -> Result<()> {
    let fixture = Fixture::start()?;
    let journey = (|| {
        fixture.fail_checks()?;
        let target_before = listing(&fixture.target.state)?;
        let before = visible(&fixture)?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let failed = fixture.logged(&fixture.source, "checks.failed")?;
        ensure!(
            failed["checks"] == json!(["gh", "gui_session", "sleep", "ai"]),
            "{failed}"
        );
        ensure!(fixture.role()? == "core");
        ensure!(visible(&fixture)? == before, "the window changed");
        ensure!(!fixture.source.state.join("core-move.json").exists());
        ensure!(
            listing(&fixture.target.state)? == target_before,
            "the target changed"
        );
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
        let files = sent[0]["uploaded"].as_array().context("first try")?.len();
        let held = sent[1]["held"].as_u64().context("held")?;
        let uploaded = sent[1]["uploaded"].as_array().context("uploaded")?;
        ensure!(
            held >= 5 && held as usize + uploaded.len() == files,
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

/// The source's core does not stop for the move (a fixture makes its stop
/// hang): past the stop's bound the process ends unsuccessfully, which a
/// login item restarts, and its next start undoes the move and runs the core
/// on its folder; the target was never touched.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_core_that_will_not_stop_is_restarted_unchanged() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let before = visible(&fixture)?;
        let target_before = listing(&fixture.target.state)?;
        fixture.kill_source()?;
        fixture
            .source
            .herdr
            .environment
            .set(hided::env::HIDE_FIXTURE_CORE_STOP, "hang");
        fixture.start_source()?;
        fixture.device_ready()?;
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        // Past the stop's 20 s bound, and before a stop with none would
        // ever end.
        let ended = fixture.source_ended_within(std::time::Duration::from_secs(40))?;
        ensure!(ended.code() == Some(3), "the source's hided: {ended}");
        let unconfirmed = fixture.logged(&fixture.source, "core.stop_unconfirmed")?;
        ensure!(unconfirmed["cause"] == "timed_out", "{unconfirmed}");
        let journal = fixture
            .source
            .record("core-move.json")?
            .context("journal")?;
        ensure!(journal["phase"]["phase"] == "stopping", "{journal}");

        fixture
            .source
            .herdr
            .environment
            .unset(hided::env::HIDE_FIXTURE_CORE_STOP);
        fixture.start_source()?;
        let journal = fixture.journal_until("rolled_back")?;
        ensure!(journal["phase"]["failed"] == "stop_core", "{journal}");
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
        // The connection's end ends the upload, well before an SFTP
        // request's 30 s limit would.
        let journal = wait_within(
            "the cut move rolled back",
            std::time::Duration::from_secs(15),
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
        let manifest: Value = serde_json::from_slice(&std::fs::read(
            fixture
                .target
                .state
                .join("move-incoming")
                .join(format!("{intent}.manifest.json")),
        )?)?;
        let files = manifest["files"].as_object().context("manifest")?.len();

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
            held >= 1 && held as usize + uploaded.len() == files,
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
        let settings = std::fs::read_to_string(fixture.source.ai_settings())?;
        let window = fixture.window();
        fixture.event("core_move", json!({"action": "start", "device": ALIAS}))?;
        let forward = fixture.journal_until("done")?;
        wait_for("the projects through the new core", || {
            Ok((fixture.projects()?.len() == 2).then_some(()))
        })?;
        ensure!(fixture.role()? == "node");
        ensure!(
            std::fs::read_to_string(fixture.target.ai_settings())? == settings,
            "the target holds other settings"
        );
        ensure!(!fixture.source.ai_settings().exists());
        ensure!(
            fixture
                .source
                .state
                .join("moved-out")
                .join(forward["intent"].as_str().context("intent")?)
                .join("ai.json")
                .is_file()
        );

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
        // Hide AI's settings went with the core and came back with it.
        ensure!(
            std::fs::read_to_string(fixture.source.ai_settings())? == settings,
            "the settings did not come back"
        );
        ensure!(!fixture.target.ai_settings().exists());
        ensure!(
            fixture
                .target
                .state
                .join("moved-out")
                .join(intent)
                .join("ai.json")
                .is_file()
        );
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

/// Amendment 3: a core started on a move's copy acts on nothing outside its
/// machine until the move's own link commits it, and then does at once.
/// This watches what such a core reaches from this process: the device its
/// copy registers (a listener its SSH config names) and Mobile's
/// `tailscale`. The bell, watch warnings, human notices, provider calls,
/// GitHub reads and the Factory are held inside the core, each tested there.
#[test]
fn a_pending_core_does_nothing_outside_until_its_link_commits() -> Result<()> {
    const INTENT: &str = "move-fixture";
    let dir = tempfile::tempdir()?;
    let home = hide_platform::fs::identity::canonical(dir.path())?;
    let state = home.join("state");
    hide_platform::fs::private::create_dir_all(&state)?;
    write_record(
        &state.join("core-handover.json"),
        &json!({"version": 1, "intent": INTENT, "source": SOURCE_NODE, "target": TARGET_NODE, "state": {"state": "pending"}}),
    )?;
    write_record(&state.join("mobile.json"), &json!({"enabled": true}))?;
    let tailscale = FakeTailscale::new(&home);
    tailscale.ready();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let running = runtime
        .block_on(hided::start_daemon(hided::env::Env {
            home: home.clone(),
            herdr_socket_path: None,
            herdr_bin_path: None,
            state_dir: state.clone(),
            legacy_state_dir: None,
            keep_alive: true,
            vite_origin: None,
            bind: "127.0.0.1:0".parse()?,
            idle_secs: 600,
            build: None,
            open_command: None,
            host_helper_root: None,
            host_cli_dir: None,
            pane_id: None,
            tailscale_bin: Some(tailscale.bin.clone()),
            search_path: None,
        }))
        .map_err(anyhow::Error::msg)?;
    // Bound after the daemon, so it closes first: a redial waiting in its
    // backlog for an SSH greeting would hold the core's stop until the
    // dial's own timeout.
    let device = std::net::TcpListener::bind("127.0.0.1:0")?;
    device.set_nonblocking(true)?;
    std::fs::create_dir_all(home.join(".ssh"))?;
    std::fs::write(
        home.join(".ssh/config"),
        format!(
            "Host probe\n  HostName 127.0.0.1\n  Port {}\n  User fixture\n  IdentityAgent none\n",
            device.local_addr()?.port()
        ),
    )?;
    let snapshot = || -> Result<Value> {
        Ok(Renderer::connect(running.port, &running.token)?
            .snapshot()
            .clone())
    };
    let attempted = |snapshot: &Value| {
        snapshot
            .pointer("/status/remote")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|row| row["target_id"] == "probe"))
    };

    Renderer::connect(running.port, &running.token)?.event(
        "register_device",
        json!({"id": "probe", "label": "Probe", "ssh_alias": "probe"}),
    )?;
    let pending = snapshot()?;
    ensure!(
        pending["ui_state"]["device_registrations"][0]["id"] == "probe",
        "the registration is kept: {}",
        pending["ui_state"]["device_registrations"]
    );
    ensure!(!attempted(&pending), "a pending core dialed the device");
    runtime.block_on(running.mobile.reconcile());
    ensure!(
        tailscale.calls().is_empty(),
        "a pending core ran tailscale: {}",
        tailscale.calls()
    );

    running
        .move_gate
        .admit(SOURCE_NODE, Some(INTENT))
        .map_err(anyhow::Error::msg)?;
    wait_for("the device dialed after the commit", || {
        match device.accept() {
            Ok(_) => Ok(Some(())),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.into()),
        }
    })?;
    ensure!(
        attempted(&snapshot()?),
        "no connection row after the commit"
    );
    wait_for("Mobile's first check after the commit", || {
        Ok(tailscale.calls().contains("status").then_some(()))
    })?;
    running.stop();
    Ok(())
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

/// D-15(3) and D-29: what the operator arranged on each machine is the same
/// before the move, once it commits and after the move back, compared by the
/// machine and path each id names, since the two cores name the machines
/// differently. The state is written by the product's own events where one
/// exists; labels and a letter held for the target are placed at rest,
/// because their writers need an AI provider and a registered agent. The
/// target's core keeps the labels it was given in memory, so its folder has
/// none to compare.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn what_each_machine_shows_survives_the_move_and_the_move_back() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        seed_at_rest(&mut fixture)?;
        seed_through_events(&fixture)?;
        let shown_before = settled_window(&fixture, SOURCE_NODE)?;

        let forward = moved_forward(&fixture)?;
        let kept_before = kept_per_machine(
            &fixture
                .source
                .state
                .join("moved-out")
                .join(forward["intent"].as_str().context("intent")?),
            SOURCE_NODE,
        )?;
        let shown_after = wait_for("the window as it was, through the new core", || {
            let shown = window_shows(&fixture, TARGET_NODE)?;
            Ok(shown.filter(|shown| *shown == shown_before))
        })
        .with_context(|| {
            format!(
                "before: {shown_before}\nafter: {:?}",
                window_shows(&fixture, TARGET_NODE)
            )
        })?;

        fixture.event("core_move", json!({"action": "back"}))?;
        let back = back_journal_until(&fixture, "done")?;
        wait_for("this machine's core", || {
            Ok((fixture.role()? == "core").then_some(()))
        })?;
        fixture.device_ready()?;
        let kept_after = kept_per_machine(
            &fixture
                .target
                .state
                .join("moved-out")
                .join(back["intent"].as_str().context("intent")?),
            TARGET_NODE,
        )?;
        wait_for("the window as it was, back on this machine", || {
            Ok((window_shows(&fixture, SOURCE_NODE)?.as_ref() == Some(&shown_after)).then_some(()))
        })
        .with_context(|| {
            format!(
                "before: {shown_after}\nreturned: {:?}",
                window_shows(&fixture, SOURCE_NODE)
            )
        })?;
        let kept_returned = kept_per_machine(&fixture.source.state, SOURCE_NODE)?;

        ensure!(
            kept_returned == kept_before,
            "before: {kept_before:#}\nreturned: {kept_returned:#}"
        );
        let mut without_labels = kept_before.clone();
        without_labels["source"]["labels"] = json!({});
        ensure!(
            kept_after == without_labels,
            "before: {kept_before:#}\nafter: {kept_after:#}"
        );
        ensure!(
            kept_before["source"]["labels"]["w1:p1"]["goal"] == "Seeded goal",
            "{kept_before:#}"
        );
        ensure!(
            kept_before["target"]["letters"].as_array().map(Vec::len) == Some(1),
            "{kept_before:#}"
        );
        Ok(())
    })();
    finish(fixture, journey)
}

/// A label on the source's pane and a letter held for an agent on the
/// target, in the stores' own shapes, before the source's core starts.
fn seed_at_rest(fixture: &mut Fixture) -> Result<()> {
    // The kill gives the save thread no last write, so both projects must be
    // on disk before it.
    let saved = fixture.source.state.join("core-state.json");
    wait_for("both machines' projects saved", || {
        let state: Value = serde_json::from_slice(&std::fs::read(&saved)?)?;
        Ok((state
            .pointer("/workspace_registrations")
            .and_then(Value::as_array)
            .map(Vec::len)
            == Some(2))
        .then_some(()))
    })?;
    fixture.kill_source()?;
    write_record(
        &fixture.source.state.join("labels.json"),
        &json!({"version": 1, "targets": {SOURCE_NODE: {"w1:p1": {"goal": "Seeded goal", "line": "Seeded line"}}}}),
    )?;
    let actor = |pane: &str, device: &str| json!({"pane_id": pane, "name": "lead", "kind": "claude", "device_id": device, "session": "lead-session"});
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let path = fixture.source.state.join("delivery-ledger.json");
    let mut ledger: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    ledger["next_id"] = json!(100);
    ledger["letters"] = json!([{"id": "letter-1", "intent": "held-for-target", "sender": actor("w1:p1", SOURCE_NODE), "recipient": actor(&format!("remote:{ALIAS}:pane:w9:p9"), ALIAS), "kind": "report", "body": "A letter held for the target", "state": "undelivered", "waiting_answer": false, "reply_to": null, "created_at_unix_ms": now, "finished_at_unix_ms": now, "bell_errors": 0, "bell_sent": false, "human_notified": false}]);
    write_record(&path, &ledger)?;
    fixture.start_source()?;
    fixture.device_ready()
}

/// Folds, path settings, Views, a second tab and an Agent split on the
/// source's checkout, each through the event the window sends.
fn seed_through_events(fixture: &Fixture) -> Result<()> {
    let source = fixture.source.project();
    let target = fixture.target.project();
    std::fs::create_dir_all(source.join("src"))?;
    std::fs::write(source.join("src/a.txt"), "a")?;
    std::fs::create_dir_all(target.join("lib"))?;
    let snapshot = fixture.snapshot()?;
    let registrations = snapshot
        .pointer("/ui_state/workspace_registrations")
        .and_then(Value::as_array)
        .context("registrations")?
        .clone();
    let project = |device: &str| {
        registrations
            .iter()
            .find(|row| row["device_id"] == device)
            .map(|row| row["id"].clone())
            .context("project")
    };
    let (source_project, target_project) = (project(SOURCE_NODE)?, project(ALIAS)?);
    let checkout = snapshot
        .pointer("/navigator/workspaces/0/checkouts/0/id")
        .cloned()
        .context("checkout")?;
    fixture.event(
        "create_tab",
        json!({"workspace_id": source_project, "checkout_id": checkout, "label": "second"}),
    )?;
    let second = wait_for("the source's second tab", || {
        let snapshot = fixture.snapshot()?;
        let tabs = snapshot
            .pointer("/navigator/workspaces/0/checkouts/0/tabs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok((tabs.len() == 2).then(|| tabs[1]["id"].clone()))
    })?;
    let bindings = snapshot
        .pointer("/ui_state/shortcut_bindings")
        .cloned()
        .unwrap_or(json!({}));
    for (kind, payload) in [
        (
            "agent_layout",
            json!({"workspace": {"device_id": SOURCE_NODE, "path": source}, "action": "split", "tab_id": second, "area_id": "a1", "edge": "right", "request_id": "seed-split"}),
        ),
        ("session_fold_toggle", json!({"key": source_project})),
        ("session_fold_toggle", json!({"key": target_project})),
        ("checkout_agents_toggle", json!({"checkout_id": checkout})),
        (
            "issue_source_set",
            json!({"project_path": source, "source": "local"}),
        ),
        (
            "issue_source_set",
            json!({"project_path": target, "source": "github"}),
        ),
        (
            "project_checkouts_fold",
            json!({"workspace_id": target_project, "expanded": false}),
        ),
        ("sessions_set_mode", json!({"mode": "memory"})),
        ("workspace_view", json!({"views": true})),
        (
            "ui_state_update",
            json!({"expanded_paths": [source.join("src")], "device_expanded_paths": {ALIAS: [target.join("lib")]}, "selected_path": source.join("src/a.txt"), "shortcut_bindings": bindings}),
        ),
    ] {
        fixture.event(kind, payload)?;
    }
    Ok(())
}

/// What the window shows once the seeded split has reached it and the
/// saves behind it had time to land.
fn settled_window(fixture: &Fixture, own: &str) -> Result<Value> {
    let shown = wait_for("the split drawn", || {
        Ok(window_shows(fixture, own)?.filter(|shown| {
            shown
                .pointer("/agent_areas/layout/root/split")
                .is_some_and(|split| !split.is_null())
        }))
    })?;
    std::thread::park_timeout(std::time::Duration::from_secs(2));
    Ok(shown)
}

/// The machine a device id or node id names in either core.
fn machine_of(device: &str, own: &str) -> Result<&'static str> {
    let device = if device == "local" { own } else { device };
    Ok(match device {
        SOURCE_NODE => "source",
        TARGET_NODE | ALIAS => "target",
        other => bail!("an id names no machine of the fixture: {other}"),
    })
}

/// `value` with every `remote:<id>:tab:` or `remote:<id>:pane:` prefix that
/// names `machine` taken off, as that machine's own Herdr spells its ids.
fn unqualify(value: &Value, machine: &str, own: &str) -> Value {
    match value {
        Value::String(text) => {
            for kind in [":tab:", ":pane:"] {
                if let Some((owner, id)) = text.split_once(kind)
                    && let Some(device) = owner.strip_prefix("remote:")
                    && machine_of(device, own).ok() == Some(machine)
                {
                    return json!(id);
                }
            }
            value.clone()
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| unqualify(item, machine, own))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| {
                    let key = match unqualify(&json!(key), machine, own) {
                        Value::String(key) => key,
                        _ => key.clone(),
                    };
                    (key, unqualify(item, machine, own))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What the window shows, read from the core whose node is `own`: the
/// machine in front with the checkout, tab and pane it shows, every
/// machine's checkouts with their tabs, and the front Workspace's Agent
/// areas, each id as its machine's Herdr spells it; `None` while a core
/// that just started names no machine in front yet.
fn window_shows(fixture: &Fixture, own: &str) -> Result<Option<Value>> {
    let snapshot = fixture.snapshot()?;
    let Some(front) = snapshot
        .pointer("/navigator/focused_device_id")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let front = machine_of(front, own)?;
    let tabs_of = |checkouts: &[Value]| -> serde_json::Map<String, Value> {
        checkouts
            .iter()
            .filter_map(|checkout| {
                let tabs: Vec<Value> = checkout["tabs"]
                    .as_array()?
                    .iter()
                    .map(|tab| json!({"id": tab["id"], "panes": tab["panes"].as_array().map(|panes| panes.iter().map(|pane| pane["id"].clone()).collect::<Vec<_>>())}))
                    .collect();
                Some((checkout["path"].as_str()?.to_owned(), json!(tabs)))
            })
            .collect()
    };
    let checkouts_in = |workspaces: Option<&Value>| -> Vec<Value> {
        workspaces
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|workspace| {
                workspace["checkouts"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    };
    let mut machines = serde_json::Map::new();
    let mut shown = Value::Null;
    let own_checkouts = checkouts_in(snapshot.pointer("/navigator/workspaces"));
    let own_machine = machine_of(own, own)?;
    machines.insert(own_machine.to_owned(), json!(tabs_of(&own_checkouts)));
    if front == own_machine {
        let focused = snapshot.pointer("/navigator/focused_checkout_id");
        let checkout = own_checkouts.iter().find(|row| Some(&row["id"]) == focused);
        shown = json!({
            "checkout": checkout.map(|row| row["path"].clone()),
            "tab": checkout.map(|row| row["active_tab_id"].clone()),
            "pane": snapshot.pointer("/ui_state/selected_pane_id"),
        });
    }
    for status in snapshot
        .pointer("/status/remote")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let machine = machine_of(status["target_id"].as_str().context("target")?, own)?;
        let session = &status["session"];
        let checkouts = checkouts_in(session.get("workspaces"));
        machines.insert(
            machine.to_owned(),
            unqualify(&json!(tabs_of(&checkouts)), machine, own),
        );
        if front == machine {
            let focused = session.get("focused_checkout_id");
            shown = unqualify(
                &json!({
                    "checkout": checkouts.iter().find(|row| Some(&row["id"]) == focused).map(|row| row["path"].clone()),
                    "tab": session["focused_tab_id"],
                    "pane": session["focused_pane_id"],
                }),
                machine,
                own,
            );
        }
    }
    let view = snapshot
        .pointer("/workspace_view")
        .cloned()
        .unwrap_or(Value::Null);
    let agent_areas = match view["device_id"].as_str() {
        Some(device) => {
            let machine = machine_of(device, own)?;
            unqualify(
                &json!({"machine": machine, "path": view["path"], "layout": {"root": view["agent_layout"]["root"], "active_area": view["agent_layout"]["active_area"], "canvases": view["agent_layout"]["canvases"]}}),
                machine,
                own,
            )
        }
        None => Value::Null,
    };
    Ok(Some(
        json!({"front": front, "shown": shown, "machines": machines, "agent_areas": agent_areas}),
    ))
}

/// What the core whose stores are in `dir`, running on node `own`, keeps
/// for each machine: every id replaced by the machine and path it names, so
/// two cores that name the machines differently compare equal when they
/// keep the same things for each. Which checkout and pane a machine shows
/// is its own Herdr's focus and is compared on the window instead.
fn kept_per_machine(dir: &std::path::Path, own: &str) -> Result<Value> {
    let read = |name: &str| -> Result<Value> {
        Ok(serde_json::from_slice(
            &std::fs::read(dir.join(name))
                .with_context(|| format!("{name} in {}", dir.display()))?,
        )?)
    };
    let core = read("core-state.json")?;
    let views = read("workspace-views.json")?;
    let labels = read("labels.json")?;
    let ledger = read("delivery-ledger.json")?;
    let registrations = core["workspace_registrations"]
        .as_array()
        .context("registrations")?;
    let place_project = |id: &str| -> Result<(&'static str, String)> {
        let registration = registrations
            .iter()
            .find(|row| row["id"] == id)
            .with_context(|| format!("{id} names no registered project"))?;
        Ok((
            machine_of(registration["device_id"].as_str().context("device")?, own)?,
            registration["path"].as_str().context("path")?.to_owned(),
        ))
    };
    // A project id as (machine, path); a checkout id as (machine, the
    // checkout's own key), which a remote form spells after `:checkout:`.
    let place = |id: &str| -> Result<(&'static str, String)> {
        if let Some((owner, checkout)) = id.split_once(":checkout:") {
            if let Some(device) = owner.strip_prefix("remote:") {
                return Ok((machine_of(device, own)?, format!("checkout {checkout}")));
            }
            let (machine, _) = place_project(owner)?;
            return Ok((machine, format!("checkout {checkout}")));
        }
        place_project(id)
    };
    let device_of = |machine: &str| -> Result<Option<String>> {
        if machine_of(own, own)? == machine {
            return Ok(None);
        }
        Ok(registrations
            .iter()
            .filter_map(|row| row["device_id"].as_str())
            .find(|device| machine_of(device, own).ok() == Some(machine))
            .map(str::to_owned))
    };
    let mut kept = serde_json::Map::new();
    for machine in ["source", "target"] {
        let device = device_of(machine)?;
        let ids = |field: &str| -> Result<Vec<String>> {
            let mut paths = Vec::new();
            for id in core[field].as_array().into_iter().flatten() {
                let (at, path) = place(id.as_str().context("id")?)?;
                if at == machine {
                    paths.push(path);
                }
            }
            paths.sort();
            Ok(paths)
        };
        let projects: Vec<String> = registrations
            .iter()
            .filter(|row| {
                row["device_id"]
                    .as_str()
                    .and_then(|device| machine_of(device, own).ok())
                    == Some(machine)
            })
            .filter_map(|row| row["path"].as_str().map(str::to_owned))
            .collect();
        let by_path = |field: &str| -> Value {
            let mut found = serde_json::Map::new();
            for path in &projects {
                if let Some(value) = core[field].get(path) {
                    found.insert(path.clone(), value.clone());
                }
            }
            Value::Object(found)
        };
        let own_or_device = |field: &str| -> Value {
            match &device {
                None => core[field].clone(),
                Some(device) => core[format!("device_{field}")][device].clone(),
            }
        };
        // A tab or pane id as this machine's Herdr names it, whichever core
        // qualifies it.
        let unqualified = |value: &Value| -> Value { unqualify(value, machine, own) };
        let mut modes = serde_json::Map::new();
        for (id, mode) in core["sessions_mode_by_project"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let (at, path) = place(id)?;
            if at == machine {
                modes.insert(path, mode.clone());
            }
        }
        let workspace_views: Vec<Value> = views["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["device_id"].as_str().and_then(|device| machine_of(device, own).ok()) == Some(machine))
            .map(|row| unqualified(&json!({"path": row["path"], "views": row["views"], "layout": row["layout"], "agent_layout": row["agent_layout"], "bookmarks": row["view_bookmarks"]})))
            .collect();
        let label_records: serde_json::Map<String, Value> = labels["targets"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(node, _)| machine_of(node, own).ok() == Some(machine))
            .flat_map(|(_, panes)| panes.as_object().cloned().unwrap_or_default())
            .map(|(pane, record)| {
                (
                    pane,
                    json!({"goal": record["goal"], "line": record["line"]}),
                )
            })
            .collect();
        let letters: Vec<Value> = ledger["letters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|letter| letter["recipient"]["device_id"].as_str().and_then(|device| machine_of(device, own).ok()) == Some(machine))
            .map(|letter| json!({"intent": letter["intent"], "pane": unqualified(&letter["recipient"]["pane_id"]), "body": letter["body"]}))
            .collect();
        kept.insert(
            machine.to_owned(),
            json!({
                "session_folds": ids("session_open_folds")?,
                "collapsed_projects": ids("collapsed_workspace_ids")?,
                "collapsed_sessions": ids("session_collapsed_checkout_ids")?,
                "sessions_modes": modes,
                "issue_sources": by_path("project_issue_sources"),
                "base_branches": by_path("project_base_branches"),
                "expanded_paths": own_or_device("expanded_paths"),
                "inactive_open": projects.iter().filter(|path| core["expanded_inactive_checkout_project_paths"].as_array().is_some_and(|open| open.contains(&json!(path)))).collect::<Vec<_>>(),
                "workspace_views": workspace_views,
                "labels": label_records,
                "letters": letters,
            }),
        );
    }
    kept.insert("selected_path".to_owned(), core["selected_path"].clone());
    Ok(Value::Object(kept))
}
