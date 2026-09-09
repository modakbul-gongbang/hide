import Foundation
import Testing
@testable import HerdrMacOS

@Suite("File document state", .serialized)
@MainActor
struct FileDocumentStateTests {
    @Test func scheduledSaveCannotWriteIntoTheNextFileTab() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("file-save-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let first = root.appendingPathComponent("first.md")
        let second = root.appendingPathComponent("second.md")
        try "First original".write(to: first, atomically: true, encoding: .utf8)
        try "Second original".write(to: second, atomically: true, encoding: .utf8)
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually { model.focusedCheckout != nil }
        model.openFile(first)
        try await eventually { bridge.snapshot?.editor.path == first.path }
        let firstID = try #require(bridge.snapshot?.editor.activeTabID)
        bridge.updateDraft("First changed")
        bridge.scheduleFileSave("First changed")
        bridge.setFileView(tabID: firstID, preview: true, wrap: true)
        model.openFile(second)
        try await eventually { bridge.snapshot?.editor.path == second.path }
        // Let the first save complete after a different file is visibly active.
        try await eventually { (try? String(contentsOf: first, encoding: .utf8)) == "First changed" }
        #expect(try String(contentsOf: second, encoding: .utf8) == "Second original")
        bridge.focusFileTab(firstID)
        try await eventually { bridge.snapshot?.editor.activeTabID == firstID }
        #expect(bridge.snapshot?.editor.contentsUTF8 == "First changed")
        #expect(bridge.snapshot?.editor.tabs.first { $0.id == firstID }?.wrap == true)
    }

    private func eventually(_ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while !predicate(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        #expect(predicate())
    }
}
