//! Pane-scoped queries and actions against the live Herdr projection.

use super::Runtime;
use crate::view_layout::DisplayKind;
use crate::workspace_control::{Action, ActionResult, Context, Query, QueryResult, Refusal, View};

const ACTION_RESULTS_KEPT: usize = 128;
const ACTION_RETRY_WINDOW_MS: u64 = 10 * 60 * 1000;

pub(super) struct RecordedAction {
    device_id: String,
    pane_id: String,
    context: Context,
    request_id: String,
    at: u64,
    action: Action,
    result: Result<ActionResult, Refusal>,
}

impl Runtime {
    pub fn workspace_control_action(
        &mut self,
        device_id: &str,
        pane_id: &str,
        expected: &Context,
        request_id: &str,
        action: Action,
    ) -> Result<ActionResult, Refusal> {
        // Membership is checked on every call, including a retry. A cached
        // answer must never keep a moved or closed pane authorized.
        let context = self
            .workspace_control_query(device_id, pane_id, Query::Info)?
            .context;
        if &context != expected {
            return Err(Refusal {
                reason: "pane_changed",
                next_action: "Reconnect the pane and run hide workspace info again",
            });
        }
        let now = super::unix_milliseconds();
        let issued = request_id
            .split_once('-')
            .and_then(|(issued, suffix)| {
                (!suffix.is_empty())
                    .then(|| issued.parse::<u64>().ok())
                    .flatten()
            })
            .ok_or(Refusal {
                reason: "invalid_request_id",
                next_action: "Run the hide command again with a new request ID",
            })?;
        if issued > now.saturating_add(60_000)
            || now.saturating_sub(issued) > ACTION_RETRY_WINDOW_MS
        {
            return Err(Refusal {
                reason: "request_expired",
                next_action: "Run hide view list and start a new request with a fresh ID",
            });
        }
        self.workspace_actions
            .retain(|record| now.saturating_sub(record.at) <= ACTION_RETRY_WINDOW_MS);
        if let Some(record) = self.workspace_actions.iter().find(|record| {
            record.device_id == device_id
                && record.pane_id == pane_id
                && record.request_id == request_id
        }) {
            return if record.context != context {
                Err(Refusal {
                    reason: "pane_changed",
                    next_action: "Reconnect the pane and run hide workspace info again",
                })
            } else if record.action == action {
                record.result.clone()
            } else {
                Err(Refusal {
                    reason: "request_id_reused",
                    next_action: "Use a new request ID for a different action",
                })
            };
        }
        if self.workspace_actions.len() >= ACTION_RESULTS_KEPT {
            return Err(Refusal {
                reason: "request_capacity",
                next_action: "Wait for earlier requests to expire, then retry",
            });
        }
        let key = (context.device_id.clone(), context.checkout_path.clone());
        self.reconcile_view_displays();
        let result = self.apply_workspace_action(&key, &context, request_id, &action);
        self.workspace_actions.push_back(RecordedAction {
            device_id: device_id.to_owned(),
            pane_id: pane_id.to_owned(),
            context,
            request_id: request_id.to_owned(),
            at: now,
            action,
            result: result.clone(),
        });
        result
    }

    fn apply_workspace_action(
        &mut self,
        key: &(String, String),
        context: &Context,
        request_id: &str,
        action: &Action,
    ) -> Result<ActionResult, Refusal> {
        let (view_id, changed, area_id) = match action {
            Action::Select { view_id } => {
                let changed = self
                    .change_view_layout(key, |layout, stamp| {
                        layout
                            .focus(view_id, stamp)
                            .map(|changed| (changed, changed))
                    })
                    .map_err(layout_refusal)?;
                let area = self
                    .view_layout_of(key)
                    .and_then(|layout| layout.area_of(view_id))
                    .map(|area| area.id.clone());
                (view_id.clone(), changed, area)
            }
            Action::Split {
                view_id,
                area_id,
                edge,
            } => {
                let area = self
                    .change_view_layout(key, |layout, stamp| {
                        layout
                            .split(view_id, area_id, *edge, stamp)
                            .map(|area| (area, true))
                    })
                    .map_err(layout_refusal)?;
                (view_id.clone(), true, Some(area))
            }
            Action::Move {
                view_id,
                area_id,
                index,
            } => {
                let changed = self
                    .change_view_layout(key, |layout, stamp| {
                        layout
                            .move_display(view_id, area_id, *index, stamp)
                            .map(|changed| (changed, changed))
                    })
                    .map_err(layout_refusal)?;
                (view_id.clone(), changed, Some(area_id.clone()))
            }
            Action::Close { view_id } => {
                let Some(display) = self
                    .view_layout_of(key)
                    .and_then(|layout| layout.display(view_id))
                    .cloned()
                else {
                    return Ok(ActionResult {
                        context: context.clone(),
                        request_id: request_id.to_owned(),
                        changed: false,
                        view_id: view_id.clone(),
                        area_id: None,
                    });
                };
                let shared = self.view_layout_of(key).is_some_and(|layout| {
                    layout.displays().any(|other| {
                        other.id != *view_id
                            && match &display.tab_id {
                                Some(tab) => other.tab_id.as_ref() == Some(tab),
                                None => other.shows(&display.path, display.kind, display.committed),
                            }
                    })
                });
                if !shared
                    && display
                        .tab_id
                        .as_deref()
                        .is_some_and(|tab| self.document_kept(tab))
                {
                    return Err(Refusal {
                        reason: "unsaved_document",
                        next_action: "Save or discard the draft in Hide before closing its last View",
                    });
                }
                let area = self
                    .view_layout_of(key)
                    .and_then(|layout| layout.area_of(view_id))
                    .map(|area| area.id.clone());
                if !shared {
                    if let Some(tab) = display.tab_id.as_deref() {
                        self.close_file_tab_now(tab);
                        self.reconcile_view_displays();
                    } else {
                        self.change_view_layout(key, |layout, _| {
                            Ok(((), layout.remove(view_id).is_some()))
                        })
                        .map_err(layout_refusal)?;
                        if display.kind == DisplayKind::File {
                            self.cancel_document_read(
                                &context.workspace_id,
                                &context.checkout_id,
                                &display.path,
                            );
                        }
                    }
                } else {
                    self.change_view_layout(key, |layout, _| {
                        Ok(((), layout.remove(view_id).is_some()))
                    })
                    .map_err(layout_refusal)?;
                }
                (view_id.clone(), true, area)
            }
        };
        Ok(ActionResult {
            context: context.clone(),
            request_id: request_id.to_owned(),
            changed,
            view_id,
            area_id,
        })
    }

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
            capabilities: vec![
                "workspace.info",
                "view.list",
                "view.select",
                "view.split",
                "view.move",
                "view.close",
            ],
            views,
        })
    }
}

fn layout_refusal(error: crate::view_layout::LayoutError) -> Refusal {
    Refusal {
        reason: error.kind(),
        next_action: "Run hide view list, choose an available View or area, and retry",
    }
}
