//! Resolve is a durable UI decision, published only after the state save succeeds.
//! All projection work is in memory. The existing save worker owns disk I/O.
use super::*;
use crate::agent_state::{escalation, sessions};

impl Runtime {
    /// Uses the existing runtime clock; idle ticks compare one deadline.
    pub(super) fn tick_session_day(&mut self, at: u64) -> bool {
        if at < self.session_next_day_unix_ms {
            return false;
        }
        let next = self.session_day_zone.as_ref().ok().and_then(|zone| {
            jiff::Timestamp::from_millisecond(i64::try_from(at).ok()?)
                .ok()?
                .to_zoned(zone.clone())
                .tomorrow()
                .ok()?
                .start_of_day()
                .ok()?
                .timestamp()
                .as_millisecond()
                .try_into()
                .ok()
        });
        let Some(next) = next else {
            self.session_next_day_unix_ms = u64::MAX;
            self.push_diagnostic(
                "session.resolve.date_unavailable",
                "Local calendar rollover could not be determined".to_owned(),
            );
            return false;
        };
        self.session_next_day_unix_ms = next;
        self.project_session_state() | self.refresh_agent_scopes()
    }
    fn session_date(&self, at: u64) -> Option<String> {
        let zone = self.session_day_zone.as_ref().ok()?;
        Some(
            jiff::Timestamp::from_millisecond(i64::try_from(at).ok()?)
                .ok()?
                .to_zoned(zone.clone())
                .date()
                .to_string(),
        )
    }

    fn session_rows(&self) -> impl Iterator<Item = &SidebarAgentSnapshot> {
        self.snapshot.navigator.agents.iter().chain(
            self.snapshot
                .status
                .remote
                .iter()
                .filter_map(|remote| remote.session.as_ref())
                .flat_map(|session| &session.agents),
        )
    }

    pub(super) fn resolve_session(&mut self, pane: &str, source: sessions::ResolveSource) -> bool {
        let Some(row) = self.session_rows().find(|row| row.pane_id == pane) else {
            self.push_diagnostic(
                "session.resolve.unavailable",
                format!("Pane {pane} is no longer an agent"),
            );
            return false;
        };
        if self.snapshot.ui_state.resolved_sessions.contains_key(pane)
            || self.pending_session_resolutions.contains_key(pane)
        {
            return false;
        }
        let now = unix_milliseconds();
        let Some(local_date) = self.session_date(now) else {
            self.push_diagnostic(
                "session.resolve.date_unavailable",
                "Local calendar date could not be read".to_owned(),
            );
            return false;
        };
        let record = sessions::Resolution {
            at_unix_ms: now,
            source,
            local_date,
            session: row.lineage_session.clone(),
            activity: row.last_activity.clone(),
        };
        self.failed_session_resolutions.remove(pane);
        self.pending_session_resolutions
            .insert(pane.to_owned(), record);
        if let Err(message) = self.write_ui_state() {
            if let Some(record) = self.pending_session_resolutions.remove(pane) {
                self.failed_session_resolutions
                    .insert(pane.to_owned(), record);
            }
            self.push_diagnostic("session.resolve.save_failed", message);
        }
        // A managed runtime waits for the worker's actual save acknowledgement.
        // A standalone runtime has already acknowledged its synchronous write.
        self.sync_session_state();
        self.refresh_agent_scopes();
        self.snapshot.ui_state.resolved_sessions.contains_key(pane)
            || self.pending_session_resolutions.contains_key(pane)
    }

    pub(super) fn acknowledge_session_save(
        &mut self,
        saved: &UiStateSnapshot,
        succeeded: bool,
    ) -> bool {
        let mut changed = false;
        for (pane, record) in &saved.resolved_sessions {
            if self.pending_session_resolutions.get(pane) != Some(record) {
                continue;
            }
            self.pending_session_resolutions.remove(pane);
            if succeeded {
                self.snapshot
                    .ui_state
                    .resolved_sessions
                    .insert(pane.clone(), record.clone());
                changed = true;
            } else {
                self.failed_session_resolutions
                    .insert(pane.clone(), record.clone());
            }
        }
        if changed {
            self.project_session_state()
                | self.refresh_inactive_groups()
                | self.refresh_agent_scopes()
        } else {
            false
        }
    }

    pub(super) fn reopen_resolved_session(&mut self, pane: &str) -> bool {
        let changed = self
            .snapshot
            .ui_state
            .resolved_sessions
            .remove(pane)
            .is_some()
            | self.pending_session_resolutions.remove(pane).is_some();
        if changed {
            self.snapshot
                .ui_state
                .session_resolution_inputs
                .insert(pane.to_owned(), unix_milliseconds());
            self.failed_session_resolutions.remove(pane);
            self.persist_ui_state();
            self.project_session_state();
            self.refresh_inactive_groups();
            self.refresh_agent_scopes();
            if let Some(context) = &self.worker_context {
                context.notifier.notify();
            }
        }
        changed
    }

    pub(super) fn prune_resolved_sessions(&mut self, gone: impl Fn(&str) -> bool) {
        let before = (
            self.snapshot.ui_state.resolved_sessions.len(),
            self.snapshot.ui_state.session_resolution_inputs.len(),
        );
        self.snapshot
            .ui_state
            .resolved_sessions
            .retain(|pane, _| !gone(pane));
        self.pending_session_resolutions
            .retain(|pane, _| !gone(pane));
        self.failed_session_resolutions
            .retain(|pane, _| !gone(pane));
        self.snapshot
            .ui_state
            .session_resolution_inputs
            .retain(|pane, _| !gone(pane));
        if before
            != (
                self.snapshot.ui_state.resolved_sessions.len(),
                self.snapshot.ui_state.session_resolution_inputs.len(),
            )
        {
            self.persist_ui_state();
        }
    }

    pub(crate) fn publish_delivery_holds(
        &mut self,
        holds: BTreeMap<String, crate::delivery::doorbell::Hold>,
    ) {
        if self.delivery_holds == holds {
            return;
        }
        self.delivery_holds = holds;
        if (self.sync_session_state() | self.refresh_agent_scopes())
            && let Some(context) = &self.worker_context
        {
            context.notifier.notify();
        }
    }

    fn project_session_state(&mut self) -> bool {
        let today = self.session_date(unix_milliseconds());
        let mut rows = self.session_rows().cloned().collect::<Vec<_>>();
        let mut invalid = Vec::new();
        for row in &mut rows {
            row.escalation = escalation::of(
                row,
                self.delivery_ledger.as_ref().ok().map(AsRef::as_ref),
                &self.delivery_holds,
            );
            row.raised_children.clear();
            let record = self.snapshot.ui_state.resolved_sessions.get(&row.pane_id);
            let pending = self.pending_session_resolutions.get(&row.pane_id);
            if record.or(pending).is_some_and(|record| {
                record.session != row.lineage_session
                    || (row.activity == "working" && record.activity != row.last_activity)
            }) {
                invalid.push(row.pane_id.clone());
                row.resolved = None;
            } else {
                row.resolved = record.cloned();
            }
            row.resolved_today = row
                .resolved
                .as_ref()
                .is_some_and(|record| Some(&record.local_date) == today.as_ref());
            crate::agent_state::rederive(row);
        }
        let mut raised: HashMap<String, Vec<escalation::RaisedChild>> = HashMap::new();
        for row in &rows {
            if row.escalation.is_none() || row.resolved.is_some() {
                continue;
            }
            if let (Some(parent), Some(tag)) = (&row.lineage_parent_pane_id, row.state.session.tag)
            {
                raised
                    .entry(parent.clone())
                    .or_default()
                    .push(escalation::RaisedChild {
                        pane_id: row.pane_id.clone(),
                        title: row.identity_label.clone(),
                        tag,
                        reason: row.request.as_ref().and_then(|r| r.line.clone()),
                        since_unix_ms: row.escalation.as_ref().and_then(|e| e.since_unix_ms),
                    });
            }
        }
        for children in raised.values_mut() {
            children.sort_by_key(|child| {
                (
                    match child.tag {
                        sessions::Tag::Approval => 0,
                        sessions::Tag::Answer => 1,
                        _ => 2,
                    },
                    child.since_unix_ms,
                )
            });
        }
        for row in &mut rows {
            row.raised_children = raised.remove(&row.pane_id).unwrap_or_default();
            crate::agent_state::rederive(row);
        }
        let mut changed = false;
        for (row, next) in self
            .snapshot
            .navigator
            .agents
            .iter_mut()
            .chain(
                self.snapshot
                    .status
                    .remote
                    .iter_mut()
                    .filter_map(|remote| remote.session.as_mut())
                    .flat_map(|session| &mut session.agents),
            )
            .zip(rows)
        {
            if *row != next {
                *row = next;
                changed = true;
            }
        }
        if !invalid.is_empty() {
            for pane in invalid {
                self.snapshot.ui_state.resolved_sessions.remove(&pane);
                self.pending_session_resolutions.remove(&pane);
                self.snapshot
                    .ui_state
                    .session_resolution_inputs
                    .insert(pane, unix_milliseconds());
            }
            self.persist_ui_state();
        }
        changed
    }

    pub(super) fn sync_session_state(&mut self) -> bool {
        let mut changed = self.project_session_state();
        let automatic: Vec<_> = self
            .session_rows()
            .filter(|row| {
                row.resolved.is_none()
                    && row.escalation.is_none()
                    && sessions::auto_resolvable(
                        row,
                        self.snapshot
                            .ui_state
                            .session_resolution_inputs
                            .get(&row.pane_id)
                            .copied(),
                    )
                    && !self.pending_session_resolutions.contains_key(&row.pane_id)
                    && self
                        .failed_session_resolutions
                        .get(&row.pane_id)
                        .is_none_or(|record| {
                            record.session != row.lineage_session
                                || record.activity != row.last_activity
                        })
            })
            .map(|row| {
                (
                    row.pane_id.clone(),
                    row.lineage_session.clone(),
                    row.last_activity.clone(),
                )
            })
            .collect();
        // Queue one batch, rather than recursively deriving each row.
        let now = unix_milliseconds();
        if let Some(local_date) = self.session_date(now) {
            for (pane, session, activity) in &automatic {
                self.pending_session_resolutions.insert(
                    pane.clone(),
                    sessions::Resolution {
                        at_unix_ms: now,
                        source: sessions::ResolveSource::Automatic,
                        local_date: local_date.clone(),
                        session: session.clone(),
                        activity: activity.clone(),
                    },
                );
            }
            if !automatic.is_empty()
                && let Err(message) = self.write_ui_state()
            {
                for (pane, _, _) in automatic {
                    self.pending_session_resolutions.remove(&pane);
                }
                self.push_diagnostic("session.resolve.save_failed", message);
            }
            changed |= self.project_session_state();
        }
        changed
    }
}
