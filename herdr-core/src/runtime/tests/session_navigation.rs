use super::*;

#[test]
fn local_tab_creation_acknowledgement_preserves_the_created_pane_focus() {
    let mut runtime = runtime();
    runtime.snapshot.terminal.pane_id = Some("w1:p1".to_owned());
    runtime.snapshot.focused.pane_id = Some("w1:p1".to_owned());
    runtime.snapshot.ui_state.selected_pane_id = Some("w1:p1".to_owned());

    assert!(runtime.ingest_local_control_result(
        RemoteControlAction::CreateTab {
            workspace_id: "w1".to_owned(),
            cwd: "/tmp/project".to_owned(),
            label: "2".to_owned(),
        },
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("w1:t2".to_owned()),
            created_pane_id: Some("w1:p2".to_owned()),
        }),
        4,
    ));

    assert_eq!(runtime.snapshot.terminal.pane_id.as_deref(), Some("w1:p2"));
    assert_eq!(runtime.snapshot.focused.pane_id.as_deref(), Some("w1:p2"));
    assert_eq!(
        runtime.snapshot.ui_state.selected_pane_id.as_deref(),
        Some("w1:p2")
    );
}

// A refused `pane.move` used to drop its request stamp, so the next
// session update re-sent it: one refusal per tick for as long as the
// tab stayed zoomed (2026-09-10 audit, ~2 refusals per second).
#[test]
fn a_refused_relocation_keeps_its_stamp_and_a_completed_one_drops_it() {
    let mut runtime = runtime();
    let action = || PaneControlAction::MoveToNewTab {
        pane_id: "w1:p2".to_owned(),
        workspace_id: "w1".to_owned(),
        label: "Child".to_owned(),
    };
    let asked_at = unix_milliseconds();
    runtime
        .pane_relocations_in_flight
        .insert("w1:p2".to_owned(), asked_at);

    runtime.ingest_pane_control_result(
        action(),
        Err("Herdr declined the move: ZoomedTab".to_owned()),
        3,
    );
    assert_eq!(
        runtime.pane_relocations_in_flight.get("w1:p2"),
        Some(&asked_at),
        "a refusal waits out the retry interval"
    );

    runtime.ingest_pane_control_result(
        action(),
        Ok(live::PaneControlOutcome::Acknowledged {
            created_pane_id: None,
        }),
        3,
    );
    assert!(
        !runtime.pane_relocations_in_flight.contains_key("w1:p2"),
        "a completed move is no longer in flight"
    );
}

/// R11/AC16: the chords move one pane's scale within bounds and reset it,
/// and the store keeps only the panes the user actually changed.
#[test]
fn pane_text_scale_steps_within_bounds_and_leaves_other_panes_alone() {
    let mut runtime = runtime();
    let scale = |pane: &str, direction: &str| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "pane_text_scale",
            "payload": {"pane_id": pane, "direction": direction}
        }))
        .expect("pane text scale event")
    };

    assert!(runtime.dispatch_json(&scale("w1:p1", "in")));
    assert_eq!(
        runtime.snapshot().ui_state.pane_text_scales.get("w1:p1"),
        Some(&1.1)
    );
    // A second pane is untouched by the first pane's zoom.
    assert!(
        !runtime
            .snapshot()
            .ui_state
            .pane_text_scales
            .contains_key("w1:p2")
    );

    // The upper bound holds however many times it is pressed, and a press
    // that changes nothing reports no change.
    for _ in 0..40 {
        runtime.dispatch_json(&scale("w1:p1", "in"));
    }
    assert_eq!(
        runtime.snapshot().ui_state.pane_text_scales.get("w1:p1"),
        Some(&MAX_PANE_TEXT_SCALE)
    );
    assert!(!runtime.dispatch_json(&scale("w1:p1", "in")));

    for _ in 0..40 {
        runtime.dispatch_json(&scale("w1:p1", "out"));
    }
    assert_eq!(
        runtime.snapshot().ui_state.pane_text_scales.get("w1:p1"),
        Some(&MIN_PANE_TEXT_SCALE)
    );

    // Reset drops the row rather than storing the default.
    assert!(runtime.dispatch_json(&scale("w1:p1", "reset")));
    assert!(runtime.snapshot().ui_state.pane_text_scales.is_empty());
    assert!(!runtime.dispatch_json(&scale("w1:p1", "reset")));

    // An unknown direction is surfaced, not silently ignored.
    assert!(runtime.dispatch_json(&scale("w1:p1", "sideways")));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.clone()),
        Some("pane.text_scale_unknown_direction".to_owned())
    );
}

#[test]
fn ui_state_update_applies_workspace_expansion_without_waiting_for_sync() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![workspace(
        "workspace-a",
        "A",
        "/tmp/hide-runtime-a",
        Vec::new(),
    )];
    let collapse = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {
            "expanded_paths": [],
            "collapsed_workspace_ids": ["workspace-a"],
            "collapsed_checkout_ids": ["checkout-a"],
            "selected_path": null,
            "selected_pane_id": null,
            "shortcut_bindings": {}
        }
    }))
    .expect("collapse workspace event");

    assert!(runtime.dispatch_json(&collapse));
    assert!(!runtime.snapshot().navigator.workspaces[0].expanded);
    assert_eq!(
        runtime.snapshot().ui_state.collapsed_checkout_ids,
        ["checkout-a"]
    );

    let expand = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {
            "expanded_paths": [],
            "collapsed_workspace_ids": [],
            "selected_path": null,
            "selected_pane_id": null,
            "shortcut_bindings": {}
        }
    }))
    .expect("expand workspace event");

    assert!(runtime.dispatch_json(&expand));
    assert!(runtime.snapshot().navigator.workspaces[0].expanded);
    // An unrelated UI save must not reopen a collapsed checkout.
    assert_eq!(
        runtime.snapshot().ui_state.collapsed_checkout_ids,
        ["checkout-a"]
    );
    let reopen = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {
            "expanded_paths": [],
            "collapsed_checkout_ids": [],
            "selected_path": null,
            "selected_pane_id": null
        }
    }))
    .unwrap();
    assert!(runtime.dispatch_json(&reopen));
    assert!(
        runtime
            .snapshot()
            .ui_state
            .collapsed_checkout_ids
            .is_empty()
    );
}

#[test]
fn focusing_checkout_selects_only_the_target_checkout_pane() {
    let mut runtime = runtime();
    let workspace_a = workspace(
        "workspace-a",
        "A",
        "/tmp/hide-runtime-a",
        vec![checkout(
            "workspace-a",
            "checkout-a",
            "/tmp/hide-runtime-a",
            Some(pane("pane-a", "/tmp/hide-runtime-a")),
        )],
    );
    let workspace_b = workspace(
        "workspace-b",
        "B",
        "/tmp/hide-runtime-b",
        vec![
            checkout(
                "workspace-b",
                "checkout-b",
                "/tmp/hide-runtime-b",
                Some(pane("pane-b", "/tmp/hide-runtime-b")),
            ),
            checkout(
                "workspace-b",
                "checkout-empty",
                "/tmp/hide-runtime-empty",
                None,
            ),
        ],
    );
    runtime.snapshot.navigator.workspaces = vec![workspace_a, workspace_b];
    runtime.snapshot.navigator.focused_workspace_id = Some("workspace-a".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some("checkout-a".to_owned());
    runtime.snapshot.navigator.root_path = Some("/tmp/hide-runtime-a".to_owned());
    runtime.snapshot.terminal.pane_id = Some("pane-a".to_owned());
    runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
        pane_id: "pane-a".to_owned(),
        closed: false,
        exit_code: None,
        ..TerminalPaneSnapshot::default()
    }];
    runtime.snapshot.ui_state.selected_pane_id = Some("pane-a".to_owned());

    let focus_b = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_checkout",
        "payload": {"workspace_id": "workspace-b", "checkout_id": "checkout-b"}
    }))
    .expect("focus B event");
    assert!(runtime.dispatch_json(&focus_b));
    assert_eq!(
        runtime.snapshot().navigator.focused_workspace_id.as_deref(),
        Some("workspace-b")
    );
    assert_eq!(
        runtime.snapshot().navigator.focused_checkout_id.as_deref(),
        Some("checkout-b")
    );
    assert_eq!(
        runtime.snapshot().navigator.root_path.as_deref(),
        Some("/tmp/hide-runtime-b")
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("pane-b")
    );
    assert_eq!(
        runtime.snapshot().ui_state.selected_pane_id.as_deref(),
        Some("pane-b")
    );
    assert!(runtime.snapshot().terminal.panes.is_empty());

    let focus_empty = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_checkout",
        "payload": {"workspace_id": "workspace-b", "checkout_id": "checkout-empty"}
    }))
    .expect("focus pane-less checkout event");
    assert!(runtime.dispatch_json(&focus_empty));
    assert_eq!(
        runtime.snapshot().navigator.root_path.as_deref(),
        Some("/tmp/hide-runtime-empty")
    );
    assert!(runtime.snapshot().terminal.pane_id.is_none());
    assert!(runtime.snapshot().active_pane_layout().is_none());
    assert!(runtime.snapshot().terminal.panes.is_empty());
}

/// Every tab in the session ships its own layout, keyed by its tab id.
/// Before this the snapshot carried one layout, so a tab the operator was
/// not looking at had no geometry to draw and switching to it had to wait
/// for Herdr to send one.
#[test]
fn tab_layouts_carry_every_tab_in_the_session() {
    let checkout_path = "/private/tmp/hide-tab-layouts-all";
    let (mut runtime, _checkout_id) = tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));

    assert_eq!(
        runtime
            .snapshot()
            .pane_layouts
            .iter()
            .map(|layout| layout.tab_id.clone())
            .collect::<Vec<_>>(),
        tabs.map(str::to_owned).to_vec()
    );
    // Each entry is that tab's own geometry rather than a copy of the
    // visible one.
    for tab_id in tabs {
        let layout = runtime
            .snapshot()
            .pane_layouts
            .iter()
            .find(|layout| layout.tab_id == tab_id)
            .expect("every tab has a layout")
            .clone();
        assert_eq!(layout.focused_pane_id, format!("{tab_id}:p"));
    }
}

/// Switching tabs empties nothing. The tab being selected already has its
/// geometry, so the canvas draws it on the same dispatch instead of
/// showing an empty canvas until Herdr confirms the focus.
#[test]
fn tab_layouts_survive_a_tab_switch_with_no_empty_canvas() {
    let checkout_path = "/private/tmp/hide-tab-layouts-switch";
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| layout.tab_id.clone()),
        Some("w-order:t1".to_owned())
    );

    let focus = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "focus_tab",
        "payload": {
            "workspace_id": "workspace:order",
            "checkout_id": checkout_id,
            "tab_id": "w-order:t3"
        }
    }))
    .expect("focus tab event");
    assert!(runtime.dispatch_json(&focus));

    // Every tab still has its layout, and the canvas is already drawing
    // the one that was asked for.
    assert_eq!(
        runtime
            .snapshot()
            .pane_layouts
            .iter()
            .map(|layout| layout.tab_id.clone())
            .collect::<Vec<_>>(),
        tabs.map(str::to_owned).to_vec()
    );
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| layout.tab_id.clone()),
        Some("w-order:t3".to_owned())
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w-order:t3:p")
    );
}

/// PRD B24. A relationship Open is completed only by the outcome carrying
/// its request id. An unrelated app-wide error and the already projected
/// layout cannot answer it while Herdr still reports the prior focus.
#[test]
fn pane_focus_request_waits_for_its_matching_authoritative_confirmation() {
    let checkout_path = "/private/tmp/hide-pane-focus-request";
    let (mut runtime, _) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));

    assert!(runtime.dispatch_json(&correlated_pane_focus_event(
        "w-order:t2:p",
        "relationship-1",
    )));
    let request = runtime
        .snapshot()
        .status
        .pane_focus_request
        .as_ref()
        .expect("the request is projected");
    assert_eq!(request.phase, "pending");
    assert_eq!(request.request_id, "relationship-1");

    runtime.set_error("pane.focus_failed", "an unrelated pane failed", true);
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .pane_focus_request
            .as_ref()
            .map(|request| request.phase.as_str()),
        Some("pending"),
        "neither global last_error nor the prior layout is this request's answer"
    );

    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    assert_eq!(
        runtime
            .snapshot()
            .status
            .pane_focus_request
            .as_ref()
            .map(|request| request.phase.as_str()),
        Some("succeeded")
    );
}

/// PRD B24, engineering rule 11. Replaying one request id produces no
/// second pane-control effect. A retry receives a new id only after the
/// core-owned timeout has ended the first wait.
#[test]
fn pane_focus_request_blocks_duplicates_times_out_and_accepts_a_retry() {
    let checkout_path = "/private/tmp/hide-pane-focus-timeout";
    let (mut runtime, _) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    let request = correlated_pane_focus_event("w-order:t2:p", "relationship-1");
    assert!(runtime.dispatch_json(&request));
    let requested_at = runtime
        .pending_pane_focus
        .as_ref()
        .expect("one pane focus is in flight")
        .requested_at_unix_ms;

    assert!(runtime.dispatch_json(&request));
    assert_eq!(
        runtime
            .pending_pane_focus
            .as_ref()
            .map(|pending| pending.requested_at_unix_ms),
        Some(requested_at),
        "the duplicate did not replace or repeat the request"
    );
    assert_eq!(
        diagnostic_count(&runtime, "pane.focus.duplicate_ignored"),
        1
    );

    assert!(runtime.expire_pending_view_focus(requested_at + VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS));
    let timed_out = runtime
        .snapshot()
        .status
        .pane_focus_request
        .as_ref()
        .expect("the timeout is projected");
    assert_eq!(timed_out.phase, "failed");
    assert!(timed_out.retryable);
    assert!(
        timed_out
            .message
            .as_deref()
            .is_some_and(|message| message.contains("did not confirm"))
    );

    assert!(runtime.dispatch_json(&correlated_pane_focus_event(
        "w-order:t2:p",
        "relationship-2",
    )));
    let retry = runtime
        .snapshot()
        .status
        .pane_focus_request
        .as_ref()
        .expect("the retry replaces the settled receipt");
    assert_eq!(retry.request_id, "relationship-2");
    assert_eq!(retry.phase, "pending");
}

#[test]
fn pane_focus_refusal_and_target_retirement_end_the_matching_request() {
    let checkout_path = "/private/tmp/hide-pane-focus-retirement";
    let (mut runtime, _) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&correlated_pane_focus_event(
        "w-order:t2:p",
        "relationship-refused",
    )));
    runtime.ingest_pane_control_result(
        PaneControlAction::Focus {
            pane_id: "w-order:t2:p".to_owned(),
        },
        Err("focus refused".to_owned()),
        8,
    );
    let refusal = runtime
        .snapshot()
        .status
        .pane_focus_request
        .as_ref()
        .expect("the refusal is projected");
    assert_eq!(refusal.phase, "failed");
    assert_eq!(refusal.message.as_deref(), Some("focus refused"));
    assert!(refusal.retryable);

    assert!(runtime.dispatch_json(&correlated_pane_focus_event(
        "w-order:t2:p",
        "relationship-retired",
    )));
    let remaining = ["w-order:t1"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &remaining,
        &remaining,
        "w-order:t1",
    )));
    let retirement = runtime
        .snapshot()
        .status
        .pane_focus_request
        .as_ref()
        .expect("retirement is projected");
    assert_eq!(retirement.request_id, "relationship-retired");
    assert_eq!(retirement.phase, "failed");
    assert!(retirement.retryable);
    assert!(
        retirement
            .message
            .as_deref()
            .is_some_and(|message| message.contains("retired"))
    );
}

/// AC1, SC1. The strip's active mark and the canvas are the same field, so
/// asking for a tab moves both on the dispatch that asked, without waiting
/// for Herdr to answer.
#[test]
fn view_authority_a_tab_switch_moves_the_strip_and_the_canvas_in_one_snapshot() {
    let checkout_path = "/private/tmp/hide-view-authority-switch";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));

    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

    let snapshot = runtime.snapshot();
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3"),
        "the strip's active mark moves on the frame the operator asked for"
    );
    assert_eq!(snapshot.tab.id.as_deref(), Some("w-order:t3"));
    assert_eq!(
        snapshot
            .active_pane_layout()
            .map(|layout| layout.tab_id.as_str()),
        Some("w-order:t3"),
        "the canvas is drawing the requested tab in the same snapshot"
    );
}

/// AC1, SC1. Herdr's next session update still names the tab the operator
/// left, because the notification has not landed. That is the answer to a
/// question already asked, not a new focus, so it must not pull the canvas
/// back.
#[test]
fn view_authority_a_stale_herdr_tab_does_not_undo_an_unconfirmed_switch() {
    let checkout_path = "/private/tmp/hide-view-authority-stale-tab";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));

    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3")
    );
    assert_eq!(
        diagnostic_count(&runtime, "tab.focus.followed"),
        0,
        "an unconfirmed notification is not an external focus"
    );

    // The confirmation ends the wait, and the value is unchanged by it.
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t3",
    )));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3")
    );
    assert!(runtime.pending_tab_focus.is_none());
}

/// AC1, R1. With nothing in flight, a Herdr session naming another tab is
/// somebody focusing that tab outside Hide. Hide follows it and says so.
#[test]
fn view_authority_an_external_tab_focus_is_followed_and_reported() {
    let checkout_path = "/private/tmp/hide-view-authority-external-tab";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.pending_tab_focus.is_none());

    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2"
    ))));

    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t2")
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 1);
    assert_eq!(
        runtime.snapshot().tab.id.as_deref(),
        Some("w-order:t2"),
        "the canvas follows the tab, not only the strip"
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w-order:t2:p"),
        "and the keyboard lands in that tab rather than staying on a pane nobody can see"
    );
}

/// AC1, SC1. Registering a device rebuilds the catalog from scratch, and a
/// freshly built checkout names no active tab. The visible tab is Hide's,
/// so it survives a rebuild Herdr had no part in.
#[test]
fn view_authority_a_catalog_rebuild_keeps_the_visible_tab() {
    let checkout_path = "/private/tmp/hide-view-authority-rebuild";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));

    let register_device = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "register_device",
        "payload": {
            "id": "device-rebuild",
            "label": "Rebuild",
            "ssh_alias": "rebuild-host"
        }
    }))
    .expect("register device event");
    assert!(runtime.dispatch_json(&register_device));

    // The rebuilt catalog carries no tabs until the next session update,
    // which is what makes this the interesting moment: the tab the
    // operator chose has to survive the gap. It survives because a
    // rebuild is not a reconcile - the catalog says nothing about which
    // tab is visible, so nothing on this path may read it as Herdr
    // naming another one.
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));

    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3"),
        "a rebuild is not Herdr moving the tab"
    );
    assert_eq!(
        runtime.snapshot().tab.id.as_deref(),
        Some("w-order:t3"),
        "and the canvas comes back on the tab it was showing"
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
}

/// AC2, R1. Herdr refusing the notification does not move the operator's
/// screen. The tab stays where they put it and the refusal is reported.
#[test]
fn view_authority_a_refused_tab_focus_keeps_the_tab_and_reports_it() {
    let checkout_path = "/private/tmp/hide-view-authority-refused-tab";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
    assert!(runtime.pending_tab_focus.is_some());

    runtime.ingest_local_control_result(
        RemoteControlAction::FocusTab {
            tab_id: "w-order:t3".to_owned(),
        },
        Err("tab.focus rejected".to_owned()),
        12,
    );

    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3"),
        "a refusal is reported, not acted on by moving the screen"
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.refused"), 1);
    assert!(runtime.pending_tab_focus.is_none());
}

/// AC2, R1. A notification Herdr never answers stops being pending, the
/// value Hide chose is kept, and the silence is reported rather than
/// waited on forever.
#[test]
fn view_authority_an_unanswered_notification_times_out_and_keeps_its_value() {
    let checkout_path = "/private/tmp/hide-view-authority-timeout";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t3")));
    let requested_at = runtime
        .pending_tab_focus
        .as_ref()
        .expect("a notification is in flight")
        .requested_at_unix_ms;

    assert!(!runtime.expire_pending_view_focus(requested_at + 1));
    assert!(runtime.pending_tab_focus.is_some());
    assert!(runtime.expire_pending_view_focus(requested_at + VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS));

    assert!(runtime.pending_tab_focus.is_none());
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t3"),
        "a silent Herdr is a reason to report, not to move the screen"
    );
    assert_eq!(diagnostic_count(&runtime, "view_focus.timed_out"), 1);
}

/// Two Herdr workspaces at one path share one checkout, and the second
/// one in payload order names a different active tab. A tab focus on the
/// first workspace is confirmed by that workspace showing the tab, even
/// while Herdr's keyboard stays in the second one.
#[test]
fn split_checkout_a_tab_focus_is_confirmed_by_the_workspace_that_owns_the_tab() {
    let checkout_path = "/private/tmp/hide-split-checkout-confirm";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let before: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
        ("wb", &["wb:t1"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &before, "wb")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wb:t1"),
        "with no tab of its own Hide shows the tab Herdr has focused"
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("wb:t1:p"),
        "and the keyboard is in that tab"
    );

    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "wa:t2")));
    assert!(runtime.pending_tab_focus.is_some());
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("wa:t2:p")
    );

    // Herdr shows the tab in its workspace; its keyboard stays in wb.
    let confirmed: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1", "wa:t2"], "wa:t2"),
        ("wb", &["wb:t1"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &confirmed, "wb")));

    assert!(
        runtime.pending_tab_focus.is_none(),
        "the notification is confirmed"
    );
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t2")
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
    assert_eq!(diagnostic_count(&runtime, "view_focus.timed_out"), 0);
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| layout.tab_id.as_str()),
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        "the tab drawn is the tab attached"
    );
}

/// The active tab of a workspace Herdr's keyboard is not in is that
/// workspace's memory, not a focus. It never moves the canvas; a move of
/// Herdr's keyboard into that workspace does.
#[test]
fn split_checkout_another_workspaces_active_tab_is_not_followed_until_herdr_focuses_it() {
    let checkout_path = "/private/tmp/hide-split-checkout-follow";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let start: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1"], "wa:t1"),
        ("wb", &["wb:t1", "wb:t2"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &start, "wa")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t1")
    );

    // wb remembers another tab; Herdr's keyboard is still in wa.
    let wb_moved: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1"], "wa:t1"),
        ("wb", &["wb:t1", "wb:t2"], "wb:t2"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &wb_moved, "wa")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t1"),
        "a non-focused workspace's active tab is not followed"
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);

    // Herdr's keyboard moves into wb: that is a focus, and it is followed.
    assert!(runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &wb_moved, "wb"))));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wb:t2")
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 1);
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("wb:t2:p"),
        "the keyboard follows the tab"
    );
}

/// A restore keeps the persisted pane and draws the tab that holds it,
/// not the tab Herdr's last workspace happens to name. The canvas drawn
/// is the canvas attached.
#[test]
fn split_checkout_a_restore_draws_the_tab_holding_the_persisted_pane() {
    let checkout_path = "/private/tmp/hide-split-checkout-restore";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    runtime.snapshot.terminal.pane_id = Some("wa:t2:p".to_owned());
    runtime.snapshot.focused.pane_id = Some("wa:t2:p".to_owned());
    runtime.snapshot.ui_state.selected_pane_id = Some("wa:t2:p".to_owned());
    let session: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
        ("wb", &["wb:t1"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wb")));

    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("wa:t2:p"),
        "the persisted pane survives the first session"
    );
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t2"),
        "the visible tab is the one holding that pane"
    );
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| layout.tab_id.as_str()),
        Some("wa:t2"),
        "and it is the layout attached"
    );
}

/// A timed-out notification keeps Hide's tab through the reconcile that
/// follows it: Herdr's focus has not moved, so there is nothing to
/// follow.
#[test]
fn split_checkout_an_expired_notification_keeps_its_tab_through_the_next_reconcile() {
    let checkout_path = "/private/tmp/hide-split-checkout-expiry";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let session: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
        ("wb", &["wb:t1"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wa")));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "wa:t2")));
    let requested_at = runtime
        .pending_tab_focus
        .as_ref()
        .expect("a notification is in flight")
        .requested_at_unix_ms;
    assert!(runtime.expire_pending_view_focus(requested_at + VIEW_FOCUS_NOTIFICATION_TIMEOUT_MS));
    assert!(runtime.pending_tab_focus.is_none());

    // Herdr never answered; its focus is where it was.
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wa")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t2"),
        "a silent Herdr is a reason to report, not to move the screen"
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
    assert_eq!(diagnostic_count(&runtime, "view_focus.timed_out"), 1);
}

/// A created tab is the visible tab from Herdr's acknowledgment, and the
/// `tab_focused` that follows confirms it rather than being followed.
#[test]
fn split_checkout_a_created_tab_is_visible_on_the_acknowledgment() {
    let checkout_path = "/private/tmp/hide-split-checkout-create";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let before: [(&str, &[&str], &str); 2] =
        [("wa", &["wa:t1"], "wa:t1"), ("wb", &["wb:t1"], "wb:t1")];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &before, "wa")));

    runtime.ingest_local_control_result(
        RemoteControlAction::CreateTab {
            workspace_id: "wa".to_owned(),
            cwd: checkout_path.to_owned(),
            label: "2".to_owned(),
        },
        Ok(RemoteControlOutcome::Acknowledged {
            created_tab_id: Some("wa:t2".to_owned()),
            created_pane_id: Some("wa:t2:p".to_owned()),
        }),
        5,
    );
    assert_eq!(
        runtime
            .visible_tab_ids
            .get(&checkout_id)
            .map(String::as_str),
        Some("wa:t2"),
        "the created tab is Hide's visible tab before Herdr lists it"
    );
    assert!(runtime.pending_tab_focus.is_some());
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("wa:t2:p")
    );

    let after: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1", "wa:t2"], "wa:t2"),
        ("wb", &["wb:t1"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &after, "wa")));
    assert!(runtime.pending_tab_focus.is_none());
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t2")
    );
    assert_eq!(diagnostic_count(&runtime, "tab.focus.followed"), 0);
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| layout.tab_id.as_str()),
        Some("wa:t2")
    );
}

/// Focusing a pane in a tab the checkout is not showing brings that tab
/// forward in the same dispatch, so the keyboard never lands on a pane
/// nobody can see.
#[test]
fn split_checkout_a_pane_focus_in_a_hidden_tab_brings_the_tab_forward() {
    let checkout_path = "/private/tmp/hide-split-checkout-align";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let session: [(&str, &[&str], &str); 2] = [
        ("wa", &["wa:t1", "wa:t2"], "wa:t1"),
        ("wb", &["wb:t1"], "wb:t1"),
    ];
    runtime.ingest_session(Ok(split_checkout_payload(checkout_path, &session, "wa")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wa:t1")
    );

    assert!(runtime.dispatch_json(&operator_focus_event("wb:t1:p")));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("wb:t1"),
        "the tab holding the focused pane is visible on the same dispatch"
    );
    assert_eq!(diagnostic_count(&runtime, "tab.visible_aligned"), 1);
    assert_eq!(
        runtime.snapshot().ui_state.selected_pane_id.as_deref(),
        Some("wb:t1:p"),
        "the persisted selection follows the ring"
    );
}

/// Closing the visible tab lands on the tab Herdr moved to. Three tabs;
/// the operator is on the third and closes it. The snapshot that follows
/// names the second tab active and no focused pane yet, and the keyboard
/// goes to that tab's pane, not to the checkout's first.
#[test]
fn closing_the_visible_tab_lands_on_the_tab_herdr_moved_to() {
    let checkout_path = "/private/tmp/hide-close-lands-on-herdr-tab";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t3",
    )));
    assert!(runtime.dispatch_json(&operator_focus_event("w-order:t3:p")));
    assert_eq!(
        runtime
            .visible_tab_ids
            .get(&checkout_id)
            .map(String::as_str),
        Some("w-order:t3")
    );

    let remaining = ["w-order:t1", "w-order:t2"];
    let mut after_close = tab_order_payload(checkout_path, &remaining, &remaining, "w-order:t2");
    after_close.focused_pane_id = None;
    runtime.ingest_session(Ok(after_close));

    assert_eq!(
        runtime.snapshot.ui_state.selected_pane_id.as_deref(),
        Some("w-order:t2:p"),
        "the keyboard follows Herdr to the tab it focused after the close"
    );
    assert_eq!(
        runtime
            .visible_tab_ids
            .get(&checkout_id)
            .map(String::as_str),
        Some("w-order:t2"),
        "the strip draws the tab Herdr moved to"
    );
}

/// AC1, AC7, SC3. The focus ring moves on the click, and the layout that
/// was already on its way carrying the old focus does not take it back.
/// The read record stays on the clicked pane through that arrival.
#[test]
fn view_authority_a_pane_click_moves_focus_and_a_stale_layout_does_not_undo_it() {
    let mut runtime = live_runtime();
    let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

    assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));
    assert_eq!(
        runtime.snapshot().focused.pane_id.as_deref(),
        Some("w1:p3"),
        "the ring moves on the click, not on Herdr's confirming event"
    );
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w1:p3")
    );

    // Herdr's in-flight frame still names the pane the tab came forward
    // with. Its geometry is taken; its focus is not.
    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

    assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p3"));
    assert_eq!(
        unread_panes(&runtime),
        vec!["w1:p1", "w1:p2"],
        "the clicked row stays read through the stale arrival"
    );
    assert_eq!(diagnostic_count(&runtime, "pane.focus.followed"), 0);
}

/// AC1, AC8, R1. With nothing in flight, a Herdr layout naming another
/// pane is a focus made outside Hide. Hide follows it and reports the
/// panes and where the change came from.
#[test]
fn view_authority_an_external_pane_focus_is_followed_and_reported() {
    let mut runtime = live_runtime();
    let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
    assert!(runtime.pending_pane_focus.is_none());

    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p2")));

    assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p2"));
    assert_eq!(diagnostic_count(&runtime, "pane.focus.followed"), 1);
}

/// AC2, R1. A refused pane focus leaves the keyboard where the operator
/// put it and reports the refusal.
#[test]
fn view_authority_a_refused_pane_focus_keeps_the_pane_and_reports_it() {
    let mut runtime = live_runtime();
    let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));
    assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));

    runtime.ingest_pane_control_result(
        PaneControlAction::Focus {
            pane_id: "w1:p3".to_owned(),
        },
        Err("pane.focus rejected".to_owned()),
        9,
    );

    assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p3"));
    assert_eq!(diagnostic_count(&runtime, "pane.focus.refused"), 1);
    assert!(runtime.pending_pane_focus.is_none());
}

/// Rule 11. Reaching for the pane that already has the keyboard converges
/// on the state it is already in and sends Herdr nothing a second time.
/// The look itself still counts, because clicking the pane you are on is
/// still looking at it.
#[test]
fn view_authority_repeating_a_focus_sends_no_second_notification() {
    let mut runtime = live_runtime();
    let panes = [("w1:p1", 6018_u64), ("w1:p2", 6019), ("w1:p3", 6020)];
    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p1")));

    assert!(runtime.dispatch_json(&operator_focus_event("w1:p3")));
    runtime.ingest_session(Ok(finished_tab_payload(&panes, "w1:p3")));
    assert!(runtime.pending_pane_focus.is_none());
    let notifications = diagnostic_count(&runtime, "pane.focus.requested");

    runtime.dispatch_json(&operator_focus_event("w1:p3"));

    assert_eq!(
        diagnostic_count(&runtime, "pane.focus.requested"),
        notifications,
        "no second request leaves for a pane Herdr has already focused"
    );
    assert_eq!(runtime.snapshot().focused.pane_id.as_deref(), Some("w1:p3"));
    assert_eq!(unread_panes(&runtime), vec!["w1:p1", "w1:p2"]);
}

#[test]
fn tab_order_follows_herdr_and_not_layout_arrival() {
    let checkout_path = "/private/tmp/hide-tab-order-arrival";
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    // Herdr reports t1, t2, t3; the layouts arrive in the order the tabs
    // were first drawn, which is the order the navigator used to take.
    let payload = tab_order_payload(
        checkout_path,
        &["w-order:t1", "w-order:t2", "w-order:t3"],
        &["w-order:t3", "w-order:t1", "w-order:t2"],
        "w-order:t1",
    );
    assert!(runtime.ingest_session(Ok(payload)));
    assert_eq!(
        ordered_tab_ids(&runtime, &checkout_id),
        vec![
            "w-order:t1".to_owned(),
            "w-order:t2".to_owned(),
            "w-order:t3".to_owned()
        ]
    );
}

#[test]
fn tab_order_follows_a_move_and_ignores_a_layout_redraw() {
    let checkout_path = "/private/tmp/hide-tab-order-moved";
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));

    // Herdr moved the second tab in front of the first.
    let moved = ["w-order:t2", "w-order:t1", "w-order:t3"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &moved,
        &tabs,
        "w-order:t1"
    ))));
    assert_eq!(
        ordered_tab_ids(&runtime, &checkout_id),
        moved.map(str::to_owned).to_vec()
    );

    // A layout redraw for an existing tab is detail, not order.
    assert!(!runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &moved,
        &["w-order:t3", "w-order:t2", "w-order:t1"],
        "w-order:t1"
    ))));
    assert_eq!(
        ordered_tab_ids(&runtime, &checkout_id),
        moved.map(str::to_owned).to_vec()
    );
}

#[test]
fn herdr_active_tab_names_the_projected_tab() {
    let checkout_path = "/private/tmp/hide-tab-order-active";
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2"
    ))));
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t2")
    );
    assert_eq!(
        runtime.snapshot().tab.id.as_deref(),
        Some("w-order:t2"),
        "the active tab projection follows the id Herdr named"
    );
}

#[test]
fn herdr_active_tab_unresolved_is_reported_not_replaced() {
    let checkout_path = "/private/tmp/hide-tab-order-unplaceable";
    let (mut runtime, checkout_id) = tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t9"
    ))));

    // Hide owns the visible tab, so a checkout that has tabs shows one of
    // them. What Herdr named is still reported, because a name that
    // matches no tab in the session is worth knowing about.
    assert_eq!(
        checkout_active_tab_id(&runtime, &checkout_id).as_deref(),
        Some("w-order:t1")
    );
    assert_eq!(
        runtime.snapshot().tab.id.as_deref(),
        Some("w-order:t1"),
        "a checkout with tabs never draws the empty-checkout state"
    );
    let unresolved = runtime
        .snapshot()
        .status
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == "tab.active_unresolved")
        .count();
    assert_eq!(unresolved, 1);

    // Session sync reconciles once a second; the same unresolved state
    // must not append a diagnostic on every tick.
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t9",
    )));
    let unresolved = runtime
        .snapshot()
        .status
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == "tab.active_unresolved")
        .count();
    assert_eq!(unresolved, 1);
}

#[test]
fn tab_strip_reorder_index_counts_positions_before_the_tab_leaves_the_list() {
    // Herdr inserts into the list it still holds and then takes the moved
    // tab out of its old place, so the index is where the tab that ends up
    // behind the moved one sits now. These three cases are the ones a live
    // 0.8.2 server was observed answering with exactly these orders.
    let current = ["t1", "t2", "t3"].map(str::to_owned);
    assert_eq!(
        herdr_insert_index(&current, &["t2", "t1", "t3"].map(str::to_owned), "t1"),
        Some(2)
    );
    assert_eq!(
        herdr_insert_index(&current, &["t2", "t3", "t1"].map(str::to_owned), "t1"),
        Some(3)
    );
    assert_eq!(
        herdr_insert_index(&current, &["t3", "t1", "t2"].map(str::to_owned), "t3"),
        Some(0)
    );
    // A file tab is not one of Herdr's, so there is no Herdr move to make.
    assert_eq!(
        herdr_insert_index(
            &current,
            &["t1", "t2", "t3"].map(str::to_owned),
            "file:notes"
        ),
        None
    );

    // A workspace split across two checkouts. The strip the operator drags
    // holds t1, t2 and t3; t5 is a sibling checkout's tab that Herdr still
    // counts. Dropping t1 at the end of that strip means "after t3", which
    // is one past the workspace's last position when t5 leads and t5's own
    // position when t5 trails. Reading the index off the strip alone gives
    // 3 in both cases, which puts the tab between t2 and t3.
    let leading = ["t5", "t1", "t2", "t3"].map(str::to_owned);
    assert_eq!(
        herdr_insert_index(&leading, &["t2", "t3", "t1"].map(str::to_owned), "t1"),
        Some(4)
    );
    let trailing = ["t1", "t2", "t3", "t5"].map(str::to_owned);
    assert_eq!(
        herdr_insert_index(&trailing, &["t2", "t3", "t1"].map(str::to_owned), "t1"),
        Some(3)
    );
    // A tab that keeps a successor in its own strip is placed in front of
    // it, wherever the workspace holds that successor.
    assert_eq!(
        herdr_insert_index(&leading, &["t2", "t1", "t3"].map(str::to_owned), "t1"),
        Some(3)
    );
}

#[test]
fn tab_strip_reorder_asks_herdr_and_lands_only_once_herdr_reports_the_order() {
    let (mut runtime, checkout_id, directory) = strip_checkout("herdr-move");
    let checkout_path = directory.to_string_lossy().into_owned();
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));
    open_file(&mut runtime, &checkout_id, &directory.join("notes.md"));
    let file_entry = strip_ids(&runtime, &checkout_id)[2].clone();
    let before = strip_ids(&runtime, &checkout_id);

    // A Unix socket path has a hard length limit and the checkout fixture
    // can sit deep, so the fixture server lives at a short one of its own.
    let socket_root = PathBuf::from("/tmp").join(format!(
        "herdr-core-tab-move-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&socket_root).expect("socket directory");
    let socket_path = socket_root.join("herdr.sock");
    let listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let (mut stream, _) = listener.accept().expect("accept tab.move");
        let mut line = String::new();
        BufReader::new(stream.try_clone().expect("clone stream"))
            .read_line(&mut line)
            .expect("read request");
        let request: serde_json::Value =
            serde_json::from_str(&line).expect("tab.move request JSON");
        writeln!(
            stream,
            "{}",
            serde_json::json!({
                "id": request["id"],
                "result": {
                    "type": "tab_list",
                    "tabs": [
                        {"tab_id": "w-order:t2"},
                        {"tab_id": "w-order:t1"}
                    ]
                }
            })
        )
        .expect("write tab_list response");
        request
    });
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
    });

    // Move the first Herdr tab behind the second. The file tab does not
    // move, so the arrangement differs from Herdr's only in Herdr's own
    // order, which is Herdr's to grant.
    assert!(reorder_tab(
        &mut runtime,
        &checkout_id,
        "herdr:w-order:t1",
        1
    ));
    let request = server.join().expect("fixture server joins");
    assert_eq!(request["method"], "tab.move");
    assert_eq!(
        request["params"],
        serde_json::json!({"tab_id": "w-order:t1", "insert_index": 2})
    );

    // The strip does not move on the operator's word alone.
    assert_eq!(strip_ids(&runtime, &checkout_id), before);
    assert_eq!(
        runtime.pending_tab_move[&checkout_id].herdr_order,
        vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()]
    );

    // Herdr reports the new order; the arrangement lands with it.
    let moved = ["w-order:t2", "w-order:t1"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &checkout_path,
        &moved,
        &tabs,
        "w-order:t1"
    ))));
    assert_eq!(
        strip_ids(&runtime, &checkout_id),
        vec![
            "herdr:w-order:t2".to_owned(),
            "herdr:w-order:t1".to_owned(),
            file_entry
        ]
    );
    assert!(runtime.pending_tab_move.is_empty());

    std::fs::remove_dir_all(&socket_root).ok();
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn tab_strip_reorder_indexes_a_move_in_the_whole_workspace_not_one_checkout() {
    let (mut runtime, checkout_id, repository, worktree) = split_workspace_checkouts("index-scope");
    let repository_path = repository.to_string_lossy().into_owned();
    let worktree_path = worktree.to_string_lossy().into_owned();
    // Herdr's list leads with the worktree's tab, so this checkout's tabs
    // do not start at the workspace's first position.
    let order = [
        ("w-order:t5", worktree_path.as_str()),
        ("w-order:t1", repository_path.as_str()),
        ("w-order:t2", repository_path.as_str()),
        ("w-order:t3", repository_path.as_str()),
    ];
    assert!(runtime.ingest_session(Ok(split_workspace_payload(&order, "w-order:t1"))));
    assert_eq!(
        strip_ids(&runtime, &checkout_id),
        vec![
            "herdr:w-order:t1".to_owned(),
            "herdr:w-order:t2".to_owned(),
            "herdr:w-order:t3".to_owned()
        ],
        "the repository's checkout holds only its own three tabs"
    );

    let socket_root = PathBuf::from("/tmp").join(format!(
        "herdr-core-split-move-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&socket_root).expect("socket directory");
    let socket_path = socket_root.join("herdr.sock");
    let listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let (mut stream, _) = listener.accept().expect("accept tab.move");
        let mut line = String::new();
        BufReader::new(stream.try_clone().expect("clone stream"))
            .read_line(&mut line)
            .expect("read request");
        let request: serde_json::Value =
            serde_json::from_str(&line).expect("tab.move request JSON");
        writeln!(
            stream,
            "{}",
            serde_json::json!({
                "id": request["id"],
                "result": {
                    "type": "tab_list",
                    "tabs": [
                        {"tab_id": "w-order:t5"},
                        {"tab_id": "w-order:t2"},
                        {"tab_id": "w-order:t3"},
                        {"tab_id": "w-order:t1"}
                    ]
                }
            })
        )
        .expect("write tab_list response");
        request
    });
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
    });

    // Drag the first tab to the end of this checkout's strip. In the
    // checkout's own coordinates that reads as index 3, which Herdr would
    // apply to its four-tab list and land the tab between t2 and t3.
    assert!(reorder_tab(
        &mut runtime,
        &checkout_id,
        "herdr:w-order:t1",
        2
    ));
    let request = server.join().expect("fixture server joins");
    assert_eq!(request["method"], "tab.move");
    assert_eq!(
        request["params"],
        serde_json::json!({"tab_id": "w-order:t1", "insert_index": 4})
    );
    assert!(
        runtime.snapshot().status.last_error.is_none(),
        "a move Herdr can make is not an error"
    );

    // Herdr reports the order the request asked for, so the arrangement
    // lands and the sibling checkout's tab is untouched.
    let moved = [
        ("w-order:t5", worktree_path.as_str()),
        ("w-order:t2", repository_path.as_str()),
        ("w-order:t3", repository_path.as_str()),
        ("w-order:t1", repository_path.as_str()),
    ];
    assert!(runtime.ingest_session(Ok(split_workspace_payload(&moved, "w-order:t1"))));
    assert_eq!(
        strip_ids(&runtime, &checkout_id),
        vec![
            "herdr:w-order:t2".to_owned(),
            "herdr:w-order:t3".to_owned(),
            "herdr:w-order:t1".to_owned()
        ]
    );
    assert!(runtime.pending_tab_move.is_empty());

    std::fs::remove_dir_all(&socket_root).ok();
    std::fs::remove_dir_all(repository.parent().expect("fixture root")).ok();
}

#[test]
fn tab_strip_reorder_keeps_herdrs_order_and_reports_when_a_move_is_refused() {
    let (mut runtime, checkout_id, directory) = strip_checkout("herdr-refused");
    let checkout_path = directory.to_string_lossy().into_owned();
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));
    let before = strip_ids(&runtime, &checkout_id);
    runtime.pending_tab_move.insert(
        checkout_id.clone(),
        PendingTabMove {
            desired: vec!["herdr:w-order:t2".to_owned(), "herdr:w-order:t1".to_owned()],
            workspace_id: "w-order".to_owned(),
            herdr_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
            generation: 7,
        },
    );

    assert!(runtime.ingest_local_control_result(
        RemoteControlAction::MoveTab {
            checkout_id: checkout_id.clone(),
            tab_id: "w-order:t1".to_owned(),
            insert_index: 2,
            expected_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
            generation: 7,
        },
        Err("tab.move failed: tab_not_found: tab w-order:t9 not found".to_owned()),
        4,
    ));
    assert_eq!(strip_ids(&runtime, &checkout_id), before);
    assert!(runtime.pending_tab_move.is_empty());
    let error = runtime
        .snapshot()
        .status
        .last_error
        .clone()
        .expect("a refused move is reported");
    assert_eq!(error.kind, "tab.move_refused");
    assert!(error.message.contains("w-order:t1"), "{}", error.message);
    assert!(error.retryable);

    std::fs::remove_dir_all(&directory).ok();
}

/// AC12, SC6 recovery. A refused move leaves the strip where Herdr last
/// put it and says so, and the operator's answer to that is to drag again.
/// The retry has to be an ordinary drag: the same request goes out, and
/// the order lands when Herdr reports it, with nothing left over from the
/// refusal to hold it back.
#[test]
fn tab_strip_reorder_a_refused_drag_can_simply_be_dragged_again() {
    let (mut runtime, checkout_id, directory) = strip_checkout("herdr-retry");
    let checkout_path = directory.to_string_lossy().into_owned();
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &checkout_path,
        &tabs,
        &tabs,
        "w-order:t1"
    ))));
    let before = strip_ids(&runtime, &checkout_id);

    let socket_root = PathBuf::from("/tmp").join(format!(
        "herdr-core-tab-retry-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&socket_root).expect("socket directory");
    let socket_path = socket_root.join("herdr.sock");
    let listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
    // Both drags are answered; what the answer says does not matter here,
    // because the worker's callback is what carries it and this test
    // drives that by hand.
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept tab.move");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone stream"))
                .read_line(&mut line)
                .expect("read request");
            let request: serde_json::Value =
                serde_json::from_str(&line).expect("tab.move request JSON");
            writeln!(
                stream,
                "{}",
                serde_json::json!({
                    "id": request["id"],
                    "result": {
                        "type": "tab_list",
                        "tabs": [{"tab_id": "w-order:t2"}, {"tab_id": "w-order:t1"}]
                    }
                })
            )
            .expect("write tab_list response");
            requests.push(request);
        }
        requests
    });
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
    });

    // The first drag is refused.
    assert!(reorder_tab(
        &mut runtime,
        &checkout_id,
        "herdr:w-order:t1",
        1
    ));
    let generation = runtime.pending_tab_move[&checkout_id].generation;
    assert!(runtime.ingest_local_control_result(
        RemoteControlAction::MoveTab {
            checkout_id: checkout_id.clone(),
            tab_id: "w-order:t1".to_owned(),
            insert_index: 2,
            expected_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
            generation,
        },
        Err("tab.move failed: tab_not_found: tab w-order:t1 not found".to_owned()),
        4,
    ));
    assert_eq!(strip_ids(&runtime, &checkout_id), before);
    assert!(runtime.pending_tab_move.is_empty());
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .expect("a refused move is reported")
            .kind,
        "tab.move_refused"
    );

    // The same drag again, with nothing else in between.
    assert!(reorder_tab(
        &mut runtime,
        &checkout_id,
        "herdr:w-order:t1",
        1
    ));
    let requests = server.join().expect("fixture server joins");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["method"], "tab.move");
    assert_eq!(
        requests[1]["params"], requests[0]["params"],
        "the retry asks Herdr for exactly what the refused drag asked for"
    );
    assert_eq!(
        runtime.pending_tab_move[&checkout_id].herdr_order,
        vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()]
    );

    // Herdr grants it this time, and the strip lands.
    let moved = ["w-order:t2", "w-order:t1"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &checkout_path,
        &moved,
        &tabs,
        "w-order:t1"
    ))));
    assert_eq!(
        strip_ids(&runtime, &checkout_id),
        vec!["herdr:w-order:t2".to_owned(), "herdr:w-order:t1".to_owned()]
    );
    assert!(runtime.pending_tab_move.is_empty());

    std::fs::remove_dir_all(&socket_root).ok();
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn tab_strip_reorder_ignores_a_result_a_later_drag_has_replaced() {
    let (mut runtime, checkout_id, directory) = strip_checkout("herdr-superseded");
    let tabs = ["w-order:t1", "w-order:t2"];
    assert!(runtime.ingest_session(Ok(tab_order_payload(
        &directory.to_string_lossy(),
        &tabs,
        &tabs,
        "w-order:t1"
    ))));
    let live = PendingTabMove {
        desired: vec!["herdr:w-order:t2".to_owned(), "herdr:w-order:t1".to_owned()],
        workspace_id: "w-order".to_owned(),
        herdr_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
        generation: 9,
    };
    runtime
        .pending_tab_move
        .insert(checkout_id.clone(), live.clone());

    // The first drag's refusal arrives after a second drag replaced it.
    assert!(runtime.ingest_local_control_result(
        RemoteControlAction::MoveTab {
            checkout_id: checkout_id.clone(),
            tab_id: "w-order:t1".to_owned(),
            insert_index: 2,
            expected_order: vec!["w-order:t2".to_owned(), "w-order:t1".to_owned()],
            generation: 8,
        },
        Err("tab.move failed: transport".to_owned()),
        4,
    ));
    assert_eq!(runtime.pending_tab_move[&checkout_id], live);
    assert!(runtime.snapshot().status.last_error.is_none());

    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn tab_label_turns_a_herdr_number_into_a_name_and_keeps_a_named_tab() {
    assert_eq!(crate::model::display_tab_label("2", "w1:t2"), "Tab 2");
    assert_eq!(crate::model::display_tab_label(" 2 ", "w1:t2"), "Tab 2");
    assert_eq!(crate::model::display_tab_label("notes", "w1:t2"), "notes");
    // A tab Herdr reports with no label at all falls back to its own id,
    // which is still its identity rather than its place in the strip.
    assert_eq!(crate::model::display_tab_label("", "w1:t2"), "w1:t2");
}

/// The shell used to name an unlabelled tab after its position, which
/// renamed every tab whenever one moved. Nothing may reintroduce that.
#[test]
fn tab_label_has_no_position_derived_path_left_in_the_shell() {
    let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&shell).expect("the shell source directory") {
        let path = entry.expect("a shell source entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("swift") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a readable Swift source");
        if source.contains("fallbackIndex") || source.contains("displayLabel") {
            offenders.push(path.display().to_string());
        }
        // The same class of defect, read the other way: the shell taking a
        // label the core formatted and parsing the number back out of it.
        // That writes the "Tab N" convention down a second time across the
        // FFI boundary, where a change to either half breaks the other in
        // silence. The core decides the next label; the shell draws it.
        if source.contains("hasPrefix(\"tab \")") || source.contains("hasPrefix(\"Tab \")") {
            offenders.push(path.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "a position-derived or reverse-parsed tab label path is back in {offenders:?}"
    );
}

#[test]
fn tab_label_names_the_next_tab_after_the_lowest_free_herdr_number() {
    let next = |labels: &[&str]| crate::model::next_tab_label(labels.iter().copied());
    assert_eq!(next(&[]), "Tab 1");
    assert_eq!(next(&["1", "2"]), "Tab 3");
    // The gap is taken before the end, so closing tab 1 and adding one
    // gives Tab 1 back rather than climbing forever.
    assert_eq!(next(&["2", "3"]), "Tab 1");
    // A named tab holds no number. A tab Herdr stores the display way
    // holds its number too: the shell hands `Tab N` to `tab.create`
    // verbatim, and counting only bare numbers made every new tab
    // `Tab 2` beside the last one.
    assert_eq!(next(&["1", "notes"]), "Tab 2");
    assert_eq!(next(&["Tab 1"]), "Tab 2");
    assert_eq!(next(&["1", "Tab 2", "Tab 2"]), "Tab 3");
    assert_eq!(next(&[" 2 ", "1"]), "Tab 3");
}

#[test]
fn a_plain_terminal_pane_cwd_is_reconciled_into_its_checkout() {
    let mut runtime = runtime();
    let checkout_path = "/private/tmp/hide-registered-checkout";
    runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
        id: "workspace:registered".to_owned(),
        label: "registered".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
    }];
    runtime.rebuild_catalog();
    let checkout_id =
        workspace::checkout_id_for_path("workspace:registered", Path::new(checkout_path));
    runtime.snapshot.navigator.focused_workspace_id = Some("workspace:registered".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
    runtime.reset_terminal_projection(None);

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "tabs": [{
            "workspace_id": "herdr-workspace",
            "tab_id": "herdr-workspace:t1",
            "label": "2"
        }],
        "panes": [{"pane_id": "plain:p1", "cwd": checkout_path}],
        "layouts": [{
            "workspace_id": "herdr-workspace",
            "tab_id": "herdr-workspace:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "plain:p1",
            "panes": [{"pane_id": "plain:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("plain pane payload");

    // Before the session arrives the registration is the only row.
    assert_eq!(runtime.snapshot.navigator.workspaces.len(), 1);
    assert_eq!(runtime.snapshot.navigator.workspaces[0].checkouts.len(), 1);
    assert!(runtime.ingest_session(Ok(payload)));
    // Once Herdr has a workspace in that directory the registration's
    // row carries it, not a second entry beside it.
    assert_eq!(runtime.snapshot().navigator.workspaces.len(), 1);
    assert_eq!(
        runtime.snapshot().navigator.workspaces[0].session_workspace_ids,
        vec!["herdr-workspace".to_owned()]
    );
    let checkout = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .find(|workspace| workspace.id == "workspace:registered")
        .and_then(|workspace| {
            workspace
                .checkouts
                .iter()
                .find(|checkout| checkout.id == checkout_id)
        })
        .expect("the checkout Herdr occupies");
    assert_eq!(
        checkout.tabs.len(),
        1,
        "plain pane layout should create one checkout tab"
    );
    assert_eq!(checkout.tabs[0].panes[0].id, "plain:p1");
    // Herdr's bare tab number reads as a label only after the display
    // rule turns it into a name.
    assert_eq!(checkout.tabs[0].label.as_deref(), Some("Tab 2"));
    assert_eq!(checkout.tabs[0].panes[0].cwd, checkout_path);
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("plain:p1")
    );
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| layout.focused_pane_id.as_str()),
        Some("plain:p1")
    );
}

#[test]
fn an_exited_panes_root_directory_does_not_become_a_checkout() {
    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "workspaces": [{"workspace_id": "w2W", "label": "modakbul"}],
        "panes": [
            {"pane_id": "w2W:p1", "cwd": "/private/tmp/hide-modakbul"},
            {"pane_id": "w2W:pM", "cwd": "/"}
        ],
        "tabs": [{"workspace_id": "w2W", "tab_id": "w2W:t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w2W",
            "tab_id": "w2W:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w2W:p1",
            "panes": [
                {"pane_id": "w2W:p1", "rect": {"x": 0, "y": 0, "width": 40, "height": 24}},
                {"pane_id": "w2W:pM", "rect": {"x": 40, "y": 0, "width": 40, "height": 24}}
            ],
            "splits": []
        }]
    }))
    .expect("exited pane payload");

    let spaces = Runtime::session_spaces(&payload, "");

    assert_eq!(spaces.len(), 1);
    assert_eq!(
        spaces[0].cwds,
        vec!["/private/tmp/hide-modakbul".to_owned()]
    );
}

#[test]
fn a_returned_pane_id_selects_its_layout_when_other_panes_share_the_cwd() {
    let mut runtime = runtime();
    let checkout_path = "/tmp/hide-selected-checkout";
    let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
    let registration = WorkspaceRegistration {
        id: workspace_id.clone(),
        label: "Selected".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
    };
    let selected_workspace = workspace(
        &workspace_id,
        "Selected",
        checkout_path,
        vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
    );
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.snapshot.navigator.workspaces = vec![selected_workspace.clone()];
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id.clone());
    runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
    runtime.snapshot.ui_state.focused_checkout_id = Some(checkout_id.clone());
    runtime.reset_terminal_projection(None);
    runtime.snapshot.terminal.pane_id = Some("w2X:pB".to_owned());
    runtime.snapshot.focused.pane_id = Some("w2X:pB".to_owned());
    runtime.snapshot.pane_layouts = vec![PaneLayoutSnapshot {
        workspace_id: "w2X".to_owned(),
        tab_id: "w2X:t1".to_owned(),
        focused_pane_id: "w2X:pB".to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: "w2X:pB".to_owned(),
        },
    }];
    runtime.snapshot.terminal.panes = vec![TerminalPaneSnapshot {
        pane_id: "w2X:pB".to_owned(),
        closed: false,
        exit_code: None,
        ..TerminalPaneSnapshot::default()
    }];

    let selected_pane = "w3V:p1";
    let select_pane = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "ui_state_update",
        "payload": {
            "expanded_paths": [],
            "selected_path": null,
            "selected_pane_id": selected_pane,
            "focused_checkout_id": checkout_id,
            "shortcut_bindings": {},
            "accent_hex": "#B9FF66",
            "font_size": 13
        }
    }))
    .expect("selected pane state event");
    assert!(runtime.dispatch_json(&select_pane));
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some(selected_pane)
    );
    assert_eq!(
        runtime.snapshot().focused.pane_id.as_deref(),
        Some(selected_pane)
    );
    assert!(runtime.snapshot().active_pane_layout().is_none());
    assert!(runtime.snapshot().terminal.panes.is_empty());
    assert_eq!(
        runtime.snapshot().ui_state.focused_checkout_id.as_deref(),
        Some(checkout_id.as_str())
    );
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("pane.projection_unavailable")
    );

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "panes": [
            {"pane_id": "w2X:pB", "cwd": checkout_path},
            {"pane_id": selected_pane, "cwd": checkout_path}
        ],
        "tabs": [{"workspace_id": "w2X", "tab_id": "w2X:t1", "label": ""}, {"workspace_id": "w3V", "tab_id": "w3V:t1", "label": ""}],
        "layouts": [
            {
                "workspace_id": "w2X",
                "tab_id": "w2X:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w2X:pB",
                "panes": [{"pane_id": "w2X:pB", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            },
            {
                "workspace_id": "w3V",
                "tab_id": "w3V:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": selected_pane,
                "panes": [{"pane_id": selected_pane, "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
                "splits": []
            }
        ]
    }))
    .expect("overlapping cwd session payload");
    let catalog = session_sync::PrecomputedCatalog {
        registrations: vec![registration],
        workspaces: vec![selected_workspace],
        roots: workspace::RootIndex::new(),
    };

    assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
    let checkout = runtime
        .snapshot()
        .navigator
        .workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
        .and_then(|workspace| {
            workspace
                .checkouts
                .iter()
                .find(|checkout| checkout.id == checkout_id)
        })
        .expect("selected checkout");
    assert!(checkout.tabs.iter().any(|tab| {
        tab.id.as_deref() == Some("w3V:t1") && tab.panes.iter().any(|pane| pane.id == selected_pane)
    }));
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some(selected_pane)
    );
    assert_eq!(
        runtime
            .snapshot()
            .active_pane_layout()
            .map(|layout| (layout.workspace_id.as_str(), layout.tab_id.as_str())),
        Some(("w3V", "w3V:t1"))
    );
    assert!(runtime.snapshot().status.last_error.is_none());
}

#[test]
fn a_missing_selected_pane_reports_without_falling_back_to_a_same_cwd_pane() {
    let mut runtime = runtime();
    let checkout_path = "/tmp/hide-missing-selected-pane";
    let workspace_id = workspace::workspace_id_for_path(Path::new(checkout_path));
    let checkout_id = workspace::checkout_id_for_path(&workspace_id, Path::new(checkout_path));
    let registration = WorkspaceRegistration {
        id: workspace_id.clone(),
        label: "Missing pane".to_owned(),
        path: checkout_path.to_owned(),
        device_id: "local".to_owned(),
    };
    let selected_workspace = workspace(
        &workspace_id,
        "Missing pane",
        checkout_path,
        vec![checkout(&workspace_id, &checkout_id, checkout_path, None)],
    );
    runtime.snapshot.ui_state.workspace_registrations = vec![registration.clone()];
    runtime.snapshot.navigator.workspaces = vec![selected_workspace.clone()];
    runtime.snapshot.navigator.focused_workspace_id = Some(workspace_id.clone());
    runtime.snapshot.navigator.focused_checkout_id = Some(checkout_id);
    runtime.snapshot.navigator.root_path = Some(checkout_path.to_owned());
    runtime.snapshot.ui_state.selected_pane_id = Some("missing:p1".to_owned());
    runtime.snapshot.terminal.pane_id = Some("missing:p1".to_owned());
    // The user chose this pane against a running session, so it is an
    // authoritative selection rather than a restore hint.
    runtime.restore_hint_pending = false;
    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "panes": [{"pane_id": "old:p1", "cwd": checkout_path}],
        "tabs": [{"workspace_id": "old-workspace", "tab_id": "old-workspace:t1", "label": ""}],
        "layouts": [{
            "workspace_id": "old-workspace",
            "tab_id": "old-workspace:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "old:p1",
            "panes": [{"pane_id": "old:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("missing selected pane payload");
    let catalog = session_sync::PrecomputedCatalog {
        registrations: vec![registration],
        workspaces: vec![selected_workspace],
        roots: workspace::RootIndex::new(),
    };

    assert!(runtime.ingest_session_with_catalog(Ok(payload), Some(catalog)));
    assert!(runtime.snapshot().active_pane_layout().is_none());
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("missing:p1")
    );
    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("pane.projection_unavailable")
    );
}

#[test]
fn a_restored_pane_the_session_no_longer_has_retargets_without_reporting() {
    let mut runtime = runtime();
    // What launch produces: ids read from disk that name the session that
    // ended, against a Herdr session that has since been restarted.
    runtime.snapshot.ui_state.selected_pane_id = Some("wW:p3".to_owned());
    runtime.snapshot.terminal.pane_id = Some("wW:p3".to_owned());
    runtime.snapshot.ui_state.focused_checkout_id = Some("checkout:gone".to_owned());
    runtime.snapshot.navigator.focused_checkout_id = Some("checkout:gone".to_owned());

    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": [],
        "panes": [{"pane_id": "w19:p1", "cwd": "/tmp/hide-restored"}],
        "focused_pane_id": "w19:p1",
        "tabs": [{"workspace_id": "w19", "tab_id": "w19:t1", "label": ""}],
        "layouts": [{
            "workspace_id": "w19",
            "tab_id": "w19:t1",
            "zoomed": false,
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "focused_pane_id": "w19:p1",
            "panes": [{"pane_id": "w19:p1", "rect": {"x": 0, "y": 0, "width": 80, "height": 24}}],
            "splits": []
        }]
    }))
    .expect("restored pane payload");

    assert!(runtime.ingest_session_with_catalog(
        Ok(payload),
        Some(session_sync::PrecomputedCatalog {
            registrations: Vec::new(),
            workspaces: Vec::new(),
            roots: workspace::RootIndex::new(),
        }),
    ));
    assert_eq!(runtime.snapshot().status.last_error, None);
    assert_eq!(
        runtime.snapshot().terminal.pane_id.as_deref(),
        Some("w19:p1")
    );
    assert!(runtime.snapshot().active_pane_layout().is_some());
}

#[test]
fn protocol_mismatch_exposes_typed_diagnostics_and_clears_them_on_recovery() {
    let mut runtime = runtime();
    runtime.ingest_session(Err(SessionFetchError::Protocol {
        message: "incompatible runtime".to_owned(),
        expected_protocol: 23,
        received_protocol: 22,
        received_version: Some("0.9.0".to_owned()),
    }));

    let mismatch = &runtime.snapshot().status.herdr;
    assert_eq!(mismatch.state, "protocol_mismatch");
    assert_eq!(mismatch.expected_protocol, Some(23));
    assert_eq!(mismatch.received_protocol, Some(22));
    assert_eq!(mismatch.received_version.as_deref(), Some("0.9.0"));

    runtime.ingest_session(Ok(working_payload()));
    let connected = &runtime.snapshot().status.herdr;
    assert_eq!(connected.state, "connected");
    assert_eq!(connected.expected_protocol, None);
    assert_eq!(connected.received_protocol, None);
    assert_eq!(connected.received_version, None);
}

/// AC11, R9, SC6. A checkout whose tabs come from two Herdr workspaces
/// refused every drag, because ownership was decided for the checkout
/// rather than for the drag. A drag that only steps over the other
/// workspace's tabs changes nothing Herdr can see, so it lands locally.
#[test]
fn tab_strip_reorder_a_drag_past_another_workspaces_tabs_needs_no_herdr_move() {
    let (mut runtime, checkout_id, directory) = strip_checkout("split-local");
    let checkout_path = directory.to_string_lossy().into_owned();
    assert!(runtime.ingest_session(Ok(split_checkout_payload(
        &checkout_path,
        &[
            ("w-left", &["w-left:t1"][..], "w-left:t1"),
            ("w-right", &["w-right:t1", "w-right:t2"][..], "w-right:t1"),
        ],
        "w-left"
    ))));
    let before = strip_ids(&runtime, &checkout_id);
    assert_eq!(
        before.len(),
        3,
        "the split checkout holds both workspaces: {before:?}"
    );

    // There is no live connection here, so any request to Herdr would fail
    // loudly. Landing silently is the assertion.
    let moved = before[0].clone();
    assert!(reorder_tab(&mut runtime, &checkout_id, &moved, 1));

    assert_eq!(
        runtime
            .snapshot()
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        None
    );
    let after = strip_ids(&runtime, &checkout_id);
    assert_eq!(after[1], moved, "the drop did not land: {after:?}");
    assert!(runtime.pending_tab_move.is_empty());

    std::fs::remove_dir_all(&directory).ok();
}

/// AC11, R9. A drag that does change the moved tab's own workspace order
/// still asks Herdr, and asks with an index counted in that workspace, not
/// in the mixed strip the operator sees.
#[test]
fn tab_strip_reorder_a_drag_within_one_workspace_uses_that_workspaces_index() {
    let (mut runtime, checkout_id, directory) = strip_checkout("split-herdr");
    let checkout_path = directory.to_string_lossy().into_owned();
    assert!(runtime.ingest_session(Ok(split_checkout_payload(
        &checkout_path,
        &[
            ("w-left", &["w-left:t1"][..], "w-left:t1"),
            ("w-right", &["w-right:t1", "w-right:t2"][..], "w-right:t1"),
        ],
        "w-left"
    ))));
    let before = strip_ids(&runtime, &checkout_id);

    let socket_root = PathBuf::from("/tmp").join(format!(
        "herdr-core-split-move-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&socket_root).expect("socket directory");
    let socket_path = socket_root.join("herdr.sock");
    let listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let (mut stream, _) = listener.accept().expect("accept tab.move");
        let mut line = String::new();
        BufReader::new(stream.try_clone().expect("clone stream"))
            .read_line(&mut line)
            .expect("read request");
        let request: serde_json::Value =
            serde_json::from_str(&line).expect("tab.move request JSON");
        writeln!(
            stream,
            "{}",
            serde_json::json!({
                "id": request["id"],
                "result": {
                    "type": "tab_list",
                    "tabs": [
                        {"tab_id": "w-right:t2"},
                        {"tab_id": "w-right:t1"}
                    ]
                }
            })
        )
        .expect("write tab_list response");
        request
    });
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
    });

    // Move the right workspace's first tab behind its second.
    let moved = before
        .iter()
        .find(|id| id.ends_with("w-right:t1"))
        .expect("the right workspace's first tab")
        .clone();
    let target = before
        .iter()
        .position(|id| id.ends_with("w-right:t2"))
        .expect("the right workspace's second tab");
    assert!(reorder_tab(&mut runtime, &checkout_id, &moved, target));

    let request = server.join().expect("fixture server joins");
    assert_eq!(request["method"], "tab.move");
    assert_eq!(
        request["params"],
        serde_json::json!({"tab_id": "w-right:t1", "insert_index": 2}),
        "the index was not counted in the moved tab's own workspace"
    );
    // Nothing lands on the operator's word alone.
    assert_eq!(strip_ids(&runtime, &checkout_id), before);

    // Herdr reports the new order for the workspace it moved a tab in.
    // The held arrangement has to land here: the checkout holds two
    // workspaces' tabs, so what Herdr reports for one of them is never the
    // whole checkout's list, and a hold that waited for the whole list
    // would never be released.
    assert!(runtime.ingest_session(Ok(split_checkout_payload(
        &checkout_path,
        &[
            ("w-left", &["w-left:t1"][..], "w-left:t1"),
            ("w-right", &["w-right:t2", "w-right:t1"][..], "w-right:t1"),
        ],
        "w-left"
    ))));
    let after = strip_ids(&runtime, &checkout_id);
    let right = after
        .iter()
        .filter(|id| id.contains("w-right:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        right,
        vec!["herdr:w-right:t2".to_owned(), "herdr:w-right:t1".to_owned()],
        "the move Herdr granted did not land: {after:?}"
    );
    assert!(
        runtime.pending_tab_move.is_empty(),
        "a granted move stayed pending in a checkout two workspaces share"
    );

    std::fs::remove_dir_all(&socket_root).ok();
    std::fs::remove_dir_all(&directory).ok();
}

/// AC11, R9, SC6. A drag can both reorder the moved tab inside its own
/// workspace and carry it past another workspace's tabs. The operator's
/// arrangement is then an interleaving Herdr never reports, because Herdr
/// only ever states one workspace's order. Holding the checkout's mixed
/// order and waiting for it to come back left the drag pending forever and
/// the strip snapped back to where the tab started.
#[test]
fn tab_strip_reorder_a_drag_that_interleaves_two_workspaces_still_lands() {
    let (mut runtime, checkout_id, directory) = strip_checkout("split-interleaved");
    let checkout_path = directory.to_string_lossy().into_owned();
    let workspaces: [(&str, &[&str], &str); 2] = [
        ("w-left", &["w-left:t1"][..], "w-left:t1"),
        ("w-right", &["w-right:t1", "w-right:t2"][..], "w-right:t1"),
    ];
    assert!(runtime.ingest_session(Ok(split_checkout_payload(
        &checkout_path,
        &workspaces,
        "w-left"
    ))));
    let before = strip_ids(&runtime, &checkout_id);
    assert_eq!(
        before,
        vec![
            "herdr:w-left:t1".to_owned(),
            "herdr:w-right:t1".to_owned(),
            "herdr:w-right:t2".to_owned()
        ],
        "the fixture strip is not the order this test reasons about: {before:?}"
    );

    let socket_root = PathBuf::from("/tmp").join(format!(
        "herdr-core-split-interleave-{}-{}",
        std::process::id(),
        NEXT_RUNTIME_STATE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&socket_root).expect("socket directory");
    let socket_path = socket_root.join("herdr.sock");
    let listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("bind fixture socket");
    let server = std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let (mut stream, _) = listener.accept().expect("accept tab.move");
        let mut line = String::new();
        BufReader::new(stream.try_clone().expect("clone stream"))
            .read_line(&mut line)
            .expect("read request");
        let request: serde_json::Value =
            serde_json::from_str(&line).expect("tab.move request JSON");
        writeln!(
            stream,
            "{}",
            serde_json::json!({
                "id": request["id"],
                "result": {
                    "type": "tab_list",
                    "tabs": [
                        {"tab_id": "w-right:t2"},
                        {"tab_id": "w-right:t1"}
                    ]
                }
            })
        )
        .expect("write tab_list response");
        request
    });
    runtime.live = Some(live::LiveContext {
        socket_path: socket_path.clone(),
        herdr_bin: None,
        runtime: std::sync::Weak::new(),
        notifier: crate::ffi::ChangeNotifier::noop(),
        api_connector: Arc::new(crate::herdr_api::UnixSocketConnector::new(&socket_path)),
    });

    // The right workspace's second tab is dragged to the very front, over
    // the left workspace's tab as well as its own sibling.
    assert!(reorder_tab(
        &mut runtime,
        &checkout_id,
        "herdr:w-right:t2",
        0
    ));
    let request = server.join().expect("fixture server joins");
    assert_eq!(request["method"], "tab.move");
    assert_eq!(
        request["params"],
        serde_json::json!({"tab_id": "w-right:t2", "insert_index": 0}),
        "the index was not counted in the moved tab's own workspace"
    );

    // Herdr states the right workspace's new order. The checkout still
    // lists its tabs workspace by workspace, so what comes back is
    // `w-left:t1, w-right:t2, w-right:t1` - never the operator's
    // interleaving.
    let moved_workspaces: [(&str, &[&str], &str); 2] = [
        ("w-left", &["w-left:t1"][..], "w-left:t1"),
        ("w-right", &["w-right:t2", "w-right:t1"][..], "w-right:t1"),
    ];
    assert!(runtime.ingest_session(Ok(split_checkout_payload(
        &checkout_path,
        &moved_workspaces,
        "w-left"
    ))));

    assert!(
        runtime.pending_tab_move.is_empty(),
        "a granted move stayed pending because the operator interleaved two workspaces"
    );
    assert_eq!(
        strip_ids(&runtime, &checkout_id),
        vec![
            "herdr:w-right:t2".to_owned(),
            "herdr:w-left:t1".to_owned(),
            "herdr:w-right:t1".to_owned()
        ],
        "the drag did not stay where the operator dropped it"
    );

    std::fs::remove_dir_all(&socket_root).ok();
    std::fs::remove_dir_all(&directory).ok();
}
