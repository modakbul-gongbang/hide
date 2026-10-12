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
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use hyper::service::Service as _;
use tokio::sync::{Notify, watch};

/// How long a closing seat lets the requests it is serving finish before
/// it ends them: a role ends its own sockets as it ends, so what is left is
/// a request on its last bytes, or a client that never lets go.
const CLOSE_WITHIN: Duration = Duration::from_secs(5);

/// Where the seat's roles stand, as a request that arrives reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mounts {
    /// No role has mounted yet: a request waits for the first, as a
    /// connection waited in the listener's backlog before seats.
    Waiting,
    /// A role is mounted.
    Mounted,
    /// The seat is closing: a request still waiting for a role, or arriving
    /// now, is answered that the service is unavailable.
    Closing,
}

/// One process's server, with the role router it forwards to.
pub struct Seat {
    pub port: u16,
    pub token: String,
    /// A move of the core, which outlives the role that took its request.
    pub moves: Arc<crate::core_move::control::MoveControl>,
    /// The process's stop request, which outlives its roles as the seat
    /// does; never announced on a seat a caller other than the process's
    /// supervisor holds.
    pub stop: crate::ending::Ending,
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
    /// Told on the first mount and when the seat closes, so a request that
    /// came before the first role waits for it or for the close.
    mounts: watch::Sender<Mounts>,
}

impl Mounted {
    /// Ends every wait for a role: the seat is closing.
    fn close(&self) {
        self.mounts.send_replace(Mounts::Closing);
    }
}

/// What a role needs from the seat it runs on.
#[derive(Clone)]
pub struct SeatParts {
    pub port: u16,
    pub token: String,
    pub moves: Arc<crate::core_move::control::MoveControl>,
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
        // A closing seat stays closing: a role that mounts while its seat
        // closes serves only the requests already past the wait.
        self.mounted.mounts.send_if_modified(|mounts| {
            let opened = *mounts == Mounts::Waiting;
            if opened {
                *mounts = Mounts::Mounted;
            }
            opened
        });
    }

    /// The mounted role's number, which `/health` reports.
    pub fn instance(&self) -> u64 {
        self.mounted.instance.load(Ordering::SeqCst)
    }
}

impl Seat {
    /// Serves `listener` with `token` until the seat is dropped. A request
    /// that arrives before the first role mounts waits for it.
    pub fn serve(
        listener: tokio::net::TcpListener,
        token: String,
        stop: crate::ending::Ending,
    ) -> Result<Self, String> {
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let mounted = Arc::new(Mounted {
            router: RwLock::new(Router::new()),
            instance: AtomicU64::new(0),
            mounts: watch::Sender::new(Mounts::Waiting),
        });
        let stopping = Arc::new(Notify::new());
        let app = Router::new()
            .fallback(forward)
            .with_state(Arc::clone(&mounted));
        let graceful = Arc::clone(&stopping);
        let served = tokio::spawn(async move {
            let served = axum::serve(listener, app)
                .with_graceful_shutdown(async move { graceful.notified().await })
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
            moves: Arc::default(),
            stop,
            mounted,
            stopping,
            served: Some(served),
        })
    }

    /// Stops serving once `role` announces its end, and then forgets the
    /// daemon's state file: a daemon started on its own seat closes its port
    /// when it stops, as it did before seats.
    pub fn end_with(&self, role: crate::ending::Ending, state_dir: std::path::PathBuf) {
        let stopping = Arc::clone(&self.stopping);
        let mounted = Arc::clone(&self.mounted);
        tokio::spawn(async move {
            role.ended().await;
            mounted.close();
            stopping.notify_one();
            crate::state_file::forget_daemon(&state_dir, std::process::id());
        });
    }

    pub fn parts(&self) -> SeatParts {
        SeatParts {
            port: self.port,
            token: self.token.clone(),
            moves: Arc::clone(&self.moves),
            mounted: Arc::clone(&self.mounted),
        }
    }

    /// Stops accepting and waits for the server to finish its graceful stop,
    /// at most [`CLOSE_WITHIN`]; what is still being served then is ended
    /// with the server, so the process's instance lock outlives no client.
    pub async fn close(mut self) {
        self.mounted.close();
        self.stopping.notify_one();
        if let Some(mut served) = self.served.take()
            && tokio::time::timeout(CLOSE_WITHIN, &mut served)
                .await
                .is_err()
        {
            served.abort();
            eprintln!(
                "{}",
                serde_json::json!({"component":"hided","kind":"server.close_unfinished","waited_ms": CLOSE_WITHIN.as_millis()})
            );
        }
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        self.mounted.close();
        self.stopping.notify_one();
    }
}

async fn forward(State(mounted): State<Arc<Mounted>>, request: Request) -> Response {
    let mut mounts = mounted.mounts.subscribe();
    let ready = mounts
        .wait_for(|mounts| *mounts != Mounts::Waiting)
        .await
        .map(|mounts| *mounts);
    if !matches!(ready, Ok(Mounts::Mounted)) {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A request that came before any role mounted (a window reconnecting to
    /// a process whose folder holds its core for a move, which mounts none)
    /// is answered when the seat closes, and the close does not wait on it.
    #[tokio::test]
    async fn a_request_waiting_for_a_first_role_ends_with_the_seat() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let seat = Seat::serve(listener, "token".to_owned(), crate::ending::Ending::new()).unwrap();
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", seat.port))
            .await
            .unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nhost: seat\r\n\r\n")
            .await
            .unwrap();
        let mounted = Arc::clone(&seat.mounted);
        tokio::time::timeout(Duration::from_secs(5), async {
            while mounted.mounts.receiver_count() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the request never waited for a role");
        tokio::time::timeout(Duration::from_secs(5), seat.close())
            .await
            .expect("the close waited on the request");
        let mut answer = vec![0; 64];
        let read = tokio::time::timeout(Duration::from_secs(5), client.read(&mut answer))
            .await
            .expect("no answer")
            .unwrap();
        let answer = String::from_utf8_lossy(&answer[..read]);
        assert!(answer.starts_with("HTTP/1.1 503"), "{answer}");
    }
}
