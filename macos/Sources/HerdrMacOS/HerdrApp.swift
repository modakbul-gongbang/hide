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
        let window = PaneCommandWindow(
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
        window.paneCommandModel = model
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
        if let rawKind = LaunchArguments.value("--verification-consequence", in: CommandLine.arguments),
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
}

@main
struct HerdrApp: App {
    @NSApplicationDelegateAdaptor(HerdrApplicationDelegate.self) private var appDelegate

    var body: some Scene {
        Settings {
            KeyboardSettingsView(model: appDelegate.model)
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

        CommandMenu("Pane") {
            paneButton(.splitRight)
            paneButton(.splitDown)
            Divider()
            paneButton(.toggleZoom)
            Divider()
            paneButton(.closePane)
        }
    }

    private func paneButton(_ command: PaneCommand) -> some View {
        let shortcut = model.shortcut(for: command)
        return Button(command.title) {
            model.performPaneCommand(command)
        }
        .keyboardShortcut(shortcut.keyEquivalent, modifiers: shortcut.eventModifiers)
    }
}
