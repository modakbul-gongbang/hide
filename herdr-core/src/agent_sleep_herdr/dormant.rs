//! Worker boundaries for a sleeping session whose old pane no longer exists.
use super::*;
use crate::agent_sleep::{DormantRecord, SleepId};
use crate::checkout_owner::OwnerOpen;
use crate::sidebar::SessionSnapshotPayload;
use hide_herdr_client::{ApiStream, ConnectionShutdown};
use std::io::{self, Read, Write};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DormantWork {
    pub id: SleepId,
    pub record: DormantRecord,
}

pub(crate) enum DormantStartOutcome {
    Started,
    NotStarted,
    Unknown,
}

pub(crate) enum DormantTabOutcome {
    Created(String, String, Box<SessionSnapshotPayload>),
    NotCreated,
    Unknown,
}

pub(crate) struct DormantStatus {
    pub snapshot: SessionSnapshotPayload,
    pub wake_tab: Result<Option<(String, String)>, String>,
}

/// Observes current topology and exact intent markers. Never creates,
/// repairs, moves, closes or starts anything, including after a restart.
pub(crate) fn spawn_dormant_status(
    context: LiveContext,
    work: DormantWork,
    generation: u64,
    owner: Option<OwnerOpen>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("hide-sleep-status".into())
        .spawn(move || {
            let result = crate::live::fetch_session_with_connector(context.api_connector.as_ref())
                .map_err(|_| {
                    crate::diagnostic!(serde_json::json!({
                        "component": "agent_sleep", "kind": "agent_sleep.status_failed",
                        "sleep_id": work.id.as_str(),
                    }));
                    "The current session could not be read".to_owned()
                })
                .map(|snapshot| {
                    let wake_tab = if work.record.closed {
                        owner
                            .as_ref()
                            .ok_or_else(|| "The saved checkout is unavailable".to_owned())
                            .and_then(|owner| {
                                crate::live::inspect_sleep_tab(
                                    context.api_connector.as_ref(),
                                    work.id.as_str(),
                                    &work.record.context,
                                    owner,
                                    &snapshot,
                                )
                            })
                    } else {
                        Ok(None)
                    };
                    DormantStatus { snapshot, wake_tab }
                });
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .ingest_dormant_status(&work, generation, result);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|_| "The sleeping-session status worker could not start".into())
}

pub(crate) fn spawn_dormant_tab(
    context: LiveContext,
    work: DormantWork,
    owner: OwnerOpen,
) -> Result<(), String> {
    thread::Builder::new()
        .name("hide-sleep-tab".into())
        .spawn(move || {
            if !work_is_current(&context, &work) {
                return;
            }
            let result =
                if !crate::node_access::is_directory(context.node.as_ref(), &work.record.cwd) {
                    DormantTabOutcome::NotCreated
                } else {
                    match crate::live::create_sleep_tab(
                        &CurrentWorkConnector {
                            context: context.clone(),
                            intent: CurrentIntent::Wake(Arc::new(work.clone())),
                        },
                        context.node.as_ref(),
                        work.id.as_str(),
                        &work.record.context,
                        &owner,
                        &work.record.cwd,
                    ) {
                        Ok((tab, pane, snapshot)) => {
                            DormantTabOutcome::Created(tab, pane, Box::new(snapshot))
                        }
                        Err(_) => {
                            crate::diagnostic!(serde_json::json!({
                                "component": "agent_sleep", "kind": "agent_sleep.tab_unconfirmed",
                                "sleep_id": work.id.as_str(),
                            }));
                            DormantTabOutcome::Unknown
                        }
                    }
                };
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .ingest_dormant_tab(&work, result);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|_| "The sleeping-session tab worker could not start".into())
}

pub(crate) fn spawn_dormant_start(
    context: LiveContext,
    work: DormantWork,
    args: Vec<String>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("hide-sleep-resume".into())
        .spawn(move || {
            if !work_is_current(&context, &work) {
                return;
            }
            let outcome = start_dormant(
                &CurrentWorkConnector {
                    context: context.clone(),
                    intent: CurrentIntent::Wake(Arc::new(work.clone())),
                },
                context.node.as_ref(),
                &work,
                args,
            );
            let Some(runtime) = context.runtime.upgrade() else {
                return;
            };
            let changed = runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .ingest_dormant_start(&work, outcome);
            if changed {
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|_| "The sleeping-session resume worker could not start".into())
}

fn work_is_current(context: &LiveContext, work: &DormantWork) -> bool {
    context.runtime.upgrade().is_some_and(|runtime| {
        runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .dormant_work_is_current(work)
    })
}

/// Shell readiness and topology reads may block. Check the exact saved
/// intent again at every socket write, rather than only at worker entry.
/// The guard is released before I/O and never holds the runtime mutex over it.
struct CurrentWorkConnector {
    context: LiveContext,
    intent: CurrentIntent,
}

#[derive(Clone)]
enum CurrentIntent {
    Wake(Arc<DormantWork>),
    Close(Arc<crate::live::CloseEffectRequest>),
}

pub(crate) fn fenced_dormant_close_connector(
    context: &LiveContext,
    effect: &crate::live::CloseEffectRequest,
) -> Box<dyn ApiConnector> {
    Box::new(CurrentWorkConnector {
        context: context.clone(),
        intent: CurrentIntent::Close(Arc::new(effect.clone())),
    })
}

fn intent_is_current(context: &LiveContext, intent: &CurrentIntent) -> bool {
    context.runtime.upgrade().is_some_and(|runtime| {
        let runtime = runtime.lock().unwrap_or_else(|error| error.into_inner());
        match intent {
            CurrentIntent::Wake(work) => runtime.dormant_work_is_current(work),
            CurrentIntent::Close(effect) => runtime.dormant_close_effect_is_current(effect),
        }
    })
}

impl ApiConnector for CurrentWorkConnector {
    fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
        let stream = self.context.api_connector.connect()?;
        Ok(Box::new(CurrentWorkStream {
            stream,
            context: self.context.clone(),
            intent: self.intent.clone(),
        }))
    }
}

struct CurrentWorkStream {
    stream: Box<dyn ApiStream>,
    context: LiveContext,
    intent: CurrentIntent,
}

impl Read for CurrentWorkStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.read(bytes)
    }
}

impl Write for CurrentWorkStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !intent_is_current(&self.context, &self.intent) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "sleep intent superseded",
            ));
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl ApiStream for CurrentWorkStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        self.stream.set_read_timeout(timeout)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> Result<(), ApiError> {
        self.stream.set_write_timeout(timeout)
    }
    fn read_line_with_timeout(&mut self, timeout: Duration) -> Result<String, ApiError> {
        self.stream.read_line_with_timeout(timeout)
    }
    fn shutdown_handle(&self) -> Result<Box<dyn ConnectionShutdown>, ApiError> {
        self.stream.shutdown_handle()
    }
}

fn start_dormant(
    connector: &dyn ApiConnector,
    node: &dyn NodeLink,
    work: &DormantWork,
    args: Vec<String>,
) -> DormantStartOutcome {
    let Some(pane) = work.record.wake_pane_id.as_deref() else {
        return DormantStartOutcome::NotStarted;
    };
    if !crate::node_access::is_directory(node, &work.record.cwd) {
        return DormantStartOutcome::NotStarted;
    }
    // No prior runtime registration or lineage token is copied into a wake.
    let name = crate::fork::fork_name("sleep", work.id.as_str());
    let params = match wire::agent_start_params(
        pane,
        &name,
        &work.record.kind,
        args,
        crate::codex_launch::CodexDaemon::Unsupported,
    ) {
        Ok(params) => params,
        Err(_) => return DormantStartOutcome::NotStarted,
    };
    match crate::agent_start::start_at_shell(
        connector,
        &format!("hide:{}:resume", work.id.as_str()),
        pane,
        params,
        Duration::from_millis(AGENT_START_TIMEOUT_MS + 5_000),
    ) {
        Ok(_) => DormantStartOutcome::Started,
        Err(StartError::NotStarted(_)) | Err(StartError::Herdr(ApiError::Remote { .. })) => {
            DormantStartOutcome::NotStarted
        }
        Err(StartError::Herdr(_)) => {
            crate::diagnostic!(serde_json::json!({
                "component": "agent_sleep", "kind": "agent_sleep.resume_unconfirmed",
                "sleep_id": work.id.as_str(),
            }));
            DormantStartOutcome::Unknown
        }
    }
}
