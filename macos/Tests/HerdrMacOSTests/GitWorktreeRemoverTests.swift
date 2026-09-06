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
        try runGit(["commit", "-m", "seed"], at: repository)
        try runGit(["worktree", "add", "-b", "linked", worktree.path], at: repository)
        try "dirty".write(to: worktree.appendingPathComponent("seed.txt"), atomically: true, encoding: .utf8)

        let head = gitOutput(["rev-parse", "HEAD"], at: worktree)
        let refused = GitWorktreeRemover.remove(repositoryRoot: repository.path, path: worktree.path,
            expectedHeadSHA: head, expectedBranch: "linked", protectedBaseBranch: "main")

        #expect(!refused.succeeded)
        #expect(FileManager.default.fileExists(atPath: worktree.path))
        try "seed".write(to: worktree.appendingPathComponent("seed.txt"), atomically: true, encoding: .utf8)

        let removed = GitWorktreeRemover.remove(repositoryRoot: repository.path, path: worktree.path,
            expectedHeadSHA: head, expectedBranch: "linked", protectedBaseBranch: "main")

        #expect(removed.succeeded)
        #expect(!FileManager.default.fileExists(atPath: worktree.path))
    }

    @Test func missingRegistrationIsRemovedAndBranchIsKept() throws {
        try withFixture { repository, worktree in
            let head = gitOutput(["rev-parse", "HEAD"], at: worktree)
            try FileManager.default.removeItem(at: worktree)
            let result = GitWorktreeRemover.remove(repositoryRoot: repository.path, path: worktree.path,
                expectedHeadSHA: head, expectedBranch: "linked", protectedBaseBranch: "main")
            #expect(result.succeeded)
            #expect(!gitOutput(["worktree", "list", "--porcelain"], at: repository).contains(worktree.path))
            #expect(gitOutput(["branch", "--list", "linked"], at: repository).contains("linked"))
        }
    }

    @Test func optionalSafeBranchDeletionRunsAfterRemoval() throws {
        try withFixture { repository, worktree in
            let head = gitOutput(["rev-parse", "HEAD"], at: worktree)
            let result = GitWorktreeRemover.remove(repositoryRoot: repository.path, path: worktree.path,
                expectedHeadSHA: head, expectedBranch: "linked", protectedBaseBranch: "main", branch: "linked")
            #expect(result.succeeded)
            #expect(!FileManager.default.fileExists(atPath: worktree.path))
            #expect(gitOutput(["branch", "--list", "linked"], at: repository).isEmpty)
        }
    }

    @Test func branchAdvancesAfterConfirmationThenSafeDeletionKeepsIt() throws {
        try withFixture { repository, worktree in
            try "later".write(to: worktree.appendingPathComponent("later.txt"), atomically: true, encoding: .utf8)
            try runGit(["add", "later.txt"], at: worktree)
            try runGit(["-c", "user.name=Hide Tests", "-c", "user.email=hide@example.invalid", "commit", "-m", "later"], at: worktree)
            let head = gitOutput(["rev-parse", "HEAD"], at: worktree)
            let result = GitWorktreeRemover.remove(repositoryRoot: repository.path, path: worktree.path,
                expectedHeadSHA: head, expectedBranch: "linked", protectedBaseBranch: "main", branch: "linked")
            #expect(result.succeeded)
            #expect(result.message.contains("branch linked remains"))
            #expect(!FileManager.default.fileExists(atPath: worktree.path))
            #expect(gitOutput(["branch", "--list", "linked"], at: repository).contains("linked"))
        }
    }

    @Test func changedIdentityAfterConfirmationIsNotRemoved() throws {
        try withFixture { repository, worktree in
            let confirmedHead = gitOutput(["rev-parse", "HEAD"], at: worktree)
            try runGit(["-c", "user.name=Hide Tests", "-c", "user.email=hide@example.invalid",
                        "commit", "--allow-empty", "-m", "changed after confirmation"], at: worktree)
            let result = GitWorktreeRemover.remove(repositoryRoot: repository.path, path: worktree.path,
                expectedHeadSHA: confirmedHead, expectedBranch: "linked", protectedBaseBranch: "main")
            #expect(!result.succeeded)
            #expect(result.message.contains("identity changed"))
            #expect(FileManager.default.fileExists(atPath: worktree.path))
        }
    }

    private func withFixture(_ body: (URL, URL) throws -> Void) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("hide-remover-\(UUID().uuidString)")
        let repository = root.appendingPathComponent("repository")
        let worktree = root.appendingPathComponent("linked")
        try FileManager.default.createDirectory(at: repository, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try runGit(["init"], at: repository)
        try runGit(["-c", "user.name=Hide Tests", "-c", "user.email=hide@example.invalid", "commit", "--allow-empty", "-m", "seed"], at: repository)
        try runGit(["worktree", "add", "-b", "linked", worktree.path], at: repository)
        try body(repository, worktree)
    }

    private func gitOutput(_ arguments: [String], at root: URL) -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", root.path] + arguments
        let pipe = Pipe()
        process.standardOutput = pipe
        do { try process.run() } catch { Issue.record("Git failed: \(error)"); return "" }
        let output = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return String(decoding: output, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Git, told to ignore whoever is running it.
    ///
    /// The throwaway repository this builds must not inherit the operator's
    /// global configuration. It did, and a machine configured to sign every
    /// commit failed the seed commit with "failed to write commit object" -
    /// a test that passed or failed on a setting that has nothing to do with
    /// removing a worktree.
    private func runGit(_ arguments: [String], at root: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = [
            "-C", root.path,
            "-c", "user.name=Hide Tests",
            "-c", "user.email=hide@example.invalid",
            "-c", "commit.gpgsign=false",
        ] + arguments
        var environment = ProcessInfo.processInfo.environment
        environment["GIT_CONFIG_GLOBAL"] = "/dev/null"
        environment["GIT_CONFIG_SYSTEM"] = "/dev/null"
        process.environment = environment
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }
}
