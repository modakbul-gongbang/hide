use super::*;

/// R8, AC14. The agent notes are what the next person reads before they
/// touch this subsystem, and both of their claims about it were wrong: the
/// shell was described as reading the snapshot on every change
/// notification, and the only measured figure in the performance guide was
/// a mutex wait from an incident measured while typing on a build three
/// rounds of work ago. A document drifts silently, so the claims that
/// matter are asserted here rather than trusted.
#[test]
fn agent_notes_state_the_view_authority_and_the_announcement_rule() {
    let notes = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../AGENTS.md"))
        .expect("the agent notes");
    let (architecture, rest) = notes
        .split_once("## Runtime Architecture")
        .expect("a Runtime Architecture section");
    assert!(
        architecture.len() < rest.len(),
        "the split must put the section body on the right"
    );
    let (architecture, _) = rest
        .split_once("## Herdr API Contract")
        .expect("Runtime Architecture ends at the Herdr API Contract");
    let (_, performance) = notes
        .split_once("## Performance Guide")
        .expect("a Performance Guide section");

    for (section, name, wanted) in [
        (
            architecture,
            "Runtime Architecture",
            vec![
                // The boundary itself, both halves of it.
                "visible tab",
                "keyboard focus pane",
                // The four paths a core-owned value can take.
                "pending",
                "followed",
                "refusal",
                // What replaced the per-change read and the second core.
                "once per burst",
                "before** it takes the lock",
                "creates the core once",
            ],
        ),
        (
            performance,
            "Performance Guide",
            vec![
                "serialize_snapshot_delta",
                "ChangeNotifier",
                "idle",
                "driven",
            ],
        ),
    ] {
        for phrase in wanted {
            assert!(
                section.contains(phrase),
                "AGENTS.md {name} does not say {phrase:?}"
            );
        }
    }
    assert!(
        !performance.contains("main thread spent 47% of wall time"),
        "the stale 47% figure is replaced by measurements with their load, not kept beside them"
    );
}

/// R5, AC9. A launch used to create a core without the resolved Herdr
/// binary, start its session sync, and then throw both away for a second
/// core once the runtime was known. Destroying the first one joined a
/// worker mid-bootstrap, which is where the measured 360 ms between the
/// runtime resolving and the second core being ready went. One core per
/// launch means one `session.snapshot`, one subscription and one catalog
/// build, and the shell is the only place that can put the second one
/// back.
#[test]
fn one_launch_creates_the_core_once_and_never_replaces_it() {
    let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
    let mut creations = Vec::new();
    let mut destructions = Vec::new();
    for entry in std::fs::read_dir(&shell).expect("the shell source directory") {
        let path = entry.expect("a shell source entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("swift") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a readable Swift source");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        for line in source.lines().map(str::trim) {
            if line.starts_with("//") {
                continue;
            }
            if line.contains("herdr_core_create(") {
                creations.push(format!("{name}: {line}"));
            }
            if line.contains("herdr_core_destroy(") {
                destructions.push(format!("{name}: {line}"));
            }
        }
    }
    assert_eq!(
        creations.len(),
        1,
        "the shell must call herdr_core_create from one place: {creations:?}"
    );
    assert_eq!(
        destructions.len(),
        1,
        "a core is destroyed only when the bridge goes away: {destructions:?}"
    );
    let bridge =
        std::fs::read_to_string(shell.join("CoreBridge.swift")).expect("the core bridge source");
    assert!(
        !bridge.contains("replaceCore"),
        "replacing a live core with a second one is the path this removed"
    );
    assert!(
        bridge.contains("runtimePreparation"),
        "the one core is created after the runtime resolves, so the resolution has to be awaited"
    );
}

/// R6, AC12. Herdr publishes a session snapshot on every event and on
/// every agent refresh, and the wire has to be sized by what changed. A
/// republish that moved nothing must leave the reader's cursor current, or
/// the whole navigator, ui state, status and pet sections ride the next
/// read for nothing.
#[test]
fn snapshot_delivery_leaves_the_rest_section_alone_when_a_republish_moves_nothing() {
    fn session() -> SessionSnapshotPayload {
        serde_json::from_value(serde_json::json!({
            "agents": [],
            "workspaces": [{"workspace_id": "w1", "label": "fixture"}],
            "panes": [{"pane_id": "w1:p1", "cwd": "/tmp/fixture"}],
            "tabs": [{"workspace_id": "w1", "tab_id": "w1:t1", "label": "1"}],
            "layouts": [{
                "workspace_id": "w1",
                "tab_id": "w1:t1",
                "zoomed": false,
                "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                "focused_pane_id": "w1:p1",
                "panes": [{
                    "pane_id": "w1:p1",
                    "rect": {"x": 0, "y": 0, "width": 80, "height": 24}
                }],
                "splits": []
            }]
        }))
        .expect("a session payload")
    }

    let mut runtime = runtime();
    runtime.ingest_session(Ok(session()));
    let first = runtime.snapshot_delta_payload(0, 0);
    assert!(first.rest.is_some(), "a fresh cursor reads the whole state");
    let caught_up = first.revision;

    runtime.ingest_session(Ok(session()));
    let second = runtime.snapshot_delta_payload(caught_up, first.terminal_sequence);
    assert!(
        second.rest.is_none(),
        "an identical republish must not restamp the rest section"
    );
    assert_eq!(
        second.revision, caught_up,
        "a republish that moved nothing leaves the reader current"
    );
}

/// R6, AC12. The delta used to be serialized with the runtime mutex held,
/// so every attach thread and the shell's next read waited behind the
/// whole navigator, ui state and terminal output going through serde.
///
/// The split is enforced twice. The signature is the first half: a
/// payload owns everything the wire needs, so the function that turns it
/// into bytes has no runtime in scope to lock. The call site is the
/// second: the guard is taken for the payload and gone before serde runs.
#[test]
fn snapshot_delivery_serializes_the_delta_outside_the_runtime_lock() {
    let mut runtime = runtime();
    let payload = runtime.snapshot_delta_payload(0, 0);
    // A payload outlives the borrow it came from, which is what lets the
    // caller drop the guard between the two halves.
    drop(runtime);
    let serialize: fn(&crate::model::SnapshotDeltaPayload) -> Result<Vec<u8>, serde_json::Error> =
        serialize_snapshot_delta;
    let bytes = serialize(&payload).expect("a payload serializes on its own");
    assert!(!bytes.is_empty(), "the wire is written from the payload");

    let ffi = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ffi.rs"))
        .expect("the ffi source");
    let entry_point = ffi
        .split_once("pub extern \"C\" fn herdr_core_snapshot(")
        .expect("the snapshot entry point")
        .1;
    let body = entry_point
        .split_once("#[unsafe(no_mangle)]")
        .expect("the entry point after it")
        .0;
    assert!(
        body.contains("serialize_snapshot_delta(&payload)"),
        "the shell's read must serialize through the free function: {body}"
    );
    let between = body
        .split_once("snapshot_delta_payload(")
        .expect("the locked half")
        .1
        .split_once("serialize_snapshot_delta(")
        .expect("the unlocked half after it")
        .0;
    assert!(
        between.lines().any(|line| line.trim() == "};"),
        "the block scoping the runtime guard must close before serialization: {body}"
    );

    let source =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime.rs"))
            .expect("the runtime source");
    let locked_half = source
        .split_once("pub fn snapshot_delta_payload(")
        .expect("the payload function")
        .1
        .split_once("\n    pub fn ")
        .expect("the function after it")
        .0;
    assert!(
        !locked_half.contains("serde_json"),
        "the half that runs under the lock must not serialize: {locked_half}"
    );
}

/// R3, AC5, SC1. A tab the operator leaves keeps its panes in the
/// projection, so the attach that feeds its terminal view is never
/// dropped and the scrollback is still arriving when they come back. The
/// projection used to be rebuilt from the arriving layout alone, which
/// took every other tab's panes out of it on each switch.
#[test]
fn retained_views_keep_a_visited_tab_in_the_projection_across_a_switch() {
    let checkout_path = "/private/tmp/hide-retained-views-switch";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2", "w-order:t3"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert_eq!(projected_pane_ids(&runtime), vec!["w-order:t1:p"]);

    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2",
    )));

    let projected = projected_pane_ids(&runtime);
    assert!(
        projected.contains(&"w-order:t1:p".to_owned()),
        "the tab left behind keeps its pane attached: {projected:?}"
    );
    assert!(projected.contains(&"w-order:t2:p".to_owned()));

    // And back again, with nothing having been rebuilt in between.
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    let returned = projected_pane_ids(&runtime);
    assert!(returned.contains(&"w-order:t1:p".to_owned()));
    assert!(returned.contains(&"w-order:t2:p".to_owned()));
}

/// R3, AC5, AC6. The half of retention the canvas cannot show: a tab the
/// operator is not looking at keeps producing output, and that output has
/// to reach the snapshot on its own sequence while it is hidden. If it
/// only arrived once the tab was visible again, coming back would replay
/// rather than resume.
#[test]
fn retained_views_keep_a_hidden_tabs_output_arriving() {
    let checkout_path = "/private/tmp/hide-retained-views-hidden-output";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    // Visit the second tab, which is what starts its attach, then leave it.
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    runtime
        .terminal_session_generations
        .insert("w-order:t2:p".to_owned(), 7);
    runtime.terminal_sessions.insert(
        "w-order:t2:p".to_owned(),
        TerminalSession::test_stub("w-order:t2:p", 7, TerminalSessionMode::Control),
    );
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t1")));
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert_eq!(runtime.snapshot().tab.id.as_deref(), Some("w-order:t1"));
    let before = runtime.snapshot().terminal.sequence;
    runtime
        .terminal_sizes
        .insert("w-order:t2:p".to_owned(), (40, 120));

    assert!(
        runtime.ingest_terminal_session_frame(
            "w-order:t2:p",
            7,
            TerminalSessionMode::Control,
            b"hidden tab still talking",
            crate::model::TerminalFrame {
                width: 120,
                height: 40,
                full: true
            },
        ) == Some(true),
        "the session behind a hidden tab is still delivering"
    );

    let snapshot = runtime.snapshot();
    assert_eq!(
        snapshot.terminal.sequence,
        before + 1,
        "the chunk rides the cursor, so a hidden tab costs one sequence step and no resend"
    );
    let arrived = snapshot
        .terminal
        .chunks
        .last()
        .expect("the chunk that just arrived");
    assert_eq!(arrived.pane_id, "w-order:t2:p");
    assert_eq!(arrived.sequence, before + 1);
    assert_eq!(
        live::decode_base64(&arrived.bytes_base64).expect("chunk bytes"),
        b"hidden tab still talking".to_vec()
    );
}

/// AC5. A hidden pane's size is what the shell last reported for it, and a
/// tab switch neither reports a new one nor lets the core invent one.
/// Geometry decides the PTY size, so a size that moved while a pane was
/// out of sight would reflow its contents behind the operator's back.
#[test]
fn retained_views_leave_a_hidden_panes_size_alone_across_a_switch() {
    let checkout_path = "/private/tmp/hide-retained-views-size";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    let resize = serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "terminal_resize",
        "payload": {"pane_id": "w-order:t1:p", "rows": 40, "cols": 120}
    }))
    .expect("resize event");
    runtime.dispatch_json(&resize);
    assert_eq!(
        runtime.terminal_sizes.get("w-order:t1:p").copied(),
        Some((40, 120))
    );

    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2",
    )));

    assert_eq!(
        runtime.terminal_sizes.get("w-order:t1:p").copied(),
        Some((40, 120)),
        "the hidden pane keeps the size it was last given"
    );
}

/// AC5, rule 1. Retention is bounded by the session. A pane whose tab has
/// gone leaves the projection on the next update rather than accumulating
/// there for the life of the process.
#[test]
fn retained_views_drop_a_pane_whose_tab_left_the_session() {
    let checkout_path = "/private/tmp/hide-retained-views-closed";
    let (mut runtime, checkout_id) = live_tab_order_runtime(checkout_path);
    let tabs = ["w-order:t1", "w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t1",
    )));
    assert!(runtime.dispatch_json(&focus_tab_event(&checkout_id, "w-order:t2")));
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &tabs,
        &tabs,
        "w-order:t2",
    )));
    assert!(projected_pane_ids(&runtime).contains(&"w-order:t1:p".to_owned()));

    let remaining = ["w-order:t2"];
    runtime.ingest_session(Ok(tab_order_payload(
        checkout_path,
        &remaining,
        &remaining,
        "w-order:t2",
    )));

    assert_eq!(projected_pane_ids(&runtime), vec!["w-order:t2:p"]);
}

/// AC5, R3. The shell drew one canvas keyed by the visible tab, so every
/// switch destroyed its terminal views and the new ones started empty and
/// reported a size. The surface now draws every visited tab and hides all
/// but one, which is the mechanism zoom already uses for panes. Removing
/// either half brings the blank frame and the switch-time resize back.
#[test]
fn retained_views_have_no_single_canvas_keyed_by_the_visible_tab() {
    let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS");
    let surface =
        std::fs::read_to_string(shell.join("HideUI.swift")).expect("the terminal surface source");
    let presentation = std::fs::read_to_string(shell.join("ShellView.swift"))
        .expect("the pane grid presentation source");
    assert!(
        surface.contains("model.retainedTabCanvases"),
        "the terminal surface no longer draws every visited tab"
    );
    assert!(
        surface.contains(".opacity(canvas.isVisible ? 1 : 0)"),
        "a hidden tab is removed from the view tree instead of being hidden"
    );
    assert!(
        presentation.contains("func retainedCanvases("),
        "the rule deciding which tabs keep a canvas is gone"
    );
}

/// The diagnostics list rides the revisioned rest section, so it keeps a
/// bounded number of the newest entries.
#[test]
fn diagnostics_keep_the_newest_entries_up_to_the_retention() {
    let mut runtime = runtime();
    for index in 0..(DIAGNOSTIC_RETENTION + 10) {
        runtime.push_diagnostic("test.entry", format!("entry {index}"));
    }
    let diagnostics = &runtime.snapshot().status.diagnostics;
    assert_eq!(diagnostics.len(), DIAGNOSTIC_RETENTION);
    assert_eq!(
        diagnostics[0].message, "entry 10",
        "the oldest entries went first"
    );
    assert_eq!(
        diagnostics[DIAGNOSTIC_RETENTION - 1].message,
        format!("entry {}", DIAGNOSTIC_RETENTION + 9)
    );
}

/// R2: the window draws no system titlebar and keeps its title string.
///
/// What the window got is an AppKit answer, so the proof lives in the
/// Swift suite `MainWindowChromeTests`, which builds a window, applies the
/// chrome, and asks AppKit. `swift test --filter` exits zero when its
/// filter matches nothing, so a check bound to that suite would go green
/// if the suite were deleted. This is the guard that closes: it fails if
/// the chrome stops being applied, and it fails if the suite that proves
/// it is gone.
#[test]
fn main_window_hides_the_system_titlebar_and_keeps_its_title() {
    let shell = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos");
    let read = |relative: &str| {
        std::fs::read_to_string(shell.join(relative))
            .unwrap_or_else(|_| panic!("the shell no longer has {relative}"))
    };

    let chrome = read("Sources/HerdrMacOS/MainWindowChrome.swift");
    for setting in [
        "static let title = \"hide\"",
        "window.styleMask.insert(.fullSizeContentView)",
        "window.titlebarAppearsTransparent = true",
        "window.titleVisibility = .hidden",
        "hosting.safeAreaRegions = []",
    ] {
        assert!(
            chrome.contains(setting),
            "the window chrome no longer says `{setting}`"
        );
    }

    let app = read("Sources/HerdrMacOS/HerdrApp.swift");
    assert!(
        app.contains("MainWindowChrome.apply(to: window"),
        "the main window no longer takes its chrome from MainWindowChrome"
    );
    assert!(
        !app.contains("window.title ="),
        "the window title is set beside the chrome again, so the two can disagree"
    );

    let suite = read("Tests/HerdrMacOSTests/MainWindowChromeTests.swift");
    for probe in [
        "window.styleMask.contains(.fullSizeContentView)",
        "window.titleVisibility == .hidden",
        "window.title == \"hide\"",
        "firstRow.origin.y == 0",
    ] {
        assert!(
            suite.contains(probe),
            "the window chrome suite no longer asks AppKit for `{probe}`"
        );
    }
}

/// R7: the strip's height, the traffic-light inset, and the spacing
/// between the first row's controls come from `HideTheme`.
///
/// A number written at the call site is how two surfaces that should
/// match drift apart, and the traffic lights are the case where drifting
/// puts a control underneath a system button.
#[test]
fn first_row_metrics_come_from_theme_tokens_and_not_from_view_literals() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS/HideUI.swift"),
    )
    .expect("the shell's SwiftUI source");

    let tokens = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../macos/Sources/HerdrMacOS/HideTheme.swift"),
    )
    .expect("the shell's theme token source");

    for token in ["tabStripHeight", "trafficLightInset"] {
        assert!(
            tokens.contains(&format!("static let {token}: CGFloat")),
            "HideTheme.Layout no longer declares {token}"
        );
    }

    let mut offenders = Vec::new();
    for declaration in [
        "private struct HideTabStrip: View {",
        "private struct HideBrandHeader: View {",
    ] {
        for line in shell_view_body(&source, declaration).lines() {
            let trimmed = line.trim();
            // Spacing between controls, the padding that clears the
            // traffic lights, and the row's own height. A square control
            // written `width:height:` is a control's size rather than one
            // of those three, so it is not this rule's business.
            let measured = trimmed
                .split_once(".padding(")
                .or_else(|| trimmed.split_once(".frame(height:"))
                .or_else(|| trimmed.split_once("HStack(spacing:"))
                .or_else(|| trimmed.split_once("VStack(spacing:"));
            let Some((_, arguments)) = measured else {
                continue;
            };
            let head = arguments.split(')').next().unwrap_or(arguments);
            let value = head.rsplit(',').next().unwrap_or(head).trim();
            // Zero is the absence of spacing rather than a design value.
            if value == "0" {
                continue;
            }
            if value.starts_with(|c: char| c.is_ascii_digit()) {
                offenders.push(format!("{declaration} -> {trimmed}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the window's first row measures itself with literals: {offenders:#?}"
    );
}
