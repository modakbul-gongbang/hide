//! The SSH transport behind `hide_node_link::device`: the node that holds
//! the account's SSH configuration and keys reaches each registered device
//! with it, and the core sees only the traits.

use super::host::{self, HelperPackages, PaneEventsSlot, PaneHook, TerminalHook};
use super::hosts::{Resolve, ssh_g};
use super::*;
use hide_node_link::attachments::AttachmentFile;
use hide_node_link::device::{
    DeviceConnector, DeviceTransport, EstablishError, Established, HostConsent, SshHostListing,
};

/// Opens the SSH transport to a device by alias. It carries this build's
/// device packages, which a transport installs on a device that lacks them.
pub struct Connector {
    packages: HelperPackages,
    /// What resolves an alias for Add device's list: `ssh -G`, or a test's
    /// answers.
    resolve: Resolve,
    /// Where each device's pane events go; none for a connector whose
    /// devices' panes do not reach this process.
    panes: Option<PaneEventsSlot>,
    /// Where each device's terminals' output and reports go; none for a
    /// connector whose devices' terminals do not reach this process.
    terminals: Option<Arc<dyn crate::terminal::device::DeviceSink>>,
}

impl Connector {
    /// `helper_dir` is the folder holding the builds devices run; `None`
    /// leaves every device's node `unsupported` with that reason.
    pub fn new(helper_dir: Option<PathBuf>) -> Self {
        Self {
            packages: HelperPackages::new(helper_dir),
            resolve: ssh_g(PathBuf::from("ssh")),
            panes: None,
            terminals: None,
        }
    }

    /// Each device's panes' terminals flow inside its node link, and their
    /// output and reports go to `sink`.
    pub fn with_terminals(mut self, sink: Arc<dyn crate::terminal::device::DeviceSink>) -> Self {
        self.terminals = Some(sink);
        self
    }

    /// Each device's node serves its panes' `hide` over its link, and what
    /// they send goes to `events`.
    pub fn with_pane_events(mut self, events: PaneEventsSlot) -> Self {
        self.panes = Some(events);
        self
    }

    /// Resolves aliases for Add device's list with `ssh` instead of the
    /// `ssh` on `PATH`.
    pub fn with_ssh_program(self, ssh: PathBuf) -> Self {
        self.with_resolve(ssh_g(ssh))
    }

    /// Resolves aliases for Add device's list with `resolve`, so a test
    /// decides each answer and no child races the `ssh -G` deadline.
    pub fn with_resolve(mut self, resolve: Resolve) -> Self {
        self.resolve = resolve;
        self
    }
}

impl DeviceConnector for Connector {
    fn transport(
        &self,
        home: &Path,
        node: &str,
        alias: &str,
        herdr_socket: Option<String>,
    ) -> Result<Arc<dyn DeviceTransport>, String> {
        // The row is what the operator reads, so it names the fix; the full
        // staged diagnostic stays in the error's diagnostic.
        let resolved =
            SshAlias::from_config_file(&home.join(".ssh/config"), alias).map_err(|error| {
                format!(
                    "SSH alias {alias} could not be read from ~/.ssh/config: {}",
                    error.diagnostic().reason
                )
            })?;
        let client = RusshRemoteClient::new(resolved)
            .map_err(|error| error.to_string())?
            .with_herdr_socket(herdr_socket);
        Ok(Arc::new(SshDevice {
            client: Arc::new(client),
            packages: self.packages.clone(),
            panes: self.panes.clone().map(|events| PaneHook {
                node: node.to_owned(),
                events,
            }),
            terminals: self.terminals.clone().map(|sink| TerminalHook {
                node: node.to_owned(),
                sink,
            }),
        }))
    }

    fn ssh_hosts(&self, home: &Path, registered: &[String], stop: &AtomicBool) -> SshHostListing {
        super::hosts::list(&self.resolve, home, registered, stop)
    }
}

/// One device reached over SSH.
pub struct SshDevice {
    client: Arc<RusshRemoteClient>,
    packages: HelperPackages,
    panes: Option<PaneHook>,
    terminals: Option<TerminalHook>,
}

impl SshDevice {
    /// The SSH client, for the forwards the shell opens itself.
    pub fn client(&self) -> &Arc<RusshRemoteClient> {
        &self.client
    }
}

impl DeviceTransport for SshDevice {
    fn herdr_api_connector(&self) -> Arc<dyn ApiConnector> {
        Arc::new(self.client.herdr_api_connector())
    }

    fn cached_herdr_version(&self) -> Option<String> {
        self.client.cached_herdr_version()
    }

    fn capability_test(&self, operation_id: &str, check: SnapshotCheck<'_>) -> CapabilityReport {
        self.client.staged_capability_test(operation_id, check)
    }

    fn establish(
        &self,
        consent: &HostConsent,
        retirement_projects: &[String],
        on_close: Box<dyn FnOnce(String) + Send + 'static>,
    ) -> Result<Established, EstablishError> {
        host::establish(
            &self.client,
            &self.packages,
            consent,
            retirement_projects,
            self.panes.clone(),
            self.terminals.clone(),
            on_close,
        )
    }

    fn stage_attachments(
        &self,
        request_id: &str,
        files: &[AttachmentFile],
        cancelled: &AtomicBool,
    ) -> Result<Vec<String>, String> {
        self.client.stage_attachments(request_id, files, cancelled)
    }

    fn remove_attachments(&self, request_id: &str, files: &[AttachmentFile]) {
        self.client.remove_attachments(request_id, files)
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
}
