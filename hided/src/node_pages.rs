//! `hide browser` pages on a screen machine whose core runs on another
//! machine (PRD core-host-node-remote-core D-17, B15). The desktop host
//! asks this machine's daemon for a View's route as it asks a core's: the
//! View's source comes from the core over the link's relay port, a page of
//! this machine loads as it is, and a loopback page of the core's machine
//! reaches that machine's loopback through the link's own SSH connection.
//! The registry, its caps and its reaper are the core's (`browser_routes`).

use std::sync::Arc;
use std::time::Duration;

use herdr_core::workspace_control::BrowserRouteSource;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::watch;

use crate::browser_routes::{PageSources, Way};
use crate::node_role::LiveLink;
use crate::server::RELAY_GRANT_HEADER;

/// How long the core may take to name a View's page.
const SOURCE_TIMEOUT: Duration = Duration::from_secs(10);
/// The largest answer the core may give for one View's page.
const MAX_SOURCE_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
struct SourceAnswer {
    source: Option<BrowserRouteSource>,
}

pub struct NodePages {
    /// This machine's node id: its pages load as they are.
    node: String,
    /// The core's machine, whose loopback pages go through the link.
    core: String,
    live: watch::Receiver<Option<Arc<LiveLink>>>,
    agent: ureq::Agent,
}

impl NodePages {
    pub fn new(node: String, core: String, live: watch::Receiver<Option<Arc<LiveLink>>>) -> Self {
        Self {
            node,
            core,
            live,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(SOURCE_TIMEOUT))
                .max_redirects(0)
                .proxy(None)
                .build()
                .into(),
        }
    }

    fn link(&self) -> Result<Arc<LiveLink>, &'static str> {
        self.live.borrow().clone().ok_or("core_unavailable")
    }
}

impl PageSources for NodePages {
    fn own_node(&self) -> &str {
        &self.node
    }

    fn source(
        &self,
        device: &str,
        checkout: &str,
        view: &str,
        load: u64,
    ) -> Result<Option<BrowserRouteSource>, &'static str> {
        let link = self.link()?;
        let body = json!({
            "device_id": device,
            "checkout_path": checkout,
            "id": view,
            "load": load,
        })
        .to_string();
        let answer = self
            .agent
            .post(format!(
                "http://127.0.0.1:{}/relay/browser-source",
                link.relay_port
            ))
            .header(RELAY_GRANT_HEADER, &link.accepted.relay_token)
            .header("Content-Type", "application/json")
            .send(body.as_bytes())
            .and_then(|mut response| {
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_SOURCE_BYTES)
                    .read_to_vec()
            });
        let bytes = answer.map_err(|error| {
            herdr_core::diagnostic!(json!({
                "component": "node_pages",
                "kind": "source.failed",
                "generation": link.generation,
                "reason": error.to_string(),
            }));
            "core_unavailable"
        })?;
        let answer: SourceAnswer =
            serde_json::from_slice(&bytes).map_err(|_| "core_unavailable")?;
        Ok(answer.source)
    }

    fn way(&self, device: &str) -> Result<Way, &'static str> {
        if device != self.core {
            return Err("host_unavailable");
        }
        Ok(Way::Core(Arc::clone(&self.link()?.upstream)))
    }
}
