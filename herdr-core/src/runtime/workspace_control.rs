//! Read-only pane-scoped queries. They inspect the live Herdr projection and
//! the View tree for that checkout, including one not in front.

use super::Runtime;
use crate::view_layout::DisplayKind;
use crate::workspace_control::{Context, Query, QueryResult, Refusal, View};

impl Runtime {
    pub fn workspace_control_query(
        &self,
        device_id: &str,
        pane_id: &str,
        query: Query,
    ) -> Result<QueryResult, Refusal> {
        let mut found = None;
        for workspace in self.catalog_workspaces() {
            if workspace.device_id != device_id {
                continue;
            }
            let connected = if workspace.device_id == "local" {
                self.snapshot.status.herdr.state == "connected"
            } else {
                self.snapshot.status.remote.iter().any(|remote| {
                    remote.target_id == workspace.device_id && remote.state == "connected"
                })
            };
            if !connected {
                continue;
            }
            for checkout in &workspace.checkouts {
                if checkout
                    .tabs
                    .iter()
                    .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
                {
                    if found.is_some() {
                        return Err(Refusal {
                            reason: "ambiguous_pane",
                            next_action: "Reconnect the pane and run hide workspace info again",
                        });
                    }
                    found = Some(Context {
                        device_id: workspace.device_id.clone(),
                        workspace_id: workspace.id.clone(),
                        checkout_id: checkout.id.clone(),
                        checkout_path: checkout.path.clone(),
                    });
                }
            }
        }
        let context = found.ok_or(Refusal {
            reason: "pane_not_connected",
            next_action: "Reconnect the pane in Hide and run hide workspace info again",
        })?;
        let key = (context.device_id.clone(), context.checkout_path.clone());
        if self.workspace_views.is_none() {
            return Err(Refusal {
                reason: "views_unavailable",
                next_action: "Open this Workspace in a Hide web or desktop window and retry",
            });
        }
        // An untouched Workspace has no stored row yet. Its View tree is the
        // default one-empty-area layout, even when it is not the front one.
        let empty = crate::view_layout::Layout::default();
        let layout = self.view_layout_of(&key).unwrap_or(&empty);
        let views = (query == Query::ViewList).then(|| {
            layout
                .areas()
                .into_iter()
                .flat_map(|area| {
                    area.displays.iter().map(|display| View {
                        area_id: area.id.clone(),
                        view_id: display.id.clone(),
                        kind: match display.kind {
                            DisplayKind::File => "file",
                            DisplayKind::Diff => "diff",
                            DisplayKind::Browser => "browser",
                        },
                        target: display.url.as_ref().unwrap_or(&display.path).clone(),
                        selected: area.active.as_deref() == Some(display.id.as_str()),
                        active_area: layout.active_area().id == area.id,
                    })
                })
                .collect()
        });
        Ok(QueryResult {
            context,
            capabilities: vec!["workspace.info", "view.list"],
            views,
        })
    }
}
