import AppKit
import SwiftTerm
import SwiftUI

/// Attaches a remote Herdr pane through the user's existing SSH configuration.
/// Hide never receives or stores credentials; ssh and the remote Herdr CLI own
/// authentication and session state.
struct RemoteTerminalHost: NSViewRepresentable {
    let remote: RemoteRuntimeModel
    let sshAlias: String
    let paneID: String

    func makeCoordinator() -> Coordinator {
        Coordinator(remote: remote, paneID: paneID)
    }

    func makeNSView(context: Context) -> LocalProcessTerminalView {
        let terminal = HideRemoteTerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(ofSize: 14, weight: .regular),
            options: .default
        )
        terminal.nativeForegroundColor = NSColor(calibratedWhite: 0.9, alpha: 1)
        terminal.nativeBackgroundColor = NSColor(calibratedRed: 0.045, green: 0.055, blue: 0.075, alpha: 1)
        terminal.processDelegate = context.coordinator
        terminal.setAccessibilityIdentifier("remote-terminal-\(paneID)")
        terminal.onPointerFocus = { [weak remote] in
            remote?.focusPane(paneID)
        }
        terminal.startProcess(
            executable: "/usr/bin/ssh",
            args: [
                sshAlias,
                "-tt",
                RemoteShellCommand.loginShell(RemoteShellCommand.attach(paneID: paneID)),
            ],
            environment: HideRuntimeEnvironment.childEnvironment().map { "\($0.key)=\($0.value)" }
        )
        context.coordinator.terminal = terminal
        return terminal
    }

    func updateNSView(_ terminal: LocalProcessTerminalView, context: Context) {}

    static func dismantleNSView(_ terminal: LocalProcessTerminalView, coordinator: Coordinator) {
        terminal.processDelegate = nil
        terminal.terminate()
    }

    final class Coordinator: NSObject, LocalProcessTerminalViewDelegate {
        let remote: RemoteRuntimeModel
        let paneID: String
        weak var terminal: LocalProcessTerminalView?

        init(remote: RemoteRuntimeModel, paneID: String) {
            self.remote = remote
            self.paneID = paneID
        }

        func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: LocalProcessTerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

        func processTerminated(source: TerminalView, exitCode: Int32?) {
            guard exitCode != 0 else { return }
            let paneID = self.paneID
            let terminationCode = exitCode
            Task { @MainActor [weak remote] in
                remote?.recordAttachFailure(paneID: paneID, exitCode: terminationCode)
            }
        }
    }
}
