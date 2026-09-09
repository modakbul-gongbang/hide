import Foundation
import Testing
@testable import HerdrMacOS

/// R12, SC8. A fork reports itself where the operator is looking: progress
/// after the pane's name, and a failure in the pane's own notice row. Neither
/// is a dialog. The modal this replaces made the operator dismiss a box before
/// they could try the fork again, and it covered the pane while they read it.
@Suite("Fork surface")
struct ForkSurfaceTests {
    @Test @MainActor func aForkThatCannotStartSaysSoOnItsOwnPaneAndRaisesNoDialog() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-fork-surface-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-fork-surface-state-\(UUID().uuidString).json")
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: stateURL)
        }

        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--verification-no-remote",
            "--workspace-root", root.path,
            "--state-path", stateURL.path,
        ])
        try await Task.sleep(for: .milliseconds(100))
        let model = ShellModel(core: bridge)
        let paneID = bridge.snapshot?.navigator.workspaces
            .flatMap(\.checkouts)
            .flatMap(\.tabs)
            .flatMap(\.panes)
            .first?
            .id ?? "w1:p1"

        model.forkPaneFromHeader(paneID)

        #expect(model.interactionNotice == nil, "a fork failure must not raise a dialog")
        #expect(model.paneNotice(for: paneID) != nil, "the reason belongs on the pane that was forked")
        #expect(model.paneActivity(for: paneID).isEmpty, "a fork that never started shows no progress")
        #expect(model.paneNotice(for: "some-other-pane") == nil, "the reason belongs to one pane only")

        // A pane the operator has not forked carries neither.
        #expect(model.paneActivity(for: "some-other-pane").isEmpty)
    }

    /// The reason has to land on the pane the operator forked. Pane ids share
    /// prefixes, so a failure for `w1:p10` must not appear on `w1:p1`, and the
    /// answer must not depend on which order a set happens to iterate in.
    @Test func aFailureLandsOnThePaneItNamesAndNotOnAPaneWhoseIdIsAPrefix() {
        let forking: Set<String> = ["w1:p1", "w1:p10", "w2:p3"]
        #expect(ForkFailurePresentation.owner(
            of: "Pane w1:p10 could not be forked: herdr refused",
            amongst: forking
        ) == "w1:p10")
        #expect(ForkFailurePresentation.owner(
            of: "Pane w1:p1 could not be forked: herdr refused",
            amongst: forking
        ) == "w1:p1")
        #expect(ForkFailurePresentation.owner(
            of: "Pane w9:p9 could not be forked: herdr refused",
            amongst: forking
        ) == nil, "a failure for a pane with no fork in flight belongs to no pane")

        // Same inputs, same answer, however the set is built.
        for _ in 0..<32 {
            #expect(ForkFailurePresentation.owner(
                of: "Pane w1:p10 could not be forked: herdr refused",
                amongst: Set(["w2:p3", "w1:p10", "w1:p1"])
            ) == "w1:p10")
        }
    }
}
