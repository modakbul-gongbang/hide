//! Pane-scoped queries and actions against the live Herdr projection.

use super::{AreaIntent, Runtime};
use crate::view_layout::DisplayKind;
use crate::workspace_control::{
    Action, ActionMaterial, ActionPreparation, ActionResult, ActionSource, BrowserRouteSource,
    Context, Query, QueryResult, Refusal, View,
};

const ACTION_RESULTS_KEPT: usize = 128;
const ACTION_RETRY_WINDOW_MS: u64 = 10 * 60 * 1000;

pub(super) struct ReportedBrowserPage {
    pub load: u64,
    pub loading: bool,
    pub failure: Option<String>,
}

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
    fn reveal_workspace_control(
        &mut self,
        context: &Context,
        key: &(String, String),
        request_id: &str,
    ) {
        if context.device_id == crate::workspace::LOCAL_DEVICE_ID {
            self.bring_device_forward(context.device_id.clone());
            self.focus_checkout(&context.workspace_id, &context.checkout_id);
        } else {
            self.request_remote_control(super::RemoteControlPayload {
                target_id: context.device_id.clone(),
                request_id: format!("{request_id}-reveal"),
                report_pane_focus_outcome: false,
                focus_device: true,
                request: super::RemoteControlRequest::FocusWorkspace {
                    workspace_id: context.workspace_id.clone(),
                    checkout_id: Some(context.checkout_id.clone()),
                },
            });
        }
        self.apply_area_intent_to(key, AreaIntent::Views);
    }

    pub fn browser_route_source(
        &self,
        device_id: &str,
        checkout_path: &str,
        view_id: &str,
        load: u64,
    ) -> Option<BrowserRouteSource> {
        let connected = if device_id == crate::workspace::LOCAL_DEVICE_ID {
            self.snapshot.status.herdr.state == "connected"
        } else {
            self.snapshot
                .status
                .remote
                .iter()
                .any(|remote| remote.target_id == device_id && remote.state == "connected")
        };
        if !connected {
            return None;
        }
        let registered = self.catalog_workspaces().into_iter().any(|workspace| {
            workspace.device_id == device_id
                && workspace
                    .checkouts
                    .iter()
                    .any(|checkout| checkout.path == checkout_path)
        });
        if !registered {
            return None;
        }
        let key = (device_id.to_owned(), checkout_path.to_owned());
        let display = self
            .view_layout_of(&key)?
            .areas()
            .into_iter()
            .flat_map(|area| area.displays.iter())
            .find(|display| {
                display.id == view_id
                    && display.kind == DisplayKind::Browser
                    && display.load == load
            })?;
        Some(BrowserRouteSource {
            device_id: device_id.to_owned(),
            checkout_path: checkout_path.to_owned(),
            url: display.url.clone()?,
            load,
        })
    }
    fn check_action_request(
        &mut self,
        device_id: &str,
        pane_id: &str,
        expected: &Context,
        request_id: &str,
        action: &Action,
    ) -> Result<(Context, Option<Result<ActionResult, Refusal>>), Refusal> {
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
            let cached = if record.context != context {
                Err(Refusal {
                    reason: "pane_changed",
                    next_action: "Reconnect the pane and run hide workspace info again",
                })
            } else if &record.action == action {
                record.result.clone()
            } else {
                Err(Refusal {
                    reason: "request_id_reused",
                    next_action: "Use a new request ID for a different action",
                })
            };
            return Ok((context, Some(cached)));
        }
        if self.workspace_actions.len() >= ACTION_RESULTS_KEPT {
            return Err(Refusal {
                reason: "request_capacity",
                next_action: "Wait for earlier requests to expire, then retry",
            });
        }
        Ok((context, None))
    }

    pub fn workspace_control_prepare_action(
        &mut self,
        device_id: &str,
        pane_id: &str,
        expected: &Context,
        request_id: &str,
        action: &Action,
    ) -> Result<ActionPreparation, Refusal> {
        let (context, cached) =
            self.check_action_request(device_id, pane_id, expected, request_id, action)?;
        if let Some(cached) = cached {
            return Ok(ActionPreparation::Cached(cached));
        }
        if let Action::OpenBrowser { url, .. } = action {
            if !crate::view_layout::browser_address(url) {
                return Err(Refusal {
                    reason: "invalid_address",
                    next_action: "Use an http, https, or checkout HTML address",
                });
            }
            if crate::view_layout::is_file_address(url) {
                let path = crate::workspace_control::local_file_path(url).ok_or(Refusal {
                    reason: "invalid_address",
                    next_action: "Use a local file URL with an absolute path",
                })?;
                let (root, channel) = self
                    .document_source(&context.workspace_id, &context.checkout_id)
                    .map_err(|_| Refusal {
                        reason: "host_unavailable",
                        next_action: "Reconnect the device and retry",
                    })?;
                return Ok(ActionPreparation::Read(ActionSource::browser(
                    root, channel, path,
                )));
            }
            return Ok(ActionPreparation::Ready);
        }
        let (path, file) = match action {
            Action::OpenFile { path, .. } => (path, true),
            Action::OpenDiff { path, .. } => (path, false),
            _ => return Ok(ActionPreparation::Ready),
        };
        if !std::path::Path::new(path).is_absolute() {
            return Err(Refusal {
                reason: "invalid_path",
                next_action: "Resolve the path from the caller's cwd and retry",
            });
        }
        let already_open = file
            && self.snapshot.editor.tabs.iter().any(|tab| {
                tab.workspace_id == context.workspace_id
                    && tab.checkout_id == context.checkout_id
                    && tab.path == *path
                    && tab.kind == crate::model::EditorTabKind::File
                    && tab.unavailable_reason.is_none()
            });
        let (root, channel) = self
            .document_source(&context.workspace_id, &context.checkout_id)
            .map_err(|_| Refusal {
                reason: "host_unavailable",
                next_action: "Reconnect the device and check file access consent, then retry",
            })?;
        let source = if file {
            ActionSource::file(root, channel, path.clone(), already_open)
        } else {
            ActionSource::diff(root, channel, path.clone())
        };
        Ok(ActionPreparation::Read(source))
    }

    pub fn workspace_control_action(
        &mut self,
        device_id: &str,
        pane_id: &str,
        expected: &Context,
        request_id: &str,
        action: Action,
        material: Result<Option<ActionMaterial>, Refusal>,
    ) -> Result<ActionResult, Refusal> {
        let (context, cached) =
            self.check_action_request(device_id, pane_id, expected, request_id, &action)?;
        if let Some(cached) = cached {
            return cached;
        }
        let result = match material {
            Ok(material) => {
                let key = (context.device_id.clone(), context.checkout_path.clone());
                self.reconcile_view_displays();
                self.apply_workspace_action(&key, &context, request_id, &action, material)
            }
            Err(refusal) => Err(refusal),
        };
        self.workspace_actions.push_back(RecordedAction {
            device_id: device_id.to_owned(),
            pane_id: pane_id.to_owned(),
            context,
            request_id: request_id.to_owned(),
            at: super::unix_milliseconds(),
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
        material: Option<ActionMaterial>,
    ) -> Result<ActionResult, Refusal> {
        if let Action::OpenBrowser { url, reveal } = action {
            if !crate::view_layout::browser_address(url) {
                return Err(Refusal {
                    reason: "invalid_address",
                    next_action: "Use an http, https, or checkout HTML address",
                });
            }
            if crate::view_layout::is_file_address(url) && material.is_none() {
                return Err(Refusal {
                    reason: "html_unavailable",
                    next_action: "Check the HTML file and retry",
                });
            }
            let load = self.next_browser_load();
            let view_id = self
                .change_view_layout(key, |layout, stamp| {
                    let existing = layout
                        .displays()
                        .find(|display| {
                            display.kind == DisplayKind::Browser
                                && display.url.as_deref() == Some(url)
                        })
                        .map(|display| display.id.clone());
                    if let Some(id) = existing {
                        layout.focus(&id, stamp)?;
                        if let Some(display) = layout.display_mut(&id) {
                            display.load = load;
                        }
                        return Ok((id, true));
                    }
                    let area_id = layout.active_area().id.clone();
                    let display = layout.new_browser_display(url, load);
                    let id = display.id.clone();
                    layout.insert(&area_id, display, stamp)?;
                    Ok((id, true))
                })
                .map_err(layout_refusal)?;
            let area_id = self
                .view_layout_of(key)
                .and_then(|layout| layout.area_of(&view_id))
                .map(|area| area.id.clone());
            if *reveal {
                self.reveal_workspace_control(context, key, request_id);
            }
            return Ok(ActionResult {
                context: context.clone(),
                request_id: request_id.to_owned(),
                changed: true,
                view_id,
                area_id,
            });
        }
        if let Action::OpenFile {
            path: _,
            beside,
            reveal,
        }
        | Action::OpenDiff {
            path: _,
            beside,
            reveal,
        } = action
        {
            let Some(material) = material else {
                return Err(Refusal {
                    reason: "read_missing",
                    next_action: "Retry the command with the same request ID",
                });
            };
            let path = &material.path;
            let is_file = matches!(action, Action::OpenFile { .. });
            if !self.admit_view_open(
                key,
                path,
                if is_file {
                    DisplayKind::File
                } else {
                    DisplayKind::Diff
                },
                if is_file { None } else { Some(false) },
                false,
                *beside,
            ) {
                return Err(Refusal {
                    reason: "view_limit",
                    next_action: "Close an unused View or area and retry",
                });
            }
            let before_generation = self
                .workspace_views
                .as_ref()
                .map(|store| store.generation)
                .unwrap_or_default();
            let tab_id = if is_file {
                let existing = self.snapshot.editor.tabs.iter().find(|tab| {
                    tab.workspace_id == context.workspace_id
                        && tab.checkout_id == context.checkout_id
                        && tab.kind == crate::model::EditorTabKind::File
                        && tab.path == *path
                        && tab.unavailable_reason.is_none()
                });
                if let Some(existing) = existing {
                    existing.id.clone()
                } else {
                    let Some((document, place)) = material.file else {
                        return Err(Refusal {
                            reason: "read_missing",
                            next_action: "Retry the command with the same request ID",
                        });
                    };
                    let tab_id =
                        self.new_file_tab_id(&context.workspace_id, &context.checkout_id, path);
                    self.insert_file_tab(
                        super::editor::PreparedFileTab::Read {
                            tab_id: tab_id.clone(),
                            document: Box::new(document),
                            place,
                        },
                        &context.workspace_id,
                        &context.checkout_id,
                        path,
                        false,
                    )
                    .ok_or(Refusal {
                        reason: "file_unavailable",
                        next_action: "Retry after refreshing the Workspace",
                    })?
                }
            } else {
                let tab_id =
                    Self::diff_tab_id(&context.workspace_id, &context.checkout_id, path, false);
                if !self.snapshot.editor.tabs.iter().any(|tab| tab.id == tab_id) {
                    self.insert_diff_tab(
                        &context.workspace_id,
                        &context.checkout_id,
                        path,
                        false,
                        false,
                    );
                }
                tab_id
            };
            self.place_document(key, &tab_id, false, *beside, None);
            let view_id = self
                .view_layout_of(key)
                .and_then(|layout| layout.active_area().active.clone())
                .ok_or(Refusal {
                    reason: "view_placement_failed",
                    next_action: "Run hide view list to inspect the Workspace and retry",
                })?;
            if !self.view_layout_of(key).is_some_and(|layout| {
                layout.display(&view_id).is_some_and(|display| {
                    display.shows(
                        path,
                        if is_file {
                            DisplayKind::File
                        } else {
                            DisplayKind::Diff
                        },
                        if is_file { None } else { Some(false) },
                    )
                })
            }) {
                return Err(Refusal {
                    reason: "view_placement_failed",
                    next_action: "Run hide view list to inspect the Workspace and retry",
                });
            }
            let area_id = self
                .view_layout_of(key)
                .map(|layout| layout.active_area().id.clone());
            if *reveal {
                self.reveal_workspace_control(context, key, request_id);
            }
            return Ok(ActionResult {
                context: context.clone(),
                request_id: request_id.to_owned(),
                changed: *reveal
                    || self
                        .workspace_views
                        .as_ref()
                        .is_some_and(|store| store.generation != before_generation),
                view_id,
                area_id,
            });
        }
        let (view_id, changed, area_id) = match action {
            Action::OpenFile { .. } | Action::OpenDiff { .. } => unreachable!("handled above"),
            Action::OpenBrowser { .. } => unreachable!("handled above"),
            Action::Select { view_id, reveal } => {
                let selected = self
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
                if *reveal {
                    self.reveal_workspace_control(context, key, request_id);
                }
                (view_id.clone(), selected || *reveal, area)
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
                        page: (display.kind == DisplayKind::Browser).then(|| {
                            let reported = self
                                .browser_pages
                                .get(&(
                                    context.device_id.clone(),
                                    context.checkout_path.clone(),
                                    display.id.clone(),
                                ))
                                .filter(|page| page.load == display.load);
                            crate::workspace_control::BrowserPage {
                                state: match reported {
                                    None => "pending",
                                    Some(page) if page.failure.is_some() => "failed",
                                    Some(page) if page.loading => "loading",
                                    Some(_) => "loaded",
                                },
                                failure: reported.and_then(|page| page.failure.clone()),
                            }
                        }),
                    })
                })
                .collect()
        });
        let mut capabilities = vec![
            "workspace.info",
            "view.list",
            "view.select",
            "view.split",
            "view.move",
            "view.close",
            "browser.open",
            "browser.status",
        ];
        if context.device_id == "local"
            || self
                .device_hosts
                .get(&context.device_id)
                .is_some_and(|host| matches!(host.phase, super::hosts::HostPhase::Ready { .. }))
        {
            capabilities.extend(["file.open", "diff.open"]);
        }
        Ok(QueryResult {
            context,
            capabilities,
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
