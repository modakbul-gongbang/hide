//! One record of stage times for each tab creation and pane operation Hide
//! starts (PRD instant-pane-topology D-14), written to the diagnostic log as
//! one `pane_op.timing` line when the operation's last stage lands or it ends
//! another way.
//!
//! The stages, each in milliseconds after the request:
//! - `herdr_ack`: Herdr answered the request.
//! - `drawn`: the snapshot that first showed the expected layout was read
//!   for the browser.
//! - `first_event` / `layout_event`: the first Herdr event, and the first
//!   layout event, naming the operation's tab or panes reached the
//!   coordinator.
//! - `applied`: Herdr's own layout showing the change was ingested.
//! - `sent`: the snapshot carrying that layout was read for the browser.
//! - `first_frame`: the first terminal frame of the pane the operation is
//!   about arrived after it was applied (the created pane, or the pane whose
//!   grid the change resized).
//!
//! Herdr's share is `layout_event - herdr_ack`; Hide's is the rest. The record
//! carries ids and times only, never terminal content or paths. It is bounded:
//! at most `OP_TIMING_LIMIT` open records, each closed `OP_TIMING_EXPIRY` after
//! its request at the latest.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(super) const OP_TIMING_LIMIT: usize = 64;
const OP_TIMING_EXPIRY: Duration = Duration::from_secs(10);
/// Herdr events kept for an operation that learns its tab or pane only from
/// the answer, when the event can arrive first.
const ARRIVAL_HISTORY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stage {
    HerdrAck,
    Drawn,
    FirstEvent,
    LayoutEvent,
    Applied,
    Sent,
    FirstFrame,
}

impl Stage {
    const ALL: [Stage; 7] = [
        Stage::HerdrAck,
        Stage::Drawn,
        Stage::FirstEvent,
        Stage::LayoutEvent,
        Stage::Applied,
        Stage::Sent,
        Stage::FirstFrame,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn field(self) -> &'static str {
        match self {
            Stage::HerdrAck => "herdr_ack_ms",
            Stage::Drawn => "drawn_ms",
            Stage::FirstEvent => "first_event_ms",
            Stage::LayoutEvent => "layout_event_ms",
            Stage::Applied => "applied_ms",
            Stage::Sent => "sent_ms",
            Stage::FirstFrame => "first_frame_ms",
        }
    }
}

/// A Herdr event as the coordinator received it: which tab or pane it
/// names, whether it carries a layout, and when it arrived.
#[derive(Clone, Debug)]
pub(crate) struct HerdrArrival {
    pub(crate) tab_id: Option<String>,
    pub(crate) pane_id: Option<String>,
    pub(crate) layout: bool,
    pub(crate) received_at: Instant,
}

#[derive(Debug)]
struct OpTiming {
    id: String,
    kind: &'static str,
    tab_id: Option<String>,
    pane_ids: Vec<String>,
    /// The pane whose first frame closes the record, and whether only a frame
    /// after `applied` counts (a resized grid) or any (a created pane).
    frame_pane: Option<(String, bool)>,
    requested_at: Instant,
    requested_at_unix_ms: u64,
    stages: [Option<Instant>; 7],
    /// The prediction is on screen but not yet read for the browser.
    drawn_pending: bool,
}

impl OpTiming {
    fn stamp(&mut self, stage: Stage, at: Instant) {
        let slot = &mut self.stages[stage.index()];
        if slot.is_none() {
            *slot = Some(at.max(self.requested_at));
        }
    }

    fn has(&self, stage: Stage) -> bool {
        self.stages[stage.index()].is_some()
    }

    fn names(&self, arrival: &HerdrArrival) -> bool {
        arrival
            .tab_id
            .as_deref()
            .is_some_and(|tab| self.tab_id.as_deref() == Some(tab))
            || arrival
                .pane_id
                .as_deref()
                .is_some_and(|pane| self.pane_ids.iter().any(|id| id == pane))
    }

    fn observe(&mut self, arrival: &HerdrArrival) {
        if arrival.received_at < self.requested_at || !self.names(arrival) {
            return;
        }
        self.stamp(Stage::FirstEvent, arrival.received_at);
        if arrival.layout {
            self.stamp(Stage::LayoutEvent, arrival.received_at);
        }
    }

    fn emit(&self, outcome: &str) {
        let mut line = serde_json::json!({
            "component": "pane_op",
            "kind": "pane_op.timing",
            "op_id": self.id,
            "op": self.kind,
            "tab_id": self.tab_id,
            "pane_ids": self.pane_ids,
            "outcome": outcome,
            "requested_at_unix_ms": self.requested_at_unix_ms,
        });
        let fields = line.as_object_mut().expect("a JSON object literal");
        for stage in Stage::ALL {
            if let Some(at) = self.stages[stage.index()] {
                let ms = at.duration_since(self.requested_at).as_secs_f64() * 1000.0;
                fields.insert(
                    stage.field().to_owned(),
                    serde_json::json!((ms * 10.0).round() / 10.0),
                );
            }
        }
        crate::diagnostic!(line);
    }
}

#[derive(Debug, Default)]
pub(super) struct OpTimings {
    open: VecDeque<OpTiming>,
    arrivals: VecDeque<HerdrArrival>,
}

impl OpTimings {
    /// Opens the record for operation `id`. At the cap the oldest open record
    /// is closed as `evicted`, so the log still says it happened.
    pub(super) fn begin(
        &mut self,
        id: &str,
        kind: &'static str,
        tab_id: Option<&str>,
        pane_ids: &[&str],
        now: Instant,
        now_unix_ms: u64,
    ) {
        if self.open.iter().any(|record| record.id == id) {
            return;
        }
        if self.open.len() >= OP_TIMING_LIMIT
            && let Some(evicted) = self.open.pop_front()
        {
            evicted.emit("evicted");
        }
        self.open.push_back(OpTiming {
            id: id.to_owned(),
            kind,
            tab_id: tab_id.map(str::to_owned),
            pane_ids: pane_ids.iter().map(|id| (*id).to_owned()).collect(),
            frame_pane: None,
            requested_at: now,
            requested_at_unix_ms: now_unix_ms,
            stages: [None; 7],
            drawn_pending: false,
        });
    }

    fn record(&mut self, id: &str) -> Option<&mut OpTiming> {
        self.open.iter_mut().find(|record| record.id == id)
    }

    pub(super) fn stamp(&mut self, id: &str, stage: Stage, at: Instant) {
        if let Some(record) = self.record(id) {
            record.stamp(stage, at);
        }
    }

    /// The expected layout of `id` is in the snapshot; the next read for the
    /// browser stamps `drawn`.
    pub(super) fn predicted(&mut self, id: &str) {
        if let Some(record) = self.record(id)
            && !record.has(Stage::Drawn)
        {
            record.drawn_pending = true;
        }
    }

    /// Names the tab and the pane the answer revealed, and reads the events
    /// already received for them.
    pub(super) fn learn(&mut self, id: &str, tab_id: Option<&str>, pane_id: Option<&str>) {
        let arrivals = std::mem::take(&mut self.arrivals);
        if let Some(record) = self.record(id) {
            if let Some(tab) = tab_id {
                record.tab_id = Some(tab.to_owned());
            }
            if let Some(pane) = pane_id
                && !record.pane_ids.iter().any(|id| id == pane)
            {
                record.pane_ids.push(pane.to_owned());
            }
            for arrival in &arrivals {
                record.observe(arrival);
            }
        }
        self.arrivals = arrivals;
    }

    /// Sets the pane whose first frame ends the record: any frame of a
    /// created pane, or for a resized grid only one after `applied`.
    pub(super) fn await_frame(&mut self, id: &str, pane_id: &str, after_applied: bool) {
        if let Some(record) = self.record(id) {
            record.frame_pane = Some((pane_id.to_owned(), after_applied));
        }
    }

    pub(super) fn observe_arrivals(&mut self, arrivals: &[HerdrArrival]) {
        for arrival in arrivals {
            for record in &mut self.open {
                record.observe(arrival);
            }
            if self.arrivals.len() >= ARRIVAL_HISTORY {
                self.arrivals.pop_front();
            }
            self.arrivals.push_back(arrival.clone());
        }
    }

    /// Herdr's session now lays out these tabs: a tab creation waiting for
    /// its tab's layout is applied.
    pub(super) fn applied_tabs<'a>(&mut self, tab_ids: impl Iterator<Item = &'a str>, at: Instant) {
        if !self.open.iter().any(|record| record.kind == "tab.create") {
            return;
        }
        let laid_out = tab_ids.collect::<std::collections::HashSet<_>>();
        for record in &mut self.open {
            if record.kind == "tab.create"
                && record
                    .tab_id
                    .as_deref()
                    .is_some_and(|tab| laid_out.contains(tab))
            {
                record.stamp(Stage::Applied, at);
            }
        }
    }

    /// A snapshot was read for the browser at `at`.
    pub(super) fn note_sent(&mut self, at: Instant) {
        let mut finished = Vec::new();
        for record in &mut self.open {
            if record.drawn_pending {
                record.drawn_pending = false;
                record.stamp(Stage::Drawn, at);
            }
            if record.has(Stage::Applied) && !record.has(Stage::Sent) {
                record.stamp(Stage::Sent, at);
                if record.frame_pane.is_none() || record.has(Stage::FirstFrame) {
                    finished.push(record.id.clone());
                }
            }
        }
        for id in finished {
            self.finish(&id, "completed");
        }
    }

    /// Whether a record waits for a frame of `pane_id`.
    pub(super) fn awaits_frame(&self, pane_id: &str) -> bool {
        self.open.iter().any(|record| {
            !record.has(Stage::FirstFrame)
                && record
                    .frame_pane
                    .as_ref()
                    .is_some_and(|(pane, _)| pane == pane_id)
        })
    }

    /// A terminal frame of `pane_id` was published at `at`.
    pub(super) fn note_frame(&mut self, pane_id: &str, at: Instant) {
        let finished = self
            .open
            .iter_mut()
            .filter(|record| match &record.frame_pane {
                Some((pane, after_applied)) => {
                    pane == pane_id
                        && !record.has(Stage::FirstFrame)
                        && (!after_applied || record.has(Stage::Applied))
                }
                None => false,
            })
            .map(|record| {
                record.stamp(Stage::FirstFrame, at);
                record.id.clone()
            })
            .collect::<Vec<_>>();
        for id in finished {
            if self
                .record(&id)
                .is_some_and(|record| record.has(Stage::Sent))
            {
                self.finish(&id, "completed");
            }
        }
    }

    /// Closes `id` with `outcome` (`completed`, `refused`, `failed`,
    /// `unknown`) and writes its line.
    pub(super) fn finish(&mut self, id: &str, outcome: &str) {
        if let Some(index) = self.open.iter().position(|record| record.id == id) {
            let record = self.open.remove(index).expect("index was just found");
            record.emit(outcome);
        }
    }

    /// Closes every record older than the expiry as `incomplete`.
    pub(super) fn expire(&mut self, now: Instant) {
        while let Some(record) = self.open.front() {
            if now.duration_since(record.requested_at) < OP_TIMING_EXPIRY {
                break;
            }
            let record = self.open.pop_front().expect("front was just read");
            record.emit("incomplete");
        }
    }

    #[cfg(test)]
    pub(super) fn open_ids(&self) -> Vec<String> {
        self.open.iter().map(|record| record.id.clone()).collect()
    }
}

/// The stage record id of a tab creation on the control lane.
pub(super) fn tab_create_op_id(admission_id: u64) -> String {
    format!("tab-create-{admission_id}")
}

/// The stage record id of a tab a task slot creates (a worktree, an agent
/// start, a Home tab, a pull request handed to a checkout).
pub(super) fn task_create_op_id(task_id: u64) -> String {
    format!("task-create-{task_id}")
}

/// The stage record id of a pane close.
pub(super) fn pane_close_op_id(close_key: &str) -> String {
    format!("pane-close-{close_key}")
}

impl super::Runtime {
    pub(crate) fn observe_herdr_arrivals(&mut self, arrivals: &[HerdrArrival]) {
        self.op_timings.observe_arrivals(arrivals);
    }

    pub(super) fn begin_op_timing(
        &mut self,
        id: &str,
        kind: &'static str,
        tab_id: Option<&str>,
        pane_ids: &[&str],
    ) {
        self.op_timings.begin(
            id,
            kind,
            tab_id,
            pane_ids,
            Instant::now(),
            super::unix_milliseconds(),
        );
    }

    /// A snapshot is being read for the browser.
    pub(super) fn note_snapshot_sent(&mut self) {
        self.op_timings.note_sent(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(tab: Option<&str>, pane: Option<&str>, layout: bool, at: Instant) -> HerdrArrival {
        HerdrArrival {
            tab_id: tab.map(str::to_owned),
            pane_id: pane.map(str::to_owned),
            layout,
            received_at: at,
        }
    }

    #[test]
    fn an_event_that_arrives_before_the_answer_counts_once_the_answer_names_the_tab() {
        let start = Instant::now();
        let mut timings = OpTimings::default();
        timings.begin("tab-create-1", "tab.create", None, &[], start, 0);
        timings.observe_arrivals(&[arrival(
            Some("w1:t2"),
            None,
            true,
            start + Duration::from_millis(9),
        )]);
        timings.stamp(
            "tab-create-1",
            Stage::HerdrAck,
            start + Duration::from_millis(12),
        );
        timings.learn("tab-create-1", Some("w1:t2"), Some("w1:p3"));
        let record = timings.record("tab-create-1").unwrap();
        assert_eq!(
            record.stages[Stage::LayoutEvent.index()],
            Some(start + Duration::from_millis(9))
        );
        assert_eq!(
            record.stages[Stage::FirstEvent.index()],
            Some(start + Duration::from_millis(9))
        );
    }

    #[test]
    fn a_record_ends_once_applied_sent_and_its_panes_frame_has_landed() {
        let start = Instant::now();
        let mut timings = OpTimings::default();
        timings.begin(
            "pane-op-1",
            "pane.split",
            Some("w1:t1"),
            &["w1:p1"],
            start,
            0,
        );
        timings.learn("pane-op-1", None, Some("w1:p2"));
        timings.await_frame("pane-op-1", "w1:p2", false);
        timings.note_frame("w1:p2", start + Duration::from_millis(60));
        assert_eq!(timings.open_ids().len(), 1, "the layout is not applied yet");
        timings.stamp(
            "pane-op-1",
            Stage::Applied,
            start + Duration::from_millis(90),
        );
        timings.note_sent(start + Duration::from_millis(91));
        assert!(timings.open_ids().is_empty());

        timings.begin(
            "pane-op-2",
            "pane.zoom",
            Some("w1:t1"),
            &["w1:p1"],
            start,
            0,
        );
        timings.await_frame("pane-op-2", "w1:p1", true);
        timings.note_frame("w1:p1", start + Duration::from_millis(5));
        timings.stamp(
            "pane-op-2",
            Stage::Applied,
            start + Duration::from_millis(40),
        );
        timings.note_sent(start + Duration::from_millis(41));
        assert_eq!(
            timings.open_ids(),
            vec!["pane-op-2".to_owned()],
            "a frame before the layout applied is not the resized grid"
        );
        timings.note_frame("w1:p1", start + Duration::from_millis(70));
        assert!(timings.open_ids().is_empty());
    }

    #[test]
    fn open_records_are_bounded_and_expire() {
        let start = Instant::now();
        let mut timings = OpTimings::default();
        for index in 0..=OP_TIMING_LIMIT {
            timings.begin(
                &format!("op-{index}"),
                "pane.zoom",
                Some("w1:t1"),
                &[],
                start,
                0,
            );
        }
        assert_eq!(timings.open_ids().len(), OP_TIMING_LIMIT);
        assert_eq!(timings.open_ids()[0], "op-1");
        timings.expire(start + OP_TIMING_EXPIRY);
        assert!(timings.open_ids().is_empty());
    }
}
