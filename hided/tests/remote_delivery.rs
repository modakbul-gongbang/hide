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

/// The `value` of a `hide agent` answer, which does not use the envelope of
/// the mailbox commands.
fn agent_value(output: fixture::PaneOutput) -> Result<Value> {
    ensure!(
        output.status == 0,
        "agent command failed: {}",
        output.stderr
    );
    let answer: Value = serde_json::from_str(&output.stdout).context("agent command JSON")?;
    ensure!(answer["ok"] == true, "agent command refused: {answer}");
    answer.get("value").cloned().context("agent command value")
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

/// A request the recipient took in and never answered stops being awaited when
/// the recipient's registration ends, and the sender, on the other machine,
/// reads that and the reason from `hide request show`; the log records it
/// without the body.
#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn ending_the_recipients_registration_ends_the_senders_answer_wait_and_says_why() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let remote_hide = fixture.remote.environment.home.join("bin/hide");
        let hide = quote(&fixture.hide);
        let recipient = agent_value(fixture.local.run_in_pane(&format!(
                "{hide} agent register --host-scope \"$HERDR_SOCKET_PATH\" \
                 --session fixture-local-session --instance {pane} --name local-parent --pane {pane}",
                pane = fixture.local.pane
            ))?)?["id"]
            .as_str()
            .context("the recipient's registration ID")?
            .to_owned();
        let sent = fixture
            .remote
            .run_in_pane(&format!(
                "{} request send local-parent --intent wait-ends-1 --body {}",
                quote(&remote_hide),
                quote("DELIVERY_WAIT_ENDS")
            ))?
            .json()?;
        let id = sent["id"].as_str().context("sent letter ID")?.to_owned();
        ensure!(
            sent["waiting_answer"] == true,
            "a new request awaits its answer"
        );
        let delivered = fixture
            .local
            .run_in_pane(&hook(&fixture.hooks, "fixture-local-session"))?;
        ensure!(
            context(&delivered.stdout)?.contains(&id),
            "the recipient did not take the request in: status {} stdout {:?} stderr {:?}",
            delivered.status,
            delivered.stdout,
            delivered.stderr
        );
        wait_for("durable delivery confirmation", || {
            Ok(fixture
                .ledger()?
                .letters
                .iter()
                .any(|letter| letter.id == id && letter.state == State::Delivered)
                .then_some(()))
        })?;
        let show = format!("{} request show {id}", quote(&remote_hide));
        let waiting = fixture.remote.run_in_pane(&show)?.json()?;
        ensure!(
            waiting["waiting_answer"] == true && waiting["answer_wait_ended"].is_null(),
            "an awaited request already reports an end: {waiting}"
        );

        agent_value(
            fixture
                .local
                .run_in_pane(&format!("{hide} agent end {recipient}"))?,
        )?;

        let shown = fixture.remote.run_in_pane(&show)?.json()?;
        ensure!(
            shown["waiting_answer"] == false,
            "the wait did not end: {shown}"
        );
        ensure!(
            shown["answer_wait_ended"] == "party_ended",
            "the sender cannot read why: {shown}"
        );
        ensure!(shown["state"] == "delivered", "the state moved: {shown}");
        ensure!(
            shown["finished_at_unix_ms"].is_u64(),
            "the finished letter has no finish time: {shown}"
        );
        let log = fixture.state.join("Logs/core.jsonl");
        wait_for("the answer_wait.ended log line", || {
            if !log.exists() {
                return Ok(None);
            }
            let text = String::from_utf8(std::fs::read(&log)?)?;
            let line = text
                .lines()
                .find(|line| line.contains("answer_wait.ended") && line.contains(id.as_str()));
            Ok(line.map(|line| {
                assert!(
                    line.contains("party_ended") && !line.contains("DELIVERY_WAIT_ENDS"),
                    "the log line carries the reason and never the body: {line}"
                );
            }))
        })?;
        Ok(())
    })();
    let cleanup = fixture.stop();
    journey?;
    cleanup
}
