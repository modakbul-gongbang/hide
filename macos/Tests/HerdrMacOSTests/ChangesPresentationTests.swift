import Foundation
import Testing
@testable import HerdrMacOS

/// The right panel's section set and the changes view's reading of what the
/// core sends. Every case here is one the operator reported or one the PRD
/// names: a panel with exactly two sections, an empty list that must not read
/// as "no changes" when it has a reason, and the four statuses.
@Suite("Changes presentation")
struct ChangesPresentationTests {
    @Test func theRightPanelOffersExactlyTheExplorerAndTheChangesView() {
        #expect(RightPanelSection.allCases == [.explorer, .changes])
        #expect(RightPanelSection.allCases.map(\.title) == ["Explorer", "Changes"])
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
