import AppKit
import Foundation
import SwiftTerm
import Testing
@testable import HerdrMacOS

@MainActor
@Suite("Pane grid presentation")
struct PaneGridPresentationTests {
    @Test func paneHeaderPrefersLabelThenFolderThenPaneIdentifier() {
        #expect(PaneHeaderPresentation.title(
            label: "  review agent  ",
            cwd: "/Users/example/projects/hide",
            paneID: "w1:p1"
        ) == "review agent")
        #expect(PaneHeaderPresentation.title(
            label: "   ",
            cwd: "/Users/example/projects/oh-my-principle",
            paneID: "w1:p1"
        ) == "oh-my-principle")
        #expect(PaneHeaderPresentation.title(
            label: "",
            cwd: "",
            paneID: "w1:p1"
        ) == "w1:p1")
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
