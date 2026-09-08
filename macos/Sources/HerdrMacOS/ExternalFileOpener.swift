import AppKit
import Foundation

/// Hands a path outside every registered checkout to macOS.
///
/// This is the same kind of boundary `ExternalBrowser` already is: Hide shows
/// what it owns and lets the operating system show the rest. A file goes to
/// its default application and a folder opens a Finder window, which is the
/// choice the operator made ("1번은 후자로").
///
/// Revealing is separate because opening is running: link detection is a guess
/// made over arbitrary agent output, and `NSWorkspace.open` on an executable,
/// an application bundle or an installer starts it. One mis-click must not run
/// a program, so those are selected in Finder instead.
enum ExternalFileOpener {
    typealias CommandRunner = (
        _ executable: URL,
        _ arguments: [String],
        _ completion: @escaping @Sendable (_ status: Int32, _ standardError: String) -> Void
    ) throws -> Void

    /// - Parameter onFailure: called with a stated reason when macOS refuses.
    ///   Opening is asynchronous, so a failure cannot be returned; reporting it
    ///   is the only way it is observable at all.
    static func open(
        _ url: URL,
        workspace: NSWorkspace = .shared,
        onFailure: @escaping @MainActor (String) -> Void
    ) {
        workspace.open(url, configuration: NSWorkspace.OpenConfiguration()) { _, error in
            guard let error else { return }
            let message = "macOS could not open \(url.path): \(error.localizedDescription)"
            Task { @MainActor in onFailure(message) }
        }
    }

    /// Selects the item in Finder without opening it.
    static func reveal(_ url: URL, workspace: NSWorkspace = .shared) {
        workspace.activateFileViewerSelecting([url])
    }

    /// Opens a checkout with the operator's default text editor.
    ///
    /// `NSWorkspace.open` treats directories as Finder targets. macOS' `open
    /// -t` is the system boundary that instead resolves the configured default
    /// text editor, including for a directory-backed project.
    static func openInDefaultEditor(
        _ url: URL,
        runner: @escaping CommandRunner = runCommand,
        onFailure: @escaping @MainActor (String) -> Void
    ) {
        do {
            try runner(URL(fileURLWithPath: "/usr/bin/open"), ["-t", url.path]) { status, standardError in
                guard status != 0 else { return }
                let detail = standardError.trimmingCharacters(in: .whitespacesAndNewlines)
                let message = detail.isEmpty
                    ? "The default editor could not open \(url.path) (open exited \(status))."
                    : "The default editor could not open \(url.path): \(detail)"
                Task { @MainActor in onFailure(message) }
            }
        } catch {
            let message = "The default editor could not open \(url.path): \(error.localizedDescription)"
            Task { @MainActor in onFailure(message) }
        }
    }

    private static func runCommand(
        executable: URL,
        arguments: [String],
        completion: @escaping @Sendable (Int32, String) -> Void
    ) throws {
        let process = Process()
        let standardError = Pipe()
        process.executableURL = executable
        process.arguments = arguments
        process.standardError = standardError
        process.terminationHandler = { process in
            let data = standardError.fileHandleForReading.readDataToEndOfFile()
            completion(process.terminationStatus, String(decoding: data, as: UTF8.self))
        }
        try process.run()
    }
}
