//! Native Browser View routes into a consented SSH device. A route belongs
//! to one core-owned View and load stamp; it never falls back to this Mac.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use herdr_core::host_access::{self, HostChannel};
use herdr_core::remote::RemoteLocalForward;
use herdr_core::workspace_control::BrowserRouteSource;
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify, Semaphore, oneshot};

use crate::core::CoreHandle;
use crate::pane_auth::process_start;
use crate::state_file::new_token;

const MAX_ROUTES: usize = 12;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE_REQUESTS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct Key {
    owner_pid: i32,
    device: String,
    checkout: String,
    view: String,
}

enum RouteKind {
    Http(RemoteLocalForward),
    File(oneshot::Sender<()>),
}

struct Route {
    owner_started: u64,
    source: BrowserRouteSource,
    url: String,
    kind: RouteKind,
}

impl Route {
    fn close(self) {
        match self.kind {
            RouteKind::Http(forward) => forward.close(),
            RouteKind::File(stop) => {
                let _ = stop.send(());
            }
        }
    }
}

#[derive(Serialize)]
pub struct Resolved {
    pub url: String,
    pub source_url: String,
    pub load: u64,
}

pub struct BrowserRoutes {
    core: Arc<CoreHandle>,
    routes: Mutex<HashMap<Key, Route>>,
}

impl BrowserRoutes {
    pub fn new(core: Arc<CoreHandle>) -> Arc<Self> {
        Arc::new(Self {
            core,
            routes: Mutex::new(HashMap::new()),
        })
    }

    pub async fn resolve(
        &self,
        device: String,
        checkout: String,
        view: String,
        load: u64,
        owner_pid: i32,
    ) -> Result<Resolved, &'static str> {
        let owner_started = process_start(owner_pid).ok_or("browser_owner_unavailable")?;
        let key = Key {
            owner_pid,
            device,
            checkout,
            view,
        };
        let core = Arc::clone(&self.core);
        let query = key.clone();
        let source = tokio::task::spawn_blocking(move || {
            core.browser_route_source(&query.device, &query.checkout, &query.view, load)
        })
        .await
        .map_err(|_| "core_unavailable")?
        .map_err(|_| "core_unavailable")?
        .ok_or("view_unavailable")?;
        if key.device == "local" {
            return Ok(Resolved {
                url: source.url.clone(),
                source_url: source.url,
                load,
            });
        }
        let mut routes = self.routes.lock().await;
        if let Some(route) = routes.get(&key)
            && route.source == source
            && route.owner_started == owner_started
        {
            return Ok(Resolved {
                url: route.url.clone(),
                source_url: source.url,
                load,
            });
        }
        if let Some(old) = routes.remove(&key) {
            old.close();
        }
        if routes.len() >= MAX_ROUTES {
            return Err("browser_route_limit");
        }
        let core = Arc::clone(&self.core);
        let device = key.device.clone();
        let route = tokio::task::spawn_blocking(move || {
            core.workspace_remote_routes()
                .ok()?
                .into_iter()
                .find(|route| route.device_id == device)
        })
        .await
        .map_err(|_| "core_unavailable")?
        .ok_or("host_unavailable")?;
        let (url, kind) = if crate::file_url::is_file_url(&source.url) {
            let (path, suffix) =
                crate::file_url::file_path(&source.url).ok_or("invalid_file_url")?;
            let relative = Path::new(&path)
                .strip_prefix(&source.checkout_path)
                .map_err(|_| "outside_checkout")?
                .to_str()
                .ok_or("invalid_file_url")?
                .to_owned();
            hide_host::relative_path(&relative).map_err(|_| "outside_checkout")?;
            let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .map_err(|_| "route_bind_failed")?;
            let port = listener
                .local_addr()
                .map_err(|_| "route_bind_failed")?
                .port();
            let secret = new_token();
            let state = FileRoute {
                core: Arc::clone(&self.core),
                key: key.clone(),
                owner_started,
                source: source.clone(),
                channel: route.channel,
                secret: secret.clone(),
                requests: Arc::new(Semaphore::new(MAX_FILE_REQUESTS)),
            };
            let app = Router::new().fallback(get(serve_file)).with_state(state);
            let (stop, done) = oneshot::channel();
            tokio::spawn(async move {
                let result = axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = done.await;
                    })
                    .await;
                if let Err(error) = result {
                    eprintln!(
                        "{}",
                        serde_json::json!({"component":"browser_routes","kind":"file_server.failed","reason":error.to_string()})
                    );
                }
            });
            let escaped = relative
                .split('/')
                .map(|part| utf8_percent_encode(part, NON_ALPHANUMERIC).to_string())
                .collect::<Vec<_>>()
                .join("/");
            (
                format!("http://127.0.0.1:{port}/{secret}/{escaped}{suffix}"),
                RouteKind::File(stop),
            )
        } else if let Some((remote_port, host, tail)) = loopback_target(&source.url) {
            let client = Arc::clone(&route.client);
            let remote_ip = if host == "[::1]" || host == "::1" {
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            } else {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            };
            let remote = SocketAddr::new(remote_ip, remote_port);
            let alternate = (host == "localhost").then_some(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::LOCALHOST),
                remote_port,
            ));
            let forward = tokio::task::spawn_blocking(move || {
                client.start_local_workspace_forward(remote, alternate)
            })
            .await
            .map_err(|_| "route_failed")?
            .map_err(|_| "route_failed")?;
            let url = format!(
                "{}://{}:{}{}",
                if source.url.starts_with("https:") {
                    "https"
                } else {
                    "http"
                },
                host,
                forward.port(),
                tail
            );
            (url, RouteKind::Http(forward))
        } else {
            return Ok(Resolved {
                url: source.url.clone(),
                source_url: source.url,
                load,
            });
        };
        routes.insert(
            key,
            Route {
                owner_started,
                source: source.clone(),
                url: url.clone(),
                kind,
            },
        );
        eprintln!(
            "{}",
            serde_json::json!({"component":"browser_routes","kind":"route.ready","device_id":source.device_id,"load":load})
        );
        Ok(Resolved {
            url,
            source_url: source.url,
            load,
        })
    }

    pub async fn release(
        &self,
        device: &str,
        checkout: &str,
        view: &str,
        load: u64,
        owner_pid: i32,
    ) {
        let key = Key {
            owner_pid,
            device: device.to_owned(),
            checkout: checkout.to_owned(),
            view: view.to_owned(),
        };
        let mut routes = self.routes.lock().await;
        if routes
            .get(&key)
            .is_some_and(|route| route.source.load == load)
            && let Some(route) = routes.remove(&key)
        {
            route.close();
        }
    }

    pub fn spawn_reaper(
        self: &Arc<Self>,
        desktop_renderers: Arc<AtomicUsize>,
        shutdown: Arc<Notify>,
    ) {
        let routes = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            loop {
                tokio::select! { _ = shutdown.notified() => break, _ = tick.tick() => {} }
                let candidates: Vec<_> = routes
                    .routes
                    .lock()
                    .await
                    .iter()
                    .map(|(key, route)| (key.clone(), route.source.clone(), route.owner_started))
                    .collect();
                for (key, source, owner_started) in candidates {
                    let load = source.load;
                    let stale = if desktop_renderers.load(Ordering::SeqCst) == 0
                        || process_start(key.owner_pid) != Some(owner_started)
                    {
                        true
                    } else {
                        let core = Arc::clone(&routes.core);
                        let key_for_query = key.clone();
                        tokio::task::spawn_blocking(move || {
                            core.browser_route_source(
                                &key_for_query.device,
                                &key_for_query.checkout,
                                &key_for_query.view,
                                source.load,
                            )
                            .ok()
                            .flatten()
                            .is_none()
                        })
                        .await
                        .unwrap_or(true)
                    };
                    if stale {
                        routes
                            .release(&key.device, &key.checkout, &key.view, load, key.owner_pid)
                            .await;
                    }
                }
            }
            for (_, route) in routes.routes.lock().await.drain() {
                route.close();
            }
        });
    }
}

fn loopback_target(raw: &str) -> Option<(u16, String, String)> {
    let (without_fragment, fragment) = raw
        .split_once('#')
        .map_or((raw, ""), |(head, tail)| (head, tail));
    let uri: Uri = without_fragment.parse().ok()?;
    let scheme = uri.scheme_str()?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = uri.authority()?;
    let host = authority.host().to_ascii_lowercase();
    if !matches!(
        host.as_str(),
        "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]" | "::1"
    ) {
        return None;
    }
    let port = authority
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let local_host = match host.as_str() {
        "0.0.0.0" => "127.0.0.1",
        "::1" => "[::1]",
        _ => &host,
    };
    let tail = uri.path_and_query().map_or("/", |value| value.as_str());
    let tail = if fragment.is_empty() {
        tail.to_owned()
    } else {
        format!("{tail}#{fragment}")
    };
    Some((port, local_host.to_owned(), tail))
}

#[derive(Clone)]
struct FileRoute {
    core: Arc<CoreHandle>,
    key: Key,
    owner_started: u64,
    source: BrowserRouteSource,
    channel: Arc<dyn HostChannel>,
    secret: String,
    requests: Arc<Semaphore>,
}

async fn serve_file(State(route): State<FileRoute>, uri: Uri) -> Response {
    let Some(path) = uri.path().strip_prefix(&format!("/{}/", route.secret)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if path.len() > 8192 {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    let Ok(decoded) = percent_decode_str(path).decode_utf8() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if hide_host::relative_path(&decoded).is_err() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = route.requests.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let check = Arc::clone(&route.core);
    let source = route.source.clone();
    let key = route.key.clone();
    let owner_started = route.owner_started;
    let channel = Arc::clone(&route.channel);
    let decoded = decoded.into_owned();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if process_start(key.owner_pid) != Some(owner_started) {
            return Err("browser_owner_unavailable");
        }
        if check
            .browser_route_source(&key.device, &key.checkout, &key.view, source.load)
            .ok()
            .flatten()
            .is_none()
        {
            return Err("view_unavailable");
        }
        let mut bytes = Vec::new();
        let mut stamp = None;
        loop {
            let range = host_access::read_bytes(
                channel.as_ref(),
                &key.checkout,
                &decoded,
                bytes.len() as u64,
                hide_host::bytes::MAX_RANGE,
            )
            .map_err(|_| "file_unavailable")?;
            if range.total > MAX_FILE_BYTES {
                return Err("file_too_large");
            }
            if stamp.as_ref().is_some_and(|old| old != &range.file) {
                return Err("file_changed");
            }
            stamp = Some(range.file.clone());
            let chunk = range.bytes().map_err(|_| "file_unavailable")?;
            if chunk.is_empty() && (bytes.len() as u64) < range.total {
                return Err("file_unavailable");
            }
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 >= range.total {
                break;
            }
        }
        Ok::<_, &'static str>(bytes)
    })
    .await;
    match result {
        Ok(Ok(bytes)) => Response::builder()
            .status(StatusCode::OK)
            .header(
                "content-type",
                crate::server::mime_for(Path::new(&decoded_path_for_mime(uri.path()))),
            )
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Ok(Err(reason)) => {
            eprintln!(
                "{}",
                serde_json::json!({"component":"browser_routes","kind":"file.refused","reason":reason})
            );
            match reason {
                "view_unavailable" => StatusCode::GONE,
                "file_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
                "file_changed" => StatusCode::CONFLICT,
                _ => StatusCode::FORBIDDEN,
            }
            .into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn decoded_path_for_mime(path: &str) -> String {
    percent_decode_str(path).decode_utf8_lossy().into_owned()
}
