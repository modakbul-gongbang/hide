use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const GATEWAY_POLL: Duration = Duration::from_millis(10);
const REQUEST_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserError {
    InvalidUrl(String),
    InvalidProfile(String),
    InvalidView(String),
    CapabilityDenied,
    CapabilityUnavailable(String),
    UpstreamUnavailable(String),
    Gateway(String),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(url) => write!(formatter, "Browser URL must use IPv4 loopback: {url}"),
            Self::InvalidProfile(path) => {
                write!(
                    formatter,
                    "Browser profile must be an absolute owned directory: {path}"
                )
            }
            Self::InvalidView(view) => write!(formatter, "invalid Browser view id: {view}"),
            Self::CapabilityDenied => write!(formatter, "CDP capability denied"),
            Self::CapabilityUnavailable(reason) => {
                write!(formatter, "CDP capability unavailable: {reason}")
            }
            Self::UpstreamUnavailable(reason) => {
                write!(formatter, "CEF CDP upstream unavailable: {reason}")
            }
            Self::Gateway(reason) => write!(formatter, "CDP gateway failed: {reason}"),
        }
    }
}

impl std::error::Error for BrowserError {}

pub type BrowserResult<T> = Result<T, BrowserError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSurfaceState {
    Opening,
    Ready,
    Detached,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserSurfaceDescriptor {
    pub view_id: String,
    pub source_pane_id: String,
    pub url: String,
    pub profile_dir: PathBuf,
    pub gateway_addr: SocketAddr,
    pub state: BrowserSurfaceState,
}

struct BrowserSurface {
    descriptor: BrowserSurfaceDescriptor,
    capability: String,
    gateway: CdpGateway,
}

/// Owns source-linked Browser surfaces. A source pane opens at most one stable view,
/// repeated opens reuse that view and profile, and a CDP detach never closes the surface.
#[derive(Default)]
pub struct BrowserSurfaceRegistry {
    surfaces: BTreeMap<String, BrowserSurface>,
}

impl BrowserSurfaceRegistry {
    pub fn open_or_reuse(
        &mut self,
        source_pane_id: &str,
        url: &str,
        profile_dir: impl Into<PathBuf>,
        cef_cdp_addr: SocketAddr,
    ) -> BrowserResult<BrowserSurfaceDescriptor> {
        validate_source_pane(source_pane_id)?;
        validate_loopback_url(url)?;
        let profile_dir = validate_profile(profile_dir.into())?;
        let view_id = format!("browser:{source_pane_id}");
        if let Some(surface) = self.surfaces.get_mut(&view_id) {
            if surface.descriptor.url == url && surface.descriptor.profile_dir == profile_dir {
                surface.descriptor.state = BrowserSurfaceState::Ready;
                return Ok(surface.descriptor.clone());
            }
            return Err(BrowserError::Gateway(format!(
                "source pane already owns view {} with a different URL or profile",
                surface.descriptor.view_id
            )));
        }
        let capability = capability_token()?;
        let gateway = CdpGateway::start(cef_cdp_addr, view_id.clone(), capability.clone())?;
        let descriptor = BrowserSurfaceDescriptor {
            view_id: view_id.clone(),
            source_pane_id: source_pane_id.to_owned(),
            url: url.to_owned(),
            profile_dir,
            gateway_addr: gateway.local_addr(),
            state: BrowserSurfaceState::Ready,
        };
        self.surfaces.insert(
            view_id,
            BrowserSurface {
                descriptor: descriptor.clone(),
                capability,
                gateway,
            },
        );
        Ok(descriptor)
    }

    pub fn descriptor(&self, view_id: &str) -> Option<BrowserSurfaceDescriptor> {
        self.surfaces
            .get(view_id)
            .map(|surface| surface.descriptor.clone())
    }

    /// Returns the ephemeral session capability for handing to the local chromux
    /// connector. Callers must keep it in memory only and never serialize or log it.
    pub fn capability_for_session(&self, view_id: &str) -> BrowserResult<String> {
        self.surfaces
            .get(view_id)
            .map(|surface| surface.capability.clone())
            .ok_or_else(|| BrowserError::InvalidView(view_id.to_owned()))
    }

    pub fn attach_chromux(
        &mut self,
        view_id: &str,
        capability: &str,
    ) -> BrowserResult<ChromuxSession> {
        let surface = self
            .surfaces
            .get_mut(view_id)
            .ok_or_else(|| BrowserError::InvalidView(view_id.to_owned()))?;
        if !constant_time_eq(surface.capability.as_bytes(), capability.as_bytes()) {
            return Err(BrowserError::CapabilityDenied);
        }
        surface.descriptor.state = BrowserSurfaceState::Ready;
        Ok(ChromuxSession {
            view_id: view_id.to_owned(),
            endpoint: surface.gateway.view_endpoint(view_id),
            capability: capability.to_owned(),
            attached: true,
        })
    }

    pub fn detach_chromux(&mut self, view_id: &str) -> BrowserResult<()> {
        let surface = self
            .surfaces
            .get_mut(view_id)
            .ok_or_else(|| BrowserError::InvalidView(view_id.to_owned()))?;
        surface.descriptor.state = BrowserSurfaceState::Detached;
        Ok(())
    }

    pub fn close(&mut self, view_id: &str) -> BrowserResult<BrowserSurfaceDescriptor> {
        let surface = self
            .surfaces
            .remove(view_id)
            .ok_or_else(|| BrowserError::InvalidView(view_id.to_owned()))?;
        let mut descriptor = surface.descriptor;
        descriptor.state = BrowserSurfaceState::Detached;
        Ok(descriptor)
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ChromuxSession {
    pub view_id: String,
    pub endpoint: String,
    capability: String,
    attached: bool,
}

impl fmt::Debug for ChromuxSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChromuxSession")
            .field("view_id", &self.view_id)
            .field("endpoint", &self.endpoint)
            .field("capability", &"[REDACTED]")
            .field("attached", &self.attached)
            .finish()
    }
}

impl ChromuxSession {
    pub fn capability(&self) -> &str {
        &self.capability
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    pub fn detach(&mut self) {
        self.attached = false;
    }

    pub fn reattach(&mut self) {
        self.attached = true;
    }
}

pub struct CdpGateway {
    local_addr: SocketAddr,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl CdpGateway {
    pub fn start(upstream: SocketAddr, view_id: String, capability: String) -> BrowserResult<Self> {
        if !is_loopback(upstream.ip()) {
            return Err(BrowserError::UpstreamUnavailable(
                "CEF CDP upstream must be loopback-only".to_owned(),
            ));
        }
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .map_err(|error| BrowserError::Gateway(format!("bind loopback: {error}")))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| BrowserError::Gateway(format!("configure listener: {error}")))?;
        let local_addr = listener
            .local_addr()
            .map_err(|error| BrowserError::Gateway(format!("read listener address: {error}")))?;
        let (stop, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name(format!("herdr-cdp-{view_id}"))
            .spawn(move || gateway_loop(listener, receiver, upstream, view_id, capability))
            .map_err(|error| BrowserError::Gateway(format!("spawn gateway: {error}")))?;
        Ok(Self {
            local_addr,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn endpoint(&self) -> String {
        format!("http://{}", self.local_addr)
    }

    pub fn view_endpoint(&self, view_id: &str) -> String {
        format!("{}/herdr/{view_id}", self.endpoint())
    }
}

impl Drop for CdpGateway {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn gateway_loop(
    listener: TcpListener,
    receiver: Receiver<()>,
    upstream: SocketAddr,
    view_id: String,
    capability: String,
) {
    loop {
        match receiver.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
        match listener.accept() {
            Ok((stream, _peer)) => {
                let view_id = view_id.clone();
                let capability = capability.clone();
                thread::spawn(move || handle_connection(stream, upstream, &view_id, &capability));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(GATEWAY_POLL);
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut client: TcpStream, upstream: SocketAddr, view_id: &str, capability: &str) {
    let mut request = vec![0_u8; REQUEST_LIMIT];
    let length = match client.read(&mut request) {
        Ok(length) if length > 0 => length,
        _ => return,
    };
    request.truncate(length);
    if !authorized_request(&request, view_id, capability) {
        let _ = client.write_all(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 18\r\nConnection: close\r\n\r\ncapability denied\n",
        );
        return;
    }
    let mut server = match TcpStream::connect_timeout(&upstream, Duration::from_secs(1)) {
        Ok(stream) => stream,
        Err(_) => {
            let _ = client.write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 19\r\nConnection: close\r\n\r\nupstream unavailable\n",
            );
            return;
        }
    };
    let upstream_request = rewrite_request(&request, view_id);
    if server.write_all(&upstream_request).is_err() {
        return;
    }
    let mut response = [0_u8; REQUEST_LIMIT];
    if let Ok(length) = server.read(&mut response) {
        let _ = client.write_all(&response[..length]);
    }
    let _ = client.shutdown(Shutdown::Both);
}

fn rewrite_request(request: &[u8], view_id: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(request);
    let Some((request_line, rest)) = text.split_once("\r\n") else {
        return request.to_vec();
    };
    let mut parts = request_line.split_whitespace();
    let Some(method) = parts.next() else {
        return request.to_vec();
    };
    let Some(path) = parts.next() else {
        return request.to_vec();
    };
    let version = parts.next().unwrap_or("HTTP/1.1");
    let prefix = format!("/herdr/{view_id}");
    let relative = path.strip_prefix(&prefix).unwrap_or(path);
    let relative = if relative.is_empty() || relative.starts_with('?') {
        if relative.starts_with('?') {
            format!("/{relative}")
        } else {
            "/".to_owned()
        }
    } else {
        relative.to_owned()
    };
    let (path, query) = relative.split_once('?').unwrap_or((relative.as_str(), ""));
    let query = query
        .split('&')
        .filter(|pair| !pair.starts_with("cap="))
        .collect::<Vec<_>>()
        .join("&");
    let rewritten_path = if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{query}")
    };
    format!("{method} {rewritten_path} {version}\r\n{rest}").into_bytes()
}

fn authorized_request(request: &[u8], view_id: &str, capability: &str) -> bool {
    let text = String::from_utf8_lossy(request);
    let first_line = text.lines().next().unwrap_or_default();
    let expected_path = format!("/herdr/{view_id}");
    let path = first_line.split_whitespace().nth(1).unwrap_or_default();
    let path_ok = !path.is_empty()
        && path.split('?').next().is_some_and(|path| {
            path == expected_path
                || path
                    .strip_prefix(&expected_path)
                    .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('?'))
        });
    let header = text.lines().find_map(|line| {
        line.strip_prefix("X-Herdr-Capability:")
            .or_else(|| line.strip_prefix("x-herdr-capability:"))
            .map(str::trim)
    });
    path_ok
        && header
            .is_some_and(|provided| constant_time_eq(provided.as_bytes(), capability.as_bytes()))
}

fn validate_loopback_url(url: &str) -> BrowserResult<()> {
    if !url.starts_with("http://127.0.0.1:") {
        return Err(BrowserError::InvalidUrl(url.to_owned()));
    }
    let port = url
        .split(':')
        .next_back()
        .and_then(|value| value.split('/').next())
        .and_then(|value| value.parse::<u16>().ok());
    if port.is_none_or(|port| port == 0) {
        return Err(BrowserError::InvalidUrl(url.to_owned()));
    }
    Ok(())
}

fn validate_profile(path: PathBuf) -> BrowserResult<PathBuf> {
    if !path.is_absolute() {
        return Err(BrowserError::InvalidProfile(path.display().to_string()));
    }
    std::fs::create_dir_all(&path)
        .map_err(|error| BrowserError::InvalidProfile(format!("{} ({error})", path.display())))?;
    Ok(path)
}

fn validate_source_pane(pane_id: &str) -> BrowserResult<()> {
    if pane_id.trim().is_empty()
        || pane_id.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':'))
        })
    {
        return Err(BrowserError::InvalidView(pane_id.to_owned()));
    }
    Ok(())
}

fn is_loopback(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(value) if value.is_loopback())
        || matches!(ip, IpAddr::V6(value) if value.is_loopback())
}

fn capability_token() -> BrowserResult<String> {
    const CAPABILITY_BYTES: usize = 32;
    let mut random = File::open("/dev/urandom")
        .map_err(|error| BrowserError::CapabilityUnavailable(format!("open OS CSPRNG: {error}")))?;
    capability_from_reader::<CAPABILITY_BYTES, _>(&mut random)
}

fn capability_from_reader<const N: usize, R: Read>(reader: &mut R) -> BrowserResult<String> {
    let mut bytes = [0_u8; N];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| BrowserError::CapabilityUnavailable(format!("read OS CSPRNG: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        });
    difference == 0
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserProfileState {
    pub schema_version: u32,
    pub profile_dir: PathBuf,
    pub persistent: bool,
}

impl BrowserProfileState {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn new(profile_dir: impl Into<PathBuf>) -> BrowserResult<Self> {
        let profile_dir = validate_profile(profile_dir.into())?;
        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            profile_dir,
            persistent: true,
        })
    }

    pub fn validate(&self) -> BrowserResult<()> {
        if self.schema_version != Self::SCHEMA_VERSION || !self.persistent {
            return Err(BrowserError::InvalidProfile(
                "profile state schema or persistence flag is invalid".to_owned(),
            ));
        }
        let _ = validate_profile(self.profile_dir.clone())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn fixture_profile(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("herdr-ide-t8-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn unused_loopback() -> SocketAddr {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
    }

    #[test]
    fn source_linked_open_is_idempotent_and_profile_persists() {
        let profile = fixture_profile("reuse");
        let upstream = unused_loopback();
        let mut registry = BrowserSurfaceRegistry::default();
        let first = registry
            .open_or_reuse(
                "pane-1",
                "http://127.0.0.1:43123/fixture",
                &profile,
                upstream,
            )
            .unwrap();
        let second = registry
            .open_or_reuse(
                "pane-1",
                "http://127.0.0.1:43123/fixture",
                &profile,
                upstream,
            )
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(registry.len(), 1);
        assert!(profile.is_dir());
        let _ = registry.close(&first.view_id);
        let _ = std::fs::remove_dir_all(profile);
    }

    #[test]
    fn cdp_gateway_is_loopback_only_and_capability_scoped() {
        let profile = fixture_profile("capability");
        let upstream = unused_loopback();
        let mut registry = BrowserSurfaceRegistry::default();
        let descriptor = registry
            .open_or_reuse(
                "pane-2",
                "http://127.0.0.1:43124/fixture",
                &profile,
                upstream,
            )
            .unwrap();
        let denied = registry.attach_chromux(&descriptor.view_id, "wrong");
        assert!(matches!(denied, Err(BrowserError::CapabilityDenied)));
        let capability = registry
            .surfaces
            .get(&descriptor.view_id)
            .unwrap()
            .capability
            .clone();
        let mut session = registry
            .attach_chromux(&descriptor.view_id, &capability)
            .unwrap();
        assert!(session.endpoint.starts_with("http://127.0.0.1:"));
        assert!(session.endpoint.ends_with("/herdr/browser:pane-2"));
        assert!(!session.endpoint.contains("cap="));
        assert!(format!("{session:?}").contains("[REDACTED]"));
        assert!(!format!("{session:?}").contains(&capability));
        assert!(session.is_attached());
        session.detach();
        assert!(!session.is_attached());
        session.reattach();
        assert!(session.is_attached());
        registry.detach_chromux(&descriptor.view_id).unwrap();
        assert_eq!(
            registry.descriptor(&descriptor.view_id).unwrap().state,
            BrowserSurfaceState::Detached
        );
        let _ = registry.close(&descriptor.view_id);
        let _ = std::fs::remove_dir_all(profile);
    }

    #[test]
    fn invalid_url_profile_and_non_loopback_upstream_fail_observably() {
        assert!(matches!(
            BrowserSurfaceRegistry::default().open_or_reuse(
                "pane-3",
                "https://example.com",
                fixture_profile("invalid-url"),
                unused_loopback(),
            ),
            Err(BrowserError::InvalidUrl(_))
        ));
        assert!(matches!(
            BrowserProfileState::new("relative/profile"),
            Err(BrowserError::InvalidProfile(_))
        ));
        let mut registry = BrowserSurfaceRegistry::default();
        assert!(matches!(
            registry.open_or_reuse(
                "pane-4",
                "http://127.0.0.1:43125/fixture",
                fixture_profile("invalid-upstream"),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 43125),
            ),
            Err(BrowserError::UpstreamUnavailable(_))
        ));
        assert!(matches!(
            BrowserSurfaceRegistry::default().open_or_reuse(
                "pane/with/slash",
                "http://127.0.0.1:43125/fixture",
                fixture_profile("invalid-view"),
                unused_loopback(),
            ),
            Err(BrowserError::InvalidView(_))
        ));
    }

    #[test]
    fn authorization_requires_view_path_and_capability_header() {
        let request =
            b"GET /herdr/browser:pane-1/json/version HTTP/1.1\r\nX-Herdr-Capability: cap\r\n\r\n";
        assert!(authorized_request(request, "browser:pane-1", "cap"));
        assert!(!authorized_request(
            b"GET /herdr/browser:pane-1/json/version?cap=cap HTTP/1.1\r\n\r\n",
            "browser:pane-1",
            "cap"
        ));
        assert!(!authorized_request(request, "browser:pane-2", "cap"));
        assert!(!authorized_request(request, "browser:pane-1", "other"));
        assert!(!authorized_request(
            b"GET /herdr/browser:pane-1/json/version HTTP/1.1\r\nX-Herdr-Capability: capx\r\n\r\n",
            "browser:pane-1",
            "cap"
        ));
    }

    #[test]
    fn capability_is_random_and_not_derived_from_view_identity() {
        let first = capability_token().unwrap();
        let second = capability_token().unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(second.len(), 64);
        assert_ne!(first, second);
        assert!(!first.contains("browser:pane"));
    }

    #[test]
    fn capability_entropy_failure_is_observable_before_gateway_start() {
        let mut short_reader = &b"too-short"[..];
        let error = capability_from_reader::<32, _>(&mut short_reader).unwrap_err();
        assert!(matches!(error, BrowserError::CapabilityUnavailable(_)));
        assert!(error.to_string().contains("read OS CSPRNG"));
    }

    #[test]
    fn gateway_proxies_only_authorized_loopback_requests_and_strips_capability_path() {
        let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        let upstream_thread = std::thread::spawn(move || {
            let (mut stream, _) = upstream_listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let size = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("GET /json/version HTTP/1.1"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
        });
        let profile = fixture_profile("proxy");
        let mut registry = BrowserSurfaceRegistry::default();
        let descriptor = registry
            .open_or_reuse(
                "pane-proxy",
                "http://127.0.0.1:43126/fixture",
                &profile,
                upstream_addr,
            )
            .unwrap();
        let capability = registry
            .capability_for_session(&descriptor.view_id)
            .unwrap();
        let endpoint = registry
            .attach_chromux(&descriptor.view_id, &capability)
            .unwrap()
            .endpoint;
        let mut client = TcpStream::connect(descriptor.gateway_addr).unwrap();
        let path = format!("/herdr/{}/json/version", descriptor.view_id);
        assert_eq!(
            endpoint,
            format!(
                "http://{}/herdr/{}",
                descriptor.gateway_addr, descriptor.view_id
            )
        );
        client
            .write_all(
                format!(
                    "GET {path} HTTP/1.1\r\nHost: localhost\r\nX-Herdr-Capability: {capability}\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        upstream_thread.join().unwrap();
        let _ = std::fs::remove_dir_all(profile);
    }
}
