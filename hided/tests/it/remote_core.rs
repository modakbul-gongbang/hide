//! A node whose core runs on another machine (PRD core-host-node-remote-core):
//! candidate hided on both sides, the pinned Herdr on each machine, and a
//! real SSH connection between them that only the screen machine opens.
#![cfg(unix)]

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use hided::node_role::{NodeIdentity, NodeRole, Phase};
use serde_json::Value;

use crate::support::remote_core::{CORE_NODE, Fixture};
use crate::support::remote_delivery::wait_for;

const LINK_BOUND: Duration = Duration::from_secs(30);

fn identity(fixture: &Fixture) -> Result<NodeIdentity> {
    Ok(NodeIdentity {
        node: herdr_core::node::NodeId::of_this_machine()
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned(),
        label: "screen-fixture".to_owned(),
        build: hided::build_id::of_file(&fixture.hided).map_err(anyhow::Error::msg)?,
        herdr_socket: fixture.screen.socket.clone(),
    })
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_dials_its_core_and_the_core_reaches_its_herdr_through_the_link() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let identity = identity(&fixture)?;
        let node = identity.node.clone();
        let role = NodeRole::start(fixture.screen_home(), fixture.placement(), identity)
            .map_err(anyhow::Error::msg)?;
        let phase = role.wait_for(LINK_BOUND, |phase| matches!(phase, Phase::Live(_)));
        ensure!(
            matches!(phase, Phase::Live(_)),
            "the node never linked: {phase:?}"
        );
        // The core took the node as a device of its own, keyed by the node
        // id, with no consent asked and its helper ready.
        let row = wait_for("the node's ready row on the core", || {
            Ok(fixture.device(&node)?.filter(|row| {
                row.pointer("/host/state")
                    .is_some_and(|state| state == "ready")
            }))
        })?;
        ensure!(
            row["kind"] == "remote",
            "the node's row is not remote: {row}"
        );
        // The core reaches the node's Herdr only through the link: a
        // checkout on the node registers and its session reads.
        let project = fixture.screen_home().join("project");
        fixture.create_workspace_on(&node, &project)?;
        wait_for("the node's checkout registered on the core", || {
            let snapshot = fixture.snapshot()?;
            Ok(snapshot
                .pointer("/status/remote")
                .and_then(Value::as_array)
                .and_then(|rows| rows.iter().find(|row| row["target_id"] == node.as_str()))
                .and_then(|remote| remote.pointer("/session/workspaces"))
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter().any(|row| {
                        row["registered"] == true
                            && row["path"] == project.to_string_lossy().as_ref()
                    })
                })
                .then_some(()))
        })?;
        ensure!(
            !fixture.core_log("node_link", "attach.linked")?.is_empty(),
            "the core logged no linked node"
        );
        // The node's end ends the link, and the core's row says so.
        drop(role);
        wait_for("the core's row after the node left", || {
            Ok(fixture
                .device(&node)?
                .filter(|row| {
                    row.pointer("/host/state")
                        .is_some_and(|state| state != "ready")
                })
                .map(|_| ()))
        })?;
        Ok(())
    })();
    match journey {
        Ok(()) => fixture.remove_run_dir(),
        Err(error) => {
            let _ = fixture.stop();
            Err(error).context(format!("run kept at {}", fixture.root.display()))
        }
    }
}

#[test]
#[ignore = "external lane requires this worktree's CLI binaries and HIDE_E2E_HERDR_BIN"]
fn a_node_of_another_build_or_the_cores_own_machine_is_refused() -> Result<()> {
    let mut fixture = Fixture::start()?;
    let journey = (|| {
        let mut other = identity(&fixture)?;
        other.build = "0".repeat(64);
        let role = NodeRole::start(fixture.screen_home(), fixture.placement(), other)
            .map_err(anyhow::Error::msg)?;
        let phase = role.wait_for(LINK_BOUND, |phase| matches!(phase, Phase::Waiting { .. }));
        ensure!(
            phase
                == Phase::Waiting {
                    reason: "other_build".to_owned()
                },
            "another build was not refused: {phase:?}"
        );
        drop(role);
        let mut own = identity(&fixture)?;
        own.node = CORE_NODE.to_owned();
        let role = NodeRole::start(fixture.screen_home(), fixture.placement(), own)
            .map_err(anyhow::Error::msg)?;
        let phase = role.wait_for(LINK_BOUND, |phase| matches!(phase, Phase::Waiting { .. }));
        ensure!(
            phase
                == Phase::Waiting {
                    reason: "own_node".to_owned()
                },
            "the core's own machine was not refused: {phase:?}"
        );
        Ok(())
    })();
    match journey {
        Ok(()) => fixture.remove_run_dir(),
        Err(error) => {
            let _ = fixture.stop();
            Err(error).context(format!("run kept at {}", fixture.root.display()))
        }
    }
}
