import AppKit
import Foundation

/// Opens a web address outside Hide.
///
/// Chrome is the operator's browser, so a link opens there when it is
/// installed and in the default browser when it is not. Both the terminal link
/// router and the pane header's port indicator send addresses here rather than
/// each deciding for itself which application to hand a URL to.
enum ExternalBrowser {
    static let chromeBundleIdentifier = "com.google.Chrome"

    /// - Parameter onFailure: called with a stated reason when the address
    ///   reaches no application. Opening is asynchronous, so a failure cannot
    ///   be returned; reporting it is the only way it is observable at all.
    static func open(
        _ url: URL,
        workspace: NSWorkspace = .shared,
        onFailure: @escaping @MainActor (String) -> Void
    ) {
        let configuration = NSWorkspace.OpenConfiguration()
        if let chrome = workspace.urlForApplication(withBundleIdentifier: chromeBundleIdentifier) {
            workspace.open([url], withApplicationAt: chrome, configuration: configuration) { _, error in
                guard let error else { return }
                let message = "Chrome could not open \(url.absoluteString): \(error.localizedDescription)"
                Task { @MainActor in onFailure(message) }
            }
            return
        }
        workspace.open(url, configuration: configuration) { _, error in
            guard let error else { return }
            let message = "The default browser could not open \(url.absoluteString): \(error.localizedDescription)"
            Task { @MainActor in onFailure(message) }
        }
    }
}
