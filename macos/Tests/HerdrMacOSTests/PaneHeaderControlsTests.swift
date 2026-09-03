import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Pane header controls")
struct PaneHeaderControlsTests {
    @Test func forkIsOfferedOnlyWhereTheCoreSaysAForkCouldSucceed() {
        // The core has already weighed the agent kind and its recorded session;
        // the header must not second-guess that answer, only render it.
        #expect(PaneHeaderControls.showsFork(CorePaneFork(available: true)))
        #expect(!PaneHeaderControls.showsFork(CorePaneFork(available: false)))
    }

    @Test func aPaneHerdrRecordsAsSpawnedFromAnotherIsMarkedAsAFork() {
        #expect(PaneHeaderControls.forkMark(CorePaneFork(forkedFromPaneID: "w1:p2")) == "w1:p2")
        #expect(PaneHeaderControls.forkMark(CorePaneFork()) == nil)
        // A blank lineage value is no lineage: a mark with nothing to name
        // would claim a parent that was never recorded.
        #expect(PaneHeaderControls.forkMark(CorePaneFork(forkedFromPaneID: "  ")) == nil)
    }

    @Test func closingAWorkingPaneStatesTheConsequenceFirst() throws {
        // The core decides. The header used to keep its own list of state
        // names, which is how the header came to offer a close the core then
        // rejected; now there is one answer and the header reads it.
        #expect(PaneHeaderControls.closeRequiresConfirmation(try pane(risky: true)))
        #expect(!PaneHeaderControls.closeRequiresConfirmation(try pane(risky: false)))
    }

    private func pane(risky: Bool) throws -> CorePaneSnapshot {
        let json = """
        {"id":"w1:p1","cwd":"/checkout","status_label":"Working",
         "requires_close_confirmation":\(risky),"summary":null,
         "activity_at_unix_ms":null,"fork":{"available":false,"forked_from_pane_id":null}}
        """
        return try JSONDecoder().decode(CorePaneSnapshot.self, from: Data(json.utf8))
    }

    @Test func aPortIndicatorOpensTheLoopbackAddressOverHttp() {
        // The listener's bound address is not the address to open: a server on
        // `*:5173` and one on `127.0.0.1:5173` are both localhost from here.
        #expect(PaneHeaderControls.portURL(5173)?.absoluteString == "http://localhost:5173")
        #expect(PaneHeaderControls.portURL(80)?.absoluteString == "http://localhost:80")
    }

    @Test func aPaneWithNoForkFactsDecodesAsNeitherForkableNorAFork() throws {
        // Every pane the core ships carries the section, but a pane snapshot
        // written before it must not decode into a header offering a fork.
        let json = """
        {"id":"w1:p1","cwd":"/checkout","status_label":"Attached",
         "requires_close_confirmation":false,"summary":null,
         "activity_at_unix_ms":null,"fork":{"available":false,"forked_from_pane_id":null}}
        """
        let pane = try JSONDecoder().decode(CorePaneSnapshot.self, from: Data(json.utf8))
        #expect(!PaneHeaderControls.showsFork(pane.fork))
        #expect(PaneHeaderControls.forkMark(pane.fork) == nil)
        #expect(pane.ports.isEmpty)
    }
}
