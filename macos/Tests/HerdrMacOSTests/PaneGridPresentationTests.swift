import AppKit
import Foundation
import SwiftTerm
import Testing
@testable import HerdrMacOS

@MainActor
@Suite("Pane grid presentation")
struct PaneGridPresentationTests {
    /// R7: a pane the user named shows that name, and only a pane with no name
    /// of its own falls back to the project every pane in it shares.
    @Test func paneHeaderPrefersHerdrLabelThenTerminalTitleThenWorkspaceThenPaneIdentifier() {
        #expect(PaneHeaderPresentation.title(
            herdrLabel: "  review agent  ",
            terminalTitle: "claude",
            workspaceLabel: "hide",
            paneID: "w1:p1"
        ) == "review agent")
        #expect(PaneHeaderPresentation.title(
            herdrLabel: nil,
            terminalTitle: "claude",
            workspaceLabel: "hide",
            paneID: "w1:p1"
        ) == "claude")
        // The sidebar's context label beats Claude Code's status title.
        #expect(PaneHeaderPresentation.title(
            herdrLabel: nil,
            agentSummary: "운영 DB 마이그레이션 실행",
            terminalTitle: "🍃 Claude is waiting fo",
            workspaceLabel: "hide",
            paneID: "w1:p1"
        ) == "운영 DB 마이그레이션 실행")
        #expect(PaneHeaderPresentation.title(
            herdrLabel: "   ",
            terminalTitle: "  ",
            workspaceLabel: "oh-my-principle",
            paneID: "w1:p1"
        ) == "oh-my-principle")
        #expect(PaneHeaderPresentation.title(
            herdrLabel: nil,
            terminalTitle: nil,
            workspaceLabel: nil,
            paneID: "w1:p1"
        ) == "w1:p1")
    }

    private func splitLayout(tabID: String, paneIDs: [String], focused: String, zoomed: Bool = false) throws -> CorePaneLayoutSnapshot {
        precondition(paneIDs.count == 2)
        let json = """
        {"workspace_id":"w1","tab_id":"\(tabID)","focused_pane_id":"\(focused)","zoomed":\(zoomed),        "root":{"type":"split","direction":"right","ratio":0.5,        "first":{"type":"pane","pane_id":"\(paneIDs[0])"},        "second":{"type":"pane","pane_id":"\(paneIDs[1])"}}}
        """
        return try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: Data(json.utf8))
    }

    /// R3, AC5. Switching tabs changes which canvas is on top, not which
    /// canvases exist. Every visited tab keeps its panes at exactly the frames
    /// it had, so its terminal views are never rebuilt and never report a new
    /// size to Herdr.
    @Test func aTabSwitchChangesVisibilityAndLeavesEveryVisitedCanvasInPlace() throws {
        let layouts = [
            try splitLayout(tabID: "w1:t1", paneIDs: ["w1:p1", "w1:p2"], focused: "w1:p2"),
            try splitLayout(tabID: "w1:t2", paneIDs: ["w1:p3", "w1:p4"], focused: "w1:p3"),
        ]
        let attached: Set<String> = ["w1:p1", "w1:p2", "w1:p3", "w1:p4"]

        let onFirst = PaneGridPresentation.retainedCanvases(
            tabIDs: ["w1:t1", "w1:t2"],
            layouts: layouts,
            attachedPaneIDs: attached,
            visibleTabID: "w1:t1",
            visibleFocusedPaneID: "w1:p2"
        )
        let onSecond = PaneGridPresentation.retainedCanvases(
            tabIDs: ["w1:t1", "w1:t2"],
            layouts: layouts,
            attachedPaneIDs: attached,
            visibleTabID: "w1:t2",
            visibleFocusedPaneID: "w1:p3"
        )

        #expect(onFirst.map(\.tabID) == ["w1:t1", "w1:t2"])
        #expect(onSecond.map(\.tabID) == ["w1:t1", "w1:t2"])
        #expect(onFirst.map(\.isVisible) == [true, false])
        #expect(onSecond.map(\.isVisible) == [false, true])
        for (before, after) in zip(onFirst, onSecond) {
            #expect(before.items.map(\.paneID) == after.items.map(\.paneID))
            #expect(
                before.items.map(\.visualFrame) == after.items.map(\.visualFrame),
                "a hidden tab keeps the geometry it had, so nothing resizes on a switch"
            )
        }
    }

    /// R3, SC2. A tab nobody has opened has no canvas, because building its
    /// terminal views would register panes Hide has not attached and report a
    /// size for them. Visiting it is what brings it in.
    @Test func anUnvisitedTabHasNoCanvasUntilItIsTheVisibleOne() throws {
        let layouts = [
            try splitLayout(tabID: "w1:t1", paneIDs: ["w1:p1", "w1:p2"], focused: "w1:p1"),
            try splitLayout(tabID: "w1:t2", paneIDs: ["w1:p3", "w1:p4"], focused: "w1:p3"),
        ]

        let beforeVisit = PaneGridPresentation.retainedCanvases(
            tabIDs: ["w1:t1", "w1:t2"],
            layouts: layouts,
            attachedPaneIDs: ["w1:p1", "w1:p2"],
            visibleTabID: "w1:t1",
            visibleFocusedPaneID: "w1:p1"
        )
        #expect(beforeVisit.map(\.tabID) == ["w1:t1"])

        let onVisit = PaneGridPresentation.retainedCanvases(
            tabIDs: ["w1:t1", "w1:t2"],
            layouts: layouts,
            attachedPaneIDs: ["w1:p1", "w1:p2"],
            visibleTabID: "w1:t2",
            visibleFocusedPaneID: "w1:p3"
        )
        #expect(onVisit.map(\.tabID) == ["w1:t1", "w1:t2"])
        #expect(
            onVisit.last?.items.map(\.paneID) == ["w1:p3", "w1:p4"],
            "the first visit draws Herdr's own layout for that tab, not a stand-in"
        )
    }

    /// R3. A hidden tab keeps its own focus ring where it left it. The core's
    /// focused pane names a pane in the visible tab, and lending it to a
    /// hidden canvas would ring nothing there at all.
    @Test func aHiddenCanvasKeepsItsOwnFocusedPaneRinged() throws {
        let layouts = [
            try splitLayout(tabID: "w1:t1", paneIDs: ["w1:p1", "w1:p2"], focused: "w1:p2"),
            try splitLayout(tabID: "w1:t2", paneIDs: ["w1:p3", "w1:p4"], focused: "w1:p4"),
        ]

        let canvases = PaneGridPresentation.retainedCanvases(
            tabIDs: ["w1:t1", "w1:t2"],
            layouts: layouts,
            attachedPaneIDs: ["w1:p1", "w1:p2", "w1:p3", "w1:p4"],
            visibleTabID: "w1:t2",
            visibleFocusedPaneID: "w1:p3"
        )

        let hidden = try #require(canvases.first(where: { !$0.isVisible }))
        let visible = try #require(canvases.first(where: { $0.isVisible }))
        #expect(hidden.items.filter(\.isFocused).map(\.paneID) == ["w1:p2"])
        #expect(visible.items.filter(\.isFocused).map(\.paneID) == ["w1:p3"])
    }

    @Test func authoritativeNestedLayoutDecodesWithoutChangingPaneOrder() throws {
        let data = Data(
            #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p3","zoomed":false,"root":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"split","direction":"down","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p2"},"second":{"type":"pane","pane_id":"w1:p3"}}}}"#.utf8
        )
        let layout = try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)

        #expect(layout.root.paneIDs == ["w1:p1", "w1:p2", "w1:p3"])
        #expect(PaneGridPresentation.visibleRoot(layout: layout).paneIDs == [
            "w1:p1", "w1:p2", "w1:p3",
        ])
    }

    @Test func nestedLayoutProjectsOneTypedDragDividerPerSplit() throws {
        let data = Data(
            #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,"root":{"type":"split","direction":"right","ratio":0.4,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"split","direction":"down","ratio":0.6,"first":{"type":"pane","pane_id":"w1:p2"},"second":{"type":"pane","pane_id":"w1:p3"}}}}"#.utf8
        )
        let layout = try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)

        let dividers = PaneGridPresentation.dividers(layout: layout)

        #expect(dividers.count == 2)
        #expect(dividers[0].paneID == "w1:p1")
        #expect(dividers[0].axis == .vertical)
        #expect(dividers[0].frame.x == 0.4)
        #expect(dividers[1].paneID == "w1:p2")
        #expect(dividers[1].axis == .horizontal)
        #expect(dividers[1].frame.y == 0.6)
    }

    @Test func aDividerReportsTheSplitItResizesRatherThanTheWholeCanvas() throws {
        // Herdr measures a resize `amount` against the split's own rectangle.
        // The outer divider owns the full width; the inner one lives in the
        // 0.4-wide first column and in half the height, so a drag measured
        // against the canvas would move it at a fraction of the pointer.
        let data = Data(
            #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,"root":{"type":"split","direction":"right","ratio":0.4,"first":{"type":"split","direction":"down","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p2"},"second":{"type":"pane","pane_id":"w1:p3"}}},"second":{"type":"pane","pane_id":"w1:p4"}}}"#.utf8
        )
        let layout = try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)

        let dividers = PaneGridPresentation.dividers(layout: layout)

        #expect(dividers.map(\.span) == [1, 1, 0.4])
        #expect(dividers[2].axis == .vertical)
    }

    @Test func aDividerKeepsItsIdentityWhileTheSplitItDragsMoves() throws {
        // `ForEach` recreates a row whose id changed, which during a drag tears
        // down the gesture mid-flight: the split moved once and then froze.
        // The id must therefore survive the ratio it is dragging.
        func dividers(ratio: String) throws -> [PaneGridDivider] {
            let data = Data(
                #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,"root":{"type":"split","direction":"right","ratio":RATIO,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"pane","pane_id":"w1:p2"}}}"#
                    .replacingOccurrences(of: "RATIO", with: ratio).utf8
            )
            return PaneGridPresentation.dividers(
                layout: try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)
            )
        }

        let before = try dividers(ratio: "0.5")
        let after = try dividers(ratio: "0.62")

        #expect(before.map(\.id) == after.map(\.id))
        #expect(before[0].frame.x != after[0].frame.x)
    }

    @Test func everyDividerInANestedLayoutCarriesItsOwnIdentity() throws {
        // Dropping the frame from the id is only safe while what remains still
        // separates the dividers of one layout.
        let data = Data(
            #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,"root":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"split","direction":"down","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"pane","pane_id":"w1:p2"}},"second":{"type":"split","direction":"down","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p3"},"second":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p4"},"second":{"type":"pane","pane_id":"w1:p5"}}}}}"#.utf8
        )
        let layout = try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)

        let dividers = PaneGridPresentation.dividers(layout: layout)

        #expect(dividers.count == 4)
        #expect(Set(dividers.map(\.id)).count == dividers.count)
    }

    @Test func zoomRetainsEveryPaneViewInsteadOfRecreatingHiddenTerminals() throws {
        let data = Data(
            #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p2","zoomed":true,"root":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"pane","pane_id":"w1:p2"}}}"#.utf8
        )
        let layout = try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)

        #expect(PaneGridPresentation.visibleRoot(layout: layout).paneIDs == ["w1:p1", "w1:p2"])
        let items = PaneGridPresentation.items(layout: layout)
        #expect(items.map(\.paneID) == ["w1:p1", "w1:p2"])
        #expect(items.allSatisfy { $0.retainedFrame.width > 0 && $0.retainedFrame.height > 0 })
        #expect(items.first { $0.paneID == "w1:p1" }?.isVisible == false)
        #expect(items.first { $0.paneID == "w1:p2" }?.visualFrame == .unit)
    }

    @Test func focusAccentFollowsOnlyTheAuthoritativePaneAndIsRepeatable() throws {
        let data = Data(
            #"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":true,"root":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"pane","pane_id":"w1:p2"}}}"#.utf8
        )
        let layout = try JSONDecoder().decode(CorePaneLayoutSnapshot.self, from: data)

        #expect(layout.focusedPaneID == "w1:p1")
        #expect(PaneGridPresentation.visibleRoot(layout: layout).paneIDs == ["w1:p1", "w1:p2"])
        let first = PaneGridPresentation.items(layout: layout)
        let repeated = PaneGridPresentation.items(layout: layout)
        #expect(first == repeated)
        #expect(first.filter(\.isFocused).map(\.paneID) == ["w1:p1"])
        #expect(first.first { $0.paneID == "w1:p2" }?.isFocused == false)
    }

    @Test func unzoomRestoresAuthoritativeFramesWithoutCollapsingHiddenPanes() throws {
        let root = #"{"type":"split","direction":"right","ratio":0.4,"first":{"type":"pane","pane_id":"w1:p1"},"second":{"type":"pane","pane_id":"w1:p2"}}"#
        let zoomed = try JSONDecoder().decode(
            CorePaneLayoutSnapshot.self,
            from: Data(#"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":true,"root":\#(root)}"#.utf8)
        )
        let restored = try JSONDecoder().decode(
            CorePaneLayoutSnapshot.self,
            from: Data(#"{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,"root":\#(root)}"#.utf8)
        )

        let zoomedItems = PaneGridPresentation.items(layout: zoomed)
        let restoredItems = PaneGridPresentation.items(layout: restored)
        #expect(zoomedItems.first { $0.paneID == "w1:p1" }?.visualFrame == .unit)
        #expect(zoomedItems.first { $0.paneID == "w1:p2" }?.retainedFrame.width == 0.6)
        #expect(zoomedItems.first { $0.paneID == "w1:p2" }?.retainedFrame.height == 1)
        #expect(restoredItems.map(\.visualFrame) == restoredItems.map(\.retainedFrame))
        #expect(restoredItems.allSatisfy { $0.visualFrame.width > 0 && $0.visualFrame.height > 0 })
    }

    @Test func terminalViewLeavesPaneCommandEquivalentsToTheMenuSystem() {
        let terminal = ImeTerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        )
        let events = [
            keyEvent(characters: "d", keyCode: 2, modifiers: [.command]),
            keyEvent(characters: "D", keyCode: 2, modifiers: [.command, .shift]),
            keyEvent(characters: "\r", keyCode: 36, modifiers: [.command, .option]),
            keyEvent(characters: "w", keyCode: 13, modifiers: [.command]),
        ]

        #expect(events.allSatisfy { !terminal.performKeyEquivalent(with: $0) })
    }

    private func keyEvent(
        characters: String,
        keyCode: UInt16,
        modifiers: NSEvent.ModifierFlags
    ) -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: modifiers,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters.lowercased(),
            isARepeat: false,
            keyCode: keyCode
        )!
    }
}
