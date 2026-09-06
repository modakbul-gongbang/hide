import Testing
@testable import HerdrMacOS

@Suite("Settled terminal size")
struct SettledTerminalSizeTests {
    @Test func splitAndZoomSendOnlyTheStableGrid() {
        var policy = SettledTerminalSize()
        for cols in [1, 2, 84, 115] { policy.report(cols: cols, rows: 45) }
        #expect(policy.displayTick() == nil)
        #expect(policy.displayTick() == .init(cols: 115, rows: 45))
        #expect(policy.displayTick() == nil)
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 57, rows: 22)
        #expect(policy.displayTick() == nil)
        #expect(policy.displayTick() == .init(cols: 57, rows: 22))
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
        #expect(policy.displayTick() == .init(cols: 56, rows: 22))
        policy.report(cols: 56, rows: 20)
        #expect(policy.displayTick() == nil)
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
        #expect(policy.displayTick() == .init(cols: 56, rows: 22))
        policy.report(cols: 56, rows: 22)
        #expect(policy.displayTick() == nil)
    }
}
