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
        // A write to a pipe whose reader has gone kills the process with
        // SIGPIPE unless the process says otherwise, and this process is
        // Swift's, so Rust's own ignore never applied to it. The core writes
        // to a herdr control session for every attached pane, and closing a
        // pane closes that session under it: the app exited with signal 13
        // on every pane close (2026-09-03, dev build, `exit=141`). Ignoring
        // the signal turns the write into the EPIPE error the core already
        // handles and logs.
        signal(SIGPIPE, SIG_IGN)
        // Before the core exists, so the tools it runs - `gh` above all - are
        // looked up on the operator's PATH rather than on Finder's.
        HideRuntimeEnvironment.applyPathToProcess()
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
        window.paneCommandModel = model
        window.minSize = NSSize(
            width: ShellMetrics.windowMinWidth,
            height: ShellMetrics.windowMinHeight
        )
        window.isRestorable = false
        // The delegate keeps the only strong reference to this window, and
        // AppKit's default for a programmatically created window is to release
        // it on close. Closing the window then left `mainWindow` pointing at
        // freed memory, and the next Dock activation crashed inside
        // `applicationShouldHandleReopen` with EXC_BAD_ACCESS while retaining
        // it. The delegate owns the window's lifetime; AppKit does not.
        window.isReleasedWhenClosed = false
        MainWindowChrome.apply(to: window, content: content)
        window.center()
        mainWindow = window
        paneKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
            guard let self else { return event }
            MainActor.assumeIsolated {
                _ = HideHintEventObserver.observe(event) { self.model.setShortcutModifiersHeld($0) }
            }
            if PaneKeyEventPolicy.isSidebarViewToggle(event) {
                MainActor.assumeIsolated {
                    self.model.toggleSidebarContent()
                }
                return nil
            }
            // The reverse chord is checked first: it is the forward chord plus
            // Shift, so testing forward first would swallow it.
            if PaneKeyEventPolicy.isTabSwitcherRetreat(event) {
                MainActor.assumeIsolated {
                    self.model.beginOrRetreatTabSwitcher()
                }
                return nil
            }
            if PaneKeyEventPolicy.isTabSwitcherAdvance(event) {
                MainActor.assumeIsolated {
                    self.model.beginOrAdvanceTabSwitcher()
                }
                return nil
            }
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
            let activeSwitchers = MainActor.assumeIsolated {
                (
                    agent: self.model.agentSwitcherCycle != nil,
                    tab: self.model.tabSwitcherCycle != nil
                )
            }
            if event.type == .keyUp,
               event.keyCode == 48,
               activeSwitchers.tab
            {
                // A real Option release arrives as flagsChanged below. Some
                // accessibility synthesizers omit that event, so re-check the
                // authoritative global flags after their chord finishes.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
                    guard let self, self.model.tabSwitcherCycle != nil else { return }
                    let globalModifiers = NSEvent.ModifierFlags(
                        rawValue: UInt(CGEventSource.flagsState(.combinedSessionState).rawValue)
                    )
                    if PaneKeyEventPolicy.shouldCommitTabSwitcherAfterKeyUp(
                        event,
                        currentModifiers: globalModifiers
                    ) {
                        self.model.commitTabSwitcher()
                    }
                }
                return nil
            }
            if event.type == .keyDown,
               event.keyCode == 53,
               activeSwitchers.agent || activeSwitchers.tab
            {
                MainActor.assumeIsolated {
                    self.model.cancelAgentSwitcher()
                    self.model.cancelTabSwitcher()
                }
                return nil
            }
            if event.type == .flagsChanged,
               activeSwitchers.tab,
               !event.modifierFlags.contains(.option)
            {
                MainActor.assumeIsolated {
                    self.model.commitTabSwitcher()
                }
                return nil
            }
            if event.type == .flagsChanged,
               activeSwitchers.agent,
               !event.modifierFlags.contains(.control)
            {
                MainActor.assumeIsolated {
                    self.model.commitAgentSwitcher()
                }
                return nil
            }
            if UnifiedTabShortcutPolicy.isClose(event) {
                MainActor.assumeIsolated {
                    self.model.performCloseShortcut()
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
        if CommandLine.arguments.contains("--verification-ui-fixture"),
           let scene = LaunchArguments.value("--verification-scene", in: CommandLine.arguments) {
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                switch scene {
                case "search": self.model.showSearch = true
                case "settings": self.model.showSettings = true
                case "new-chat": self.model.showComposer = true
                case "file-search": self.model.showFileSearch = true
                case "add-device": self.presentVerificationAddDeviceSheet()
                default:
                    self.model.interactionNotice = "Unknown verification scene: \(scene)"
                }
            }
        }
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
            #if DEBUG
            if CommandLine.arguments.contains("--verification-ui-fixture"),
               CommandLine.arguments.contains("--verification-background") {
                mainWindow.orderBack(nil)
            } else {
                MainWindowPresentation.present(mainWindow)
            }
            #else
            MainWindowPresentation.present(mainWindow)
            #endif
            HideLaunchTrace.mark(
                "main_window.pre_runtime",
                detail: "visible_\(mainWindow.isVisible)_windows_\(NSApplication.shared.windows.count)"
            )
            self.model.core.startRuntimeInitialization()
        }
    }

    #if DEBUG
    private func presentVerificationAddDeviceSheet() {
        guard let mainWindow else {
            model.interactionNotice = "Cannot present verification scene: main window is missing"
            return
        }
        let sheet = NSWindow(contentViewController: NSHostingController(rootView: AddDeviceSheet(model: model)))
        sheet.styleMask = [.titled, .fullSizeContentView]
        sheet.titleVisibility = .hidden
        sheet.titlebarAppearsTransparent = true
        sheet.backgroundColor = .clear
        sheet.isOpaque = false
        mainWindow.beginSheet(sheet)
    }
    #endif

    func applicationDidResignActive(_ notification: Notification) {
        model.cancelAgentSwitcher()
        model.cancelTabSwitcher()
        // Command-Tab releases Command while another app is frontmost, so the
        // flagsChanged release never reaches this monitor and the keycap hints
        // would stay on screen.
        model.clearShortcutHints()
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
            menuButton(.newTab) { model.addTab() }
            menuButton(.newChat) { model.openComposer() }
            menuButton(.newWorkspace) { model.openNewWorkspace() }
            menuButton(.search) { model.openSearch() }
            menuButton(.openFile) { model.openFileSearch() }

            Divider()

            menuButton(.closeTab) { model.performCloseShortcut() }
        }

        CommandMenu("Navigate") {
            // Titles stay static so the menu does not rebuild on every
            // snapshot tick; an empty slot is a no-op inside the model.
            ForEach(1...TabShortcutNumbering.capacity, id: \.self) { number in
                Button("Select Tab \(number)") {
                    model.selectTab(shortcutNumber: number)
                }
                .keyboardShortcut(KeyEquivalent(Character("\(number)")), modifiers: .command)
            }

            Divider()

            ForEach(1...AgentShortcutNumbering.capacity, id: \.self) { number in
                Button("Select Agent \(number)") {
                    model.selectAgent(shortcutNumber: number)
                }
                .keyboardShortcut(KeyEquivalent(Character("\(number)")), modifiers: .control)
            }

            Divider()

            menuButton(.toggleLeftSidebar) { model.toggleLeftSidebar() }
            menuButton(.toggleSidebarView) { model.toggleSidebarContent() }
            menuButton(.toggleRightPanel) { model.toggleRightPanel() }

            Divider()

            menuButton(.findInPane) { model.showFindInFocusedSurface() }
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

    private func menuButton(
        _ command: ShellMenuCommand,
        action: @escaping () -> Void
    ) -> some View {
        Button(command.title, action: action)
            .keyboardShortcut(
                command.shortcut.keyEquivalent,
                modifiers: command.shortcut.eventModifiers
            )
    }

    private func paneButton(_ command: PaneCommand) -> some View {
        return Button(command.title) {
            model.performPaneCommand(command)
        }
    }
}
