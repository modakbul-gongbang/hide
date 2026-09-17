import Testing

@testable import HerdrMacOS

@Test func paneSelectionOperationMatchesOnlyItsSourceAndTarget() {
    let pending = PaneSelectionOperation(
        requestID: "request-1",
        sourcePaneID: "w1:p1",
        targetPaneID: "w1:p2",
        targetLabel: "Child task",
        phase: .pending
    )
    let failed = PaneSelectionOperation(
        requestID: pending.requestID,
        sourcePaneID: pending.sourcePaneID,
        targetPaneID: pending.targetPaneID,
        targetLabel: pending.targetLabel,
        phase: .failed(reason: "focus refused", retryable: true)
    )

    #expect(pending.isPending)
    #expect(!failed.isPending)
    #expect(pending.isFor(sourcePaneID: "w1:p1", targetPaneID: "w1:p2"))
    #expect(!pending.isFor(sourcePaneID: "w1:p9", targetPaneID: "w1:p2"))
    #expect(!pending.isFor(sourcePaneID: "w1:p1", targetPaneID: "w1:p9"))
}
