import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Git worktree removal")
struct GitWorktreeRemoverTests {
    @Test func cleanLinkedWorktreeIsRemovedButDirtyWorktreeIsPreserved() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-worktree-remove-\(UUID().uuidString)", isDirectory: true)
        let repository = root.appendingPathComponent("repository", isDirectory: true)
        let worktree = root.appendingPathComponent("linked", isDirectory: true)
        try FileManager.default.createDirectory(at: repository, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try runGit(["init"], at: repository)
        try "seed".write(to: repository.appendingPathComponent("seed.txt"), atomically: true, encoding: .utf8)
        try runGit(["add", "seed.txt"], at: repository)
        try runGit(["-c", "user.name=Hide Tests", "-c", "user.email=hide@example.invalid", "commit", "-m", "seed"], at: repository)
        try runGit(["worktree", "add", "-b", "linked", worktree.path], at: repository)
        try "dirty".write(to: worktree.appendingPathComponent("seed.txt"), atomically: true, encoding: .utf8)

        let refused = GitWorktreeRemover.remove(path: worktree.path)

        #expect(!refused.succeeded)
        #expect(FileManager.default.fileExists(atPath: worktree.path))
        try "seed".write(to: worktree.appendingPathComponent("seed.txt"), atomically: true, encoding: .utf8)

        let removed = GitWorktreeRemover.remove(path: worktree.path)

        #expect(removed.succeeded)
        #expect(!FileManager.default.fileExists(atPath: worktree.path))
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
