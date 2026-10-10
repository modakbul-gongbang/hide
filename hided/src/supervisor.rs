//! The process's supervisor (PRD core-host-node-move Q12): it holds the
//! instance lock and the seat for the process's life, runs one role at a
//! time on the seat, and drives a core move, which switches the role from
//! the core to the move screen to the node, or back to the core when the
//! move is undone. A start that finds a move unresolved in the journal
//! resolves it before it runs any role of its own choosing.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use herdr_core::node_migration::{self, copy};
use serde_json::json;

use crate::core_move::back::{self, Released, Resumed};
use crate::core_move::control::{
    Asked, CheckId, FailedCheck, MoveControl, MoveRequest, MoveState, MoveView, Release,
};
use crate::core_move::driver::{self, Remote, TargetSays};
use crate::core_move::handover::{self, Handover, HandoverState};
use crate::core_move::journal::{self, Direction, Journal, MoveFailure, MoveStep, Phase};
use crate::core_move::screen::MoveScreen;
use crate::env::Env;
use crate::placement::{self, Placement};
use crate::{RunningDaemon, RunningNode, seat, server, state_file};

mod move_back;
use move_back::{
    back, back_checks_failed, back_rollback, prepare_back, release_core, retire_and_commit,
};

/// How long a pending core waits for the link that carries its move.
const PENDING_LEASE: Duration = Duration::from_secs(60);
/// How long past the target's lease a driver that cannot reach it waits
/// before it takes the lease as ended: clocks of two machines differ.
const LEASE_MARGIN: Duration = Duration::from_secs(30);
/// How long the node's first link has to be taken.
const LINK_WITHIN: Duration = Duration::from_secs(30);
/// How often a driver that cannot reach the other machine tries again.
const RETRY_EVERY: Duration = Duration::from_secs(2);
/// How long the core role has to stop before its process ends instead.
const CORE_STOP_WITHIN: Duration = Duration::from_secs(20);
/// The exit of a process whose core did not stop in time.
const STOP_UNCONFIRMED_EXIT: i32 = 3;

enum Role {
    Core(RunningDaemon),
    Node(RunningNode),
}

pub async fn run(env: Env) -> Result<(), String> {
    let state_dir = env.state_dir.clone();
    if let Some(error) = crate::env::herdr_bin_error(&env) {
        return Err(error);
    }
    let lock = state_file::acquire_lock(&env.state_dir).map_err(|error| error.to_string())?;
    // The process's records go beside its state from its start, so a move
    // resumed before any role runs is logged where its roles log.
    if let Err(error) = herdr_core::diagnostics::install(&env.state_dir.join("core-state.json")) {
        eprintln!(
            "{}",
            json!({"kind": "diagnostics.open_failed", "message": error.to_string()})
        );
    }
    let seat = seat::Seat::serve(server::bind(env.bind).await?, state_file::new_token())?;
    let mut requests = seat
        .moves
        .take_requests()
        .ok_or("the move's requests were taken twice")?;
    // Recorded once for the process, so a window finds it whichever role
    // runs, the move screen included; a request before the first role
    // waits for it (`seat`).
    crate::record_daemon(&state_dir, &seat.parts(), env.herdr_socket_path.clone())?;
    let ran = async {
        let mut role = resume(&env, &seat).await?;
        while let Some(current) = role.take() {
            role = match current {
                Role::Core(running) => core_turn(&env, &seat, running, &mut requests).await?,
                Role::Node(running) => node_turn(&env, &seat, running, &mut requests).await?,
            };
        }
        Ok::<(), String>(())
    }
    .await;
    seat.close().await;
    // Its own state only, before the instance lock is released: a daemon
    // started after it writes its own.
    state_file::forget_daemon(&state_dir, std::process::id());
    drop(lock);
    ran
}

/// Runs the core until it stops or a move takes it; answers the next role.
async fn core_turn(
    env: &Env,
    seat: &seat::Seat,
    running: RunningDaemon,
    requests: &mut tokio::sync::mpsc::Receiver<Asked>,
) -> Result<Option<Role>, String> {
    enum Event {
        Stop,
        Request(MoveRequest),
        Release(Release),
        LeaseEnded,
    }
    let gate = Arc::clone(&running.move_gate);
    let event = tokio::select! {
        _ = running.shutdown.notified() => Event::Stop,
        () = crate::stop_requested() => Event::Stop,
        Some(asked) = requests.recv() => match asked {
            Asked::Window(request) => Event::Request(request),
            Asked::Release(release) => Event::Release(release),
        },
        () = lease_end(&gate) => Event::LeaseEnded,
    };
    match event {
        Event::Release(release) => {
            let checked = release_core(env, &running, &release).await;
            let refused = checked.as_ref().err().cloned();
            log(
                "release.checked",
                json!({"intent": release.intent, "target": release.target, "refused": refused}),
            );
            let _ = release.reply.send(checked);
            if refused.is_some() {
                return Ok(Some(Role::Core(running)));
            }
            // The stop is recorded: this process runs no role after it, and
            // its next start runs none until the move resumes it.
            stop_core(&env.state_dir, running).await;
            Ok(None)
        }
        Event::Request(MoveRequest::CheckBack | MoveRequest::Back) => {
            seat.moves.set(MoveView {
                state: MoveState::ChecksFailed,
                direction: Some(Direction::Back),
                failed: vec![FailedCheck {
                    check: CheckId::Connection,
                    detail: "the core runs on this machine".to_owned(),
                }],
                ..MoveView::default()
            });
            Ok(Some(Role::Core(running)))
        }
        Event::Stop => {
            stop_core(&env.state_dir, running).await;
            Ok(None)
        }
        Event::Request(MoveRequest::Check { device }) => {
            match prepare(env, &running, &device, &seat.moves).await {
                Ok(_) => seat.moves.set(MoveView {
                    state: MoveState::Ready,
                    device: Some(device),
                    ..MoveView::default()
                }),
                Err(failed) => checks_failed(&seat.moves, &device, failed),
            }
            Ok(Some(Role::Core(running)))
        }
        Event::Request(MoveRequest::Start { device }) => {
            if !running.move_gate.is_open() {
                return Ok(Some(Role::Core(running)));
            }
            forward(env, seat, running, &device).await.map(Some)
        }
        Event::LeaseEnded => {
            let intent = gate.pending_intent();
            stop_core(&env.state_dir, running).await;
            give_back_pending(&env.state_dir, intent.as_deref());
            Ok(None)
        }
    }
}

async fn node_turn(
    env: &Env,
    seat: &seat::Seat,
    running: RunningNode,
    requests: &mut tokio::sync::mpsc::Receiver<Asked>,
) -> Result<Option<Role>, String> {
    loop {
        tokio::select! {
            _ = running.shutdown.notified() => break,
            () = crate::stop_requested() => {
                running.stop();
                break;
            }
            Some(asked) = requests.recv() => match asked {
                Asked::Release(release) => {
                    let _ = release.reply.send(Err("this machine runs no core".to_owned()));
                }
                Asked::Window(MoveRequest::CheckBack) => {
                    match prepare_back(env, &running, &seat.moves).await {
                        Ok(prepared) => seat.moves.set(MoveView {
                            state: MoveState::Ready,
                            direction: Some(Direction::Back),
                            device: Some(prepared.journal.peer.device),
                            ..MoveView::default()
                        }),
                        Err(failed) => back_checks_failed(&seat.moves, failed),
                    }
                }
                Asked::Window(MoveRequest::Back) => {
                    return back(env, seat, running).await.map(Some);
                }
                Asked::Window(MoveRequest::Check { device } | MoveRequest::Start { device }) => {
                    seat.moves.set(MoveView {
                        state: MoveState::ChecksFailed,
                        direction: Some(Direction::Forward),
                        device: Some(device),
                        failed: vec![FailedCheck {
                            check: CheckId::Connection,
                            detail: "the core does not run on this machine".to_owned(),
                        }],
                        ..MoveView::default()
                    });
                }
            },
        }
    }
    stop_node(running).await;
    Ok(None)
}

async fn stop_node(running: RunningNode) {
    let ended = tokio::task::spawn_blocking(move || drop(running)).await;
    if ended.is_err() {
        log("node.stop_failed", json!({}));
    }
}

/// Never answers while the gate is open; answers when a pending core's
/// lease ran out before its move's link came.
async fn lease_end(gate: &crate::core_move::gate::MoveGate) {
    if gate.is_open() {
        return std::future::pending().await;
    }
    let mut open = gate.subscribe();
    tokio::select! {
        _ = open.wait_for(|open| *open) => std::future::pending().await,
        () = tokio::time::sleep(PENDING_LEASE) => {}
    }
}

/// A pending core whose move never linked gives its copy back to
/// `move-incoming`, where the driver's retry finds it, and leaves the
/// folder with no brain state.
fn give_back_pending(state_dir: &Path, intent: Option<&str>) {
    let Some(intent) = intent else { return };
    let given = (|| -> Result<(), String> {
        let held = crate::core_move::handover::hold(state_dir)?;
        match held.read()? {
            Some(record)
                if record.intent == intent
                    && record.state == crate::core_move::handover::HandoverState::Pending => {}
            _ => return Ok(()),
        }
        copy::unplace(
            state_dir,
            &crate::core_move::target::incoming(state_dir, intent),
        )
        .map_err(|refusal| refusal.to_string())?;
        held.remove()
    })();
    log(
        "pending.lease_ended",
        json!({"intent": intent, "given_back": given.is_ok(), "error": given.err()}),
    );
}

/// Stops the core role and waits until its last saves are on disk. A core
/// that has not stopped within [`CORE_STOP_WITHIN`], or whose stop
/// panicked, cannot be ended apart from this process, so the process ends,
/// unsuccessfully: its next start
/// (a login item's keep-alive, or the window's `hide connect`) resolves
/// the move its journal records, and a move that stopped nothing yet is
/// undone.
async fn stop_core(state_dir: &Path, running: RunningDaemon) {
    // On a thread of its own, so a stop that never returns is never
    // dropped on this one.
    let stopping = tokio::task::spawn_blocking(move || {
        if crate::env::fixture_core_stop_hangs() {
            loop {
                std::thread::park();
            }
        }
        // A graceful stop takes hide's `tailscale serve` entry with it; a
        // crash leaves it to the next start's reconcile (PRD D-07).
        tokio::runtime::Handle::current().block_on(running.mobile.shutdown());
        drop(running);
    });
    let unconfirmed = match tokio::time::timeout(CORE_STOP_WITHIN, stopping).await {
        Ok(Ok(())) => return,
        // A stop that panicked may have left its last saves unwritten, so
        // nothing is made from the folder in this process either.
        Ok(Err(error)) => json!({"cause": "panicked", "message": error.to_string()}),
        Err(_) => json!({"cause": "timed_out", "within_ms": CORE_STOP_WITHIN.as_millis() as u64}),
    };
    let mut record = json!({"component": "core_move", "kind": "core.stop_unconfirmed"});
    if let (Some(record), Some(fields)) = (record.as_object_mut(), unconfirmed.as_object()) {
        record.extend(fields.clone());
    }
    // Written at once: the queued sink's records end with the process.
    if let Err(error) = herdr_core::diagnostics::record_now(
        &state_dir.join(herdr_core::node_migration::CORE_STATE),
        record,
    ) {
        eprintln!("the unconfirmed stop could not be logged: {error}");
    }
    std::process::exit(STOP_UNCONFIRMED_EXIT);
}

fn log(kind: &str, fields: serde_json::Value) {
    let mut record = json!({"component": "core_move", "kind": kind});
    if let (Some(record), Some(fields)) = (record.as_object_mut(), fields.as_object()) {
        record.extend(fields.clone());
    }
    herdr_core::diagnostic!(record);
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// The role a start runs: a move the journal left unresolved first, then
/// the placement record's choice.
async fn resume(env: &Env, seat: &seat::Seat) -> Result<Option<Role>, String> {
    // A core this folder stopped or retired for a move back starts again
    // only when that move resumes it; a start meanwhile (a login, launchd's
    // keep-alive) ends at once and successfully, so it is not retried.
    if let Some(record) = crate::core_move::handover::read(&env.state_dir)?
        && matches!(
            record.state,
            HandoverState::StoppedFor | HandoverState::Retired
        )
    {
        log(
            "start.held",
            json!({"intent": record.intent, "state": record.state}),
        );
        return Ok(None);
    }
    if let Some(journal) = journal::read(&env.state_dir)?
        && journal.phase.holds_the_core()
        && journal.direction == Direction::Back
    {
        log(
            "move.resumed",
            json!({"intent": journal.intent, "phase": journal.phase}),
        );
        let remote = Arc::new(remote_for(env, &journal).map_err(|failure| {
            format!(
                "the move {} cannot reach its peer: {failure:?}",
                journal.intent
            )
        })?);
        let screen = MoveScreen::mount(&seat.parts(), env.vite_origin.as_deref());
        let step = journal.phase.step();
        let role = if journal.phase == Phase::Retiring {
            retire_and_commit(env, seat, journal, remote, screen).await?
        } else {
            back_rollback(
                env,
                seat,
                journal,
                &remote,
                screen,
                step,
                MoveFailure::Local {
                    reason: "the move was interrupted".to_owned(),
                },
            )
            .await?
        };
        return Ok(Some(role));
    }
    resume_forward(env, seat).await.map(Some)
}

async fn resume_forward(env: &Env, seat: &seat::Seat) -> Result<Role, String> {
    if let Some(journal) = journal::read(&env.state_dir)?
        && journal.phase.holds_the_core()
    {
        log(
            "move.resumed",
            json!({"intent": journal.intent, "phase": journal.phase}),
        );
        let remote = Arc::new(remote_for(env, &journal).map_err(|failure| {
            format!(
                "the move {} cannot reach its peer: {failure:?}",
                journal.intent
            )
        })?);
        return match journal.phase {
            Phase::Committed => {
                let node = crate::start_node_role(env.clone(), seat.parts()).await?;
                commit(env, &seat.moves, journal, &remote).await;
                Ok(Role::Node(node))
            }
            Phase::AttachSent => {
                let node = crate::start_node_role(env.clone(), seat.parts()).await?;
                link_outcome(env, seat, journal, remote, node).await
            }
            _ => {
                let step = journal.phase.step();
                rollback(
                    env,
                    seat,
                    journal,
                    &remote,
                    step,
                    MoveFailure::Local {
                        reason: "the move was interrupted".to_owned(),
                    },
                )
                .await
            }
        };
    }
    let node = herdr_core::node::NodeId::of_this_machine()?;
    if placement::read(&env.state_dir, node.as_str())?.is_some() {
        return Ok(Role::Node(
            crate::start_node_role(env.clone(), seat.parts()).await?,
        ));
    }
    Ok(Role::Core(
        crate::start_core_role(env.clone(), seat.parts()).await?,
    ))
}

fn remote_for(env: &Env, journal: &Journal) -> Result<Remote, MoveFailure> {
    Ok(
        Remote::new(&env.home, &journal.peer.alias, &journal.peer.program)?
            .with_state_dir(&journal.peer.state_dir),
    )
}

/// What a move is made from, once every check passed.
struct Prepared {
    journal: Journal,
    labels: serde_json::Value,
    remote: Remote,
}

fn checks_failed(moves: &MoveControl, device: &str, failed: Vec<FailedCheck>) {
    log(
        "checks.failed",
        json!({"device": device, "checks": failed.iter().map(|check| check.check).collect::<Vec<_>>(), "failed": failed}),
    );
    moves.set(MoveView {
        state: MoveState::ChecksFailed,
        device: Some(device.to_owned()),
        failed,
        ..MoveView::default()
    });
}

/// Runs every check of a move to `device` with nothing changed on either
/// machine, and answers what the move is made from.
async fn prepare(
    env: &Env,
    running: &RunningDaemon,
    device: &str,
    moves: &MoveControl,
) -> Result<Prepared, Vec<FailedCheck>> {
    moves.set(MoveView {
        state: MoveState::Checking,
        device: Some(device.to_owned()),
        ..MoveView::default()
    });
    let fail = |check: CheckId, detail: String| vec![FailedCheck { check, detail }];
    let core = Arc::clone(&running.core);
    let asked = device.to_owned();
    let answer = tokio::task::spawn_blocking(move || core.move_source(&asked))
        .await
        .map_err(|error| fail(CheckId::Connection, error.to_string()))?
        .map_err(|error| fail(CheckId::Connection, error))?;
    let (source, labels) =
        answer.map_err(|refusal| fail(CheckId::Connection, refusal.code().to_owned()))?;
    let mut failed = Vec::new();
    if !source.other_linked_nodes.is_empty() {
        failed.push(FailedCheck {
            check: CheckId::OtherNode,
            detail: source.other_linked_nodes.join(", "),
        });
    }
    let own_socket = env.herdr_socket_path.clone();
    if own_socket.is_none() {
        failed.push(FailedCheck {
            check: CheckId::Herdr,
            detail: "this machine has no Herdr server".to_owned(),
        });
    }
    let asks = match herdr_core::hide_ai_asks(&env.home) {
        Ok(asks) => asks,
        Err(detail) => {
            failed.push(FailedCheck {
                check: CheckId::Ai,
                detail,
            });
            Vec::new()
        }
    };
    let home = env.home.clone();
    let (alias, program) = (source.ssh_alias.clone(), source.helper_path.clone());
    let inspected = tokio::task::spawn_blocking(move || {
        let remote = Remote::new(&home, &alias, &program)?;
        let inspected = driver::inspect(&remote, &asks)?;
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
    if inspected.node != source.device_node {
        failed.push(FailedCheck {
            check: CheckId::Identity,
            detail: inspected.node.clone(),
        });
    }
    match env
        .build
        .clone()
        .map_or_else(crate::build_id::of_current_exe, Ok)
    {
        Ok(own) if own == inspected.build => {}
        Ok(own) => failed.push(FailedCheck {
            check: CheckId::Build,
            detail: format!("the device runs {}, this machine {own}", inspected.build),
        }),
        Err(reason) => failed.push(FailedCheck {
            check: CheckId::Build,
            detail: reason,
        }),
    }
    failed.extend(inspected.failed.iter().cloned());
    if !source.dormant.is_empty() {
        failed.push(FailedCheck {
            check: CheckId::Dormant,
            detail: source.dormant.join(", "),
        });
    }

    let retry = journal::read(&env.state_dir)
        .ok()
        .flatten()
        .filter(|journal| {
            journal.peer.device == device && matches!(journal.phase, Phase::RolledBack { .. })
        });
    // A core the device retired for a move back holds nothing there.
    let other_move = inspected.handover.as_ref().is_some_and(|handover| {
        handover.state != HandoverState::Retired
            && retry
                .as_ref()
                .is_none_or(|retry| retry.intent != handover.intent)
    });
    let mut held = inspected.brain.clone();
    if let Some(handover) = inspected.handover.as_ref().filter(|_| other_move) {
        held.push(format!("move {}", handover.intent));
    }
    // Hide AI settings move with the core and are never merged.
    match (
        &inspected.ai,
        herdr_core::stored_hide_ai_settings(&env.home),
    ) {
        (Ok(None), _) => {}
        (Ok(Some(theirs)), Ok(Some(ours))) if *theirs == ours => {}
        (Ok(Some(_)), _) => held.push("Hide AI settings unlike this machine's".to_owned()),
        (Err(reason), _) => held.push(reason.clone()),
    }
    if !held.is_empty() {
        failed.push(FailedCheck {
            check: CheckId::TargetState,
            detail: format!("{}: {}", inspected.state_dir, held.join(", ")),
        });
    }
    let Some(target_socket) = inspected.herdr_socket.clone() else {
        if !inspected
            .failed
            .iter()
            .any(|check| check.check == CheckId::Herdr)
        {
            failed.push(FailedCheck {
                check: CheckId::Herdr,
                detail: "the device has no Herdr server".to_owned(),
            });
        }
        return Err(failed);
    };
    if !failed.is_empty() {
        return Err(failed);
    }
    let Some(own_socket) = own_socket else {
        return Err(failed);
    };
    let own_label = crate::host_name().unwrap_or_else(|| source.node.clone());
    let (change, ids) =
        driver::forward_change(&source, &env.home, &own_label, own_socket, target_socket)
            .map_err(|failure| fail(CheckId::Connection, format!("{failure:?}")))?;
    // A retry keeps its intent, so the copy the device holds from it is
    // checked by digest rather than sent again.
    let intent = retry
        .map(|journal| journal.intent)
        .unwrap_or_else(crate::core_move::new_intent);
    let journal = Journal::new(
        intent,
        Direction::Forward,
        driver::peer(&source, &inspected),
        change,
        ids,
    );
    Ok(Prepared {
        journal,
        labels,
        remote,
    })
}

/// A forward move of this machine's core to `device`.
async fn forward(
    env: &Env,
    seat: &seat::Seat,
    running: RunningDaemon,
    device: &str,
) -> Result<Role, String> {
    let moves = Arc::clone(&seat.moves);
    let prepared = match prepare(env, &running, device, &moves).await {
        Ok(prepared) => prepared,
        Err(failed) => {
            checks_failed(&moves, device, failed);
            return Ok(Role::Core(running));
        }
    };
    let Prepared {
        journal,
        labels,
        remote,
    } = prepared;
    if let Err(reason) = journal::write(&env.state_dir, &journal) {
        moves.set(MoveView {
            state: MoveState::RolledBack,
            device: Some(device.to_owned()),
            step: Some(MoveStep::StopCore),
            cause: Some(MoveFailure::Local { reason }),
            ..MoveView::default()
        });
        return Ok(Role::Core(running));
    }
    log(
        "move.started",
        json!({"intent": journal.intent, "device": device, "target": journal.peer.node}),
    );
    let view = |state: MoveState| MoveView {
        state,
        device: Some(device.to_owned()),
        intent: Some(journal.intent.clone()),
        ..MoveView::default()
    };
    moves.set(view(MoveState::Stopping));
    stop_core(&env.state_dir, running).await;
    let screen = MoveScreen::mount(&seat.parts(), env.vite_origin.as_deref());
    moves.set(view(MoveState::Copying));
    let remote = Arc::new(remote);
    let steps = {
        let (state_dir, remote, moves) = (
            env.state_dir.clone(),
            Arc::clone(&remote),
            Arc::clone(&moves),
        );
        tokio::task::spawn_blocking(move || {
            copy_and_start(&state_dir, journal, &labels, &remote, &moves)
        })
    };
    let journal = match steps.await {
        Ok(Ok(journal)) => journal,
        Ok(Err((journal, step, cause))) => {
            drop(screen);
            return rollback(env, seat, *journal, &remote, step, cause).await;
        }
        Err(error) => return Err(format!("the move's steps failed: {error}")),
    };
    link(env, seat, journal, remote, screen).await
}

type StepFailure = (Box<Journal>, MoveStep, MoveFailure);

/// Stages, sends, places and starts the copy, recording each phase.
fn copy_and_start(
    state_dir: &Path,
    mut journal: Journal,
    labels: &serde_json::Value,
    remote: &Remote,
    moves: &MoveControl,
) -> Result<Journal, StepFailure> {
    let manifest = match driver::stage(state_dir, &journal, labels) {
        Ok(manifest) => manifest,
        Err(cause) => return Err((Box::new(journal), MoveStep::Copy, cause)),
    };
    let progress = |sent: u64, total: u64| {
        moves.update(|view| {
            view.sent = sent;
            view.total = total;
        });
    };
    if let Err(cause) = driver::send(remote, state_dir, &journal, &manifest, &progress) {
        return Err((Box::new(journal), MoveStep::Copy, cause));
    }
    let record = |journal: &mut Journal, phase: Phase| {
        journal.phase = phase;
        journal::write(state_dir, journal).map_err(|reason| MoveFailure::Local { reason })
    };
    if let Err(cause) = record(&mut journal, Phase::Sent) {
        return Err((Box::new(journal), MoveStep::Copy, cause));
    }
    // Recorded first: a placement whose answer is lost is undone all the
    // same.
    if let Err(cause) =
        record(&mut journal, Phase::Placed).and_then(|()| driver::place(remote, &journal))
    {
        return Err((Box::new(journal), MoveStep::Copy, cause));
    }
    moves.update(|view| view.state = MoveState::Starting);
    journal.target_started_unix_ms = Some(now_unix_ms());
    if let Err(cause) = record(&mut journal, Phase::TargetStarted)
        .and_then(|()| driver::start_target(remote, &journal))
    {
        return Err((Box::new(journal), MoveStep::StartTarget, cause));
    }
    Ok(journal)
}

/// Points this machine at the new core and links to it as a node; the link
/// carrying the intent is the commit.
async fn link(
    env: &Env,
    seat: &seat::Seat,
    mut journal: Journal,
    remote: Arc<Remote>,
    screen: MoveScreen,
) -> Result<Role, String> {
    seat.moves.update(|view| view.state = MoveState::Linking);
    let written = placement::write(
        &env.state_dir,
        &Placement {
            alias: journal.peer.alias.clone(),
            node: journal.peer.node.clone(),
            program: journal.peer.program.clone(),
            state_dir: Some(journal.peer.state_dir.clone()),
            move_intent: Some(journal.intent.clone()),
        },
    );
    if let Err(reason) = written {
        drop(screen);
        return rollback(
            env,
            seat,
            journal,
            &remote,
            MoveStep::Reattach,
            MoveFailure::Local { reason },
        )
        .await;
    }
    journal.phase = Phase::AttachSent;
    if let Err(reason) = journal::write(&env.state_dir, &journal) {
        drop(screen);
        return rollback(
            env,
            seat,
            journal,
            &remote,
            MoveStep::Reattach,
            MoveFailure::Local { reason },
        )
        .await;
    }
    let node = crate::start_node_role(env.clone(), seat.parts()).await;
    drop(screen);
    match node {
        Ok(node) => link_outcome(env, seat, journal, remote, node).await,
        Err(reason) => {
            // No link left this machine: the peer cannot have committed,
            // which its handover confirms before the move is undone.
            log(
                "link.not_started",
                json!({"intent": journal.intent, "reason": reason}),
            );
            resolve_unlinked(env, seat, journal, remote, None).await
        }
    }
}

/// Waits for the node's first link and goes forward or back on what the
/// peer's handover says.
async fn link_outcome(
    env: &Env,
    seat: &seat::Seat,
    journal: Journal,
    remote: Arc<Remote>,
    node: RunningNode,
) -> Result<Role, String> {
    let role = node.node.role_handle();
    let phase = tokio::task::spawn_blocking(move || {
        role.wait_for(LINK_WITHIN, |phase| {
            matches!(
                phase,
                crate::node_role::Phase::Live(_)
                    | crate::node_role::Phase::Waiting {
                        reason: crate::node_role::LinkFailure::Refused(_)
                    }
            )
        })
    })
    .await
    .map_err(|error| error.to_string())?;
    if matches!(phase, crate::node_role::Phase::Live(_)) {
        commit(env, &seat.moves, journal, &remote).await;
        return Ok(Role::Node(node));
    }
    log(
        "link.unanswered",
        json!({"intent": journal.intent, "phase": format!("{phase:?}")}),
    );
    resolve_unlinked(env, seat, journal, remote, Some(node)).await
}

/// After the link carrying the intent may have been sent and none was
/// seen taken: the peer's handover decides, and while the peer cannot be
/// reached this machine stays a node and asks again.
async fn resolve_unlinked(
    env: &Env,
    seat: &seat::Seat,
    journal: Journal,
    remote: Arc<Remote>,
    mut node: Option<RunningNode>,
) -> Result<Role, String> {
    loop {
        let asked = {
            let (remote, journal) = (Arc::clone(&remote), journal.clone());
            tokio::task::spawn_blocking(move || driver::target_status(&remote, &journal))
                .await
                .map_err(|error| error.to_string())?
        };
        match asked {
            Ok(TargetSays::Active) => {
                let node = match node {
                    Some(node) => node,
                    None => crate::start_node_role(env.clone(), seat.parts()).await?,
                };
                commit(env, &seat.moves, journal, &remote).await;
                return Ok(Role::Node(node));
            }
            Ok(TargetSays::NotCommitted) => {
                // The node's link stops before the peer is asked to stop,
                // so no link of it can commit the move meanwhile; the abort
                // itself answers if one did.
                if let Some(node) = node.take() {
                    let _ = tokio::task::spawn_blocking(move || drop(node)).await;
                }
                return rollback(
                    env,
                    seat,
                    journal,
                    &remote,
                    MoveStep::Reattach,
                    MoveFailure::LinkRefused {
                        reason: "the new core did not take this machine's link".to_owned(),
                    },
                )
                .await;
            }
            Err(failure) => {
                seat.moves.update(|view| {
                    view.state = MoveState::Waiting;
                    view.cause = Some(failure.clone());
                });
                tokio::time::sleep(RETRY_EVERY).await;
            }
        }
    }
}

/// The move committed: this machine's brain state is set aside and the
/// peer's records of the move are cleared; a step that fails here is done
/// on the next start, since the journal stays `Committed`.
async fn commit(env: &Env, moves: &MoveControl, mut journal: Journal, remote: &Arc<Remote>) {
    journal.phase = Phase::Committed;
    if let Err(reason) = journal::write(&env.state_dir, &journal) {
        log(
            "commit.unrecorded",
            json!({"intent": journal.intent, "reason": reason}),
        );
    }
    log(
        "move.committed",
        json!({"intent": journal.intent, "target": journal.peer.node}),
    );
    let finished = {
        let (state_dir, remote, journal) =
            (env.state_dir.clone(), Arc::clone(remote), journal.clone());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            copy::set_aside(&state_dir, &journal.intent).map_err(|refusal| refusal.to_string())?;
            let staging = node_migration::staging_dir(&state_dir, &journal.intent);
            if staging.exists() {
                std::fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
            }
            driver::finish_target(&remote, &journal).map_err(|failure| format!("{failure:?}"))?;
            let node = herdr_core::node::NodeId::of_this_machine()?;
            if let Some(mut placement) = placement::read(&state_dir, node.as_str())? {
                placement.move_intent = None;
                placement::write(&state_dir, &placement)?;
            }
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|done| done)
    };
    match finished {
        Ok(()) => {
            journal.phase = Phase::Done;
            if let Err(reason) = journal::write(&env.state_dir, &journal) {
                log(
                    "done.unrecorded",
                    json!({"intent": journal.intent, "reason": reason}),
                );
            }
            log("move.done", json!({"intent": journal.intent}));
        }
        Err(reason) => log(
            "commit.unfinished",
            json!({"intent": journal.intent, "reason": reason}),
        ),
    }
    moves.set(MoveView {
        state: MoveState::Done,
        device: Some(journal.peer.device.clone()),
        intent: Some(journal.intent.clone()),
        node: Some(journal.change.old_owner.clone()),
        ..MoveView::default()
    });
}

/// Undoes a move that did not commit: the peer's pending core is stopped
/// and its copy taken back first, then this machine's core starts again on
/// its untouched folder. A peer that cannot be reached is asked again until
/// its lease has surely ended.
async fn rollback(
    env: &Env,
    seat: &seat::Seat,
    mut journal: Journal,
    remote: &Arc<Remote>,
    step: MoveStep,
    cause: MoveFailure,
) -> Result<Role, String> {
    log(
        "move.rolling_back",
        json!({"intent": journal.intent, "step": step, "cause": cause}),
    );
    seat.moves.set(MoveView {
        state: MoveState::RollingBack,
        device: Some(journal.peer.device.clone()),
        intent: Some(journal.intent.clone()),
        step: Some(step),
        cause: Some(cause.clone()),
        ..MoveView::default()
    });
    let screen = MoveScreen::mount(&seat.parts(), env.vite_origin.as_deref());
    let placed = matches!(
        journal.phase,
        Phase::Placed | Phase::TargetStarted | Phase::AttachSent
    );
    if placed {
        let mut last_failure = None;
        loop {
            let asked = {
                let (remote, journal) = (Arc::clone(remote), journal.clone());
                tokio::task::spawn_blocking(move || driver::abort_target(&remote, &journal))
                    .await
                    .map_err(|error| error.to_string())?
            };
            match asked {
                Ok(TargetSays::NotCommitted) => break,
                Ok(TargetSays::Active) => {
                    // The link committed the move after all: go forward.
                    drop(screen);
                    let node = crate::start_node_role(env.clone(), seat.parts()).await?;
                    commit(env, &seat.moves, journal, remote).await;
                    return Ok(Role::Node(node));
                }
                Err(failure) => {
                    if last_failure.as_ref() != Some(&failure) {
                        log(
                            "abort.failed",
                            json!({"intent": journal.intent, "failure": failure}),
                        );
                        last_failure = Some(failure.clone());
                    }
                    let lease_over = journal.target_started_unix_ms.is_none_or(|started| {
                        now_unix_ms() > started + (PENDING_LEASE + LEASE_MARGIN).as_millis() as u64
                    });
                    // Past the lease no link was sent: the peer's pending
                    // core gave its copy back on its own.
                    if lease_over && journal.phase != Phase::AttachSent {
                        log(
                            "abort.lease_over",
                            json!({"intent": journal.intent, "failure": failure}),
                        );
                        break;
                    }
                    seat.moves.update(|view| view.state = MoveState::Waiting);
                    tokio::time::sleep(RETRY_EVERY).await;
                }
            }
        }
    }
    let mut undone = placement::remove(&env.state_dir);
    let staging = node_migration::staging_dir(&env.state_dir, &journal.intent);
    if staging.exists()
        && let Err(error) = std::fs::remove_dir_all(&staging)
    {
        undone = Err(error.to_string());
    }
    if let Err(reason) = undone {
        log(
            "rollback.unclean",
            json!({"intent": journal.intent, "reason": reason}),
        );
    }
    journal.phase = Phase::RolledBack {
        failed: step,
        cause: cause.clone(),
    };
    journal::write(&env.state_dir, &journal)?;
    drop(screen);
    let running = crate::start_core_role(env.clone(), seat.parts()).await?;
    log(
        "move.rolled_back",
        json!({"intent": journal.intent, "step": step}),
    );
    seat.moves.set(MoveView {
        state: MoveState::RolledBack,
        device: Some(journal.peer.device.clone()),
        intent: Some(journal.intent.clone()),
        step: Some(step),
        cause: Some(cause),
        ..MoveView::default()
    });
    Ok(Role::Core(running))
}
