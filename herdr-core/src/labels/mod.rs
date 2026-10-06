//! Agent labels, made by the core (PRD labels-in-hided).
//!
//! What each agent pane is doing (the session's goal, one line for its turn,
//! how the turn ended) used to come from a separate Herdr plugin through
//! pane tokens. The core now makes them itself: each Herdr
//! server's session-sync coordinator owns a [`worker::LabelWorker`], the
//! core owns one [`analyzer::LabelAnalyzer`] for all of them, and
//! [`store::LabelStore`] keeps `labels.json` beside the daemon's state.
//! `docs/status-model.md` owns how labels reach the screen.

pub(crate) mod analysis;
pub(crate) mod analyzer;
mod context_label;
pub(crate) mod facts;
pub(crate) mod generator;
mod import;
pub(crate) mod input;
pub(crate) mod overlay;
pub(crate) mod store;
pub(crate) mod worker;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use hide_host::protocol::Call;
use hide_session::label_transcript::{LabelTranscript, LabelTranscriptRequest};

use crate::host_access::{HostCallError, HostChannel, call_as};
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
    /// How the agent Hide AI runs on stood at the analyzer's last job.
    pub(crate) standing: Arc<crate::ai::AiStanding>,
    /// The operator's submits the runtime records for every worker.
    pub(crate) input: Arc<input::OperatorInput>,
    /// Wakes this Mac's worker, for news that reaches the runtime rather
    /// than its coordinator (new pull request creation times).
    local_wake: Mutex<Option<Wake>>,
    home: Option<PathBuf>,
    /// The daemon's state folder, which holds the device generator locks.
    state_dir: Option<PathBuf>,
}

impl LabelServices {
    /// Opens the store beside `state_dir` and starts the analyzer, which
    /// reads the operator's provider choice from the runtime before each
    /// analysis, or from the saved file while the runtime has not read it
    /// yet, so no conversation goes to a provider the operator did not
    /// choose.
    pub(crate) fn start(
        state_dir: Option<&Path>,
        home: Option<PathBuf>,
        runtime: Weak<Mutex<Runtime>>,
    ) -> Result<Self, String> {
        let store = Arc::new(LabelStore::open(state_dir, home.as_deref()));
        let settings_home = home.clone();
        let standing = Arc::new(crate::ai::AiStanding::default());
        let analyzer = LabelAnalyzer::spawn(
            Box::new(move || analysis_settings(&runtime, settings_home.as_deref())),
            Arc::clone(&standing),
        )?;
        Ok(Self {
            store,
            analyzer: Arc::new(analyzer),
            standing,
            input: Arc::default(),
            local_wake: Mutex::new(None),
            home,
            state_dir: state_dir.map(Path::to_path_buf),
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
        *self
            .local_wake
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::clone(&wake));
        LabelWorker::spawn(
            WorkerConfig {
                target: store::LOCAL_TARGET.to_owned(),
                lock_path: Some(generator::local_lock_path(socket_path)),
                input: Arc::clone(&self.input),
            },
            Arc::clone(&self.store),
            Arc::clone(&self.analyzer),
            Arc::new(worker::LocalTranscripts { home }),
            wake,
        )
        .map(Some)
    }

    /// Wakes this Mac's worker, which takes the runtime's news on its wake.
    pub(crate) fn wake_local(&self) {
        let wake = self
            .local_wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(wake) = wake {
            wake();
        }
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
                lock_path: self
                    .state_dir
                    .as_deref()
                    .map(|state_dir| generator::device_lock_path(state_dir, device_id)),
                input: Arc::clone(&self.input),
            },
            Arc::clone(&self.store),
            Arc::clone(&self.analyzer),
            Arc::new(DeviceTranscripts::of_device(device_id, runtime)),
            wake,
        )
    }
}

/// The provider choice an analysis runs with: the runtime's once it has read
/// the settings, the saved file before then, and the defaults only when no
/// choice was saved or the file cannot be read, which is logged.
pub(crate) fn analysis_settings(
    runtime: &Weak<Mutex<Runtime>>,
    home: Option<&Path>,
) -> hide_ai::AiSettings {
    let loaded = runtime.upgrade().and_then(|runtime| {
        runtime
            .lock()
            .ok()
            .and_then(|guard| guard.label_ai_settings())
    });
    if let Some(settings) = loaded {
        return settings;
    }
    let Some(home) = home else {
        return hide_ai::AiSettings::default();
    };
    hide_ai::settings::load(home).unwrap_or_else(|error| {
        crate::diagnostic!(serde_json::json!({
            "component": "labels",
            "kind": "settings.unreadable",
            "message": error.to_string(),
        }));
        hide_ai::AiSettings::default()
    })
}

/// The device helper's channel when it has one, or the stable reason it does
/// not (no connection yet, a helper still starting).
pub(crate) type ChannelSource =
    Box<dyn Fn() -> Result<Arc<dyn HostChannel>, &'static str> + Send + Sync>;

/// A device's conversations, read by its helper and brought here in memory
/// only (PRD D-03); nothing of them is stored.
pub(crate) struct DeviceTranscripts {
    channel: ChannelSource,
}

impl DeviceTranscripts {
    pub(crate) fn new(channel: ChannelSource) -> Self {
        Self { channel }
    }

    /// The registered device's helper, taken under a brief runtime lock;
    /// the read itself runs outside it.
    fn of_device(device_id: &str, runtime: Weak<Mutex<Runtime>>) -> Self {
        let device_id = device_id.to_owned();
        Self::new(Box::new(move || {
            let runtime = runtime.upgrade().ok_or("runtime_gone")?;
            let mut guard = runtime.lock().map_err(|_| "runtime_poisoned")?;
            guard
                .device_channel(&device_id)
                .map_err(|_| "device_helper_not_ready")
        }))
    }
}

impl TranscriptSource for DeviceTranscripts {
    fn read(&self, request: &LabelTranscriptRequest) -> Result<LabelTranscript, ReadFailure> {
        let channel =
            (self.channel)().map_err(|reason| ReadFailure::Unavailable(reason.to_owned()))?;
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
