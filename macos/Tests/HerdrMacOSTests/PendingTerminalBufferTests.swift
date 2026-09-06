import Foundation
import Testing
@testable import HerdrMacOS

/// AC10, R8. A pane in a tab nobody has opened has no view to hand its bytes
/// to, so they were held with no bound at all and grew for the life of the
/// process. The core keeps 512 chunks and no more, so anything held past that
/// could never be drawn.
@Suite("Held terminal bytes")
struct PendingTerminalBufferTests {
    @Test func aPaneWithNoViewHoldsAtMostTheChunksTheCoreItselfKeeps() {
        var buffer = PendingTerminalBuffer<[UInt8]>()
        var dropped = 0
        for index in 0..<600 {
            dropped += buffer.append([UInt8(index % 251)], for: "w1:p1")
        }

        #expect(buffer.count(for: "w1:p1") == PendingTerminalBuffer<[UInt8]>.chunkLimit)
        #expect(dropped == 600 - PendingTerminalBuffer<[UInt8]>.chunkLimit)
    }

    /// The oldest chunks are the ones that go, so what is drawn when a view
    /// finally registers is the most recent output rather than the first.
    @Test func theOldestChunksAreTheOnesDropped() {
        var buffer = PendingTerminalBuffer<[UInt8]>()
        for index in 0..<(PendingTerminalBuffer<[UInt8]>.chunkLimit + 3) {
            buffer.append([UInt8(index % 251)], for: "w1:p1")
        }

        let held = buffer.take("w1:p1")
        #expect(held?.first == [3])
        #expect(held?.last == [UInt8((PendingTerminalBuffer<[UInt8]>.chunkLimit + 2) % 251)])
        #expect(buffer.count(for: "w1:p1") == 0)
    }

    /// A released session or a pane that has left means the next visit redraws
    /// from Herdr's own full frame, so the held bytes are dropped rather than
    /// carried.
    @Test func aPaneThatIsNoLongerDrawableLosesWhatWasHeldForIt() {
        var buffer = PendingTerminalBuffer<[UInt8]>()
        buffer.append([1], for: "w1:p1")
        buffer.append([2], for: "w1:p2")
        buffer.append([3], for: "w1:p3")

        let leaving = buffer.retain(paneIDs: ["w1:p2"])

        #expect(Set(leaving) == ["w1:p1", "w1:p3"])
        #expect(buffer.paneIDs == ["w1:p2"])
        #expect(buffer.count(for: "w1:p1") == 0)

        buffer.clear("w1:p2")
        #expect(buffer.paneIDs.isEmpty)
    }

    @Test func takingFromAPaneThatHeldNothingReturnsNothing() {
        var buffer = PendingTerminalBuffer<[UInt8]>()
        #expect(buffer.take("w1:p9") == nil)
        #expect(buffer.retain(paneIDs: []).isEmpty)
    }
}

/// R8. What may be dropped, and what must survive a tick that says nothing.
@Suite("Pending terminal retention")
struct PendingTerminalRetentionTests {
    private func layouts(_ json: String) throws -> [CorePaneLayoutSnapshot] {
        try JSONDecoder().decode([CorePaneLayoutSnapshot].self, from: Data(json.utf8))
    }

    private func transportPanes(_ json: String) throws -> [CoreTerminalPaneSnapshot] {
        try JSONDecoder().decode([CoreTerminalPaneSnapshot].self, from: Data(json.utf8))
    }

    private let twoPaneTab = """
    [{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,
      "root":{"type":"split","direction":"right","ratio":0.5,
              "first":{"type":"pane","pane_id":"w1:p1"},
              "second":{"type":"pane","pane_id":"w1:p2"}}}]
    """

    /// The regression. A tick taken while the core has emptied the transport
    /// projection lists no pane, and the frames held for a view that is still
    /// being built must not be thrown away on it.
    @Test func anEmptyTransportProjectionDropsNothing() throws {
        let keep = try PendingTerminalRetention.keep(
            layouts: layouts(twoPaneTab),
            transportPanes: transportPanes("[]")
        )
        #expect(keep == ["w1:p1", "w1:p2"])
    }

    @Test func aReleasedPanesBytesGo() throws {
        let keep = try PendingTerminalRetention.keep(
            layouts: layouts(twoPaneTab),
            transportPanes: transportPanes("""
            [{"pane_id":"w1:p1","closed":false,"transport_state":"controlling"},
             {"pane_id":"w1:p2","closed":false,"transport_state":"released"}]
            """)
        )
        #expect(keep == ["w1:p1"])
    }

    @Test func aPaneNoLayoutHoldsAnyMoreIsNotKept() throws {
        let keep = try PendingTerminalRetention.keep(
            layouts: layouts(twoPaneTab),
            transportPanes: transportPanes("""
            [{"pane_id":"w1:p9","closed":false,"transport_state":"controlling"}]
            """)
        )
        #expect(keep?.contains("w1:p9") == false)
    }

    /// With no layout at all the snapshot cannot say which panes exist, so it
    /// cannot justify dropping anything either.
    @Test func noLayoutsDecideNothing() throws {
        #expect(try PendingTerminalRetention.keep(
            layouts: layouts("[]"),
            transportPanes: transportPanes("[]")
        ) == nil)
    }
}
