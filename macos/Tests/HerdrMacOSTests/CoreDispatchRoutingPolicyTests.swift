import Testing

@testable import HerdrMacOS

@Suite("Core dispatch routing policy")
struct CoreDispatchRoutingPolicyTests {
    @Test func remoteTerminalEventsReachTheTargetScopedCoreSession() {
        for kind in [
            "key",
            "terminal_click",
            "terminal_resize",
            "terminal_scroll",
            "terminal_viewport",
        ] {
            #expect(
                !CoreDispatchRoutingPolicy.blocks(
                    kind: kind,
                    payload: ["pane_id": "remote:mini:pane:w1:p1"],
                    remoteDeviceID: "mini"
                )
            )
        }
    }

    @Test func remoteTerminalEventsCannotFallThroughToALocalOrDifferentRemotePane() {
        for kind in [
            "key",
            "terminal_click",
            "terminal_resize",
            "terminal_scroll",
            "terminal_viewport",
        ] {
            #expect(CoreDispatchRoutingPolicy.blocks(
                kind: kind,
                payload: ["pane_id": "w1:p1"],
                remoteDeviceID: "mini"
            ))
            #expect(CoreDispatchRoutingPolicy.blocks(
                kind: kind,
                payload: ["pane_id": "remote:build:pane:w1:p1"],
                remoteDeviceID: "mini"
            ))
            #expect(CoreDispatchRoutingPolicy.blocks(
                kind: kind,
                remoteDeviceID: "mini"
            ))
        }
    }

    @Test func remoteLocalHerdrMutationsStillRequireTheExplicitRemoteControlContract() {
        for kind in [
            "close_tab",
            "close_pane",
            "create_pane",
            "create_tab",
            "create_worktree",
            "create_workspace",
            "focus_checkout",
            "focus_pane",
            "focus_tab",
            "fork_pane",
            "git_worktree_open",
            "migrate_main_branch",
            "reconnect_pane",
            "reopen_closed",
            "remove_worktree",
            "reorder_tab",
            "resize_pane",
            "toggle_zoom",
        ] {
            #expect(
                CoreDispatchRoutingPolicy.blocks(
                    kind: kind,
                    remoteDeviceID: "mini"
                )
            )
        }
    }

    @Test func localDeviceNeverTripsTheRemoteSafetyBoundary() {
        #expect(
            !CoreDispatchRoutingPolicy.blocks(
                kind: "focus_pane",
                remoteDeviceID: nil
            )
        )
    }
}

@Suite("Local Herdr mutation dispatch policy")
struct LocalHerdrMutationDispatchPolicyTests {
    @Test @MainActor func everyLocalHerdrMutationIsRejectedBeforeTheRuntimeIsReady() {
        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--state-path",
            "/tmp/hide-local-herdr-dispatch-policy-test-state.json",
        ])
        var rejectedReadiness: [LocalHerdrMutationReadiness] = []
        bridge.localHerdrMutationRejectionHandler = { readiness in
            rejectedReadiness.append(readiness)
        }
        let expectedMessage = HideStartupDiagnostic.initializing
        let localMutationKinds = [
            "close_pane",
            "close_tab",
            "create_pane",
            "create_tab",
            "create_worktree",
            "create_workspace",
            "focus_checkout",
            "focus_pane",
            "focus_tab",
            "fork_pane",
            "git_worktree_open",
            "key",
            "migrate_main_branch",
            "reconnect_pane",
            "reopen_closed",
            "remove_worktree",
            "reorder_tab",
            "resize_pane",
            "terminal_click",
            "terminal_resize",
            "terminal_scroll",
            "terminal_viewport",
            "toggle_zoom",
        ]

        for kind in localMutationKinds {
            #expect(bridge.dispatch(kind: kind, payload: [:]) == .rejected(expectedMessage))
        }

        // The two geometry reports are refused like the rest but never
        // presented; see the test below.
        #expect(rejectedReadiness.count == localMutationKinds.count - 2)
        #expect(rejectedReadiness.allSatisfy { $0 == .initializing(expectedMessage) })
    }

    /// A terminal view lays itself out before the first Herdr connection and
    /// reports its grid on its own. That report is refused, and the host
    /// replays it once connected, but it is not an operator action: showing
    /// its refusal put "Waiting for the first herdr connection attempt" in a
    /// modal on every launch.
    @Test @MainActor func aRefusedGeometryReportIsNotPresentedAsARefusedAction() {
        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--state-path",
            "/tmp/hide-local-herdr-geometry-report-test-state.json",
        ])
        var presented = 0
        bridge.localHerdrMutationRejectionHandler = { _ in presented += 1 }
        let expectedMessage = HideStartupDiagnostic.initializing

        for kind in ["terminal_viewport", "terminal_resize"] {
            #expect(LocalHerdrMutationDispatchPolicy.requiresConnectedHerdr(kind: kind))
            #expect(!LocalHerdrMutationDispatchPolicy.presentsRejection(kind: kind))
            #expect(bridge.dispatch(kind: kind, payload: [:]) == .rejected(expectedMessage))
        }
        #expect(presented == 0)

        // An operator's action before the connection is still presented.
        #expect(LocalHerdrMutationDispatchPolicy.presentsRejection(kind: "create_tab"))
        #expect(bridge.dispatch(kind: "create_tab", payload: [:]) == .rejected(expectedMessage))
        #expect(presented == 1)
    }

    @Test func localOnlyAndRemoteEventsRemainAvailableDuringRecovery() {
        for kind in [
            "editor_text_scale",
            "file_open",
            "pet_toggle_visible",
            "remote_control",
            "remote_file_list",
            "ui_state_update",
        ] {
            #expect(!LocalHerdrMutationDispatchPolicy.requiresConnectedHerdr(kind: kind))
        }
    }

    @Test func remoteTerminalInputDoesNotDependOnLocalHerdrReadiness() {
        #expect(!LocalHerdrMutationDispatchPolicy.requiresConnectedHerdr(
            kind: "key",
            whenDeviceIsRemote: true
        ))
    }
}
