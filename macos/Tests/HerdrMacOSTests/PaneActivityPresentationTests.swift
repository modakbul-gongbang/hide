import Testing

@testable import HerdrMacOS

@Suite struct PaneActivityPresentationTests {
    @Test func pendingAndReadyPaneOperationsStayVisible() {
        #expect(suffix("pane.split.right.requested") == " · splitting right…")
        #expect(suffix("pane.split.down.requested") == " · splitting down…")
        #expect(suffix("pane.zoom.requested") == " · toggling zoom…")
        #expect(suffix("pane.attach.requested") == " · attaching…")
        #expect(suffix("pane.attach.ready") == " · attached")
    }

    @Test func theNewestPaneTransitionWins() {
        let diagnostics = [
            diagnostic("pane.split.right.requested", occurredAt: 1),
            diagnostic("pane.split.right", occurredAt: 2),
            diagnostic("pane.attach.requested", occurredAt: 3),
            diagnostic("pane.attach.ready", occurredAt: 4),
        ]
        #expect(PaneActivityPresentation.suffix(for: diagnostics) == " · attached")
    }

    @Test func unrelatedDiagnosticsDoNotInventPaneActivity() {
        #expect(PaneActivityPresentation.suffix(for: [
            diagnostic("ui_state.missing", occurredAt: 1),
        ]).isEmpty)
    }

    private func suffix(_ kind: String) -> String {
        PaneActivityPresentation.suffix(for: [diagnostic(kind, occurredAt: 1)])
    }

    private func diagnostic(_ kind: String, occurredAt: UInt64) -> CoreDiagnostic {
        CoreDiagnostic(kind: kind, message: kind, occurredAt: occurredAt)
    }
}
