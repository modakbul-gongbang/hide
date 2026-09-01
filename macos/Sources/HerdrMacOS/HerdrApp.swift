import AppKit
import Combine
import SwiftUI

@MainActor
enum MainWindowPresentation {
    static func present(_ window: NSWindow, application: NSApplication = .shared) {
        application.setActivationPolicy(.regular)
        application.activate(ignoringOtherApps: true)
        window.orderFrontRegardless()
        window.makeKey()
    }
}

@MainActor
final class HerdrApplicationDelegate: NSObject, NSApplicationDelegate {
    let model: ShellModel
    private var mainWindow: NSWindow?
    private var petWindowController: PetWindowController?
    private var petMenuBarController: PetMenuBarController?
    private var petHotkeyRegistrar: PetHotkeyRegistrar?
    private var petVisibilityObservation: AnyCancellable?
    private var paneKeyMonitor: Any?

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
        paneKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { [weak self] event in
            guard let self else { return event }
            // The reverse chord is checked first: it is the forward chord plus
            // Shift, so testing forward first would swallow it.
            if PaneKeyEventPolicy.isAgentSwitcherRetreat(event) {
                MainActor.assumeIsolated {
                    self.model.beginOrRetreatAgentSwitcher()
                }
                return nil
            }
            if PaneKeyEventPolicy.isAgentSwitcherAdvance(event) {
                MainActor.assumeIsolated {
                    self.model.beginOrAdvanceAgentSwitcher()
                }
                return nil
            }
            let switcherIsActive = MainActor.assumeIsolated {
                self.model.agentSwitcherCycle != nil
            }
            if event.type == .keyDown,
               event.keyCode == 53,
               switcherIsActive
            {
                MainActor.assumeIsolated {
                    self.model.cancelAgentSwitcher()
                }
                return nil
            }
            if event.type == .flagsChanged,
               switcherIsActive,
               !event.modifierFlags.contains(.option)
            {
                MainActor.assumeIsolated {
                    self.model.commitAgentSwitcher()
                }
                return nil
            }
            if UnifiedTabShortcutPolicy.isClose(event) {
                MainActor.assumeIsolated {
                    if self.model.performCloseShortcut() == .closeWindow {
                        self.mainWindow?.performClose(nil)
                    }
                }
                return nil
            }
            return event
        }
        presentMainWindow(window, source: "launch")
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
        // Let AppKit return to its launch loop and draw the main window
        // before runtime discovery can invoke a CLI or trigger a privacy
        // prompt. The diagnostic then has a real window to render into.
        DispatchQueue.main.async { [weak self] in
            guard let self, let mainWindow = self.mainWindow else {
                HideLaunchTrace.mark("runtime_initialization.failed", detail: "main_window_missing")
                return
            }
            if let menu = NSApplication.shared.mainMenu,
               PaneMenuPolicy.reserveCloseShortcut(in: menu)
            {
                HideLaunchTrace.mark("pane.close_shortcut.reserved")
            } else {
                HideLaunchTrace.mark(
                    "pane.close_shortcut.failed",
                    detail: "command_w_menu_item_missing"
                )
            }
            MainWindowPresentation.present(mainWindow)
            HideLaunchTrace.mark(
                "main_window.pre_runtime",
                detail: "visible_\(mainWindow.isVisible)_windows_\(NSApplication.shared.windows.count)"
            )
            self.model.core.startRuntimeInitialization()
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        guard let mainWindow else { return false }
        if !flag || !mainWindow.isVisible {
            presentMainWindow(mainWindow, source: "reopen")
        } else {
            NSApplication.shared.activate(ignoringOtherApps: true)
        }
        NSApplication.shared.setActivationPolicy(.regular)
        HideLaunchTrace.mark("application.reopen.handled", detail: flag ? "visible" : "restored")
        return true
    }

    /// LaunchServices may deliver applicationDidFinishLaunching while the
    /// app is still not active. `makeKeyAndOrderFront` is conditional in that
    /// state and can leave a live foreground process with no visible window.
    /// Activate first, then use unconditional ordering and verify the result
    /// on the next main-run-loop turn.
    private func presentMainWindow(_ window: NSWindow, source: String) {
        let application = NSApplication.shared
        MainWindowPresentation.present(window, application: application)
        HideLaunchTrace.mark(
            "main_window.visible",
            detail: "source_\(source)_visible_\(window.isVisible)_windows_\(application.windows.count)"
        )
        DispatchQueue.main.async { [weak self, weak window] in
            guard let self, let window else { return }
            if !window.isVisible {
                MainWindowPresentation.present(window, application: application)
                HideLaunchTrace.mark(
                    "main_window.reasserted",
                    detail: "source_\(source)_visible_\(window.isVisible)_windows_\(application.windows.count)"
                )
            } else {
                HideLaunchTrace.mark(
                    "main_window.observed",
                    detail: "source_\(source)_visible_true_windows_\(application.windows.count)"
                )
            }
            _ = self
        }
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
        false
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let paneKeyMonitor {
            NSEvent.removeMonitor(paneKeyMonitor)
            self.paneKeyMonitor = nil
        }
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
        CommandGroup(replacing: .newItem) {
            Button("New Tab") {
                model.addTab()
            }
            .keyboardShortcut("t", modifiers: .command)

            Button("New Agent") {
                model.openNewAgent()
            }
            .keyboardShortcut("n", modifiers: .command)

            Button("New Workspace") {
                model.openNewWorkspace()
            }
            .keyboardShortcut("n", modifiers: [.command, .shift])

            Button("Search") {
                model.openSearch()
            }
            .keyboardShortcut("k", modifiers: .command)

            Button("Open File") {
                model.openFileSearch()
            }
            .keyboardShortcut("p", modifiers: .command)

            Divider()

            Button("Close Tab") {
                if model.performCloseShortcut() == .closeWindow {
                    NSApplication.shared.keyWindow?.performClose(nil)
                }
            }
            .keyboardShortcut("w", modifiers: .command)
        }

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

            Divider()

            Button("Toggle Left Sidebar") {
                model.toggleLeftSidebar()
            }
            .keyboardShortcut("b", modifiers: .command)

            Button("Toggle Workbench") {
                model.toggleRightWorkbench()
            }
            .keyboardShortcut("b", modifiers: [.command, .option])
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
        return Button(command.title) {
            model.performPaneCommand(command)
        }
    }
}
