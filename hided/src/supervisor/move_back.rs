//! The supervisor's side of a move back (PRD core-host-node-move B6,
//! amendment 2): on the core's machine, its core role answering a release;
//! on the node, its node role driving the move from checks to the commit,
//! which is the core's machine retiring its core, after which this
//! machine's own core starts. Before the commit every failure resumes the
//! core's machine's core from its untouched folder and this machine goes
//! back to its node role; while the core's machine cannot be reached after
//! it stopped, this machine waits and starts no core, since that machine
//! may still start its own.

use super::*;
use crate::core_move::journal::Peer;

/// What a move back is made from, once every check passed.
pub(super) struct BackPrepared {
    pub journal: Journal,
    remote: Remote,
}

/// Checks and records a release asked of this core: the node it goes back
/// to is linked and alone, and the facts only this machine has are written
/// beside the copy to come.
pub(super) async fn release_core(
    env: &Env,
    running: &RunningDaemon,
    release: &Release,
) -> Result<(), String> {
    if !running.move_gate.is_open() {
        return Err("core_pending".to_owned());
    }
    let core = Arc::clone(&running.core);
    let (state_dir, home) = (env.state_dir.clone(), env.home.clone());
    let (intent, target) = (release.intent.clone(), release.target.clone());
    tokio::task::spawn_blocking(move || {
        let (source, labels) = core
            .release_source(&target)?
            .map_err(|refusal| refusal.code().to_owned())?;
        if !source.other_linked_nodes.is_empty() {
            return Err(format!(
                "other_node: {}",
                source.other_linked_nodes.join(", ")
            ));
        }
        let held = handover::hold(&state_dir)?;
        match held.read()? {
            Some(record)
                if record.state != HandoverState::Retired
                    && !(record.intent == intent && record.state == HandoverState::StoppedFor) =>
            {
                return Err(format!(
                    "another move ({}) holds this machine",
                    record.intent
                ));
            }
            _ => {}
        }
        let export = back::Export::new(&intent, &source, &target, labels, &home);
        back::write_export(&state_dir, &export)?;
        held.write(&Handover::new(
            &intent,
            &source.node,
            &target,
            HandoverState::StoppedFor,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(super) fn back_checks_failed(moves: &MoveControl, failed: Vec<FailedCheck>) {
    let checked =
        crate::core_move::control::checks_run(&crate::core_move::control::BACK_CHECKS, &failed);
    log(
        "checks.failed",
        json!({"direction": "back", "checks": failed.iter().map(|check| check.check).collect::<Vec<_>>(), "checked": checked}),
    );
    moves.set(MoveView {
        state: MoveState::ChecksFailed,
        direction: Some(Direction::Back),
        checked,
        failed,
        ..MoveView::default()
    });
}

/// Runs every check of a move back with nothing changed on either machine.
pub(super) async fn prepare_back(
    env: &Env,
    running: &RunningNode,
    moves: &MoveControl,
) -> Result<BackPrepared, Vec<FailedCheck>> {
    moves.set(MoveView {
        state: MoveState::Checking,
        direction: Some(Direction::Back),
        ..MoveView::default()
    });
    let fail = |check: CheckId, detail: String| vec![FailedCheck { check, detail }];
    let own = herdr_core::node::NodeId::of_this_machine()
        .map_err(|error| fail(CheckId::Connection, error))?;
    let placement = placement::read(&env.state_dir, own.as_str())
        .map_err(|error| fail(CheckId::Connection, error))?
        .ok_or_else(|| fail(CheckId::Connection, "this machine names no core".to_owned()))?;
    let mut failed = Vec::new();
    if !matches!(
        running.node.role_handle().phase(),
        crate::node_role::Phase::Live(_)
    ) {
        failed.push(FailedCheck {
            check: CheckId::Link,
            detail: placement.alias.clone(),
        });
    }
    let own_socket = env.herdr_socket_path.clone();
    if own_socket.is_none() {
        failed.push(FailedCheck {
            check: CheckId::Herdr,
            detail: "this machine has no Herdr server".to_owned(),
        });
    }
    let present = copy::brain_present(&env.state_dir);
    if !present.is_empty() {
        failed.push(FailedCheck {
            check: CheckId::OwnState,
            detail: present.join(", "),
        });
    }
    let (home, alias, program) = (
        env.home.clone(),
        placement.alias.clone(),
        placement.program.clone(),
    );
    let state_dir = placement.state_dir.clone();
    let build = own_build(env).map_err(|reason| fail(CheckId::Connection, reason))?;
    let inspected = tokio::task::spawn_blocking(move || {
        let mut remote = Remote::new(&home, &alias, &program, &build);
        if let Some(state_dir) = &state_dir {
            remote = remote.with_state_dir(state_dir);
        }
        let inspected = driver::inspect(&remote, &[])?;
        Ok::<_, MoveFailure>((remote, inspected))
    })
    .await
    .map_err(|error| fail(CheckId::Connection, error.to_string()))?;
    let (remote, inspected) = match inspected {
        Ok(found) => found,
        Err(failure) => {
            failed.push(FailedCheck {
                check: CheckId::Connection,
                detail: format!("{failure:?}"),
            });
            return Err(failed);
        }
    };
    if inspected.node != placement.node {
        failed.push(FailedCheck {
            check: CheckId::Identity,
            detail: inspected.node.clone(),
        });
    }
    // A journal that cannot be read fails the check rather than reading as
    // no earlier move, which would lose the retry's intent and the forward
    // move's ids.
    let previous = match journal::read(&env.state_dir) {
        Ok(previous) => previous,
        Err(detail) => {
            failed.push(FailedCheck {
                check: CheckId::Connection,
                detail,
            });
            return Err(failed);
        }
    };
    let forward_ids = back::forward_ids_reversed(previous.as_ref(), &placement.node);
    let retry = previous.filter(|journal| {
        journal.phase.direction() == Direction::Back
            && journal.peer.node == placement.node
            && journal.phase.is_rolled_back()
    });
    let other_move = inspected.handover.as_ref().is_some_and(|handover| {
        handover.state != HandoverState::Retired
            && retry
                .as_ref()
                .is_none_or(|retry| retry.intent != handover.intent)
    });
    if other_move {
        failed.push(FailedCheck {
            check: CheckId::TargetState,
            detail: inspected.state_dir.clone(),
        });
    }
    // The core's settings come back with it and are never merged with
    // settings this machine kept.
    match &inspected.ai {
        Ok(Some(_)) => {}
        Ok(None) => failed.push(FailedCheck {
            check: CheckId::Ai,
            detail: "the core's machine holds no Hide AI settings".to_owned(),
        }),
        Err(reason) => failed.push(FailedCheck {
            check: CheckId::Ai,
            detail: reason.clone(),
        }),
    }
    let unlike = match (
        &inspected.ai,
        herdr_core::stored_hide_ai_settings(&env.home),
    ) {
        (_, Ok(None)) => None,
        (Ok(Some(theirs)), Ok(Some(ours))) if *theirs == ours => None,
        (_, Ok(Some(_))) => Some("Hide AI settings unlike the core's".to_owned()),
        (_, Err(reason)) => Some(reason),
    };
    if let Some(unlike) = unlike {
        match failed
            .iter_mut()
            .find(|check| check.check == CheckId::OwnState)
        {
            Some(check) => check.detail = format!("{}, {unlike}", check.detail),
            None => failed.push(FailedCheck {
                check: CheckId::OwnState,
                detail: unlike,
            }),
        }
    }
    let (Some(peer_socket), Some(own_socket)) = (inspected.herdr_socket.clone(), own_socket) else {
        if inspected.herdr_socket.is_none() {
            failed.push(FailedCheck {
                check: CheckId::Herdr,
                detail: "the core's machine has no Herdr server".to_owned(),
            });
        }
        return Err(failed);
    };
    if !failed.is_empty() {
        return Err(failed);
    }
    let peer = Peer {
        device: placement.alias.clone(),
        alias: placement.alias.clone(),
        node: placement.node.clone(),
        program: placement.program.clone(),
        state_dir: inspected.state_dir.clone(),
    };
    // The registration this machine's core kept for the core's machine
    // before the forward move; a node that never held the core makes one
    // the operator consents to again.
    let registration = match back::kept_registration(&env.state_dir, &peer.device) {
        Some(registration) => registration,
        None => serde_json::from_value(json!({
            "id": peer.device,
            "label": peer.device,
            "ssh_alias": peer.alias,
            "herdr_socket_path": peer_socket,
        }))
        .map_err(|error| fail(CheckId::Connection, error.to_string()))?,
    };
    let change = back::back_change(&peer, own.as_str(), registration, peer_socket, own_socket);
    // A retry keeps the table its first try started from.
    let ids = match &retry {
        Some(retry) => retry.ids.clone(),
        None => forward_ids,
    };
    let intent = retry
        .map(|journal| journal.intent)
        .unwrap_or_else(crate::core_move::new_intent);
    let journal = Journal::new(intent, Phase::Back(BackPhase::Releasing), peer, change, ids);
    Ok(BackPrepared { journal, remote })
}

/// A move of the core back to this machine, from its node role.
pub(super) async fn back(env: &Env, seat: &seat::Seat, running: RunningNode) -> Result<Role, Halt> {
    let moves = Arc::clone(&seat.moves);
    let Some(prepared) = unless_stopped(&seat.stop, prepare_back(env, &running, &moves)).await
    else {
        stop_node(running).await;
        return Err(Halt::Stopped);
    };
    let BackPrepared { journal, remote } = match prepared {
        Ok(prepared) => prepared,
        Err(failed) => {
            back_checks_failed(&moves, failed);
            return Ok(Role::Node(running));
        }
    };
    if let Err(reason) = journal::write(&env.state_dir, &journal) {
        moves.set(MoveView {
            state: MoveState::RolledBack,
            direction: Some(Direction::Back),
            step: Some(MoveStep::StopCore),
            cause: Some(MoveFailure::Local { reason }),
            ..MoveView::default()
        });
        return Ok(Role::Node(running));
    }
    log(
        "move.started",
        json!({"intent": journal.intent, "direction": "back", "source": journal.peer.node}),
    );
    let (device, intent) = (journal.peer.device.clone(), journal.intent.clone());
    let view = move |state: MoveState| MoveView {
        state,
        direction: Some(Direction::Back),
        device: Some(device.clone()),
        intent: Some(intent.clone()),
        ..MoveView::default()
    };
    moves.set(view(MoveState::Stopping));
    // The move's screen is mounted first, so a window the node's end closes
    // reconnects to it and hears the move (W1). The node's link ends before
    // the core it links to stops, so no window of this machine is held on a
    // link that will not come back.
    let screen = MoveScreen::mount(&seat.parts(), env.vite_origin.as_deref());
    stop_node(running).await;
    let remote = Arc::new(remote);
    let own = herdr_core::node::NodeId::of_this_machine()?;
    let released = unless_stopped(
        &seat.stop,
        blocking({
            let (remote, journal) = (Arc::clone(&remote), journal.clone());
            move || back::release(&remote, &journal, own.as_str())
        }),
    )
    .await
    .ok_or(Halt::Stopped)??;
    let mut journal = journal;
    match released {
        Ok(Released::Staged) => {}
        Ok(Released::Retired) => {
            return retire_and_commit(env, seat, journal, remote, screen).await;
        }
        Err(cause) => {
            return back_rollback(
                env,
                seat,
                journal,
                &remote,
                screen,
                MoveStep::StopCore,
                cause,
            )
            .await;
        }
    }
    journal.phase = Phase::Back(BackPhase::Released);
    if let Err(reason) = journal::write(&env.state_dir, &journal) {
        let cause = MoveFailure::Local { reason };
        return back_rollback(env, seat, journal, &remote, screen, MoveStep::Copy, cause).await;
    }
    moves.set(view(MoveState::Copying));
    let copied = unless_stopped(
        &seat.stop,
        blocking({
            let (state_dir, settings, remote, journal, moves) = (
                env.state_dir.clone(),
                herdr_core::hide_ai_settings_path(&env.home),
                Arc::clone(&remote),
                journal.clone(),
                Arc::clone(&moves),
            );
            move || -> Result<Journal, MoveFailure> {
                let progress = |sent: u64, total: u64| {
                    moves.update(|view| {
                        view.sent = sent;
                        view.total = total;
                    });
                };
                back::pull(&remote, &state_dir, &journal, &progress)?;
                let mut journal = journal;
                journal.ids = back::rekey(&state_dir, &journal)?;
                // Recorded first: a placement whose end is not seen is taken
                // back out all the same.
                journal.phase = Phase::Back(BackPhase::PlacedHere);
                journal::write(&state_dir, &journal)
                    .map_err(|reason| MoveFailure::Local { reason })?;
                if let Err(not_placed) = back::place_here(&state_dir, &settings, &journal) {
                    // Nothing of the copy is in the folder, so what it holds is
                    // this machine's own and the rollback takes none of it.
                    if not_placed.left.is_empty() {
                        journal.phase = Phase::Back(BackPhase::Released);
                        journal::write(&state_dir, &journal)
                            .map_err(|reason| MoveFailure::Local { reason })?;
                    }
                    return Err(MoveFailure::Staging {
                        file: not_placed.refusal.file.display().to_string(),
                        reason: not_placed.refusal.reason,
                    });
                }
                Ok(journal)
            }
        }),
    )
    .await
    .ok_or(Halt::Stopped)??;
    match copied {
        Ok(journal) => retire_and_commit(env, seat, journal, remote, screen).await,
        Err(cause) => {
            // The journal on disk says how far the copy got.
            let journal = journal::read(&env.state_dir)?.unwrap_or(journal);
            back_rollback(env, seat, journal, &remote, screen, MoveStep::Copy, cause).await
        }
    }
}

/// Has the core's machine retire its core, which commits the move back,
/// then starts this machine's core. A machine that cannot be reached is
/// asked again; one that refuses is resumed and the move undone.
pub(super) async fn retire_and_commit(
    env: &Env,
    seat: &seat::Seat,
    mut journal: Journal,
    remote: Arc<Remote>,
    screen: MoveScreen,
) -> Result<Role, Halt> {
    seat.moves.update(|view| {
        view.state = MoveState::Starting;
        view.direction = Some(Direction::Back);
        view.intent = Some(journal.intent.clone());
    });
    if journal.phase != Phase::Back(BackPhase::Retiring) {
        journal.phase = Phase::Back(BackPhase::Retiring);
        journal::write(&env.state_dir, &journal)?;
    }
    let retired = wait_on_peer(
        &seat.stop,
        &seat.moves,
        "retire.failed",
        &journal.intent,
        Permanent::Ends,
        {
            let (remote, journal) = (Arc::clone(&remote), journal.clone());
            move || back::retire(&remote, &journal)
        },
        |_| false,
    )
    .await?;
    if let Err(cause) = retired {
        return back_rollback(
            env,
            seat,
            journal,
            &remote,
            screen,
            MoveStep::StartTarget,
            cause,
        )
        .await;
    }
    log(
        "move.committed",
        json!({"intent": journal.intent, "direction": "back"}),
    );
    // The journal stays at Retiring until both are gone, so a start
    // meanwhile commits again, which the peer answers as retired.
    until_removed(seat, &journal.intent, "commit.unfinished", || {
        placement::remove(&env.state_dir)?;
        back::remove_staging(&env.state_dir, &journal.intent)
    })
    .await?;
    journal.phase = Phase::Back(BackPhase::Done);
    journal::write(&env.state_dir, &journal)?;
    drop(screen);
    let running = crate::start_core_role(env.clone(), seat.parts()).await?;
    log(
        "move.done",
        json!({"intent": journal.intent, "direction": "back"}),
    );
    seat.moves.set(MoveView {
        state: MoveState::Done,
        direction: Some(Direction::Back),
        device: Some(journal.peer.device.clone()),
        intent: Some(journal.intent.clone()),
        ..MoveView::default()
    });
    Ok(Role::Core(running))
}

/// Undoes a move back before its commit: the core's machine starts its
/// core again, the placed copy is taken back out, and this machine's node
/// role links to it. While that machine cannot be reached this machine
/// waits; if it says it retired after all (a retirement whose answer was
/// lost), the move goes forward on the copy still placed here.
pub(super) async fn back_rollback(
    env: &Env,
    seat: &seat::Seat,
    mut journal: Journal,
    remote: &Arc<Remote>,
    screen: MoveScreen,
    step: MoveStep,
    cause: MoveFailure,
) -> Result<Role, Halt> {
    log(
        "move.rolling_back",
        json!({"intent": journal.intent, "direction": "back", "step": step, "cause": cause}),
    );
    seat.moves.set(MoveView {
        state: MoveState::RollingBack,
        direction: Some(Direction::Back),
        device: Some(journal.peer.device.clone()),
        intent: Some(journal.intent.clone()),
        step: Some(step),
        cause: Some(cause.clone()),
        ..MoveView::default()
    });
    let resumed = wait_on_peer(
        &seat.stop,
        &seat.moves,
        "resume.failed",
        &journal.intent,
        Permanent::Ends,
        {
            let (remote, journal) = (Arc::clone(remote), journal.clone());
            move || back::resume(&remote, &journal)
        },
        |_| false,
    )
    .await?;
    match resumed {
        Ok(Resumed::Running) => {}
        Ok(Resumed::Retired) => {
            log("rollback.found_retired", json!({"intent": journal.intent}));
            if copy::brain_present(&env.state_dir).is_empty() {
                return Err(format!(
                    "the move back {} committed on the core's machine with no copy placed here",
                    journal.intent
                )
                .into());
            }
            return Box::pin(retire_and_commit(
                env,
                seat,
                journal,
                Arc::clone(remote),
                screen,
            ))
            .await;
        }
        // The core's machine answered, and asking again answers the same:
        // this machine goes back to its node role, starting no core of its
        // own, and the window names the failure.
        Err(failure) => log(
            "rollback.unclean",
            json!({"intent": journal.intent, "peer": failure}),
        ),
    }
    let undone = blocking({
        let (state_dir, settings, journal) = (
            env.state_dir.clone(),
            herdr_core::hide_ai_settings_path(&env.home),
            journal.clone(),
        );
        move || back::unplace_here(&state_dir, &settings, &journal)
    })
    .await?;
    if let Err(reason) = undone {
        // The node role starts regardless of brain files beside it; the
        // next move's check names what is left.
        log(
            "rollback.unclean",
            json!({"intent": journal.intent, "reason": reason}),
        );
    }
    journal.phase = Phase::Back(BackPhase::RolledBack {
        failed: step,
        cause: cause.clone(),
    });
    journal::write(&env.state_dir, &journal)?;
    drop(screen);
    let node = crate::start_node_role(env.clone(), seat.parts()).await?;
    log(
        "move.rolled_back",
        json!({"intent": journal.intent, "direction": "back", "step": step}),
    );
    seat.moves.set(MoveView {
        state: MoveState::RolledBack,
        direction: Some(Direction::Back),
        device: Some(journal.peer.device.clone()),
        intent: Some(journal.intent.clone()),
        step: Some(step),
        cause: Some(cause),
        ..MoveView::default()
    });
    Ok(Role::Node(node))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())
}
