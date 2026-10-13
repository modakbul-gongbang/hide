use super::*;
use crate::model::SidebarTabKey;

impl Runtime {
    pub(super) fn set_sidebar_tab_expanded(
        &mut self,
        payload: events::SidebarTabExpandedPayload,
    ) -> bool {
        let workspaces = if payload.device_id == self.node.as_str() {
            Some(self.snapshot.navigator.workspaces.as_slice())
        } else {
            self.snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == payload.device_id && status.state == "connected")
                .and_then(|status| status.session.as_ref())
                .map(|session| session.workspaces.as_slice())
        };
        let admitted = workspaces.is_some_and(|spaces| {
            spaces
                .iter()
                .flat_map(|space| &space.checkouts)
                .any(|checkout| {
                    checkout.id == payload.checkout_id
                        && checkout
                            .tabs
                            .iter()
                            .any(|tab| tab.id.as_deref() == Some(payload.tab_id.as_str()))
                })
        });
        if !admitted {
            crate::diagnostic!(serde_json::json!({
                "event": "sidebar_tab.unavailable", "device_id": payload.device_id,
                "checkout_id": payload.checkout_id, "tab_id": payload.tab_id,
            }));
            return false;
        }
        let key = SidebarTabKey {
            device_id: payload.device_id,
            checkout_id: payload.checkout_id,
            tab_id: payload.tab_id,
        };
        let collapsed = &mut self.snapshot.ui_state.sidebar_collapsed_tabs;
        if collapsed.contains(&key) == !payload.expanded {
            return false;
        }
        collapsed.retain(|entry| entry != &key);
        if !payload.expanded {
            collapsed.push(key);
            collapsed.sort();
        }
        self.persist_ui_state();
        true
    }

    /// Called only with an accepted session, never a boot catalog or a failed fetch.
    /// A live but temporarily unplaced tab retains its choice until placement settles.
    pub(super) fn prune_sidebar_tabs(&mut self, device: &str, live: &BTreeSet<String>) -> bool {
        if !self
            .snapshot
            .ui_state
            .sidebar_collapsed_tabs
            .iter()
            .any(|key| key.device_id == device)
        {
            return false;
        }
        let spaces = if device == self.node.as_str() {
            self.snapshot.navigator.workspaces.as_slice()
        } else {
            match self
                .snapshot
                .status
                .remote
                .iter()
                .find(|status| status.target_id == device)
                .and_then(|status| status.session.as_ref())
            {
                Some(session) => session.workspaces.as_slice(),
                None => return false,
            }
        };
        let placed: BTreeMap<&str, &str> = spaces
            .iter()
            .flat_map(|space| &space.checkouts)
            .flat_map(|checkout| {
                checkout
                    .tabs
                    .iter()
                    .filter_map(|tab| Some((tab.id.as_deref()?, checkout.id.as_str())))
            })
            .collect();
        let collapsed = &mut self.snapshot.ui_state.sidebar_collapsed_tabs;
        let before = collapsed.len();
        collapsed.retain(|key| {
            key.device_id != device
                || (live.contains(&key.tab_id)
                    && placed
                        .get(key.tab_id.as_str())
                        .is_none_or(|checkout| *checkout == key.checkout_id))
        });
        let changed = before != collapsed.len();
        if changed {
            self.persist_ui_state();
        }
        changed
    }
}
