import AppKit
import SwiftUI

@MainActor
final class HerdrApplicationDelegate: NSObject, NSApplicationDelegate {
    let model = ShellModel()
    private var mainWindow: NSWindow?
    private var petWindowController: PetWindowController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard mainWindow == nil else { return }
        let content = ShellView()
            .environmentObject(model)
            .frame(
                minWidth: ShellMetrics.windowMinWidth,
                minHeight: ShellMetrics.windowMinHeight
            )
        let window = NSWindow(
            contentRect: NSRect(
                x: 0,
                y: 0,
                width: ShellMetrics.windowDefaultWidth,
                height: ShellMetrics.windowDefaultHeight
            ),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Herdr IDE"
        window.minSize = NSSize(
            width: ShellMetrics.windowMinWidth,
            height: ShellMetrics.windowMinHeight
        )
        window.isRestorable = false
        window.titlebarSeparatorStyle = .automatic
        window.contentView = NSHostingView(rootView: content)
        window.center()
        mainWindow = window
        window.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
        petWindowController = PetWindowController(mainWindow: window)
        petWindowController?.refreshVisibility()

        if CommandLine.arguments.contains("--verification-browser-open") {
            model.browser.openOrFocus()
        } else if CommandLine.arguments.contains("--verification-browser-refresh") {
            model.browser.refresh()
        }
        if CommandLine.arguments.contains("--verification-remote-mini") {
            model.remote.refreshMini()
        }
        #if DEBUG
        if let rawKind = Self.argumentValue("--verification-consequence", in: CommandLine.arguments),
           let kind = DestructiveTargetKind(rawValue: rawKind) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in
                self?.model.previewConsequence(kind)
            }
        }
        #endif
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    private static func argumentValue(_ flag: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else {
            return nil
        }
        return arguments[index + 1]
    }
}

@main
struct HerdrApp: App {
    @NSApplicationDelegateAdaptor(HerdrApplicationDelegate.self) private var appDelegate

    var body: some Scene {
        Settings {
            Text("Herdr IDE settings are not part of this additive batch.")
                .padding(24)
        }
        .commands {
            ShellCommands(model: appDelegate.model)
        }
    }
}

struct ShellCommands: Commands {
    @ObservedObject var model: ShellModel

    var body: some Commands {
        CommandMenu("Navigate") {
            Button("Focus Agents") {
                model.focus(.agents)
            }
            .keyboardShortcut("1", modifiers: .command)

            Button("Focus Terminal") {
                model.focus(.terminal)
            }
            .keyboardShortcut("2", modifiers: .command)

            Button("Focus Workbench") {
                model.focus(.workbench)
            }
            .keyboardShortcut("3", modifiers: .command)
        }
    }
}
