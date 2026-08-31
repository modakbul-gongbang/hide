import Testing

@testable import HerdrMacOS

@Suite("Agent MRU and switcher")
struct AgentMRUTests {
    @Test func focusObservationIsDeterministicAndRemovesUnavailablePanes() {
        var mru = AgentMRU()

        mru.observe(focusedPaneID: nil, availablePaneIDs: ["p1", "p2"])
        #expect(mru.paneIDs.isEmpty)

        mru.observe(focusedPaneID: "p1", availablePaneIDs: ["p1", "p2"])
        mru.observe(focusedPaneID: "p1", availablePaneIDs: ["p1", "p2"])
        #expect(mru.paneIDs == ["p1"])

        mru.observe(focusedPaneID: "p2", availablePaneIDs: ["p1", "p2"])
        #expect(mru.paneIDs == ["p2", "p1"])

        mru.observe(focusedPaneID: nil, availablePaneIDs: ["p1"])
        #expect(mru.paneIDs == ["p1"])

        mru.observe(focusedPaneID: "removed", availablePaneIDs: ["p1"])
        #expect(mru.paneIDs == ["p1"])
    }

    @Test func emptyAndSingleAgentNeverOpenACycle() {
        #expect(AgentSwitcherCycle(originalPaneID: nil, paneIDs: []) == nil)
        #expect(AgentSwitcherCycle(originalPaneID: "p1", paneIDs: ["p1"]) == nil)
    }

    @Test func manyAgentsCycleFromThePreviousPaneAndWrapOncePerAdvance() throws {
        var cycle = try #require(AgentSwitcherCycle(
            originalPaneID: "p1",
            paneIDs: ["p1", "p2", "p2", "p3"]
        ))

        #expect(cycle.paneIDs == ["p1", "p2", "p3"])
        #expect(cycle.selectedPaneID == "p2")
        cycle.advance()
        #expect(cycle.selectedPaneID == "p3")
        cycle.advance()
        #expect(cycle.selectedPaneID == "p1")
        #expect(cycle.committedPaneID(availablePaneIDs: ["p1", "p2", "p3"]) == "p1")
    }

    @Test func removedSelectionCannotCommitAndCancelKeepsTheOriginalFocus() throws {
        let focusedPaneID = "p1"
        var cycle: AgentSwitcherCycle? = try #require(AgentSwitcherCycle(
            originalPaneID: focusedPaneID,
            paneIDs: ["p1", "p2", "p3"]
        ))

        cycle?.advance()
        #expect(cycle?.selectedPaneID == "p3")
        #expect(cycle?.committedPaneID(availablePaneIDs: ["p1", "p2"]) == nil)
        #expect(cycle?.originalPaneID == focusedPaneID)

        cycle = nil
        #expect(cycle == nil)
        #expect(focusedPaneID == "p1")
    }
}
