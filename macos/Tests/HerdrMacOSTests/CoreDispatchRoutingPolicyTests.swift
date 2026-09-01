import Testing

@testable import HerdrMacOS

@Suite("Core dispatch routing policy")
struct CoreDispatchRoutingPolicyTests {
    @Test func remoteTerminalEventsReachTheTargetScopedCoreSession() {
        for kind in ["key", "terminal_resize", "terminal_scroll"] {
            #expect(
                !CoreDispatchRoutingPolicy.blocks(
                    kind: kind,
                    whenDeviceIsRemote: true
                )
            )
        }
    }

    @Test func remoteTopologyEventsStillRequireTheExplicitRemoteControlContract() {
        for kind in [
            "reconnect_pane",
            "focus_pane",
            "focus_checkout",
            "focus_tab",
            "create_tab",
            "create_pane",
            "toggle_zoom",
            "close_pane",
        ] {
            #expect(
                CoreDispatchRoutingPolicy.blocks(
                    kind: kind,
                    whenDeviceIsRemote: true
                )
            )
        }
    }

    @Test func localDeviceNeverTripsTheRemoteSafetyBoundary() {
        #expect(
            !CoreDispatchRoutingPolicy.blocks(
                kind: "focus_pane",
                whenDeviceIsRemote: false
            )
        )
    }
}
