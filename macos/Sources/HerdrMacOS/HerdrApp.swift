import AppKit
import Combine
import SwiftUI

@MainActor
enum ReopenShortcutPolicy {
    static func shouldReopen(_ event: NSEvent) -> Bool {
        event.type == .keyDown
            && ShellMenuCommand.reopenClosedTab.shortcut.matches(event)
    }
}

@MainActor
enum MainWindowPresentation {
    static func present(_ window: NSWindow, application: NSApplication = .shared, background: Bool = false) {
        if background {
            window.orderBack(nil)
            return
        }
        application.setActivationPolicy(.regular)
        application.activate(ignoringOtherApps: true)
        window.orderFrontRegardless()
        window.makeKey()
    }
}

@MainActor
enum ReopenWindowMenuPolicy {
    static let itemIdentifier = NSUserInterfaceItemIdentifier("me.grab.hide.reopen-closed-tab")

    @discardableResult
    static func install(
        in menu: NSMenu,
        target: AnyObject,
        action: Selector
    ) -> NSMenuItem {
        if let existing = menu.item(withTag: itemTag) {
            return existing
        }
        let command = ShellMenuCommand.reopenClosedTab
        let item = NSMenuItem(
            title: command.title,
            action: action,
            keyEquivalent: command.shortcut.menuKeyEquivalent
        )
        item.identifier = itemIdentifier
        item.tag = itemTag
        item.keyEquivalentModifierMask = command.shortcut.modifierFlags
        item.target = target

        // AppKit owns this app's main window rather than a SwiftUI WindowGroup,
        // so SwiftUI's `.windowList` placement never materialises. Insert at
        // the final separator, immediately before AppKit's window list.
        let insertionIndex = menu.items.lastIndex(where: \.isSeparatorItem)
            ?? menu.numberOfItems
        menu.insertItem(item, at: insertionIndex)
        return item
    }

    private static let itemTag = 0x48494445
}

@MainActor
final class HerdrApplicationDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSMenuItemValidation {
    let model: ShellModel
    private var mainWindow: NSWindow?
    private var switcherReleaseProbe: Timer?
    private var petWindowController: PetWindowController?
    private var petMenuBarController: PetMenuBarController?
    private var petHotkeyRegistrar: PetHotkeyRegistrar?
    private var petVisibilityObservation: AnyCancellable?
    private var usageWindowObservation: AnyCancellable?
    private var paneKeyMonitor: Any?
    private var reopenClosedMenuItem: NSMenuItem?

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
        let visibilityNotifications: [Notification.Name] = [
            NSWindow.didChangeOcclusionStateNotification,
            NSWindow.didMiniaturizeNotification,
            NSWindow.didDeminiaturizeNotification,
            NSWindow.willCloseNotification,
        ]
        usageWindowObservation = Publishers.MergeMany(
            visibilityNotifications.map {
                NotificationCenter.default.publisher(for: $0, object: window)
            }
        )
        .receive(on: RunLoop.main)
        .sink { [weak self] _ in
            MainActor.assumeIsolated { self?.publishUsageWindowVisibility() }
        }
        model.core.runtimeReadyHandler = { [weak self] in
            self?.publishUsageWindowVisibility()
        }
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
            if let number = PaneKeyEventPolicy.agentSelectionNumber(event) {
                MainActor.assumeIsolated {
                    self.model.selectAgent(shortcutNumber: number)
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
            if PaneKeyEventPolicy.isProjectSwitcherRetreat(event) {
                MainActor.assumeIsolated {
                    self.model.beginOrRetreatProjectSwitcher()
                }
                return nil
            }
            if PaneKeyEventPolicy.isProjectSwitcherAdvance(event) {
                MainActor.assumeIsolated {
                    self.model.beginOrAdvanceProjectSwitcher()
                }
                return nil
            }
            let activeSwitchers = MainActor.assumeIsolated {
                (
                    project: self.model.projectSwitcherCycle != nil,
                    tab: self.model.tabSwitcherCycle != nil
                )
            }
            if event.type == .keyUp, event.keyCode == 48,
               activeSwitchers.tab || activeSwitchers.project {
                // One replaceable release probe for synthetic chords. Repeated
                // keyUp events cannot accumulate delayed commits.
                self.switcherReleaseProbe?.invalidate()
                let probe = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: false) { [weak self] _ in
                    guard let self else { return }
                    let flags = NSEvent.ModifierFlags(
                        rawValue: UInt(CGEventSource.flagsState(.combinedSessionState).rawValue))
                    MainActor.assumeIsolated {
                        if PaneKeyEventPolicy.shouldCommitTabSwitcherAfterKeyUp(event, currentModifiers: flags) {
                            self.model.commitTabSwitcher()
                        }
                        if PaneKeyEventPolicy.shouldCommitProjectSwitcherAfterKeyUp(event, currentModifiers: flags) {
                            self.model.commitProjectSwitcher()
                        }
                    }
                }
                self.switcherReleaseProbe = probe
                return nil
            }
            if event.type == .keyDown, event.keyCode == 53,
               activeSwitchers.project || activeSwitchers.tab {
                self.switcherReleaseProbe?.invalidate()
                MainActor.assumeIsolated {
                    self.model.cancelProjectSwitcher()
                    self.model.cancelTabSwitcher()
                }
                return nil
            }
            if event.type == .flagsChanged {
                MainActor.assumeIsolated {
                    if activeSwitchers.tab && PaneKeyEventPolicy.isTabSwitcherRelease(event) {
                        self.model.commitTabSwitcher()
                    }
                    if activeSwitchers.project && PaneKeyEventPolicy.isProjectSwitcherRelease(event) {
                        self.model.commitProjectSwitcher()
                    }
                }
            }
            if UnifiedTabShortcutPolicy.isClose(event) {
                MainActor.assumeIsolated {
                    self.model.performCloseShortcut()
                }
                return nil
            }
            if ReopenShortcutPolicy.shouldReopen(event) {
                MainActor.assumeIsolated {
                    self.model.reopenClosed()
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
                MainActor.assumeIsolated {
                    self?.petMenuBarController?.refresh()
                }
                // ShellCommands observes the same core and may rebuild the
                // SwiftUI-owned main menu on this run-loop turn. Restore the
                // AppKit-owned Window item one turn later if that rebuild
                // removed it. The policy is a no-op while the item exists.
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        self?.installReopenWindowMenuItem()
                    }
                }
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
                case "settings-agents":
                    self.model.settingsInitialTab = .agents
                    self.model.showSettings = true
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
            MainWindowPresentation.present(mainWindow, background: self.verificationBackground)
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
        switcherReleaseProbe?.invalidate()
        model.cancelProjectSwitcher()
        model.cancelTabSwitcher()
        // Command-Tab releases Command while another app is frontmost, so the
        // flagsChanged release never reaches this monitor and the keycap hints
        // would stay on screen.
        model.clearShortcutHints()
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        installReopenWindowMenuItem()
    }

    private func installReopenWindowMenuItem() {
        let application = NSApplication.shared
        guard let windowMenu = application.mainMenu?
            .items.first(where: { $0.title == "Window" })?.submenu
            ?? application.windowsMenu
        else {
            HideLaunchTrace.mark(
                "reopen_closed.window_menu.failed",
                detail: "window_menu_missing"
            )
            return
        }
        reopenClosedMenuItem = ReopenWindowMenuPolicy.install(
            in: windowMenu,
            target: self,
            action: #selector(reopenClosedFromWindowMenu(_:))
        )
        windowMenu.delegate = self
        HideLaunchTrace.mark("reopen_closed.window_menu.installed")
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        reopenClosedMenuItem = ReopenWindowMenuPolicy.install(
            in: menu,
            target: self,
            action: #selector(reopenClosedFromWindowMenu(_:))
        )
    }

    @objc private func reopenClosedFromWindowMenu(_ sender: NSMenuItem) {
        model.reopenClosed()
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        if menuItem.identifier == ReopenWindowMenuPolicy.itemIdentifier {
            return model.canReopenClosed
        }
        return true
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
    private var verificationBackground: Bool {
        #if DEBUG
        CommandLine.arguments.contains("--verification-background")
        #else
        false
        #endif
    }

    private func presentMainWindow(_ window: NSWindow, source: String) {
        let application = NSApplication.shared
        MainWindowPresentation.present(window, application: application, background: verificationBackground)
        publishUsageWindowVisibility()
        HideLaunchTrace.mark(
            "main_window.visible",
            detail: "source_\(source)_visible_\(window.isVisible)_windows_\(application.windows.count)"
        )
        DispatchQueue.main.async { [weak self, weak window] in
            guard let self, let window else { return }
            if !window.isVisible {
                MainWindowPresentation.present(window, application: application, background: verificationBackground)
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
            self.installReopenWindowMenuItem()
            self.publishUsageWindowVisibility()
        }
    }

    private func publishUsageWindowVisibility() {
        guard let mainWindow else { return }
        let visible = mainWindow.isVisible
            && !mainWindow.isMiniaturized
            && mainWindow.occlusionState.contains(.visible)
        model.core.setUsageWindowVisible(visible)
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
        model.core.runtimeReadyHandler = nil
        usageWindowObservation?.cancel()
        usageWindowObservation = nil
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
            menuButton(.newWorkspace) { model.openNewWorkspace() }
            menuButton(.search) { model.openSearch() }
            menuButton(.openFile) { model.openFileSearch() }

            Divider()

            menuButton(.closeTab) { model.performCloseShortcut() }
        }

        CommandMenu("Navigate") {
            menuButton(.recentTab) { model.beginOrAdvanceTabSwitcher(); model.commitTabSwitcher() }
            menuButton(.previousRecentTab) { model.beginOrRetreatTabSwitcher(); model.commitTabSwitcher() }
            menuButton(.recentProject) { model.beginOrAdvanceProjectSwitcher(); model.commitProjectSwitcher() }
            menuButton(.previousRecentProject) { model.beginOrRetreatProjectSwitcher(); model.commitProjectSwitcher() }
            Divider()
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
                .keyboardShortcut(KeyEquivalent(Character("\(number)")), modifiers: .option)
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
