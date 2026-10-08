//! Candidate CLI + pinned Herdr lane. It crosses actual pane ancestry, SSH,
//! helper attestation over the node link and the durable mailbox.
#![cfg(unix)]

#[path = "support/remote_delivery/mod.rs"]
mod fixture;

use anyhow::{Context, Result, ensure};
use fixture::{Fixture, quote, wait_for};
use herdr_core::delivery::ledger::State;
use serde_json::Value;
use std::time::Duration;

fn context(stdout: &str) -> Result<String> {
    if stdout.is_empty() {
        return Ok(String::new());
    }
    let output: Value = serde_json::from_str(stdout)?;
    Ok(output
        .pointer("/hookSpecificOutput/additionalContext")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned())
}

/// The prompt hook of the turn Hide's own bell opened: only that turn receives
/// the letter bodies, an operator's prompt receives a count.
fn hook(binary: &std::path::Path, session: &str) -> String {
    format!(
        "printf '%s' {} | {} hook --runtime claude-code --event UserPromptSubmit",
        quote(
            serde_json::json!({"session_id":session,"prompt":hide_agent_hooks::delivery::BELL_PROMPT})
                .to_string()
        ),
        quote(binary)
    )
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn connected_remote_panes_receive_once_and_disconnect_keeps_the_same_letter_pending() -> Result<()>
{
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let remote_hide = fixture.remote.environment.home.join("bin/hide");
        let remote_hooks = std::fs::canonicalize(&remote_hide)?
            .parent()
            .context("installed private CLI directory")?
            .join("hide-agent-hooks");
        ensure!(
            remote_hooks.is_file(),
            "private remote kit lacks its hook sibling"
        );
        let send = format!(
            "{} request send remote-child --intent bridge-1 --body {}",
            quote(&fixture.hide),
            quote("DELIVERY_REMOTE_ONCE")
        );
        let first = fixture.local.run_in_pane(&send)?.json()?;
        let first_id = first["id"].as_str().context("sent letter ID")?.to_owned();
        let inbox = fixture
            .remote
            .run_in_pane(&format!("{} inbox", quote(&remote_hide)))?
            .json()?;
        ensure!(
            inbox.to_string().contains(&first_id),
            "remote inbox cannot see its letter"
        );
        ensure!(
            fixture
                .ledger()?
                .letters
                .iter()
                .any(|letter| letter.id == first_id && letter.state == State::Pending),
            "manual remote inbox changed delivery state"
        );
        let delivered = fixture
            .remote
            .run_in_pane(&hook(&remote_hooks, "fixture-remote-session"))?;
        ensure!(
            delivered.status == 0 && delivered.elapsed < Duration::from_secs(2),
            "connected prompt hook did not complete within its budget"
        );
        let intake = context(&delivered.stdout)?;
        ensure!(
            intake.contains(&first_id) && intake.contains("DELIVERY_REMOTE_ONCE"),
            "remote hook did not receive the real letter"
        );
        wait_for("durable remote delivery confirmation", || {
            Ok(fixture
                .ledger()?
                .letters
                .iter()
                .any(|letter| letter.id == first_id && letter.state == State::Delivered)
                .then_some(()))
        })?;
        let again = fixture
            .remote
            .run_in_pane(&hook(&remote_hooks, "fixture-remote-session"))?;
        ensure!(
            again.status == 0 && context(&again.stdout)?.is_empty(),
            "remote hook repeated a delivered letter"
        );

        let watch = fixture
            .local
            .run_in_pane(&format!(
                "{} watch start remote-child",
                quote(&fixture.hide)
            ))?
            .json()?;
        let watch_id = watch["id"].as_str().context("started watch ID")?.to_owned();
        let report = fixture
            .remote
            .run_in_pane(&format!(
                "{} request send local-parent --intent bridge-report --kind report --body {}",
                quote(&remote_hide),
                quote("DELIVERY_LOCAL_REPORT")
            ))?
            .json()?;
        let report_id = report["id"]
            .as_str()
            .context("report letter ID")?
            .to_owned();
        ensure!(
            fixture
                .ledger()?
                .watches
                .iter()
                .any(|watch| watch.id == watch_id),
            "sending a remote report stopped its watch before intake"
        );
        let parent = fixture
            .local
            .run_in_pane(&hook(&fixture.hooks, "fixture-local-session"))?;
        ensure!(
            context(&parent.stdout)?.contains(&report_id),
            "local parent did not receive the report sent over the node link"
        );
        wait_for("report confirmation stops watch", || {
            Ok((!fixture
                .ledger()?
                .watches
                .iter()
                .any(|watch| watch.id == watch_id))
            .then_some(()))
        })?;

        let pending = fixture
            .local
            .run_in_pane(&format!(
                "{} request send remote-child --intent bridge-2 --body {}",
                quote(&fixture.hide),
                quote("DELIVERY_AFTER_RECONNECT")
            ))?
            .json()?;
        let pending_id = pending["id"]
            .as_str()
            .context("pending letter ID")?
            .to_owned();
        fixture.ssh.online(false)?;
        wait_for("private SSH device disconnected", || {
            let snapshot = fixture.snapshot()?;
            Ok(snapshot
                .pointer("/status/remote")
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter()
                        .any(|row| row["target_id"] == "remote" && row["state"] != "connected")
                })
                .then_some(()))
        })?;
        let disconnected = fixture
            .remote
            .run_in_pane(&hook(&remote_hooks, "fixture-remote-session"))?;
        ensure!(
            disconnected.status == 0,
            "disconnected prompt hook failed the provider turn"
        );
        ensure!(
            disconnected.elapsed < Duration::from_secs(2),
            "disconnected prompt hook exceeded two seconds"
        );
        ensure!(
            context(&disconnected.stdout)?.is_empty(),
            "disconnected prompt hook emitted a letter"
        );
        ensure!(
            fixture
                .ledger()?
                .letters
                .iter()
                .any(|letter| letter.id == pending_id && letter.state == State::Pending),
            "disconnect acknowledged or lost the pending letter"
        );

        fixture.ssh.online(true)?;
        fixture.reconnect_device()?;
        fixture.wait_bridge(2)?;
        let resumed = fixture
            .remote
            .run_in_pane(&hook(&remote_hooks, "fixture-remote-session"))?;
        let intake = context(&resumed.stdout)?;
        ensure!(
            resumed.status == 0
                && intake.contains(&pending_id)
                && intake.contains("DELIVERY_AFTER_RECONNECT"),
            "reconnected remote hook did not receive the same letter"
        );
        wait_for("reconnected delivery confirmation", || {
            Ok(fixture
                .ledger()?
                .letters
                .iter()
                .any(|letter| letter.id == pending_id && letter.state == State::Delivered)
                .then_some(()))
        })?;
        let replay = fixture
            .local
            .run_in_pane(&format!(
                "{} request send remote-child --intent bridge-2 --body {}",
                quote(&fixture.hide),
                quote("DELIVERY_AFTER_RECONNECT")
            ))?
            .json()?;
        ensure!(
            replay["id"] == pending_id,
            "same intent created a new letter after reconnect"
        );
        ensure!(
            fixture
                .ledger()?
                .letters
                .iter()
                .filter(|letter| letter.id == pending_id)
                .count()
                == 1,
            "reconnect duplicated a durable letter"
        );
        let once = fixture
            .remote
            .run_in_pane(&hook(&remote_hooks, "fixture-remote-session"))?;
        ensure!(
            once.status == 0 && context(&once.stdout)?.is_empty(),
            "reconnect caused duplicate prompt intake"
        );
        ensure!(
            !fixture
                .remote
                .environment
                .home
                .join(".hide/state/delivery-ledger.json")
                .exists(),
            "remote account gained a mailbox spool"
        );
        Ok(())
    })();
    // Teardown is an assertion, and it never hides the first journey failure.
    let cleanup = fixture.stop();
    journey?;
    cleanup
}
