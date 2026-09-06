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
}
