import Foundation

struct GitWorktreeRemovalResult: Sendable {
    let succeeded: Bool
    let message: String
}

enum GitWorktreeRemover {
    private struct RegisteredWorktree {
        let path: String
        let headSHA: String?
        let branch: String?
    }

    static func remove(repositoryRoot: String, path: String,
                       expectedHeadSHA: String?, expectedBranch: String?,
                       protectedBaseBranch: String?, branch: String? = nil) -> GitWorktreeRemovalResult {
        let target = URL(fileURLWithPath: standardized(path), isDirectory: true)
        let root = URL(fileURLWithPath: standardized(repositoryRoot), isDirectory: true)
        HideLaunchTrace.mark("worktree.remove.started", detail: "repository_root=\(root.path) checkout_path=\(target.path)")
        let authorization = authorize(root: root, target: target, expectedHeadSHA: expectedHeadSHA,
                                      expectedBranch: expectedBranch,
                                      protectedBaseBranch: protectedBaseBranch)
        guard authorization.succeeded else { return authorization }
        let removal = run(arguments: ["-C", root.path, "worktree", "remove", "--", target.path])
        HideLaunchTrace.mark("worktree.remove.finished", detail: "checkout_path=\(target.path) status=\(removal.status)")
        guard removal.status == 0 else {
            let detail = String(decoding: removal.errors, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return GitWorktreeRemovalResult(succeeded: false,
                message: "git worktree remove failed: \(detail). The worktree remains; panes already closed stay closed.")
        }
        if let branch {
            HideLaunchTrace.mark("worktree.branch_delete.started", detail: "checkout_path=\(target.path) branch=\(branch)")
            let deletion = run(arguments: ["-C", root.path, "branch", "-d", "--", branch])
            HideLaunchTrace.mark("worktree.branch_delete.finished", detail: "checkout_path=\(target.path) branch=\(branch) status=\(deletion.status)")
            if deletion.status != 0 {
                let detail = String(decoding: deletion.errors, as: UTF8.self)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                return GitWorktreeRemovalResult(succeeded: true,
                    message: "Deleted \(target.path); branch \(branch) remains: \(detail)")
            }
            return GitWorktreeRemovalResult(succeeded: true,
                message: "Deleted \(target.path) and local branch \(branch).")
        }
        return GitWorktreeRemovalResult(succeeded: true, message: "Deleted \(target.path). Its local branch was kept.")
    }

    private static func authorize(root: URL, target: URL, expectedHeadSHA: String?,
                                  expectedBranch: String?, protectedBaseBranch: String?)
        -> GitWorktreeRemovalResult
    {
        let listing = run(arguments: ["-C", root.path, "worktree", "list", "--porcelain"])
        guard listing.status == 0 else {
            return refused("could not re-read the worktree registration: \(errorText(listing))")
        }
        let records = parseWorktrees(String(decoding: listing.output, as: UTF8.self))
        guard let current = records.first(where: { standardized($0.path) == target.path }) else {
            return refused("the worktree is no longer registered at this path")
        }
        guard records.first.map({ standardized($0.path) != target.path }) ?? false else {
            return refused("the main worktree cannot be deleted")
        }
        guard current.headSHA == expectedHeadSHA, current.branch == expectedBranch else {
            return refused("the worktree identity changed after confirmation")
        }
        if current.branch == protectedBaseBranch {
            return refused("the worktree now holds the protected base branch")
        }
        let prefix = target.path.hasSuffix("/") ? target.path : target.path + "/"
        if records.contains(where: { standardized($0.path).hasPrefix(prefix) }) {
            return refused("the worktree now contains a nested worktree")
        }
        if FileManager.default.fileExists(atPath: target.path) {
            let status = run(arguments: ["-C", target.path, "status", "--porcelain", "--untracked-files=all"])
            guard status.status == 0 else {
                return refused("could not recheck the worktree state: \(errorText(status))")
            }
            guard status.output.isEmpty else {
                return refused("the worktree became dirty after confirmation")
            }
        }
        return GitWorktreeRemovalResult(succeeded: true, message: "authorized")
    }

    private static func parseWorktrees(_ output: String) -> [RegisteredWorktree] {
        output.components(separatedBy: "\n\n").compactMap { record in
            var path: String?
            var head: String?
            var branch: String?
            for line in record.split(separator: "\n", omittingEmptySubsequences: true) {
                if line.hasPrefix("worktree ") { path = String(line.dropFirst("worktree ".count)) }
                if line.hasPrefix("HEAD ") { head = String(line.dropFirst("HEAD ".count)) }
                if line.hasPrefix("branch refs/heads/") {
                    branch = String(line.dropFirst("branch refs/heads/".count))
                }
            }
            return path.map { RegisteredWorktree(path: $0, headSHA: head, branch: branch) }
        }
    }

    private static func standardized(_ path: String) -> String {
        let url = URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL
        let parent = url.deletingLastPathComponent().resolvingSymlinksInPath()
        return parent.appendingPathComponent(url.lastPathComponent, isDirectory: true)
            .standardizedFileURL.path
    }

    private static func refused(_ detail: String) -> GitWorktreeRemovalResult {
        GitWorktreeRemovalResult(succeeded: false,
            message: "Worktree removal stopped: \(detail). The worktree remains; panes already closed stay closed.")
    }

    private static func errorText(_ result: (status: Int32, output: Data, errors: Data)) -> String {
        String(decoding: result.errors, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func run(arguments: [String]) -> (status: Int32, output: Data, errors: Data) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = arguments
        let output = Pipe()
        let errors = Pipe()
        process.standardOutput = output
        process.standardError = errors
        do {
            try process.run()
            process.waitUntilExit()
            return (
                process.terminationStatus,
                output.fileHandleForReading.readDataToEndOfFile(),
                errors.fileHandleForReading.readDataToEndOfFile()
            )
        } catch {
            return (127, Data(), Data(error.localizedDescription.utf8))
        }
    }
}
