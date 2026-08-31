import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Workspace file search")
struct WorkspaceFileSearchTests {
    @Test func fuzzyRankingRewardsContiguousPathBoundaryMatches() {
        let ranked = WorkspaceFileSearchIndex.ranked(
            paths: ["Sources/WorkbenchViewer.swift", "docs/workbench-view.md", "Sources/WindowBridge.swift"],
            query: "wbv"
        )

        #expect(ranked.map(\.relativePath) == ["docs/workbench-view.md", "Sources/WorkbenchViewer.swift"])
    }

    @Test func gitIndexIncludesUntrackedFilesAndHonorsIgnoreRules() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-file-search-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try runGit(["init"], at: root)
        try "ignored.txt\n".write(to: root.appendingPathComponent(".gitignore"), atomically: true, encoding: .utf8)
        try "visible".write(to: root.appendingPathComponent("visible.txt"), atomically: true, encoding: .utf8)
        try "ignored".write(to: root.appendingPathComponent("ignored.txt"), atomically: true, encoding: .utf8)

        let paths = try await WorkspaceFileSearchIndex.load(root: root)

        #expect(paths.contains(".gitignore"))
        #expect(paths.contains("visible.txt"))
        #expect(!paths.contains("ignored.txt"))
    }

    private func runGit(_ arguments: [String], at root: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", root.path] + arguments
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }
}
