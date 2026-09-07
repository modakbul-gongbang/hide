import Foundation
import AppKit
import SwiftUI
import Testing
@testable import HerdrMacOS

/// The right panel's section set and the changes view's reading of what the
/// core sends. Every case here is one the operator reported or one the PRD
/// names: a panel with exactly three sections, an empty list that must not read
/// as "no changes" when it has a reason, and the four statuses.
@Suite("Changes presentation")
struct ChangesPresentationTests {
    @Test(arguments: [false, true]) @MainActor
    func diffContentUsesTheFullEditorViewportFromTheTop(longDiff: Bool) async throws {
        let text = "@@ -1 +1 @@\n-old\n+new\n" + String(repeating: " context\n", count: longDiff ? 100 : 0)
        let diff = CoreChangedFileDiff(path: "/repo/settings.json", text: text, notice: nil)
        let host = NSHostingView(rootView: DiffText(diff: diff)
            .frame(maxWidth: .infinity, maxHeight: .infinity))
        host.frame = NSRect(x: 0, y: 0, width: 900, height: 600)
        let window = NSWindow(contentRect: host.frame, styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        defer { window.close() }
        host.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(50))
        host.layoutSubtreeIfNeeded()
        func descendants(_ view: NSView) -> [NSView] {
            view.subviews.flatMap { [$0] + descendants($0) }
        }
        let scrollView = try #require(descendants(host).compactMap { $0 as? NSScrollView }.first)
        let viewport = scrollView.convert(scrollView.bounds, to: host)
        #expect(viewport.height >= 599, "Even a three-line diff must occupy the tab's full viewport")
        #expect(viewport.minY <= 1, "Diff scrolling must start directly below the tab strip")
        if longDiff {
            #expect(try #require(scrollView.documentView).frame.height > viewport.height,
                    "Long diffs must remain scrollable below the viewport")
        }
        let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        var firstChangedRow: Int?
        for y in 0..<bitmap.pixelsHigh {
            for x in 0..<bitmap.pixelsWide {
                if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
                   color.redComponent > 0.4,
                   color.redComponent > color.greenComponent + 0.05 {
                    firstChangedRow = y
                    break
                }
            }
            if firstChangedRow != nil { break }
        }
        let firstRow = try #require(firstChangedRow)
        #expect(firstRow < 80, "Short diff content must start at the top, not the vertical center")
    }

    @Test func theRightPanelOffersExplorerChangesAndGit() {
        #expect(RightPanelSection.allCases == [.explorer, .changes, .git])
        #expect(RightPanelSection.allCases.map(\.title) == ["Explorer", "Changes", "Git"])
    }

    @Test func aChangesPayloadDecodesItsEntriesStatusesAndDiff() throws {
        let payload = """
        {
            "root_path": "/repo",
            "entries": [
                {"path": "/repo/a.rs", "relative_path": "a.rs", "status": "modified"},
                {"path": "/repo/b.rs", "relative_path": "b.rs", "status": "added"},
                {"path": "/repo/c.rs", "relative_path": "c.rs", "status": "deleted"},
                {"path": "/repo/d.txt", "relative_path": "d.txt", "status": "untracked"}
            ],
            "selected_path": "/repo/a.rs",
            "diff": {
                "path": "/repo/a.rs",
                "text": "@@ -1 +1 @@\\n-old\\n+new\\n",
                "notice": null
            },
            "unavailable_reason": null
        }
        """

        let decoded = try JSONDecoder().decode(CoreChangesSnapshot.self, from: Data(payload.utf8))

        #expect(decoded.entries.map(\.status) == [.modified, .added, .deleted, .untracked])
        #expect(decoded.entries.map(\.status.badge) == ["M", "A", "D", "U"])
        #expect(decoded.selectedPath == "/repo/a.rs")
        #expect(decoded.diff?.path == "/repo/a.rs")
        #expect(decoded.unavailableReason == nil)
    }

    @Test func aFailureCarriesItsReasonSoAnEmptyListIsNeverReadAsNoChanges() throws {
        let payload = """
        {
            "root_path": "/tmp/plain",
            "entries": [],
            "selected_path": null,
            "diff": null,
            "unavailable_reason": "/tmp/plain is not inside a Git repository"
        }
        """

        let decoded = try JSONDecoder().decode(CoreChangesSnapshot.self, from: Data(payload.utf8))

        #expect(decoded.entries.isEmpty)
        #expect(decoded.unavailableReason == "/tmp/plain is not inside a Git repository")
    }

    @Test func diffLinesAreClassifiedByPrefixWithHeadersReadBeforeTheSingleCharacterForms() {
        #expect(DiffLineKind.of("+added") == .added)
        #expect(DiffLineKind.of("-removed") == .removed)
        #expect(DiffLineKind.of(" context") == .context)
        #expect(DiffLineKind.of("@@ -1 +1 @@") == .hunk)
        // A file header starts with the same characters as a changed line and
        // would otherwise be tinted as an addition or a removal.
        #expect(DiffLineKind.of("+++ b/a.rs") == .hunk)
        #expect(DiffLineKind.of("--- a/a.rs") == .hunk)
        #expect(DiffLineKind.of("diff --git a/a.rs b/a.rs") == .hunk)
    }

    @Test func diffRowsCarryTheOldAndNewLineNumbersTheOperatorReads() {
        let rows = DiffLinePresentation.rows(in: """
        diff --git a/a.rs b/a.rs
        @@ -8,3 +12,4 @@
         shared
        -old
        +new
        +another
        """)

        #expect(rows.map(\.oldNumber) == [nil, nil, 8, 9, nil, nil])
        #expect(rows.map(\.newNumber) == [nil, nil, 12, nil, 13, 14])
    }

    @Test func editorLineNumbersFollowUTF16LocationsAcrossNewlines() {
        let text: NSString = "첫째\nsecond\nthird"
        #expect(CodeLineNumbers.number(at: 0, in: text) == 1)
        #expect(CodeLineNumbers.number(at: 3, in: text) == 2)
        #expect(CodeLineNumbers.number(at: text.length, in: text) == 3)
    }

    @Test func aStoredSectionSurvivesTheUIStateRoundTripAndDefaultsToTheExplorer() throws {
        let stored = """
        {
            "left_sidebar_visible": true,
            "right_panel_visible": true,
            "right_panel_section": "changes",
            "expanded_paths": [],
            "selected_path": null,
            "selected_pane_id": null
        }
        """
        let decoded = try JSONDecoder().decode(CoreUIStateSnapshot.self, from: Data(stored.utf8))
        #expect(decoded.rightPanelSection == .changes)

        let withoutSection = """
        {
            "left_sidebar_visible": true,
            "right_panel_visible": true,
            "expanded_paths": [],
            "selected_path": null,
            "selected_pane_id": null
        }
        """
        let defaulted = try JSONDecoder().decode(
            CoreUIStateSnapshot.self,
            from: Data(withoutSection.utf8)
        )
        #expect(defaulted.rightPanelSection == .explorer)
    }
}
