import Foundation

struct GitWorktreeRemovalResult: Sendable {
    let succeeded: Bool
    let message: String
}

enum GitWorktreeRemover {
    static func remove(path: String) -> GitWorktreeRemovalResult {
        let target = URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL
        guard FileManager.default.fileExists(atPath: target.path) else {
            return GitWorktreeRemovalResult(
                succeeded: false,
                message: "The linked worktree no longer exists at \(target.path). Nothing was deleted."
            )
        }
        let inspection = run(arguments: ["-C", target.path, "rev-parse", "--is-inside-work-tree"])
        guard inspection.status == 0,
              String(decoding: inspection.output, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines) == "true"
        else {
            return GitWorktreeRemovalResult(
                succeeded: false,
                message: "Git did not recognize \(target.path) as a worktree. Nothing was deleted."
            )
        }
        let removal = run(arguments: ["-C", target.path, "worktree", "remove", "--", target.path])
        guard removal.status == 0 else {
            let detail = String(decoding: removal.errors, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return GitWorktreeRemovalResult(
                succeeded: false,
                message: detail.isEmpty
                    ? "git worktree remove failed for \(target.path). The checkout registration was kept."
                    : "git worktree remove failed: \(detail)"
            )
        }
        return GitWorktreeRemovalResult(succeeded: true, message: "Deleted \(target.path)")
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
