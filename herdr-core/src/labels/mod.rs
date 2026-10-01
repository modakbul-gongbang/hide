//! Agent labels, made by the core (PRD labels-in-hided).
//!
//! What each agent pane is doing (its task title, progress, the reply it
//! asks for, whether it asked a question) used to come from a separate Herdr
//! plugin through pane tokens. The core now makes them itself: each Herdr
//! server's session-sync coordinator owns a [`worker::LabelWorker`], the
//! core owns one [`analyzer::LabelAnalyzer`] for all of them, and
//! [`store::LabelStore`] keeps `labels.json` beside the daemon's state.
//! `docs/status-model.md` owns how labels reach the screen.

pub(crate) mod analysis;
pub(crate) mod analyzer;
mod context_label;
pub(crate) mod generator;
mod import;
pub(crate) mod store;
pub(crate) mod worker;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use hide_host::protocol::Call;
use hide_session::label_transcript::{LabelTranscript, LabelTranscriptRequest};

use crate::host_access::{HostCallError, call_as};
use crate::runtime::Runtime;
use analyzer::LabelAnalyzer;
use store::LabelStore;
use worker::{LabelWorker, ReadFailure, TranscriptSource, Wake, WorkerConfig};

/// A device read waits at most this long for its helper.
const DEVICE_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// What every coordinator's worker shares: the store and the analyzer.
pub(crate) struct LabelServices {
    pub(crate) store: Arc<LabelStore>,
    pub(crate) analyzer: Arc<LabelAnalyzer>,
    home: Option<PathBuf>,
}

impl LabelServices {
    /// Opens the store beside `state_dir` and starts the analyzer, which
    /// reads the operator's provider choice from the runtime before each
    /// analysis.
    pub(crate) fn start(
        state_dir: Option<&Path>,
        home: Option<PathBuf>,
        runtime: Weak<Mutex<Runtime>>,
    ) -> Result<Self, String> {
        let store = Arc::new(LabelStore::open(state_dir, home.as_deref()));
        let analyzer = LabelAnalyzer::spawn(Box::new(move || {
            runtime
                .upgrade()
                .and_then(|runtime| runtime.lock().ok().map(|guard| guard.label_ai_settings()))
                .unwrap_or_default()
        }))?;
        Ok(Self {
            store,
            analyzer: Arc::new(analyzer),
            home,
        })
    }

    /// The worker for this machine's Herdr server at `socket_path`.
    pub(crate) fn local_worker(
        &self,
        socket_path: &Path,
        wake: Wake,
    ) -> Result<Option<LabelWorker>, String> {
        let Some(home) = self.home.clone() else {
            return Ok(None);
        };
        let server_key = std::fs::canonicalize(socket_path)
            .unwrap_or_else(|_| socket_path.to_path_buf())
            .display()
            .to_string();
        LabelWorker::spawn(
            WorkerConfig {
                target: store::LOCAL_TARGET.to_owned(),
                server_key: format!("local:{server_key}"),
                lock_dir: Some(generator::lock_dir(&home)),
            },
            Arc::clone(&self.store),
            Arc::clone(&self.analyzer),
            Arc::new(worker::LocalTranscripts { home }),
            wake,
        )
        .map(Some)
    }

    /// The worker for a device's Herdr server, reading through its helper.
    pub(crate) fn device_worker(
        &self,
        device_id: &str,
        runtime: Weak<Mutex<Runtime>>,
        wake: Wake,
    ) -> Result<LabelWorker, String> {
        LabelWorker::spawn(
            WorkerConfig {
                target: format!("device:{device_id}"),
                server_key: format!("device:{device_id}"),
                lock_dir: self.home.as_deref().map(generator::lock_dir),
            },
            Arc::clone(&self.store),
            Arc::clone(&self.analyzer),
            Arc::new(DeviceTranscripts {
                device_id: device_id.to_owned(),
                runtime,
            }),
            wake,
        )
    }
}

/// A device's conversations, read by its helper and brought here in memory
/// only (PRD D-03); nothing of them is stored.
struct DeviceTranscripts {
    device_id: String,
    runtime: Weak<Mutex<Runtime>>,
}

impl TranscriptSource for DeviceTranscripts {
    fn read(&self, request: &LabelTranscriptRequest) -> Result<LabelTranscript, ReadFailure> {
        let channel = {
            let runtime = self
                .runtime
                .upgrade()
                .ok_or_else(|| ReadFailure::Unavailable("runtime_gone".to_owned()))?;
            let mut guard = runtime
                .lock()
                .map_err(|_| ReadFailure::Unavailable("runtime_poisoned".to_owned()))?;
            guard
                .device_channel(&self.device_id)
                .map_err(|_| ReadFailure::Unavailable("device_helper_not_ready".to_owned()))?
        };
        call_as::<LabelTranscript>(
            channel.as_ref(),
            Call::LabelTranscript {
                request: request.clone(),
            },
            DEVICE_READ_TIMEOUT,
        )
        .map_err(|error| match error {
            HostCallError::NotConnected(_) => {
                ReadFailure::Unavailable("device_helper_not_connected".to_owned())
            }
            HostCallError::Busy => ReadFailure::Unavailable("device_helper_busy".to_owned()),
            HostCallError::Unknown(_) => {
                ReadFailure::Unavailable("device_helper_unknown".to_owned())
            }
            // A helper older than protocol 12 does not know the call; the
            // device's kit status already offers the reinstall (B15).
            HostCallError::Refused(error) if error.code == hide_host::ErrorCode::InvalidRequest => {
                ReadFailure::Refused("device_helper_unsupported".to_owned())
            }
            HostCallError::Refused(error) => ReadFailure::Refused(error.message),
        })
    }
}
