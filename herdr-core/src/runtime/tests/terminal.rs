use super::*;

#[test]
fn a_foreign_grid_is_held_until_a_matching_full_frame_arrives() {
    let mut runtime = runtime();
    let pane = "w-grid:p1";
    runtime.terminal_sessions.insert(
        pane.to_owned(),
        TerminalSession::test_stub(pane, 1, TerminalSessionMode::Control),
    );
    runtime
        .terminal_session_generations
        .insert(pane.to_owned(), 1);
    runtime
        .terminal_view_sizes
        .insert(pane.to_owned(), (45, 115));
    let before = runtime.snapshot.terminal.sequence;
    for width in [84, 2] {
        assert_eq!(
            runtime.ingest_terminal_session_frame(
                pane,
                1,
                TerminalSessionMode::Control,
                b"foreign",
                crate::model::TerminalFrame {
                    width,
                    height: 9,
                    full: true
                }
            ),
            Some(false)
        );
    }
    assert_eq!(runtime.snapshot.terminal.sequence, before);
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            1,
            TerminalSessionMode::Control,
            b"partial",
            crate::model::TerminalFrame {
                width: 115,
                height: 45,
                full: false
            }
        ),
        Some(false)
    );
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            1,
            TerminalSessionMode::Control,
            b"matching",
            crate::model::TerminalFrame {
                width: 115,
                height: 45,
                full: true
            }
        ),
        Some(true)
    );
    assert_eq!(runtime.snapshot.terminal.sequence, before + 1);
    assert_eq!(
        runtime
            .snapshot
            .terminal
            .chunks
            .last()
            .unwrap()
            .frame
            .as_ref()
            .unwrap()
            .width,
        115
    );
}

/// B2. Herdr draws an observer at the grid it attached with and an observer
/// cannot resize, so a view that changed size after the attach (the web
/// pane beside a Swift window that holds control) held every frame and the
/// pane froze. The observer attaches again at the view's grid instead.
#[test]
fn an_observed_frame_at_an_old_grid_reattaches_the_observer_at_the_views_grid() {
    let mut runtime = runtime();
    runtime.suppress_terminal_session_workers = true;
    let pane = "w-observed:p1";
    runtime.terminal_sessions.insert(
        pane.to_owned(),
        TerminalSession::test_stub(pane, 7, TerminalSessionMode::Observe),
    );
    runtime
        .terminal_session_generations
        .insert(pane.to_owned(), 7);
    runtime.terminal_session_lifecycles.insert(
        pane.to_owned(),
        TerminalSessionLifecycle {
            state: "observing",
            attempt: 5,
            mode: Some(TerminalSessionMode::Observe),
            ..TerminalSessionLifecycle::default()
        },
    );
    runtime
        .terminal_view_sizes
        .insert(pane.to_owned(), (33, 143));
    let frame = |width| crate::model::TerminalFrame {
        width,
        height: 33,
        full: false,
    };
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            7,
            TerminalSessionMode::Observe,
            b"old",
            frame(186)
        ),
        None,
        "the old observer retires"
    );
    let generation = runtime.terminal_session_generations[pane];
    assert_ne!(generation, 7);
    assert_eq!(runtime.terminal_sizes[pane], (33, 143));
    assert_eq!(
        runtime.terminal_sessions[pane].mode,
        TerminalSessionMode::Observe
    );
    assert_eq!(runtime.terminal_session_lifecycles[pane].attempt, 5);
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            generation,
            TerminalSessionMode::Observe,
            b"new",
            crate::model::TerminalFrame {
                width: 143,
                height: 33,
                full: true
            }
        ),
        Some(true)
    );
}

/// A view that reported 41x18 while the settled size still said 50x25
/// had every attach frame held and its retries exhausted: the attach
/// asks for the grid the frame guard accepts.
#[test]
fn a_control_session_attaches_at_the_grid_the_view_reported() {
    let mut runtime = runtime();
    runtime.suppress_terminal_session_workers = true;
    let pane = "w-view:p1";
    runtime.terminal_sizes.insert(pane.into(), (25, 50));
    runtime.terminal_view_sizes.insert(pane.into(), (18, 41));
    runtime.start_terminal_session(
        pane,
        TerminalSessionMode::Control,
        1,
        "automatic_initial",
        None,
    );
    assert_eq!(runtime.terminal_sizes[pane], (18, 41));
    let generation = runtime.terminal_session_generations[pane];
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            generation,
            TerminalSessionMode::Control,
            b"attach frame",
            crate::model::TerminalFrame {
                width: 41,
                height: 18,
                full: true
            }
        ),
        Some(true)
    );
}

#[test]
fn retry_preserves_the_canvas_until_a_matching_replacement_frame() {
    let mut runtime = runtime();
    runtime.suppress_terminal_session_workers = true;
    let pane = "w-retry:p1";
    runtime.terminal_sizes.insert(pane.into(), (24, 80));
    runtime.append_terminal_chunk(pane.into(), live::encode_base64(b"last valid screen"));
    let before = runtime.snapshot.terminal.sequence;
    runtime.start_terminal_session(
        pane,
        TerminalSessionMode::Control,
        2,
        "automatic_bounded",
        None,
    );
    assert_eq!(
        runtime.snapshot.terminal.sequence, before,
        "retry must not blank the canvas"
    );
    let recovery = &runtime.terminal_recovery[pane];
    assert!(recovery.last_attempt_at_unix_ms.is_some());
    assert_eq!(
        runtime.terminal_session_lifecycles[pane].retry_decision,
        "automatic_bounded"
    );
    let generation = runtime.terminal_session_generations[pane];
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            generation,
            TerminalSessionMode::Control,
            b"wrong grid",
            crate::model::TerminalFrame {
                width: 2,
                height: 9,
                full: true
            }
        ),
        Some(false)
    );
    assert_eq!(runtime.snapshot.terminal.sequence, before);
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            generation,
            TerminalSessionMode::Control,
            b"replacement",
            crate::model::TerminalFrame {
                width: 80,
                height: 24,
                full: true
            }
        ),
        Some(true)
    );
    assert_eq!(runtime.snapshot.terminal.sequence, before + 1);
    assert_eq!(
        live::decode_base64(
            &runtime
                .snapshot
                .terminal
                .chunks
                .last()
                .unwrap()
                .bytes_base64
        )
        .unwrap(),
        b"\x1bcreplacement"
    );
    assert!(!runtime.terminal_recovery.contains_key(pane));
    assert!(runtime.terminal_session_lifecycles[pane].message.is_none());
}

#[test]
fn repeated_sync_updates_do_not_start_a_second_terminal_session() {
    assert!(terminal_control_request_allowed("idle", false));
    for state in [
        "starting",
        "controlling",
        "observing",
        "unavailable",
        "ended",
    ] {
        assert!(!terminal_control_request_allowed(state, false), "{state}");
    }
    assert!(!terminal_control_request_allowed("idle", true));
}

#[test]
fn pane_mutation_receipts_never_publish_topology_ahead_of_session_sync() {
    let mut runtime = runtime();
    let pane_id = "w1:p1";
    let layout = PaneLayoutSnapshot {
        workspace_id: "w1".to_owned(),
        tab_id: "w1:t1".to_owned(),
        focused_pane_id: pane_id.to_owned(),
        zoomed: false,
        root: PaneLayoutNodeSnapshot::Pane {
            pane_id: pane_id.to_owned(),
        },
    };
    let panes = vec![TerminalPaneSnapshot {
        pane_id: pane_id.to_owned(),
        closed: false,
        ..TerminalPaneSnapshot::default()
    }];
    runtime.snapshot.pane_layouts = vec![layout.clone()];
    runtime.snapshot.focused.surface = Surface::Terminal;
    runtime.snapshot.focused.pane_id = Some(pane_id.to_owned());
    runtime.snapshot.terminal.pane_id = Some(pane_id.to_owned());
    runtime.snapshot.terminal.panes = panes.clone();
    runtime.snapshot.ui_state.selected_pane_id = Some(pane_id.to_owned());

    let receipts = [
        (
            PaneControlAction::Split {
                pane_id: pane_id.to_owned(),
                direction: PaneSplitDirection::Right,
                cwd: Some("/tmp".to_owned()),
            },
            PaneControlOutcome::Acknowledged {
                created_pane_id: Some("w1:p2".to_owned()),
            },
        ),
        (
            PaneControlAction::Focus {
                pane_id: "w1:p2".to_owned(),
            },
            PaneControlOutcome::Acknowledged {
                created_pane_id: None,
            },
        ),
        (
            PaneControlAction::Resize {
                pane_id: pane_id.to_owned(),
                direction: PaneResizeDirection::Right,
                amount: 0.1,
            },
            PaneControlOutcome::Acknowledged {
                created_pane_id: None,
            },
        ),
        (
            PaneControlAction::ToggleZoom {
                pane_id: pane_id.to_owned(),
            },
            PaneControlOutcome::Acknowledged {
                created_pane_id: None,
            },
        ),
        (
            PaneControlAction::Close {
                pane_id: pane_id.to_owned(),
            },
            PaneControlOutcome::Acknowledged {
                created_pane_id: None,
            },
        ),
    ];

    for (action, receipt) in receipts {
        assert!(runtime.ingest_pane_control_result(action, Ok(receipt), 3));
        assert_eq!(runtime.snapshot.pane_layouts, vec![layout.clone()]);
        assert_eq!(runtime.snapshot.terminal.panes, panes);
        assert_eq!(runtime.snapshot.terminal.pane_id.as_deref(), Some(pane_id));
        assert_eq!(runtime.snapshot.focused.pane_id.as_deref(), Some(pane_id));
        assert_eq!(
            runtime.snapshot.ui_state.selected_pane_id.as_deref(),
            Some(pane_id)
        );
    }
}

/// AC6's failure half: an attach that fails names its reason on the pane
/// it failed for and leaves every other pane alone.
///
/// A launch pointed at a Herdr binary that is not there reaches the
/// runtime as exactly this spawn error. That launch cannot be staged from
/// outside the app - `HerdrRuntimeResolver.resolve` searches absolute
/// paths that ignore both HOME and PATH, so any staging finds the
/// operator's installed Herdr - which is why the failure is proven here,
/// at the boundary the failure actually crosses, rather than by a window
/// screenshot.
#[test]
fn attach_failure_names_its_reason_on_that_pane_and_leaves_the_others_idle() {
    let mut runtime = runtime();
    runtime.suppress_terminal_session_workers = true;
    // Both panes are projected the way the runtime projects them, so the
    // untouched one carries a real resting state rather than a zero value
    // a hand-built struct would have handed the assertion for free.
    runtime.ensure_terminal_pane("w1:p1");
    runtime.ensure_terminal_pane("w1:p2");
    runtime
        .terminal_session_generations
        .insert("w1:p1".to_owned(), 7);
    runtime.terminal_session_lifecycles.insert(
        "w1:p1".to_owned(),
        TerminalSessionLifecycle {
            state: "starting",
            generation: 7,
            attempt: 1,
            mode: Some(TerminalSessionMode::Control),
            ..TerminalSessionLifecycle::default()
        },
    );

    let reason = "herdr terminal control failed: no such file or directory";
    assert!(runtime.ingest_terminal_session_spawn(
        7,
        "w1:p1",
        TerminalSessionMode::Control,
        Err(reason.to_owned()),
        12,
        Weak::new(),
        crate::ffi::ChangeNotifier::noop(),
    ));

    let failed = runtime
        .snapshot
        .terminal
        .panes
        .iter()
        .find(|pane| pane.pane_id == "w1:p1")
        .expect("the pane whose attach failed is still projected");
    assert_eq!(failed.transport_state, "unavailable");
    assert!(failed.transport_message.as_ref().unwrap().contains(reason));
    assert!(
        failed
            .transport_message
            .as_ref()
            .unwrap()
            .contains("Retrying in 5 seconds")
    );
    assert_eq!(
        failed.transport_exit_category.as_deref(),
        Some("spawn_failed")
    );
    assert_eq!(failed.transport_retry_decision, "automatic_bounded");

    let untouched = runtime
        .snapshot
        .terminal
        .panes
        .iter()
        .find(|pane| pane.pane_id == "w1:p2")
        .expect("the other pane is still projected");
    assert_eq!(
        untouched.transport_state, "idle",
        "one pane's attach failure must not mark another pane unavailable"
    );
    assert_eq!(untouched.transport_message, None);
    assert_eq!(untouched.transport_exit_category, None);

    // The reason is also written into that pane's own byte stream, so the
    // operator reads it where the terminal would have been and nowhere
    // else on the canvas.
    let notices: Vec<&TerminalChunk> = runtime
        .snapshot
        .terminal
        .chunks
        .iter()
        .filter(|chunk| {
            String::from_utf8(live::decode_base64(&chunk.bytes_base64).expect("chunk bytes"))
                .is_ok_and(|text| text.contains(reason))
        })
        .collect();
    assert_eq!(notices.len(), 1, "the reason is announced once");
    assert_eq!(notices[0].pane_id, "w1:p1");
}

#[test]
fn runtime_owner_conflict_observes_ignores_stale_delivery_and_reconnects_once() {
    for reason in [
        "terminal attach failed: terminal 42 already has an attached client; retry with --takeover",
        "terminal attach taken over",
    ] {
        assert_owner_conflict_observes_and_reconnects(reason);
    }
}

/// The shell used to draw an even grid of the tab's panes whenever it had
/// no layout, which is a geometry Herdr never applied and which decides
/// the PTY size. With every tab's layout in the snapshot there is nothing
/// left for it to stand in for, and nothing may bring it back.
#[test]
fn tab_layouts_have_no_uniform_grid_stand_in_left_in_the_shell() {
    let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&shell).expect("the shell source directory") {
        let path = entry.expect("a shell source entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("swift") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a readable Swift source");
        if source.contains("uniformItems") {
            offenders.push(path.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "a uniform pane grid stand-in is back in {offenders:?}"
    );
}

/// R5, AC9. Herdr sizes a pane's PTY from the attach, so an attach before
/// any view has reported a size draws a full frame at a guess and a
/// second one after the resize corrects it. The wait is reported, because
/// a pane that never attaches must not look like a pane with no output.
#[test]
fn one_launch_holds_an_attach_until_the_view_reports_a_size() {
    let mut runtime = runtime();
    runtime.suppress_terminal_session_workers = true;
    runtime.live = None;

    runtime.request_terminal_control("w-size:p1");
    assert!(
        !runtime.terminal_sessions.contains_key("w-size:p1"),
        "no session may be started for a pane with no reported size"
    );
    assert!(
        runtime
            .snapshot
            .status
            .diagnostics
            .iter()
            .any(|entry| entry.kind == "terminal.attach_deferred"),
        "the wait must be reported: {:?}",
        runtime.snapshot.status.diagnostics
    );
    assert!(runtime.panes_awaiting_size.contains("w-size:p1"));
    assert_eq!(
        runtime.terminal_session_lifecycles["w-size:p1"].state,
        "waiting_size"
    );
    assert!(
        runtime.terminal_session_lifecycles["w-size:p1"]
            .message
            .as_ref()
            .unwrap()
            .contains("Waiting")
    );
}

fn wheel_event(pane_id: &str, direction: &str, lines: u16) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "terminal_scroll",
        "payload": {"pane_id": pane_id, "direction": direction, "lines": lines}
    }))
    .expect("scroll event")
}

/// B2. A wheel that arrives before the pane has a session (its view has not
/// reported a size, or the attach is still starting after a tab switch) is
/// kept, said once, and sent with the pane's first frame through the mode it
/// attached in; a burst is one summed scroll, not one per wheel.
#[test]
fn a_wheel_before_the_attach_is_sent_with_the_first_frame() {
    let mut runtime = runtime();
    let pane = "w1:p1";
    assert!(runtime.dispatch_json(&wheel_event(pane, "up", 3)));
    for _ in 0..20 {
        runtime.dispatch_json(&wheel_event(pane, "up", 3));
    }
    runtime.dispatch_json(&wheel_event(pane, "down", 3));
    let deferred = runtime
        .snapshot()
        .status
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == "terminal.scroll_deferred")
        .count();
    assert_eq!(deferred, 1, "a wheel burst filled the diagnostics list");

    runtime.terminal_sessions.insert(
        pane.to_owned(),
        TerminalSession::test_stub(pane, 1, TerminalSessionMode::Control),
    );
    runtime
        .terminal_session_generations
        .insert(pane.to_owned(), 1);
    runtime.terminal_view_sizes.insert(pane.to_owned(), (9, 40));
    assert_eq!(
        runtime.ingest_terminal_session_frame(
            pane,
            1,
            TerminalSessionMode::Control,
            b"frame",
            crate::model::TerminalFrame {
                width: 40,
                height: 9,
                full: true
            }
        ),
        Some(true)
    );
    let written = runtime.terminal_sessions[pane].test_written_lines();
    let scrolls = written
        .iter()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("a protocol line"))
        .filter(|line| line["type"] == "terminal.scroll")
        .map(|line| (line["direction"].clone(), line["lines"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        scrolls,
        vec![(serde_json::json!("up"), serde_json::json!(60))]
    );
    assert!(runtime.wheel_before_attach.is_empty());
}

/// A fake Herdr holding one pane's scroll position, answering `pane.get` and
/// `pane.scroll` the way the pinned server does: an overshoot at the top is
/// clamped, and the answer carries the metrics after the move.
fn scrolling_herdr(name: &str, max: Arc<Mutex<u64>>) -> FakeHerdr {
    let offset = Arc::new(Mutex::new(0_u64));
    FakeHerdr::start(name, move |method, params| {
        let max = *max.lock().unwrap();
        let mut offset = offset.lock().unwrap();
        match method {
            "pane.get" => {}
            "pane.scroll" => {
                *offset = params["offset_from_bottom"].as_u64().unwrap().min(max);
            }
            other => panic!("unexpected {other}"),
        }
        serde_json::json!({"type": "pane_info", "pane": {
            "pane_id": params["pane_id"], "terminal_id": "t", "workspace_id": "w1",
            "tab_id": "w1:t1", "focused": false, "agent_status": "idle", "revision": 1,
            "scroll": {"offset_from_bottom": *offset, "max_offset_from_bottom": max, "viewport_rows": 20}
        }})
    })
}

fn observed_runtime(herdr: &FakeHerdr, pane: &str) -> Arc<Mutex<Runtime>> {
    let shared = Arc::new(Mutex::new(runtime()));
    {
        let mut runtime = shared.lock().unwrap();
        runtime.live = Some(live::LiveContext {
            socket_path: herdr.socket_path().to_path_buf(),
            herdr_bin: None,
            runtime: Arc::downgrade(&shared),
            notifier: crate::ffi::ChangeNotifier::noop(),
            api_connector: Arc::new(herdr.connector()),
        });
        runtime.terminal_sessions.insert(
            pane.to_owned(),
            TerminalSession::test_stub(pane, 1, TerminalSessionMode::Observe),
        );
        runtime.terminal_session_lifecycles.insert(
            pane.to_owned(),
            TerminalSessionLifecycle {
                state: "observing",
                mode: Some(TerminalSessionMode::Observe),
                ..TerminalSessionLifecycle::default()
            },
        );
        runtime.ensure_terminal_pane(pane);
    }
    shared
}

fn settle(shared: &Arc<Mutex<Runtime>>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !shared.lock().unwrap().viewport_scrolls.is_empty() {
        assert!(Instant::now() < deadline, "a viewport scroll never landed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn scroll_offsets(herdr: &FakeHerdr) -> Vec<u64> {
    herdr
        .calls()
        .into_iter()
        .filter(|(method, _)| method == "pane.scroll")
        .map(|(_, params)| params["offset_from_bottom"].as_u64().unwrap())
        .collect()
}

/// B1, B2. A pane another client controls (the Swift shell on the same
/// server) is observed, and an observer has no terminal writer; its wheel
/// moves Herdr's viewport with `pane.scroll` instead of being dropped. One
/// request is in flight per pane, and the wheels that arrive meanwhile go
/// out as one summed request when it lands.
#[test]
fn a_wheel_on_an_observed_pane_moves_herdrs_viewport() {
    let max = Arc::new(Mutex::new(300));
    let herdr = scrolling_herdr("observed-wheel", Arc::clone(&max));
    let pane = "w1:p1";
    let shared = observed_runtime(&herdr, pane);
    {
        let mut runtime = shared.lock().unwrap();
        runtime.dispatch_json(&wheel_event(pane, "up", 3));
        runtime.dispatch_json(&wheel_event(pane, "up", 2));
        runtime.dispatch_json(&wheel_event(pane, "down", 1));
    }
    settle(&shared);
    assert_eq!(scroll_offsets(&herdr), vec![3, 4]);
    shared
        .lock()
        .unwrap()
        .dispatch_json(&wheel_event(pane, "down", 10));
    settle(&shared);
    assert_eq!(scroll_offsets(&herdr), vec![3, 4, 0]);
    let runtime = shared.lock().unwrap();
    assert!(!runtime.terminal_pane_snapshot(pane).scroll_held_elsewhere);
    assert!(runtime.snapshot.status.last_error.is_none());
}

/// B3. When Herdr moves nothing for an observer (an alternate-screen program
/// has no history in the viewport, and only the controlling client's wheel
/// reaches the program), the pane says another client holds its scrolling
/// rather than dropping the wheel silently; a later wheel that moves clears
/// it.
#[test]
fn an_observed_wheel_herdr_cannot_move_shows_the_hold_until_one_moves() {
    let max = Arc::new(Mutex::new(0));
    let herdr = scrolling_herdr("observed-held", Arc::clone(&max));
    let pane = "w1:p1";
    let shared = observed_runtime(&herdr, pane);
    shared
        .lock()
        .unwrap()
        .dispatch_json(&wheel_event(pane, "up", 3));
    settle(&shared);
    {
        let runtime = shared.lock().unwrap();
        let projected = runtime
            .snapshot
            .terminal
            .panes
            .iter()
            .find(|row| row.pane_id == pane)
            .expect("the pane is projected");
        assert!(projected.scroll_held_elsewhere);
        let wire = serde_json::to_value(projected).unwrap();
        assert_eq!(wire["scroll_held_elsewhere"], true);
    }
    *max.lock().unwrap() = 300;
    shared
        .lock()
        .unwrap()
        .dispatch_json(&wheel_event(pane, "up", 3));
    settle(&shared);
    let runtime = shared.lock().unwrap();
    let projected = runtime
        .snapshot
        .terminal
        .panes
        .iter()
        .find(|row| row.pane_id == pane)
        .unwrap();
    assert!(!projected.scroll_held_elsewhere);
    assert!(
        serde_json::to_value(projected)
            .unwrap()
            .get("scroll_held_elsewhere")
            .is_none(),
        "the field stays off the wire while false"
    );
}

/// B3's marker says another client holds the pane's scrolling, so only
/// Herdr's own answer may raise it. A pane with no route to its Herdr, or a
/// Herdr that cannot be reached, logs the failed wheel and leaves the pane's
/// transport state to say why; it never claims another client holds it.
#[test]
fn an_observed_wheel_that_cannot_reach_herdr_is_logged_not_held() {
    let pane = "w1:p1";
    let herdr = scrolling_herdr("observed-unreachable", Arc::new(Mutex::new(300)));
    let shared = observed_runtime(&herdr, pane);
    drop(herdr);
    shared
        .lock()
        .unwrap()
        .dispatch_json(&wheel_event(pane, "up", 3));
    settle(&shared);
    {
        let mut runtime = shared.lock().unwrap();
        assert!(!runtime.terminal_pane_snapshot(pane).scroll_held_elsewhere);
        runtime.live = None;
        runtime.dispatch_json(&wheel_event(pane, "up", 3));
        assert!(runtime.viewport_scrolls.is_empty());
        assert!(!runtime.terminal_pane_snapshot(pane).scroll_held_elsewhere);
        let failed = runtime
            .snapshot
            .status
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == "terminal.scroll_failed")
            .count();
        assert_eq!(failed, 2, "each unreachable wheel is logged");
    }
}

/// B6. A device pane is searched through that device's Herdr under the id
/// that Herdr knows it by; a device that is not connected answers the reason
/// in the find bar instead of searching this machine.
#[test]
fn a_device_pane_is_searched_on_its_own_herdr() {
    let herdr = FakeHerdr::start("device-find", |method, params| {
        assert_eq!(method, "pane.read");
        assert_eq!(params["pane_id"], "w1:p2");
        serde_json::json!({"type": "pane_read", "read": {
            "pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1",
            "source": params["source"], "format": "text",
            "text": "alpha\nneedle\nomega\n", "revision": 1, "truncated": false
        }})
    });
    let shared = Arc::new(Mutex::new(runtime()));
    let pane = "remote:mini:pane:w1:p2";
    let find = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "pane_find",
        "payload": {"pane_id": pane, "term": "needle", "case_sensitive": false,
                    "whole_word": false, "regex": false, "step": 1}
    }))
    .unwrap();
    {
        let mut runtime = shared.lock().unwrap();
        runtime
            .snapshot
            .ui_state
            .device_registrations
            .push(crate::model::DeviceRegistration {
                id: "mini".to_owned(),
                label: "Mini".to_owned(),
                ssh_alias: Some("mini".to_owned()),
                herdr_socket_path: None,
                host_consent: None,
            });
        runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
            target_id: "mini".to_owned(),
            state: "reconnecting".to_owned(),
            message: None,
            herdr_version: Some("0.9.1".to_owned()),
            session: None,
            files: RemoteFileListSnapshot::idle(),
            catalog: Default::default(),
        });
        runtime.install_remote_control(live::RemoteControlContext::new(
            "mini",
            Arc::new(herdr.connector()),
            Arc::downgrade(&shared),
            crate::ffi::ChangeNotifier::noop(),
        ));
        runtime.dispatch_json(&find);
        assert_eq!(
            runtime.snapshot.find.unavailable_reason.as_deref(),
            Some("Mini is not connected")
        );
        assert!(herdr.requests().is_empty());
        runtime.snapshot.status.remote[0].state = "connected".to_owned();
        runtime.dispatch_json(&find);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let runtime = shared.lock().unwrap();
        if runtime.snapshot.find.total == 1 {
            assert_eq!(runtime.snapshot.find.index, 1);
            assert_eq!(runtime.snapshot.find.pane_id.as_deref(), Some(pane));
            assert!(runtime.snapshot.find.unavailable_reason.is_none());
            break;
        }
        drop(runtime);
        assert!(Instant::now() < deadline, "the device search never landed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn retiring_a_pane_clears_every_pane_keyed_terminal_state() {
    let mut runtime = runtime();
    let pane = "w-retired:p1";
    runtime.terminal_session_generations.insert(pane.into(), 7);
    runtime
        .terminal_session_lifecycles
        .insert(pane.into(), TerminalSessionLifecycle::default());
    runtime.terminal_recovery.insert(
        pane.into(),
        crate::terminal_recovery::Recovery::new(Instant::now(), "retry".into()),
    );
    runtime.terminal_sizes.insert(pane.into(), (80, 24));
    runtime.terminal_view_sizes.insert(pane.into(), (120, 40));
    runtime.terminal_frames_need_full.insert(pane.into());
    runtime
        .terminal_foreign_frame_sizes
        .insert(pane.into(), (9, 84));
    runtime.panes_awaiting_size.insert(pane.into());
    runtime.wheel_before_attach.insert(pane.into(), 3);
    runtime.viewport_scrolls.insert(pane.into(), 0);
    runtime.panes_scroll_held.insert(pane.into());
    runtime.panes_closing.insert(pane.into());

    assert!(runtime.retain_terminal_pane_state(|known| known != pane));
    assert!(!runtime.terminal_session_generations.contains_key(pane));
    assert!(!runtime.terminal_session_lifecycles.contains_key(pane));
    assert!(!runtime.terminal_recovery.contains_key(pane));
    assert!(!runtime.terminal_sizes.contains_key(pane));
    assert!(!runtime.terminal_view_sizes.contains_key(pane));
    assert!(!runtime.terminal_frames_need_full.contains(pane));
    assert!(!runtime.terminal_foreign_frame_sizes.contains_key(pane));
    assert!(!runtime.panes_awaiting_size.contains(pane));
    assert!(!runtime.wheel_before_attach.contains_key(pane));
    assert!(!runtime.viewport_scrolls.contains_key(pane));
    assert!(!runtime.panes_scroll_held.contains(pane));
    assert!(!runtime.panes_closing.contains(pane));
}

/// R6, R7. A pane keeps its session while its canvas is rebuilt - a zoom,
/// a tab visit, a return to a checkout - and the view that comes back has
/// an empty grid. Herdr sent the attach frame to the view that came
/// before it and sends nothing more until the pane produces output, so
/// the operator sees a blank pane that a single wheel notch repairs.
/// The repaint has to reach Herdr even though the size did not change.
#[test]
fn a_rebuilt_view_for_an_attached_pane_is_given_a_frame_to_draw() {
    let checkout_path = "/private/tmp/hide-pane-repaint";
    let (mut runtime, _checkout_id) = live_tab_order_runtime(checkout_path);
    runtime.suppress_terminal_session_workers = true;
    let tabs = ["w-repaint:t1"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-repaint:t1",
    )));
    let pane_id = "w-repaint:t1:p";
    let resize = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "terminal_resize",
        "payload": {"pane_id": pane_id, "rows": 30, "cols": 100}
    }))
    .expect("resize event");
    // The first view reporting its size is what starts the attach.
    runtime.dispatch_json(&resize);
    assert!(runtime.terminal_sessions.contains_key(pane_id));
    let _attach_writes = runtime.terminal_sessions[pane_id].test_written_lines();
    runtime.terminal_frames_need_full.remove(pane_id);

    // The same size reported again is still swallowed: that is the report
    // every settled view makes, and answering it would double the frames.
    runtime.dispatch_json(&resize);
    assert!(
        runtime.terminal_sessions[pane_id]
            .test_written_lines()
            .is_empty(),
        "a size that did not change was forwarded to Herdr"
    );

    // A new view reports its first geometry, then its settled geometry.
    runtime.dispatch_json(
        &serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": "terminal_viewport",
            "payload": {"pane_id": pane_id, "rows": 30, "cols": 100, "new_view": true}
        }))
        .unwrap(),
    );
    runtime.dispatch_json(&resize);
    let written = runtime.terminal_sessions[pane_id].test_written_lines();
    assert_eq!(
        written.len(),
        1,
        "the repaint did not reach Herdr exactly once"
    );
    let line: serde_json::Value =
        serde_json::from_str(written[0].trim_end()).expect("repaint line is JSON");
    assert_eq!(line["type"], "terminal.resize");
    assert_eq!(line["rows"], 30);
    assert_eq!(line["cols"], 100);
    assert!(runtime.snapshot().status.last_error.is_none());
}

/// AC8, R7, SC5. Attaching every tab the operator ever visited left a
/// child process and a server-side render alive for each one. Only the tab
/// on screen and the four before it keep their panes attached; the rest
/// are released, say so, and attach again on the next visit.
#[test]
fn only_the_last_five_shown_tabs_keep_their_panes_attached() {
    let checkout_path = "/private/tmp/hide-attach-window";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    runtime.suppress_terminal_session_workers = true;
    let tabs = [
        "w-order:t1",
        "w-order:t2",
        "w-order:t3",
        "w-order:t4",
        "w-order:t5",
        "w-order:t6",
        "w-order:t7",
    ];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    // The first tab is already on screen; its view reports, which is what
    // starts an attach.
    let report_size = |runtime: &mut Runtime, tab_id: &str| {
        let resize = serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "kind": "terminal_resize",
            "payload": {"pane_id": format!("{tab_id}:p"), "rows": 30, "cols": 100}
        }))
        .expect("resize event");
        runtime.dispatch_json(&resize);
    };
    report_size(&mut runtime, "w-order:t1");
    assert!(runtime.terminal_sessions.contains_key("w-order:t1:p"));
    for tab_id in &tabs[1..] {
        assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, tab_id)));
        report_size(&mut runtime, tab_id);
    }

    let attached = runtime
        .terminal_sessions
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        attached,
        [
            "w-order:t3:p",
            "w-order:t4:p",
            "w-order:t5:p",
            "w-order:t6:p",
            "w-order:t7:p"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>(),
        "the attach window is not the last five tabs shown"
    );

    // A released pane says what happened to it rather than looking broken.
    let released = runtime
        .snapshot()
        .terminal
        .panes
        .iter()
        .find(|pane| pane.pane_id == "w-order:t1:p")
        .expect("a released pane keeps its projection entry")
        .clone();
    assert_eq!(released.transport_state, "released");
    assert!(released.transport_exit_category.is_none());
    assert_eq!(
        runtime
            .snapshot()
            .status
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == "terminal.session_released"
                && diagnostic.message.contains("w-order:t1:p"))
            .count(),
        1
    );

    // A tick that changes nothing must not attach any of them again.
    let before = runtime
        .terminal_sessions
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t7",
    )));
    assert_eq!(
        runtime
            .terminal_sessions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        before,
        "an idle tick re-attached a released pane"
    );

    // Going back attaches that tab's pane and nothing else.
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
    assert!(runtime.terminal_sessions.contains_key("w-order:t1:p"));
    assert_eq!(runtime.terminal_sessions.len(), ATTACHED_TAB_LIMIT);
}

/// AC15, R11, SC7. Herdr closes the PTY before it reports the pane gone,
/// so the attach child ends while the pane is still drawn. Reading that as
/// a transport failure is what flashed "terminal attach ended" over a pane
/// the operator had just closed.
#[test]
fn close_projection_a_close_hide_asked_for_is_not_a_transport_failure() {
    let checkout_path = "/private/tmp/hide-close-projection";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    runtime.suppress_terminal_session_workers = true;
    let tabs = ["w-order:t1"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    let pane_id = "w-order:t1:p";
    let resize = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "terminal_resize",
        "payload": {"pane_id": pane_id, "rows": 30, "cols": 100}
    }))
    .expect("resize event");
    runtime.dispatch_json(&resize);
    assert!(runtime.terminal_sessions.contains_key(pane_id));
    let generation = runtime.terminal_session_generations[pane_id];
    let chunks_before = runtime.snapshot().terminal.chunks.len();

    let close = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "close_pane",
        "payload": {"pane_id": pane_id, "confirmed": true}
    }))
    .expect("close pane event");
    runtime.dispatch_json(&close);
    assert!(runtime.ingest_terminal_session_closed(
        pane_id,
        generation,
        TerminalSessionMode::Control,
        None,
    ));

    let pane = runtime
        .snapshot()
        .terminal
        .panes
        .iter()
        .find(|pane| pane.pane_id == pane_id)
        .expect("the pane is still drawn until Herdr removes it")
        .clone();
    assert_eq!(pane.transport_state, "closing");
    assert!(pane.transport_message.is_none());
    assert_eq!(
        runtime.snapshot().terminal.chunks.len(),
        chunks_before,
        "a notice was written over the pane's last frame"
    );
    let _ = checkout_id;
}

/// The other half of the same rule: a close that is not the pane going
/// away still reports itself exactly as it did.
#[test]
fn close_projection_a_failure_on_a_living_pane_still_reports_ended() {
    let checkout_path = "/private/tmp/hide-close-failure";
    let (mut runtime, _checkout_id) = live_tab_order_runtime(checkout_path);
    runtime.suppress_terminal_session_workers = true;
    let tabs = ["w-order:t1"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    let pane_id = "w-order:t1:p";
    let resize = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "terminal_resize",
        "payload": {"pane_id": pane_id, "rows": 30, "cols": 100}
    }))
    .expect("resize event");
    runtime.dispatch_json(&resize);
    let generation = runtime.terminal_session_generations[pane_id];

    assert!(runtime.ingest_terminal_session_closed(
        pane_id,
        generation,
        TerminalSessionMode::Control,
        Some("herdr terminal session control exited with status 1".to_owned()),
    ));

    let pane = runtime
        .snapshot()
        .terminal
        .panes
        .iter()
        .find(|pane| pane.pane_id == pane_id)
        .expect("the pane is still projected")
        .clone();
    assert_eq!(pane.transport_state, "ended");
    assert!(pane.transport_message.is_some());
}
