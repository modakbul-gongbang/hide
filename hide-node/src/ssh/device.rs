//! The SSH transport behind `hide_node_link::device`: the node that holds
//! the account's SSH configuration and keys reaches each registered device
//! with it, and the core sees only the traits.

use super::host::{self, HelperPackages};
use super::*;
use hide_node_link::attachments::AttachmentFile;
use hide_node_link::device::{
    DeviceConnector, DeviceTransport, EstablishError, Established, HostConsent, SshHostListing,
    TerminalSessionParts,
};

/// Opens the SSH transport to a device by alias. It carries this build's
/// device packages, which a transport installs on a device that lacks them.
pub struct Connector {
    packages: HelperPackages,
    /// The `ssh` program that resolves an alias for Add device's list.
    ssh: PathBuf,
}

impl Connector {
    /// `helper_dir` is the folder holding the builds devices run; `None`
    /// leaves every device's node `unsupported` with that reason.
    pub fn new(helper_dir: Option<PathBuf>) -> Self {
        Self {
            packages: HelperPackages::new(helper_dir),
            ssh: PathBuf::from("ssh"),
        }
    }

    /// Resolves aliases for Add device's list with `ssh` instead of the
    /// `ssh` on `PATH`.
    pub fn with_ssh_program(mut self, ssh: PathBuf) -> Self {
        self.ssh = ssh;
        self
    }
}

impl DeviceConnector for Connector {
    fn transport(
        &self,
        home: &Path,
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
        }))
    }

    fn ssh_hosts(&self, home: &Path, registered: &[String], stop: &AtomicBool) -> SshHostListing {
        super::hosts::list(&self.ssh, home, registered, stop)
    }
}

/// One device reached over SSH.
pub struct SshDevice {
    client: Arc<RusshRemoteClient>,
    packages: HelperPackages,
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
            on_close,
        )
    }

    fn open_terminal_session(
        &self,
        pane_id: &str,
        mode: &str,
        rows: u16,
        cols: u16,
    ) -> RemoteResult<TerminalSessionParts> {
        self.client
            .open_terminal_session(pane_id, mode, rows, cols)
            .map(RemoteTerminalProcess::into_parts)
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
