import Testing

@testable import HerdrMacOS

@Suite("Agent MRU and switcher")
struct AgentMRUTests {
    @Test func focusObservationIsDeterministicAndRemovesUnavailablePanes() {
        var mru = AgentMRU()

        mru.observe(focusedPaneID: nil, availablePaneIDs: ["p1", "p2"])
        #expect(mru.paneIDs == ["p1", "p2"])

        mru.observe(focusedPaneID: "p1", availablePaneIDs: ["p1", "p2"])
        mru.observe(focusedPaneID: "p1", availablePaneIDs: ["p1", "p2"])
        #expect(mru.paneIDs == ["p1", "p2"])

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
        #expect(
            AgentSwitcherCycle(originalPaneID: "p1", paneIDs: ["p1"], direction: .backward) == nil
        )
    }

    @Test func openingBackwardStartsAtTheLeastRecentAgent() {
        // Index 0 is the agent already focused. Forward skips to the previous
        // one; backward has to land on the far end, not back on the current.
        var forward = AgentSwitcherCycle(originalPaneID: "p1", paneIDs: ["p1", "p2", "p3", "p4"])
        var backward = AgentSwitcherCycle(
            originalPaneID: "p1",
            paneIDs: ["p1", "p2", "p3", "p4"],
            direction: .backward
        )

        #expect(forward?.selectedPaneID == "p2")
        #expect(backward?.selectedPaneID == "p4")

        forward?.advance()
        backward?.retreat()
        #expect(forward?.selectedPaneID == "p3")
        #expect(backward?.selectedPaneID == "p3")
    }

    @Test func retreatWrapsPastTheFirstEntryOntoTheLast() {
        var cycle = AgentSwitcherCycle(originalPaneID: "p1", paneIDs: ["p1", "p2", "p3"])

        cycle?.retreat()
        #expect(cycle?.selectedPaneID == "p1")
        cycle?.retreat()
        #expect(cycle?.selectedPaneID == "p3")
    }

    @Test func advanceAndRetreatAreInverses() {
        var cycle = AgentSwitcherCycle(originalPaneID: "p1", paneIDs: ["p1", "p2", "p3", "p4"])
        let start = cycle?.selectedPaneID

        cycle?.advance()
        cycle?.advance()
        cycle?.retreat()
        cycle?.retreat()

        #expect(cycle?.selectedPaneID == start)
    }

    @Test func twoAgentsOpenOnTheSameEntryInBothDirections() {
        // With one other agent there is nowhere else to go, so the reverse
        // chord must not select the already-focused pane.
        let forward = AgentSwitcherCycle(originalPaneID: "p1", paneIDs: ["p1", "p2"])
        let backward = AgentSwitcherCycle(
            originalPaneID: "p1",
            paneIDs: ["p1", "p2"],
            direction: .backward
        )

        #expect(forward?.selectedPaneID == "p2")
        #expect(backward?.selectedPaneID == "p2")
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
