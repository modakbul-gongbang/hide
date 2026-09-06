import AppKit
import Foundation

/// Opens a directory in Finder.
///
/// A terminal link that names a directory outside the explorer's tree has no
/// view inside Hide, so it is handed to Finder the way a web address is
/// handed to the browser.
enum ExternalFinder {
    /// - Parameter onFailure: called with a stated reason when Finder does
    ///   not open the directory. Opening is asynchronous, so a failure cannot
    ///   be returned; reporting it is the only way it is observable at all.
    static func open(
        _ directory: URL,
        workspace: NSWorkspace = .shared,
        onFailure: @escaping @MainActor (String) -> Void
    ) {
        workspace.open(directory, configuration: NSWorkspace.OpenConfiguration()) { _, error in
            guard let error else { return }
            let message = "Finder could not open \(directory.path): \(error.localizedDescription)"
            Task { @MainActor in onFailure(message) }
        }
    }
}
