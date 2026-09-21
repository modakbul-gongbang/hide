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

    @Test func keyboardSelectionOpensHighlightedFileAndClearsFilteredOrRetiredResults() throws {
        let paths = ["first.txt", "second.txt", "한글-검토.md"]
        let rows = WorkspaceFileSearchIndex.ranked(paths: paths, query: "")
        var selection = HideSearchSelection()
        selection.reconcile(rows.map(\.id))
        #expect(selection.entry(in: rows)?.relativePath == "first.txt")
        selection.move(.down, among: rows.map(\.id))
        #expect(selection.entry(in: rows)?.relativePath == "second.txt")
        selection.move(.up, among: rows.map(\.id))
        #expect(selection.entry(in: rows)?.relativePath == "first.txt")
        selection.move(.down, among: rows.map(\.id))
        let retired = rows.filter { $0.relativePath != "second.txt" }
        #expect(selection.entry(in: retired) == nil)
        selection.reconcile(retired.map(\.id))
        #expect(selection.entry(in: retired)?.relativePath == "first.txt")

        let empty = WorkspaceFileSearchIndex.ranked(paths: paths, query: "no matching file")
        selection.reconcile(empty.map(\.id))
        for _ in 0..<1000 {
            selection.move(.down, among: [])
            selection.move(.up, among: [])
        }
        #expect(selection.entry(in: empty) == nil)
        let korean = WorkspaceFileSearchIndex.ranked(paths: paths, query: "한글")
        selection.reconcile(korean.map(\.id))
        #expect(selection.entry(in: korean)?.relativePath == "한글-검토.md")
        let bounded = WorkspaceFileSearchIndex.ranked(paths: (0..<1000).map { "file-\($0).txt" }, query: "")
        #expect(bounded.count == WorkspaceFileSearchIndex.resultLimit)
        selection.reconcile(bounded.map(\.id))
        for _ in 0..<1000 { selection.move(.down, among: bounded.map(\.id)) }
        #expect(selection.entry(in: bounded)?.relativePath == "file-79.txt")
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

    /// A listing wider than the 64 KiB pipe buffer completes: the load
    /// drains git's output before waiting, so git is never left blocked on a
    /// write nobody reads. The first version waited first, and one large
    /// repository hung its git and this load for good.
    @Test(.timeLimit(.minutes(1))) func gitIndexLoadsAListingWiderThanThePipeBuffer() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-file-search-wide-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try runGit(["init"], at: root)
        let count = 3000
        for index in 0..<count {
            let name = "untracked-file-with-a-long-enough-name-\(index).txt"
            try "".write(to: root.appendingPathComponent(name), atomically: true, encoding: .utf8)
        }

        let started = Date()
        let paths = try await WorkspaceFileSearchIndex.load(root: root)

        #expect(paths.count == count)
        #expect(Date().timeIntervalSince(started) < 30)
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
