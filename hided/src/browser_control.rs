//! App-lifetime browser control registration. Only the authenticated desktop
//! registers a loopback gateway; a pane receives a checkout-scoped capability.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::Uri;
use herdr_core::workspace_control::QueryResult;
use hide_host::pane_peer::process_start;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

const MAX_HOSTS: usize = 4;
const MAX_REQUESTS: usize = 8;
const MAX_ANSWER_BYTES: u64 = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

pub type Failure = (&'static str, &'static str);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub owner_pid: i32,
    pub endpoint: String,
    pub token: String,
}

#[derive(Clone)]
struct Host {
    started: u64,
    endpoint: String,
    token: String,
    caller_key: String,
}

pub struct BrowserControl {
    hosts: Mutex<HashMap<i32, Host>>,
    slots: Arc<Semaphore>,
}

impl Default for BrowserControl {
    fn default() -> Self {
        Self {
            hosts: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(MAX_REQUESTS)),
        }
    }
}

fn unavailable() -> Failure {
    (
        "browser_control_unavailable",
        "Reconnect the Hide desktop app and retry",
    )
}

fn live(hosts: &mut HashMap<i32, Host>) {
    hosts.retain(|pid, host| process_start(*pid) == Some(host.started));
}

/// Numeric IPv4 loopback only, with no credentials, path, query or redirect.
/// This is the private registration URL, never a public capability URL.
fn endpoint_valid(endpoint: &str) -> bool {
    let Ok(uri) = endpoint.parse::<Uri>() else {
        return false;
    };
    uri.scheme_str() == Some("http")
        && uri.host() == Some("127.0.0.1")
        && uri.port_u16().is_some_and(|port| port != 0)
        && uri
            .authority()
            .is_some_and(|authority| !authority.as_str().contains('@'))
        && matches!(
            uri.path_and_query().map(|value| value.as_str()),
            None | Some("/")
        )
}

impl BrowserControl {
    pub fn register(&self, registration: Registration) -> Result<(), Failure> {
        if !endpoint_valid(&registration.endpoint)
            || registration.token.len() < 32
            || registration.token.len() > 128
            || !registration
                .token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err((
                "invalid_browser_control",
                "Use the desktop app's loopback gateway",
            ));
        }
        let started = process_start(registration.owner_pid).ok_or_else(unavailable)?;
        let mut hosts = self.hosts.lock().map_err(|_| unavailable())?;
        live(&mut hosts);
        if let Some(host) = hosts.get(&registration.owner_pid) {
            if host.started == started
                && host.endpoint == registration.endpoint
                && host.token == registration.token
            {
                return Ok(());
            }
            return Err((
                "browser_control_conflict",
                "Reconnect this desktop window before registering its gateway",
            ));
        }
        if hosts.len() >= MAX_HOSTS {
            return Err((
                "browser_control_limit",
                "Close an unused Hide desktop window and retry",
            ));
        }
        let caller_key = crate::state_file::new_token();
        hosts.insert(
            registration.owner_pid,
            Host {
                started,
                endpoint: registration.endpoint.trim_end_matches('/').to_owned(),
                token: registration.token,
                caller_key,
            },
        );
        Ok(())
    }

    pub fn release(&self, registration: &Registration) -> Result<(), Failure> {
        let mut hosts = self.hosts.lock().map_err(|_| unavailable())?;
        // A stale app cannot remove a later app's registration after PID reuse.
        if hosts.get(&registration.owner_pid).is_some_and(|host| {
            host.endpoint == registration.endpoint.trim_end_matches('/')
                && crate::server::token_matches(&registration.token, &host.token)
        }) {
            hosts.remove(&registration.owner_pid);
        }
        Ok(())
    }

    pub fn caller(&self, pid: i32, checkout_path: &str) -> Result<String, Failure> {
        let mut hosts = self.hosts.lock().map_err(|_| unavailable())?;
        live(&mut hosts);
        let host = hosts.get(&pid).ok_or_else(unavailable)?;
        Ok(herdr_core::workspace_control::checkout_caller_id(
            &host.caller_key,
            checkout_path,
        ))
    }

    pub fn acquire(&self) -> Result<tokio::sync::OwnedSemaphorePermit, Failure> {
        self.slots.clone().try_acquire_owned().map_err(|_| {
            (
                "browser_control_busy",
                "Wait for an earlier browser request to finish and retry",
            )
        })
    }

    pub fn connect(
        &self,
        source: &QueryResult,
        display_id: Option<&str>,
    ) -> Result<Value, Failure> {
        let _slot = self.acquire()?;
        let host = {
            let mut hosts = self.hosts.lock().map_err(|_| unavailable())?;
            live(&mut hosts);
            match hosts.len() {
                0 => return Err(unavailable()),
                1 => hosts.values().next().ok_or_else(unavailable)?.clone(),
                _ => {
                    return Err((
                        "browser_control_ambiguous",
                        "Keep one desktop window attached to this daemon and retry",
                    ));
                }
            }
        };
        let (area_id, display_id) = connection_scope(source, display_id)?;
        let workspace = format!(
            "{}\0{}",
            source.context.device_id, source.context.checkout_path
        );
        let mut body = json!({"workspace":workspace,"area_id":area_id});
        if let Some(id) = &display_id {
            body["display_id"] = json!(id);
        }
        let body = body.to_string();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .max_redirects(0)
            .proxy(None)
            .build()
            .into();
        let mut response = agent
            .post(format!("{}/connect", host.endpoint))
            .header("Authorization", format!("Bearer {}", host.token))
            .header("Content-Type", "application/json")
            .send(body.as_bytes())
            .map_err(|_| unavailable())?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_ANSWER_BYTES)
            .read_to_vec()
            .map_err(|_| unavailable())?;
        let answer: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        let answer = answer.get("result").unwrap_or(&answer);
        let http = answer["cdp_http_url"].as_str().ok_or_else(unavailable)?;
        let ws = answer["browser_ws_url"].as_str().ok_or_else(unavailable)?;
        if !capability_url_valid(http, "http", &host.endpoint)
            || !capability_url_valid(ws, "ws", &host.endpoint)
            || http.contains(&host.token)
            || ws.contains(&host.token)
        {
            return Err(unavailable());
        }
        // Return only the bounded public contract, never a registration token
        // or arbitrary fields the desktop sent back.
        Ok(
            json!({"context":source.context,"area_id":area_id,"display_id":display_id,
            "cdp_http_url":http,"browser_ws_url":ws}),
        )
    }
}

fn capability_url_valid(value: &str, scheme: &str, endpoint: &str) -> bool {
    let Ok(uri) = value.parse::<Uri>() else {
        return false;
    };
    let Ok(base) = endpoint.parse::<Uri>() else {
        return false;
    };
    value.len() <= 2048
        && uri.scheme_str() == Some(scheme)
        && uri.authority() == base.authority()
        && uri.query().is_none()
        && uri.path().len() > 32
        && !uri.path().contains("..")
}

fn connection_scope(
    source: &QueryResult,
    display_id: Option<&str>,
) -> Result<(String, Option<String>), Failure> {
    let views = source.views.as_deref().ok_or_else(unavailable)?;
    if let Some(id) = display_id {
        let view = views
            .iter()
            .find(|view| view.view_id == id && view.kind == "browser")
            .ok_or((
                "browser_display_missing",
                "Run hide view list and choose a browser display",
            ))?;
        return Ok((view.area_id.clone(), Some(view.view_id.clone())));
    }
    let view = views.iter().find(|view| view.active_area).ok_or((
        "view_area_missing",
        "Open this checkout's View area and retry",
    ))?;
    Ok((view.area_id.clone(), None))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAction {
    pub owner_pid: i32,
    pub device_id: String,
    pub checkout_path: String,
    pub area_id: String,
    pub action: String,
    pub url: Option<String>,
    pub display_id: Option<String>,
    pub request_id: String,
}

impl BrowserAction {
    pub fn valid(&self) -> bool {
        self.owner_pid > 0
            && !self.device_id.is_empty()
            && self.device_id.len() <= 256
            && hide_platform::path::is_wire_absolute(&self.checkout_path)
            && self.checkout_path.len() <= 8192
            && !self.area_id.is_empty()
            && self.area_id.len() <= 256
            && crate::workspace_cli::valid_request_id(&self.request_id)
    }

    pub fn command(
        &self,
        source: &QueryResult,
    ) -> Result<herdr_core::workspace_control::Action, Failure> {
        use herdr_core::workspace_control::Action;
        if source.context.device_id != self.device_id
            || source.context.checkout_path != self.checkout_path
        {
            return Err((
                "workspace_changed",
                "Reconnect the browser capability and retry",
            ));
        }
        match self.action.as_str() {
            "open" if self.display_id.is_none() => Ok(Action::OpenBrowser {
                url: self.url.clone().filter(|url| !url.is_empty()).ok_or((
                    "invalid_address",
                    "Use an http, https, or checkout HTML address",
                ))?,
                reveal: false,
                area_id: Some(self.area_id.clone()),
                new_target: true,
            }),
            "close" | "select" if self.url.is_none() => {
                let id = self.display_id.as_deref().ok_or((
                    "browser_display_missing",
                    "Choose a current browser display",
                ))?;
                Ok(if self.action == "close" {
                    Action::Close {
                        view_id: id.to_owned(),
                    }
                } else {
                    Action::Select {
                        view_id: id.to_owned(),
                        reveal: false,
                    }
                })
            }
            _ => Err((
                "invalid_browser_action",
                "Use a scoped browser open, close, or select action",
            )),
        }
    }

    /// Check a fresh intent after the core has checked its retry record.
    /// A completed close can return its receipt after its page is gone.
    pub fn validate_scope(&self, source: &QueryResult) -> Result<(), Failure> {
        let views = source.views.as_deref().ok_or_else(unavailable)?;
        if !views.iter().any(|view| view.area_id == self.area_id) {
            return Err(("view_area_missing", "Reconnect from an existing View area"));
        }
        if let Some(id) = self.display_id.as_deref()
            && !views.iter().any(|view| {
                view.view_id == id && view.area_id == self.area_id && view.kind == "browser"
            })
        {
            return Err((
                "browser_display_missing",
                "Reconnect the browser capability and retry",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> QueryResult {
        QueryResult {
            context: herdr_core::workspace_control::Context {
                device_id: "local".into(),
                workspace_id: "workspace".into(),
                checkout_id: "checkout".into(),
                checkout_path: "/checkout".into(),
            },
            capabilities: vec![],
            views: Some(vec![herdr_core::workspace_control::View {
                area_id: "area-a".into(),
                view_id: "browser-a".into(),
                kind: "browser",
                target: "about:blank".into(),
                selected: true,
                active_area: true,
                page: None,
            }]),
        }
    }

    #[test]
    fn display_lookup_never_accepts_a_terminal_or_foreign_display() {
        let mut source = source();
        assert_eq!(
            connection_scope(&source, Some("browser-a")).unwrap(),
            ("area-a".into(), Some("browser-a".into()))
        );
        assert!(connection_scope(&source, Some("other-checkout-browser")).is_err());
        source.views.as_mut().unwrap()[0].kind = "terminal";
        assert!(connection_scope(&source, Some("browser-a")).is_err());
        assert_eq!(
            connection_scope(&source, None).unwrap(),
            ("area-a".into(), None)
        );
    }

    #[test]
    fn action_refuses_wrong_checkout_area_and_non_browser_display() {
        let source = source();
        let mut action = BrowserAction {
            owner_pid: std::process::id() as i32,
            device_id: "local".into(),
            checkout_path: "/checkout".into(),
            area_id: "area-a".into(),
            action: "close".into(),
            url: None,
            display_id: Some("browser-a".into()),
            request_id: "100-close".into(),
        };
        assert!(action.command(&source).is_ok());
        assert!(action.validate_scope(&source).is_ok());
        action.checkout_path = "/foreign".into();
        assert_eq!(action.command(&source).unwrap_err().0, "workspace_changed");
        action.checkout_path = "/checkout".into();
        action.area_id = "other-area".into();
        assert_eq!(
            action.validate_scope(&source).unwrap_err().0,
            "view_area_missing"
        );
        action.area_id = "area-a".into();
        action.display_id = Some("shell-renderer".into());
        assert_eq!(
            action.validate_scope(&source).unwrap_err().0,
            "browser_display_missing"
        );
    }

    #[test]
    fn requests_have_one_shared_admission_cap() {
        let registry = BrowserControl::default();
        let slots: Vec<_> = (0..MAX_REQUESTS)
            .map(|_| registry.acquire().unwrap())
            .collect();
        assert_eq!(registry.acquire().unwrap_err().0, "browser_control_busy");
        drop(slots);
        assert!(registry.acquire().is_ok());
    }

    #[test]
    fn registration_cannot_turn_the_daemon_into_an_external_proxy() {
        assert!(endpoint_valid("http://127.0.0.1:9322"));
        for url in [
            "http://localhost:9322",
            "http://example.com:9322",
            "https://127.0.0.1:9322",
            "http://127.0.0.1:0",
            "http://user@127.0.0.1:9322",
            "http://127.0.0.1:9322/secret",
            "http://127.0.0.1:9322/?to=external",
        ] {
            assert!(!endpoint_valid(url), "{url}");
        }
    }

    #[test]
    fn returned_capability_stays_on_the_registered_port() {
        let base = "http://127.0.0.1:9322";
        assert!(capability_url_valid(
            "http://127.0.0.1:9322/c/012345678901234567890123456789012345",
            "http",
            base
        ));
        assert!(!capability_url_valid(
            "http://127.0.0.1:9323/c/012345678901234567890123456789012345",
            "http",
            base
        ));
        assert!(!capability_url_valid(
            "http://external:9322/c/012345678901234567890123456789012345",
            "http",
            base
        ));
    }

    #[test]
    fn registration_is_idempotent_and_stale_release_does_not_remove_it() {
        let registry = BrowserControl::default();
        let pid = std::process::id() as i32;
        let make = |token: &str| Registration {
            owner_pid: pid,
            endpoint: "http://127.0.0.1:9322".into(),
            token: token.into(),
        };
        let token = "0123456789abcdef0123456789abcdef";
        registry.register(make(token)).unwrap();
        registry.register(make(token)).unwrap();
        registry
            .release(&make("ffffffffffffffffffffffffffffffff"))
            .unwrap();
        assert!(registry.caller(pid, "/checkout").is_ok());
        registry.release(&make(token)).unwrap();
        assert!(registry.caller(pid, "/checkout").is_err());
    }
}
