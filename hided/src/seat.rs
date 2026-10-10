//! The listening place a hided process keeps for its whole life: its port,
//! its token, its instance lock and one HTTP server (PRD core-host-node-move
//! Q12).
//!
//! A process starts in one role, and a core move changes it in place: the
//! core stops, a move screen answers while the brain state crosses, and the
//! node role (or, on a rollback, the core again) takes over. Each role mounts
//! its router here instead of binding its own, so the window's address and
//! token never change, `/health` never stops answering, and the desktop host
//! never reads the daemon as lost. A request is served by the role mounted
//! when it arrives; a connection open across a change (a screen's
//! WebSocket) stays with the role that accepted it until that role ends it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use hyper::service::Service as _;
use tokio::sync::Notify;

/// One process's server, with the role router it forwards to.
pub struct Seat {
    pub port: u16,
    pub token: String,
    mounted: Arc<Mounted>,
    stopping: Arc<Notify>,
    served: Option<tokio::task::JoinHandle<()>>,
}

struct Mounted {
    router: RwLock<Router>,
    /// Which role is mounted: changed on every mount, so a client that
    /// remembers it (the desktop host's browser control) knows to register
    /// again.
    instance: AtomicU64,
}

/// What a role needs from the seat it runs on.
#[derive(Clone)]
pub struct SeatParts {
    pub port: u16,
    pub token: String,
    mounted: Arc<Mounted>,
}

impl SeatParts {
    /// Makes `router` the one every new request reaches.
    pub fn mount(&self, router: Router) {
        *self
            .mounted
            .router
            .write()
            .unwrap_or_else(|error| error.into_inner()) = router;
        self.mounted.instance.fetch_add(1, Ordering::SeqCst);
    }

    /// The mounted role's number, which `/health` reports.
    pub fn instance(&self) -> u64 {
        self.mounted.instance.load(Ordering::SeqCst)
    }
}

impl Seat {
    /// Serves `listener` with `token` until the seat is dropped. Until a
    /// role mounts, every request answers 503.
    pub fn serve(listener: tokio::net::TcpListener, token: String) -> Result<Self, String> {
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let mounted = Arc::new(Mounted {
            router: RwLock::new(Router::new().fallback(not_yet)),
            instance: AtomicU64::new(0),
        });
        let stopping = Arc::new(Notify::new());
        let app = Router::new()
            .fallback(forward)
            .with_state(Arc::clone(&mounted));
        let stop = Arc::clone(&stopping);
        let served = tokio::spawn(async move {
            let served = axum::serve(listener, app)
                .with_graceful_shutdown(async move { stop.notified().await })
                .await;
            if let Err(error) = served {
                eprintln!(
                    "{}",
                    serde_json::json!({"component":"hided","kind":"server.exit","message": error.to_string()})
                );
            }
        });
        Ok(Self {
            port,
            token,
            mounted,
            stopping,
            served: Some(served),
        })
    }

    /// Stops serving once `role` announces its end, and then forgets the
    /// daemon's state file: a daemon started on its own seat closes its port
    /// when it stops, as it did before seats.
    pub fn end_with(&self, role: Arc<Notify>, state_dir: std::path::PathBuf) {
        let stopping = Arc::clone(&self.stopping);
        tokio::spawn(async move {
            role.notified().await;
            stopping.notify_one();
            crate::state_file::forget_daemon(&state_dir, std::process::id());
        });
    }

    pub fn parts(&self) -> SeatParts {
        SeatParts {
            port: self.port,
            token: self.token.clone(),
            mounted: Arc::clone(&self.mounted),
        }
    }

    /// Stops accepting and waits for the server to finish its graceful stop.
    pub async fn close(mut self) {
        self.stopping.notify_one();
        if let Some(served) = self.served.take() {
            let _ = served.await;
        }
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        self.stopping.notify_one();
    }
}

async fn forward(State(mounted): State<Arc<Mounted>>, request: Request) -> Response {
    let router = mounted
        .router
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    match hyper_util::service::TowerToHyperService::new(router)
        .call(request)
        .await
    {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

async fn not_yet() -> Response {
    axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response()
}
