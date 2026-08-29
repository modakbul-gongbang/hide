import AppKit
import Combine
import SwiftUI

@MainActor
final class HerdrApplicationDelegate: NSObject, NSApplicationDelegate {
    let model: ShellModel
    private var mainWindow: NSWindow?
    private var petWindowController: PetWindowController?
    private var petMenuBarController: PetMenuBarController?
    private var petHotkeyRegistrar: PetHotkeyRegistrar?
    private var petVisibilityObservation: AnyCancellable?

    override init() {
        let startedAt = Date()
        HideLaunchTrace.mark("delegate.init.begin")
        model = ShellModel()
        super.init()
        HideLaunchTrace.mark(
            "delegate.init.ready",
            durationMilliseconds: Int(Date().timeIntervalSince(startedAt) * 1_000)
        )
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard mainWindow == nil else { return }
        let startedAt = Date()
        HideLaunchTrace.mark("application.did_finish.begin")
        NSApplication.shared.setActivationPolicy(.regular)
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
        window.title = "hide"
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
        HideLaunchTrace.mark("main_window.visible")
        model.core.startRuntimeInitialization()
        petWindowController = PetWindowController(mainWindow: window, model: model)
        petWindowController?.refreshVisibility()

        // All four toggle surfaces write the same core visibility state, so
        // the menu bar only has to follow the snapshot to stay in step with
        // Settings, the shortcut, and the URL scheme.
        petMenuBarController = PetMenuBarController(model: model) { [weak self] in
            self?.openSettings()
        }
        petVisibilityObservation = model.core.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.petMenuBarController?.refresh() }
            }
        }

        let registrar = PetHotkeyRegistrar { [weak self] in
            self?.model.core.togglePetVisible()
        }
        petHotkeyRegistrar = registrar
        model.petShortcutRegistrar = { [weak registrar] accelerator in
            MainActor.assumeIsolated { registrar?.apply(accelerator: accelerator) }
        }
        // A stored accelerator is re-registered at launch; an unset one
        // registers nothing at all.
        if let stored = model.core.pet?.shortcut {
            let failure = registrar.apply(accelerator: stored)
            if failure != nil {
                model.core.updatePetShortcut(accelerator: stored, error: failure)
            }
        }

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
        HideLaunchTrace.mark(
            "application.did_finish.ready",
            durationMilliseconds: Int(Date().timeIntervalSince(startedAt) * 1_000)
        )
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        guard let mainWindow else { return false }
        if !flag || !mainWindow.isVisible {
            mainWindow.makeKeyAndOrderFront(sender)
        }
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)
        HideLaunchTrace.mark("application.reopen.handled", detail: flag ? "visible" : "restored")
        return true
    }

    /// `herdr-ide://show|hide|toggle`. The retired app's `herdr-pet://`
    /// scheme is not registered, so it never reaches this handler.
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            guard let command = PetURLCommand.parse(url) else {
                FileHandle.standardError.write(Data(("""
                {"component":"pet","kind":"url.unrecognised","url":"\(url.absoluteString)"}

                """).utf8))
                continue
            }
            switch command {
            case .show: model.core.setPetVisible(true)
            case .hide: model.core.setPetVisible(false)
            case .toggle: model.core.togglePetVisible()
            }
        }
        petMenuBarController?.refresh()
    }

    /// Opens the Settings scene from the pet's menu bar entry.
    ///
    /// SwiftUI installs the Settings action on the responder chain and its
    /// own menu item, not on NSApplication, so performing the menu item it
    /// already built is the reliable route; the selector send is the
    /// fallback, and its name changed in macOS 14.
    func openSettings() {
        NSApplication.shared.activate(ignoringOtherApps: true)
        if let appMenu = NSApplication.shared.mainMenu?.item(at: 0)?.submenu,
           let index = appMenu.items.firstIndex(where: {
               $0.title.hasPrefix("Settings") || $0.title.hasPrefix("Preferences")
           })
        {
            appMenu.performActionForItem(at: index)
            return
        }
        for selector in [
            Selector(("showSettingsWindow:")),
            Selector(("showPreferencesWindow:")),
        ] where NSApplication.shared.sendAction(selector, to: nil, from: nil) {
            return
        }
        FileHandle.standardError.write(Data("""
        {"component":"pet","kind":"settings.unavailable","message":"No Settings menu item or action was found"}

        """.utf8))
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
            HideSettingsView(model: appDelegate.model)
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
