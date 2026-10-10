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
    let mut fixture = Fixture::start()?;
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
