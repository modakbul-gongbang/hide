import Testing
@testable import HerdrMacOS

@Suite("Settled terminal size")
struct SettledTerminalSizeTests {
    @Test func splitAndZoomSendOnlyTheStableGrid() {
        var policy = SettledTerminalSize()
        for cols in [1, 2, 84, 115] { policy.report(cols: cols, rows: 45) }
        #expect(policy.displayTick() == nil)
        let first = SettledTerminalSize.Grid(cols: 115, rows: 45)
        #expect(policy.displayTick() == first)
        policy.markDelivered(first)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 57, rows: 22)
        #expect(policy.displayTick() == nil)
        let second = SettledTerminalSize.Grid(cols: 57, rows: 22)
        #expect(policy.displayTick() == second)
        policy.markDelivered(second)
        #expect(policy.displayTick() == nil)
    }

    @Test func zeroLayoutCancelsAWaitingCandidate() {
        var policy = SettledTerminalSize()
        policy.report(cols: 80, rows: 24)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 0, rows: 0)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 100, rows: 30)
        #expect(policy.displayTick() == nil)
        #expect(policy.displayTick() == .init(cols: 100, rows: 30))
    }

    @Test func returningThroughATransientGridRequestsOneSettledRepaint() {
        var policy = SettledTerminalSize()
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
        let first = SettledTerminalSize.Grid(cols: 56, rows: 22)
        #expect(policy.displayTick() == first)
        policy.markDelivered(first)
        policy.report(cols: 56, rows: 20)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
        #expect(policy.displayTick() == first)
        policy.markDelivered(first)
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
    }

    @Test func rejectedResizeRemainsPendingUntilAConnectedRetrySucceeds() {
        var policy = SettledTerminalSize()
        let grid = SettledTerminalSize.Grid(cols: 80, rows: 24)
        policy.report(cols: grid.cols, rows: grid.rows)

        #expect(policy.displayTick() == nil)
        #expect(policy.displayTick() == grid)
        #expect(policy.displayTick() == grid)

        policy.markDelivered(grid)
        #expect(policy.displayTick() == nil)
    }
}
