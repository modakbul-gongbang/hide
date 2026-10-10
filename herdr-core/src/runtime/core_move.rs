//! What a core move reads from the running core before it stops it (PRD
//! core-host-node-move B4): the device the core goes to, how this machine
//! reaches it, and every project of the two machines with its checkouts,
//! from which the move makes the id table (`node_migration::id_table`).
//! Taken under the lock as owned data; nothing here waits.

use super::*;
use crate::model::DeviceRegistration;

/// The core's facts a move is made from.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveSource {
    /// This core's node.
    pub node: String,
    /// The device's registration id, which every key of the device carries.
    pub device: String,
    pub ssh_alias: String,
    /// The device's machine id: the node it is once it holds the core.
    pub device_node: String,
    /// The `hided` the core installed there, which runs the move's steps.
    pub helper_path: String,
    pub device_herdr_socket: Option<String>,
    /// The device's registration as `core-state.json` stores it, which a
    /// move back restores.
    pub registration: serde_json::Value,
    /// The projects of this machine and of the device.
    pub projects: Vec<MoveProject>,
    /// Nodes other than the device that dialed in and are linked now.
    pub other_linked_nodes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveProject {
    pub id: String,
    pub device_id: String,
    pub path: String,
    /// Each checkout's id and path.
    pub checkouts: Vec<(String, String)>,
}

/// Why the core cannot be moved to a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MoveSourceRefusal {
    /// No device is registered under the id.
    UnknownDevice,
    /// The device's node dials this core; only a device this core dials
    /// over SSH can take it.
    NotDialed,
    /// The device's link is not up.
    NotConnected,
    /// The device has not reported its machine id or its helper.
    NotReady,
    /// The registration could not be written in its stored form.
    Unstorable(String),
}

impl MoveSourceRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownDevice => "unknown_device",
            Self::NotDialed => "not_dialed",
            Self::NotConnected => "not_connected",
            Self::NotReady => "not_ready",
            Self::Unstorable(_) => "unstorable",
        }
    }
}

impl Runtime {
    pub(crate) fn move_source(&self, device: &str) -> Result<MoveSource, MoveSourceRefusal> {
        let registration: &DeviceRegistration = self
            .device_registration(device)
            .ok_or(MoveSourceRefusal::UnknownDevice)?;
        let LinkOrigin::Dialed { ssh_alias } = &registration.origin else {
            return Err(MoveSourceRefusal::NotDialed);
        };
        let connected = self
            .snapshot
            .status
            .remote
            .iter()
            .any(|status| status.target_id == device && status.state == "connected");
        let Some(hosts::HostPhase::Ready {
            helper_path: Some(helper_path),
            ..
        }) = self.device_hosts.get(device).map(|host| &host.phase)
        else {
            return Err(if connected {
                MoveSourceRefusal::NotReady
            } else {
                MoveSourceRefusal::NotConnected
            });
        };
        if !connected {
            return Err(MoveSourceRefusal::NotConnected);
        }
        let device_node = self
            .device_machine_ids
            .get(device)
            .cloned()
            .ok_or(MoveSourceRefusal::NotReady)?;
        let node = self.node.as_str().to_owned();
        let checkouts = |id: &str| -> Vec<(String, String)> {
            self.snapshot
                .navigator
                .workspaces
                .iter()
                .filter(|row| row.id == id)
                .flat_map(|row| row.checkouts.iter())
                .map(|checkout| (checkout.id.clone(), checkout.path.clone()))
                .collect()
        };
        let projects = self
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .filter(|project| project.device_id == node || project.device_id == device)
            .map(|project| MoveProject {
                id: project.id.clone(),
                device_id: project.device_id.clone(),
                path: project.path.clone(),
                checkouts: checkouts(&project.id),
            })
            .collect();
        Ok(MoveSource {
            node,
            device: device.to_owned(),
            ssh_alias: ssh_alias.clone(),
            device_node,
            helper_path: helper_path.clone(),
            device_herdr_socket: registration.herdr_socket_path.clone(),
            registration: serde_json::to_value(registration)
                .map_err(|error| MoveSourceRefusal::Unstorable(error.to_string()))?,
            projects,
            other_linked_nodes: self
                .linked_nodes()
                .into_iter()
                .filter(|linked| linked != device)
                .collect(),
        })
    }
}
