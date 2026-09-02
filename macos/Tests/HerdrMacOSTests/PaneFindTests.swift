import AppKit
import SwiftTerm
import Testing
@testable import HerdrMacOS

/// `⌘F` routing and the find bar's result counter.
///
/// The defect behind these: the shell never wired a find chord at all, so
/// `⌘F` reached the terminal and was swallowed. The two things that can
/// silently regress are which surface the chord reveals, and whether a term
/// with no match says so instead of looking inert.
@Suite("Pane find")
struct PaneFindTests {
    @Test func theChordActsOnWhicheverSurfaceIsOnScreen() {
        // The editor overlays the terminal whenever a file tab is open, which
        // is the same rule the zoom chords use for their target.
        #expect(
            PaneFindPolicy.target(
                activeFileTabID: "file:w:c:/repo/a.swift",
                focusedPaneID: "w1:p1",
                isRemoteContext: false
            ) == .fileEditor
        )
        #expect(
            PaneFindPolicy.target(
                activeFileTabID: nil,
                focusedPaneID: "w1:p1",
                isRemoteContext: false
            ) == .terminal(paneID: "w1:p1")
        )
        // A remote context has no local editor, so an open file tab does not
        // redirect the chord away from the pane the operator is reading.
        #expect(
            PaneFindPolicy.target(
                activeFileTabID: "file:w:c:/repo/a.swift",
                focusedPaneID: "w1:p1",
                isRemoteContext: true
            ) == .terminal(paneID: "w1:p1")
        )
        // Nothing on screen reveals nothing, rather than a bar over an empty
        // surface.
        #expect(
            PaneFindPolicy.target(
                activeFileTabID: nil,
                focusedPaneID: nil,
                isRemoteContext: false
            ) == .none
        )
    }

    @Test func theCounterReportsAQueryWithNoMatchInsteadOfStayingBlank() {
        #expect(terminalFindBarSummary(term: "", index: 0, total: 0) == "")
        #expect(terminalFindBarSummary(term: "needle", index: 0, total: 0) == "No matches")
        #expect(terminalFindBarSummary(term: "needle", index: 2, total: 14) == "2/14")
        // Matches exist but the caret is not standing on one yet.
        #expect(terminalFindBarSummary(term: "needle", index: 0, total: 3) == "3 matches")
    }

    @Test @MainActor func everyMatchIsReportedWithItsPositionNotOnlyTheCurrentOne() {
        let terminal = ImeTerminalView(
            frame: NSRect(x: 0, y: 0, width: 640, height: 240),
            font: NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        )
        terminal.feed(text: "needle one\r\nsecond needle\r\nno match here\r\nneedle again\r\n")

        let positions = terminal.searchMatchPositions("needle")

        // Three lines carry the term, and the selection-only API could never
        // have said so: it reports one match at a time.
        #expect(positions.count == 3)
        #expect(positions.allSatisfy { $0.length == 6 })
        #expect(positions.map(\.col) == [0, 7, 0])
    }

    @Test @MainActor func theFindChordIsClaimedByTheShellAndNotLeftToTheTerminal() {
        #expect(ShellMenuCommand.findInPane.shortcut.key == "f")
        #expect(ShellMenuCommand.findInPane.shortcut.modifiers == [.command])
        #expect(ShellMenuCommand.findInPane.displayShortcut == "⌘F")
    }
}
