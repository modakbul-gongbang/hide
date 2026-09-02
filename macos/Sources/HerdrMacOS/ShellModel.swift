import AppKit
import Combine
import Foundation

enum ShellSurface: String, CaseIterable, Hashable {
    case agents
    case terminal
    case rightPanel

    var title: String {
        switch self {
        case .agents:
            "Agents"
        case .terminal:
            "Terminal"
        case .rightPanel:
            "Right Panel"
        }
    }
}

enum SidebarContent: String, CaseIterable, Hashable, Identifiable {
    case projects
    case agents

    var id: String { rawValue }

    var title: String {
        switch self {
        case .projects: "Projects"
        case .agents: "Agents"
        }
    }

    var systemImage: String {
        switch self {
        case .projects: "square.stack.3d.up"
        case .agents: "person.2"
        }
    }

    var alternate: SidebarContent {
        self == .projects ? .agents : .projects
    }
}

enum PaneSplitDirection: String, Decodable, Equatable, Sendable {
    case right
    case down
}

enum PaneResizeDirection: String, Sendable {
    case left
    case right
    case up
    case down
}

enum CheckoutSelectionAction: Equatable {
    case focusExisting
    case startTerminal
}

enum CheckoutSelectionPolicy {
    static func action(for checkout: CoreCheckoutSnapshot) -> CheckoutSelectionAction {
        checkout.tabs.flatMap { $0.panes }.isEmpty ? .startTerminal : .focusExisting
    }
}

enum CheckoutStartState {
    case idle
    case starting
    case started
    case failed(String)
}

enum CloseShortcutAction: Equatable {
    case closeFile
    case closeHerdr
    case nothingToClose
    case blocked
}

enum CloseShortcutPolicy {
    /// Closing the last tab used to close the window, which is how the
    /// operator lost the whole application by pressing `⌘W` one time too many.
    /// The close shortcut closes what it names - a file tab or a Herdr tab -
    /// and when nothing is left it says so. Closing the window stays on the
    /// window's own close control, where the operator means it.
    static func action(
        hasWorkspace: Bool,
        hasActiveFileTab: Bool,
        hasActiveHerdrTab: Bool,
        tabCount: Int
    ) -> CloseShortcutAction {
        if hasActiveFileTab { return .closeFile }
        if !hasWorkspace { return .nothingToClose }
        if hasActiveHerdrTab { return .closeHerdr }
        return tabCount == 0 ? .nothingToClose : .blocked
    }
}

enum HerdrTabLabelPresentation {
    static func displayLabel(rawLabel: String?, fallbackIndex: Int) -> String {
        let label = rawLabel?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if let number = Int(label) { return "Tab \(number)" }
        return label.isEmpty ? "Tab \(fallbackIndex + 1)" : label
    }

    static func nextLabel(rawLabels: [String?]) -> String {
        let used = Set(rawLabels.compactMap { rawLabel -> Int? in
            let label = rawLabel?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            if let number = Int(label) { return number }
            guard label.lowercased().hasPrefix("tab ") else { return nil }
            return Int(label.dropFirst(4).trimmingCharacters(in: .whitespacesAndNewlines))
        })
        let number = (1...).first(where: { !used.contains($0) }) ?? (rawLabels.count + 1)
        return "Tab \(number)"
    }
}

enum ShellTabKind {
    case herdr(CoreTabSnapshot)
    case file(CoreFileTabSnapshot)
}

struct ShellTabItem: Identifiable {
    let id: String
    let label: String
    let dirty: Bool
    let active: Bool
    let kind: ShellTabKind
}

enum TerminalLayoutPolicy {
    static func belongs(
        layout: CorePaneLayoutSnapshot,
        to checkout: CoreCheckoutSnapshot
    ) -> Bool {
        belongs(
            workspaceID: layout.workspaceID,
            tabID: layout.tabID,
            paneIDs: layout.root.paneIDs,
            checkout: checkout
        )
    }

    static func belongs(
        workspaceID: String,
        tabID: String,
        paneIDs: [String],
        checkout: CoreCheckoutSnapshot
    ) -> Bool {
        // `layout.workspace_id` is Herdr's live session workspace ID, while
        // `checkout.workspaceID` is Hide's catalog workspace ID. They are
        // intentionally different identity domains. The Herdr tab ID and
        // its complete pane set are the stable cross-domain projection key.
        _ = workspaceID
        guard let tab = checkout.tabs.first(where: { $0.id == tabID }) else { return false }
        return Set(paneIDs) == Set(tab.panes.map(\.id))
    }
}

enum HerdrStatusPresentation {
    static func localMessage(
        bridgeError: String?,
        state: String?,
        providerMessage: String?
    ) -> String {
        if let bridgeError {
            return bridgeError
        }
        if let providerMessage {
            return providerMessage
        }
        return state == "connected" ? "Connected to Herdr" : "Waiting for Herdr"
    }
}

@MainActor
final class ShellModel: ObservableObject {
    @Published var activeSurface: ShellSurface = .terminal
    @Published private(set) var sidebarContent: SidebarContent = .projects
    @Published var consequenceNotice: ConsequenceNotice?
    @Published var consequenceResult: String?
    @Published var showNewAgent = false
    @Published var showSearch = false
    @Published var showFileSearch = false
    @Published var showSettings = false
    @Published var showPetDashboard = false
    @Published private(set) var agentSwitcherCycle: AgentSwitcherCycle?
    @Published private(set) var tabSwitcherCycle: TabSwitcherCycle?
    @Published var workspaceToRemove: CoreWorkspaceSnapshot?
    @Published var worktreeToDelete: CoreCheckoutSnapshot?
    @Published var interactionNotice: String?
    @Published var selectedAgentKind = "claude"
    @Published var selectedAgentCheckoutID: String?
    @Published var selectedAgentDeviceID = "local"
    @Published private(set) var paneShortcuts: [PaneCommand: PaneShortcut]
    @Published private(set) var shortcutErrors: [PaneCommand: String] = [:]
    @Published private(set) var shortcutDiagnostic: String?
    @Published private(set) var checkoutStartState: CheckoutStartState = .idle
    /// True once Command has been held past the reveal delay, which is what
    /// draws the ⌘n keycaps on the agent rows.
    @Published private(set) var agentShortcutHintsVisible = false
    let core: CoreBridge
    let browser: BrowserRuntimeModel
    let remote: RemoteRuntimeModel
    private var coreSubscription: AnyCancellable?
    private var browserSubscription: AnyCancellable?
    private var remoteSubscription: AnyCancellable?
    private var pendingPaneCloseTarget: PaneCloseTarget?
    private var pendingTabCloseTarget: TabCloseTarget?
    private var lastRemoteDevice: CoreDeviceSnapshot?
    @Published private(set) var activeRemoteDevice: CoreDeviceSnapshot?
    private var pendingCheckoutStarts: Set<String> = []
    private var agentMRU = AgentMRU()
    private var commandModifierHeld = false
    private var agentShortcutHintTask: Task<Void, Never>?
    private var tabMRU = TabMRU()

    init(
        core: CoreBridge = CoreBridge(),
        browser: BrowserRuntimeModel = BrowserRuntimeModel(),
        remote: RemoteRuntimeModel = RemoteRuntimeModel()
    ) {
        self.core = core
        self.browser = browser
        self.remote = remote
        let shortcutResolution = PaneShortcutPolicy.resolve(
            stored: core.snapshot?.uiState.shortcutBindings ?? [:]
        )
        paneShortcuts = shortcutResolution.bindings
        shortcutDiagnostic = shortcutResolution.diagnostic ?? Self.uiStateDiagnostic(core.snapshot)
        observeAgentFocus(in: core.snapshot)
        remote.ingest(core.snapshot?.status.remote ?? [])
        observeTabFocus()
        coreSubscription = core.$snapshot.sink { [weak self] snapshot in
            guard let self else { return }
            self.observeAgentFocus(in: snapshot)
            self.remote.ingest(snapshot?.status.remote ?? [])
            self.observeTabFocus()
            self.objectWillChange.send()
        }
        browserSubscription = browser.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        remoteSubscription = remote.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        browser.environmentStateProvider = { [weak core] key in
            core?.environmentState(for: key)
        }
        browser.onReceipt = { [weak core] receipt in
            core?.recordBrowserStatus(receipt)
        }
    }

    var workspaces: [CoreWorkspaceSnapshot] {
        if isRemoteContext {
            return remote.navigation?.workspaces ?? []
        }
        return core.snapshot?.navigator.workspaces ?? []
    }

    var devices: [CoreDeviceSnapshot] {
        core.snapshot?.navigator.devices ?? []
    }

    var agents: [SidebarAgent] {
        if isRemoteContext {
            return remote.navigation?.agents ?? []
        }
        return core.snapshot?.navigator.agents ?? []
    }

    var agentsNeedingAttention: [SidebarAgent] {
        SidebarGrouping.needingAttention(agents)
    }

    func agents(in checkout: CoreCheckoutSnapshot) -> [SidebarAgent] {
        SidebarGrouping.agents(agents, in: checkout)
    }

    var focusedWorkspace: CoreWorkspaceSnapshot? {
        if isRemoteContext {
            guard let navigation = remote.navigation else { return nil }
            if let id = navigation.focusedWorkspaceID {
                return navigation.workspaces.first(where: { $0.id == id }) ?? navigation.workspaces.first
            }
            return navigation.workspaces.first
        }
        guard let id = core.snapshot?.navigator.focusedWorkspaceID else { return nil }
        return workspaces.first(where: { $0.id == id })
    }

    var focusedCheckout: CoreCheckoutSnapshot? {
        if isRemoteContext {
            guard let navigation = remote.navigation else { return nil }
            if let id = navigation.focusedCheckoutID,
               let checkout = navigation.workspaces.lazy.compactMap({ workspace in
                   workspace.checkouts.first(where: { $0.id == id })
               }).first {
                return checkout
            }
            return focusedWorkspace?.checkouts.first
        }
        if let id = core.snapshot?.navigator.focusedCheckoutID,
           let checkout = workspaces.lazy.compactMap({ workspace in
               workspace.checkouts.first(where: { $0.id == id })
           }).first {
            return checkout
        }
        return nil
    }

    var focusedTabs: [CoreTabSnapshot] {
        focusedCheckout?.tabs ?? []
    }

    var unifiedTabs: [ShellTabItem] {
        guard let checkout = focusedCheckout else { return [] }
        let activeFileID = isRemoteContext ? nil : core.snapshot?.editor.activeTabID
        let herdrItems = focusedTabs.enumerated().map { index, tab in
            return ShellTabItem(
                id: "herdr:\(tab.stableID)",
                label: HerdrTabLabelPresentation.displayLabel(
                    rawLabel: tab.label,
                    fallbackIndex: index
                ),
                dirty: false,
                active: activeFileID == nil && tab.id == focusedTab?.id,
                kind: .herdr(tab)
            )
        }
        guard !isRemoteContext else { return herdrItems }
        let fileItems = (core.snapshot?.editor.tabs ?? [])
            .filter { $0.workspaceID == checkout.workspaceID && $0.checkoutID == checkout.id }
            .map { tab in
                ShellTabItem(
                    id: "file:\(tab.id)",
                    label: tab.label,
                    dirty: tab.dirty,
                    active: tab.id == activeFileID,
                    kind: .file(tab)
                )
            }
        return herdrItems + fileItems
    }

    var focusedTab: CoreTabSnapshot? {
        guard let checkout = focusedCheckout else { return nil }
        let preferredTabID = isRemoteContext
            ? remote.navigation?.focusedTabID
            : core.snapshot?.paneLayout?.tabID
        if let preferredTabID,
           let tab = checkout.tabs.first(where: { $0.id == preferredTabID }) {
            return tab
        }
        return checkout.tabs.first
    }

    var focusedPanes: [CorePaneSnapshot] {
        focusedTab?.panes ?? []
    }

    var focusedPaneGridItems: [PaneGridItem] {
        if isRemoteContext {
            guard paneProjectionNotice == nil,
                  let layout = remote.navigation?.focusedPaneLayout
            else { return [] }
            let items = PaneGridPresentation.items(
                remoteLayout: layout,
                focusedPaneID: focusedPaneID
            )
            return items
        } else if let layout = focusedPaneLayout {
            return PaneGridPresentation.items(
                layout: layout,
                focusedPaneID: focusedPaneID
            )
        }
        return PaneGridPresentation.uniformItems(
            paneIDs: focusedPanes.map(\.id),
            focusedPaneID: focusedPaneID
        )
    }

    var focusedPaneGridDividers: [PaneGridDivider] {
        guard !isRemoteContext, let layout = focusedPaneLayout, !layout.zoomed else { return [] }
        return PaneGridPresentation.dividers(layout: layout)
    }

    func resizePane(_ paneID: String, direction: PaneResizeDirection, amount: Double) {
        guard !isRemoteContext else {
            interactionNotice = "Remote pane resizing is not available from this Mac."
            return
        }
        core.resizePane(paneID, direction: direction, amount: amount)
    }

    var focusedPaneLayout: CorePaneLayoutSnapshot? {
        guard let layout = core.snapshot?.paneLayout,
              let checkout = focusedCheckout,
              TerminalLayoutPolicy.belongs(layout: layout, to: checkout)
        else { return nil }
        return layout
    }

    var focusedPath: URL? {
        if isRemoteContext {
            guard let path = focusedCheckout?.path, !path.isEmpty else { return nil }
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        guard let path = core.snapshot?.navigator.rootPath else { return nil }
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    var localProjectionNotice: String? {
        guard !isRemoteContext,
              let error = core.snapshot?.status.lastError,
              error.kind == "pane.projection_unavailable"
        else { return nil }
        return "\(error.kind): \(error.message)"
    }

    var paneProjectionNotice: String? {
        guard isRemoteContext, !focusedPanes.isEmpty else {
            return localProjectionNotice
        }
        guard let layout = remote.navigation?.focusedPaneLayout else {
            return "remote.pane_layout_unavailable: The selected remote tab has panes but no layout in the Herdr snapshot. Retry the Mac mini connection."
        }
        let expectedPaneIDs = Set(focusedPanes.map(\.id))
        let layoutPaneIDs = Set(layout.frames.map(\.paneID))
        guard !layout.frames.isEmpty, layoutPaneIDs == expectedPaneIDs else {
            return "remote.pane_layout_mismatch: The selected remote tab's panes do not match its layout. Retry the Mac mini connection."
        }
        return nil
    }

    var isRemoteContext: Bool { activeRemoteDevice != nil }

    var selectedDeviceID: String {
        activeRemoteDevice?.id
            ?? devices.first(where: { $0.kind != "remote" })?.id
            ?? "local"
    }

    var activeContextLabel: String? {
        activeRemoteDevice?.label
    }

    var focusedPaneID: String? {
        if isRemoteContext {
            return remote.navigation?.focusedPaneID
        }
        return core.snapshot?.paneLayout?.focusedPaneID
            ?? core.snapshot?.terminal.paneID
    }

    func paneMetadata(for paneID: String) -> CorePaneSnapshot? {
        focusedPanes.first(where: { $0.id == paneID })
            ?? workspaces
                .lazy
                .flatMap(\.checkouts)
                .flatMap(\.tabs)
                .flatMap(\.panes)
                .first(where: { $0.id == paneID })
    }

    func paneStatus(for paneID: String) -> String {
        if isRemoteContext {
            return paneMetadata(for: paneID)?.state ?? "unavailable"
        }
        guard let pane = core.snapshot?.terminal.panes.first(where: { $0.paneID == paneID }) else {
            return "idle"
        }
        return pane.closed ? "closed" : pane.transportState
    }

    func paneTransportMessage(for paneID: String) -> String? {
        core.snapshot?.terminal.panes.first(where: { $0.paneID == paneID })?.transportMessage
    }

    var petDashboard: PetDashboardProjection {
        PetDashboardProjector.project(
            agents: agents,
            workspaces: workspaces,
            connection: core.snapshot?.status.herdr.state ?? "disconnected",
            connectionMessage: core.snapshot?.status.herdr.message
        )
    }

    func openPetDashboard() {
        showPetDashboard = true
    }

    func selectAgent(paneID: String) {
        guard let agent = agents.first(where: { $0.paneID == paneID }) else {
            interactionNotice = "Agent pane \(paneID) is no longer available."
            return
        }
        selectAgent(agent)
    }

    func reconnectPane(_ paneID: String) {
        guard !isRemoteContext else {
            interactionNotice = "Reconnect is currently available for local panes only."
            return
        }
        core.reconnectPane(paneID)
    }

    func focusPane(_ paneID: String) {
        if isRemoteContext {
            guard let workspace = focusedWorkspace,
                  let checkout = focusedCheckout
            else {
                interactionNotice = "The remote pane cannot be focused because its workspace selection is unavailable."
                return
            }
            remote.focus(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                paneID: paneID
            )
            if let targetID = remote.navigation?.deviceID {
                core.focusRemotePane(targetID: targetID, paneID: paneID)
            }
            HideLaunchTrace.mark("pane.selection", detail: "remote_\(paneID)")
        } else {
            core.focusPane(paneID)
            HideLaunchTrace.mark("pane.selection", detail: "local_\(paneID)")
        }
        focus(.terminal)
    }

    func openTerminalLink(_ rawValue: String, paneID: String) {
        if isRemoteContext {
            // A web address means the same thing from either machine, so it
            // still opens; a path names a file on the other machine, which the
            // read-only snapshot contract cannot fetch.
            if let url = TerminalLinkResolver.webURL(in: rawValue) {
                ExternalBrowser.open(url) { [weak self] message in
                    self?.interactionNotice = message
                    HideLaunchTrace.mark("terminal.link.failed", detail: "external_open")
                }
                interactionNotice = nil
                HideLaunchTrace.mark("terminal.link.opened", detail: "external")
                return
            }
            interactionNotice = "\(rawValue) was printed by a remote pane. Remote file preview is not available in the current read-only snapshot contract."
            HideLaunchTrace.mark("terminal.link.failed", detail: "remote_file_contract")
            return
        }
        switch TerminalLinkResolver.route(
            rawValue,
            paneCWD: paneMetadata(for: paneID)?.cwd ?? "",
            checkoutRoot: focusedCheckout.map { URL(fileURLWithPath: $0.path, isDirectory: true) }
        ) {
        case .web(let url):
            ExternalBrowser.open(url) { [weak self] message in
                self?.interactionNotice = message
                HideLaunchTrace.mark("terminal.link.failed", detail: "external_open")
            }
            interactionNotice = nil
            HideLaunchTrace.mark("terminal.link.opened", detail: "external")
        case .file(let url):
            openFile(url)
            focus(.rightPanel)
            interactionNotice = nil
            HideLaunchTrace.mark("terminal.link.opened", detail: "local_file")
        case .unresolved(let message):
            interactionNotice = message
            HideLaunchTrace.mark("terminal.link.failed", detail: "unresolved_target")
        }
    }

    var herdrIsConnected: Bool {
        if isRemoteContext {
            return remote.phase == .ready
        }
        return core.snapshot?.status.herdr.state == "connected"
    }

    func openNewWorkspace() {
        interactionNotice = nil
        let panel = NSOpenPanel()
        panel.title = "Choose a folder for the workspace"
        panel.prompt = "Choose"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.begin { [weak self] response in
            guard let self, response == .OK, let url = panel.url else { return }
            let label = url.lastPathComponent.isEmpty ? "Workspace" : url.lastPathComponent
            let hasGitMetadata = FileManager.default.fileExists(
                atPath: url.appendingPathComponent(".git").path
            )
            self.core.createWorkspace(
                path: url,
                label: label,
                initializeGit: !hasGitMetadata
            )
        }
    }

    func openNewAgent(checkoutID: String? = nil) {
        selectedAgentCheckoutID = checkoutID ?? focusedCheckout?.id
        selectedAgentDeviceID = core.snapshot?.navigator.focusedDeviceID ?? "local"
        showNewAgent = true
        interactionNotice = nil
    }

    func openSearch() {
        showSearch = true
        interactionNotice = nil
    }

    func openFileSearch() {
        guard !isRemoteContext else {
            interactionNotice = "Workspace file search is available for local checkouts only."
            return
        }
        guard let checkout = focusedCheckout, checkout.exists else {
            interactionNotice = "Select an available local checkout before searching files."
            return
        }
        showFileSearch = true
        interactionNotice = nil
    }

    func selectCheckout(_ checkout: CoreCheckoutSnapshot) {
        let action = CheckoutSelectionPolicy.action(for: checkout)
        HideLaunchTrace.mark(
            "checkout.selection",
            detail: "\(isRemoteContext ? "remote" : "local")_\(checkout.id)_\(action == .startTerminal ? "start_terminal" : "focus_existing")"
        )
        if isRemoteContext {
            remote.focus(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
            focus(.terminal)
            guard let targetID = remote.navigation?.deviceID else {
                interactionNotice = "The selected remote checkout has no target identity. No command was sent."
                return
            }
            if action == .startTerminal {
                core.createRemoteTab(
                    targetID: targetID,
                    workspaceID: checkout.workspaceID,
                    cwd: checkout.path,
                    label: "hide \(checkout.label)"
                )
            } else {
                core.focusRemoteWorkspace(
                    targetID: targetID,
                    workspaceID: checkout.workspaceID
                )
            }
            return
        }
        core.focusCheckout(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
        focus(.terminal)
        switch action {
        case .focusExisting:
            checkoutStartState = .idle
        case .startTerminal:
            startTerminal(for: checkout, focusHerdr: false)
        }
    }

    func selectDevice(_ device: CoreDeviceSnapshot) {
        if device.kind == "remote" {
            guard device.sshAlias?
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .isEmpty == false
            else {
                interactionNotice = "\(device.label) is registered as remote but has no SSH alias. Repair the device in Settings before retrying."
                HideLaunchTrace.mark("device.selection.failed", detail: "remote_alias_missing_\(device.id)")
                return
            }
            activeRemoteDevice = device
            lastRemoteDevice = device
            checkoutStartState = .idle
            core.selectCommandDevice(id: device.id, label: device.label, isRemote: true)
            HideLaunchTrace.mark("device.selection", detail: "remote_\(device.id)")
            core.focusDevice(device.id)
            remote.refresh(targetID: device.id, label: device.label)
            focus(.terminal)
            return
        }
        activeRemoteDevice = nil
        remote.clearNavigation()
        checkoutStartState = .idle
        core.selectCommandDevice(id: device.id, label: device.label, isRemote: false)
        HideLaunchTrace.mark("device.selection", detail: "local_\(device.id)")
        core.focusDevice(device.id)
    }

    func selectAgent(_ agent: SidebarAgent) {
        guard let identity = workspaces.lazy.compactMap({ workspace in
            workspace.checkouts.lazy.compactMap { checkout in
                checkout.tabs.contains(where: { tab in
                    tab.panes.contains(where: { $0.id == agent.paneID })
                }) ? (workspace, checkout) : nil
            }.first
        }).first
        else {
            interactionNotice = "The selected agent is not attached to a known checkout."
            return
        }
        let (workspace, checkout) = identity
        if isRemoteContext {
            remote.focus(workspaceID: workspace.id, checkoutID: checkout.id, paneID: agent.paneID)
            guard let targetID = remote.navigation?.deviceID else {
                interactionNotice = "The selected remote agent has no target identity. No focus command was sent."
                return
            }
            core.focusRemotePane(targetID: targetID, paneID: agent.paneID)
        } else {
            core.focusCheckout(workspaceID: workspace.id, checkoutID: checkout.id)
            core.focusPane(agent.paneID)
        }
        focus(.terminal)
    }

    /// ⌘1…⌘9 select the nth agent in the same order the sidebar lists them.
    /// A number past the end of the list is a miss, not an error: the user is
    /// reaching for a slot that is simply empty right now.
    func selectAgent(shortcutNumber: Int) {
        guard let agent = AgentShortcutNumbering.agent(atNumber: shortcutNumber, in: agents) else {
            return
        }
        selectAgent(agent)
    }

    func agentShortcutNumber(paneID: String) -> Int? {
        AgentShortcutNumbering.number(ofPaneID: paneID, in: agents)
    }

    /// Command held past the reveal delay shows the keycaps; releasing it
    /// hides them at once. The delay exists so that every ordinary ⌘ chord -
    /// ⌘K, ⌘W, ⌘C - does not flash the whole sidebar on its way through.
    func setCommandModifierHeld(_ held: Bool) {
        guard held != commandModifierHeld else { return }
        commandModifierHeld = held
        agentShortcutHintTask?.cancel()
        agentShortcutHintTask = nil
        guard held else {
            agentShortcutHintsVisible = false
            return
        }
        agentShortcutHintTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: ShellModel.agentShortcutHintDelayNanoseconds)
            guard !Task.isCancelled, let self, self.commandModifierHeld else { return }
            self.agentShortcutHintsVisible = true
        }
    }

    static let agentShortcutHintDelayNanoseconds: UInt64 = 150_000_000

    func loadRemoteFiles(path: String) {
        guard isRemoteContext,
              remote.phase == .ready,
              let targetID = remote.navigation?.deviceID,
              !path.isEmpty
        else { return }
        core.listRemoteFiles(targetID: targetID, rootPath: path)
    }

    func beginOrAdvanceAgentSwitcher() {
        cancelTabSwitcher()
        observeAgentFocus(in: core.snapshot)
        if agentSwitcherCycle != nil {
            agentSwitcherCycle?.advance()
            return
        }
        agentSwitcherCycle = AgentSwitcherCycle(
            originalPaneID: focusedPaneID,
            paneIDs: agentMRU.paneIDs
        )
    }

    /// Option+Shift+Tab. Opening the switcher with the reverse chord starts at
    /// the least recent agent, which is where walking backwards from the
    /// current one arrives.
    func beginOrRetreatAgentSwitcher() {
        cancelTabSwitcher()
        observeAgentFocus(in: core.snapshot)
        if agentSwitcherCycle != nil {
            agentSwitcherCycle?.retreat()
            return
        }
        agentSwitcherCycle = AgentSwitcherCycle(
            originalPaneID: focusedPaneID,
            paneIDs: agentMRU.paneIDs,
            direction: .backward
        )
    }

    func commitAgentSwitcher() {
        guard let cycle = agentSwitcherCycle else { return }
        defer { agentSwitcherCycle = nil }
        let available = Set(agents.map(\.paneID))
        guard let paneID = cycle.committedPaneID(availablePaneIDs: available),
              let agent = agents.first(where: { $0.paneID == paneID })
        else { return }
        selectAgent(agent)
    }

    func cancelAgentSwitcher() {
        agentSwitcherCycle = nil
    }

    private func observeAgentFocus(in snapshot: CoreSnapshot?) {
        let currentAgents = snapshot?.navigator.agents ?? []
        agentMRU.observe(
            focusedPaneID: snapshot?.paneLayout?.focusedPaneID ?? snapshot?.terminal.paneID,
            availablePaneIDs: currentAgents.map(\.paneID)
        )
    }

    func beginOrAdvanceTabSwitcher() {
        cancelAgentSwitcher()
        observeTabFocus()
        if tabSwitcherCycle != nil {
            tabSwitcherCycle?.advance()
            return
        }
        tabSwitcherCycle = TabSwitcherCycle(
            originalTabID: unifiedTabs.first(where: { $0.active })?.id,
            tabIDs: tabMRU.tabIDs
        )
    }

    /// Control+Shift+Tab walks the checkout-local recent tab order backwards.
    func beginOrRetreatTabSwitcher() {
        cancelAgentSwitcher()
        observeTabFocus()
        if tabSwitcherCycle != nil {
            tabSwitcherCycle?.retreat()
            return
        }
        tabSwitcherCycle = TabSwitcherCycle(
            originalTabID: unifiedTabs.first(where: { $0.active })?.id,
            tabIDs: tabMRU.tabIDs,
            direction: .backward
        )
    }

    func commitTabSwitcher() {
        guard let cycle = tabSwitcherCycle else { return }
        defer { tabSwitcherCycle = nil }
        let tabs = unifiedTabs
        let available = Set(tabs.map(\.id))
        guard let tabID = cycle.committedTabID(availableTabIDs: available),
              let tab = tabs.first(where: { $0.id == tabID })
        else { return }
        focusUnifiedTab(tab)
    }

    func cancelTabSwitcher() {
        tabSwitcherCycle = nil
    }

    private func observeTabFocus() {
        let tabs = unifiedTabs
        tabMRU.observe(
            contextID: tabSwitcherContextID,
            focusedTabID: tabs.first(where: { $0.active })?.id,
            availableTabIDs: tabs.map(\.id)
        )
    }

    private var tabSwitcherContextID: String? {
        guard let checkout = focusedCheckout else { return nil }
        let deviceID = isRemoteContext
            ? remote.navigation?.deviceID ?? "remote"
            : "local"
        return "\(deviceID):\(checkout.workspaceID):\(checkout.id)"
    }

    func toggleWorkspace(_ workspace: CoreWorkspaceSnapshot) {
        var collapsed = Set(core.snapshot?.uiState.collapsedWorkspaceIDs ?? [])
        if workspace.expanded {
            collapsed.insert(workspace.id)
        } else {
            collapsed.remove(workspace.id)
        }
        core.persistUIState(collapsedWorkspaceIDs: collapsed.sorted())
    }

    func requestRemoveWorkspace(_ workspace: CoreWorkspaceSnapshot) {
        workspaceToRemove = workspace
    }

    func confirmRemoveWorkspace() {
        guard let workspace = workspaceToRemove else { return }
        core.removeWorkspace(workspace.id)
        workspaceToRemove = nil
        interactionNotice = "Workspace removed from Hide. Its folder, repository, and worktrees were not changed."
    }

    func requestDeleteWorktree(_ checkout: CoreCheckoutSnapshot) {
        guard checkout.isWorktree else {
            interactionNotice = "Only linked worktree checkouts can be deleted from this menu."
            return
        }
        worktreeToDelete = checkout
    }

    func confirmDeleteWorktree() {
        guard let checkout = worktreeToDelete else { return }
        worktreeToDelete = nil
        let path = checkout.path
        Task { @MainActor [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                GitWorktreeRemover.remove(path: path)
            }.value
            guard let self else { return }
            if result.succeeded {
                interactionNotice = "Deleted linked worktree at \(path)."
            } else {
                interactionNotice = result.message
            }
        }
    }

    func addTab() {
        guard let workspace = focusedWorkspace else {
            interactionNotice = "Create or register a workspace before adding a tab."
            return
        }
        guard let checkout = focusedCheckout else {
            interactionNotice = "Select a checkout before adding a tab."
            return
        }
        let label = nextHerdrTabLabel
        if isRemoteContext {
            guard let targetID = remote.navigation?.deviceID
            else {
                interactionNotice = "The selected remote workspace has no routable checkout context. No tab was created."
                return
            }
            core.createRemoteTab(
                targetID: targetID,
                workspaceID: checkout.workspaceID,
                cwd: checkout.path,
                label: label
            )
            return
        }
        if focusedTabs.isEmpty {
            startTerminal(for: checkout, focusHerdr: true)
        } else {
            core.createTab(workspaceID: workspace.id, checkoutID: checkout.id, label: label)
        }
    }

    private var nextHerdrTabLabel: String {
        HerdrTabLabelPresentation.nextLabel(rawLabels: focusedTabs.map(\.label))
    }

    func focusTab(_ tab: CoreTabSnapshot) {
        guard let tabID = tab.id,
              let workspace = focusedWorkspace,
              let checkout = focusedCheckout
        else {
            interactionNotice = "The selected tab has no routable workspace context. No tab focus command was sent."
            return
        }
        if isRemoteContext {
            remote.focus(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                tabID: tabID,
                paneID: tab.panes.first?.id
            )
            guard let targetID = remote.navigation?.deviceID else {
                interactionNotice = "The selected remote tab has no target identity. No focus command was sent."
                return
            }
            core.focusRemoteTab(targetID: targetID, tabID: tabID)
        } else {
            core.focusTab(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                tabID: tabID
            )
        }
        focus(.terminal)
    }

    func openFile(_ url: URL) {
        guard !isRemoteContext,
              let workspace = focusedWorkspace,
              let checkout = focusedCheckout
        else {
            interactionNotice = "Select a local workspace before opening a file."
            return
        }
        core.openFile(url, workspaceID: workspace.id, checkoutID: checkout.id)
        interactionNotice = nil
    }

    func focusFileTab(_ tab: CoreFileTabSnapshot) {
        guard !isRemoteContext else {
            interactionNotice = "Remote file tabs are not available in the current read-only contract."
            return
        }
        core.focusFileTab(tab.id)
    }

    func closeFileTab(_ tab: CoreFileTabSnapshot) {
        core.closeFileTab(tab.id)
    }

    func startAgent(bypassWarnings: Bool) {
        guard let checkout = selectedAgentCheckout else {
            interactionNotice = "Choose a checkout before starting an agent."
            return
        }
        guard selectedAgentDeviceID == "local" else {
            interactionNotice = "Remote agent start is delegated to the remote Herdr session in v1. Connect that device first."
            return
        }
        core.startAgent(
            agent: selectedAgentKind,
            checkoutPath: checkout.path,
            checkoutID: checkout.id,
            workspaceID: HerdrLiveWorkspaceIdentity.workspaceID(for: checkout.tabs),
            bypassWarnings: bypassWarnings
        )
        showNewAgent = false
    }

    func updatePreferences(accentHex: String? = nil, fontSize: Double? = nil) {
        core.persistUIState(
            accentHex: accentHex,
            fontSize: fontSize
        )
    }

    func clearInteractionNotice() {
        interactionNotice = nil
    }

    func addDevice(label: String, alias: String) {
        let trimmedAlias = alias.trimmingCharacters(in: .whitespacesAndNewlines)
        let id = "device:\(trimmedAlias.lowercased().replacingOccurrences(of: "[^a-z0-9]+", with: "-", options: .regularExpression))"
        core.registerDevice(
            id: id,
            label: label.trimmingCharacters(in: .whitespacesAndNewlines),
            sshAlias: trimmedAlias
        )
        interactionNotice = "Device registration requested. Hide will use your existing SSH environment."
    }

    func removeDevice(_ device: CoreDeviceSnapshot) {
        core.removeDevice(device.id)
        interactionNotice = "Device removed from Hide. The remote host and its sessions were not changed."
    }

    func testDevice(_ device: CoreDeviceSnapshot) {
        core.testDevice(device.id)
        lastRemoteDevice = device
        if device.sshAlias != nil {
            remote.refresh(targetID: device.id, label: device.label)
        }
        interactionNotice = "Connection test requested for \(device.label). Authentication remains owned by SSH."
    }

    func retryRemote() {
        guard let device = lastRemoteDevice ?? devices.first(where: { $0.kind == "remote" }),
              device.sshAlias != nil else {
            interactionNotice = "Add an SSH device before retrying a remote connection."
            return
        }
        remote.refresh(targetID: device.id, label: device.label)
    }

    private func startTerminal(for checkout: CoreCheckoutSnapshot, focusHerdr: Bool) {
        guard pendingCheckoutStarts.insert(checkout.id).inserted else {
            checkoutStartState = .starting
            interactionNotice = "A terminal is already starting for this checkout."
            return
        }
        guard let runtime = core.runtimeSelection else {
            pendingCheckoutStarts.remove(checkout.id)
            checkoutStartState = .failed("The bundled Herdr runtime is still starting. Wait for Herdr status, then select this checkout again.")
            interactionNotice = "The bundled Herdr runtime is not ready yet. Select this checkout again when Herdr status is available."
            return
        }
        checkoutStartState = .starting
        let checkoutID = checkout.id
        let checkoutPath = checkout.path
        let checkoutLabel = checkout.label
        Task { @MainActor [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                HerdrTerminalLauncher.launch(
                    herdrPath: runtime.path,
                    checkoutPath: checkoutPath,
                    checkoutLabel: checkoutLabel,
                    focus: focusHerdr
                )
            }.value
            guard let self else { return }
            pendingCheckoutStarts.remove(checkoutID)
            if result.succeeded, let paneID = result.paneID {
                // The returned pane is the local projection anchor until
                // session sync supplies its authoritative layout.
                core.persistUIState(
                    selectedPaneID: paneID,
                    focusedCheckoutID: checkoutID
                )
                checkoutStartState = .started
                HideLaunchTrace.mark("checkout.terminal_start.ready", detail: "\(checkoutID)_\(paneID)")
            } else {
                let message = result.succeeded
                    ? "Herdr created a workspace but did not return its terminal pane. Check Herdr status and retry."
                    : result.message
                checkoutStartState = .failed(message)
                interactionNotice = message
                HideLaunchTrace.mark("checkout.terminal_start.failed", detail: checkoutID)
            }
        }
    }

    var selectedAgentCheckout: CoreCheckoutSnapshot? {
        guard let selectedAgentCheckoutID else { return focusedCheckout }
        return workspaces.lazy.compactMap { workspace in
            workspace.checkouts.first(where: { $0.id == selectedAgentCheckoutID })
        }.first
    }

    var leftSidebarVisible: Bool {
        core.snapshot?.uiState.leftSidebarVisible ?? true
    }

    var rightPanelVisible: Bool {
        core.snapshot?.uiState.rightPanelVisible ?? true
    }

    var rightPanelSection: RightPanelSection {
        core.snapshot?.uiState.rightPanelSection ?? .explorer
    }

    func selectRightPanelSection(_ section: RightPanelSection) {
        guard section != rightPanelSection else { return }
        core.persistUIState(rightPanelSection: section)
    }

    var changes: CoreChangesSnapshot {
        core.snapshot?.changes ?? .empty
    }

    func selectChangedFile(_ path: String?) {
        core.selectChangedFile(path: path)
    }

    func toggleLeftSidebar() {
        core.persistUIState(leftSidebarVisible: !leftSidebarVisible)
    }

    func showSidebarContent(_ content: SidebarContent) {
        sidebarContent = content
        if !leftSidebarVisible {
            core.persistUIState(leftSidebarVisible: true)
        }
    }

    func toggleSidebarContent() {
        showSidebarContent(sidebarContent.alternate)
    }

    func toggleRightPanel() {
        core.persistUIState(rightPanelVisible: !rightPanelVisible)
    }

    /// Reveals the find bar on whichever surface is on screen. Both surfaces
    /// carry their own bar, so this focuses the right one and hands it AppKit's
    /// standard find action.
    func showFindInFocusedSurface() {
        switch PaneFindPolicy.target(
            activeFileTabID: core.snapshot?.editor.activeTabID,
            focusedPaneID: focusedPaneID,
            isRemoteContext: isRemoteContext
        ) {
        case .none:
            return
        case .fileEditor:
            // Focus may sit on the tree or the tab strip, so the editor's own
            // text view is put in front of the action rather than assumed to
            // already be the first responder.
            if let textView = NSApp.keyWindow?.contentView?.firstDescendantFindableTextView() {
                NSApp.keyWindow?.makeFirstResponder(textView)
            }
        case .terminal(let paneID):
            core.focusTerminal(paneID: paneID)
        }
        FindResponderAction.send(.showFindInterface)
    }

    /// The scale key for the file editor. Herdr pane ids always carry a `:`,
    /// so this cannot collide with one.
    static let fileEditorScaleKey = "file-editor"

    /// What the zoom chords act on: whatever the user is actually looking at.
    /// The editor overlays the terminal surface whenever a file tab is open,
    /// so an open tab means the editor is what is on screen.
    var textScaleTarget: String? {
        if !isRemoteContext, core.snapshot?.editor.activeTabID != nil {
            return Self.fileEditorScaleKey
        }
        return focusedPaneID
    }

    /// A target the user has never zoomed is absent from the map, which reads
    /// as the default rather than as a missing value.
    func textScale(for target: String) -> CGFloat {
        CGFloat(core.snapshot?.uiState.paneTextScales[target] ?? 1)
    }

    var editorTextScale: CGFloat { textScale(for: Self.fileEditorScaleKey) }

    func focus(_ surface: ShellSurface) {
        switch surface {
        case .agents where !leftSidebarVisible:
            core.persistUIState(leftSidebarVisible: true)
        case .rightPanel where !rightPanelVisible:
            core.persistUIState(rightPanelVisible: true)
        default:
            break
        }
        activeSurface = surface
    }

    private enum PaneCommandRoute {
        case local
        case remote(paneID: String)
        case unavailable(String)
    }

    private enum PaneCloseTarget {
        case local(paneID: String)
        case remote(paneID: String)

        var paneID: String {
            switch self {
            case .local(let paneID), .remote(let paneID): paneID
            }
        }
    }

    private enum TabCloseTarget {
        case local(tabID: String)
        case remote(targetID: String, tabID: String)

        var tabID: String {
            switch self {
            case .local(let tabID), .remote(_, let tabID): tabID
            }
        }
    }

    private var paneCommandRoute: PaneCommandRoute {
        guard let device = activeRemoteDevice else { return .local }
        guard remote.phase == .ready else {
            return .unavailable("\(device.label) is not ready. No pane command was sent to either device.")
        }
        guard let navigation = remote.navigation,
              navigation.deviceID == device.id
        else {
            return .unavailable("\(device.label) has no matching navigation snapshot. No pane command was sent to either device.")
        }
        guard let paneID = navigation.focusedPaneID else {
            return .unavailable("Select a pane on \(device.label) before using a pane command. No local pane was changed.")
        }
        return .remote(paneID: paneID)
    }

    private func splitCurrentPane(_ direction: PaneSplitDirection) {
        guard let checkout = focusedCheckout,
              checkout.exists,
              !checkout.path.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            interactionNotice = "The selected checkout has no usable local path. No pane was created."
            return
        }
        core.splitCurrentPane(direction: direction, cwd: checkout.path)
        focus(.terminal)
    }

    func performCloseShortcut() {
        let activeFile = core.snapshot?.editor.activeTabID.flatMap { activeID in
            core.snapshot?.editor.tabs.first(where: { $0.id == activeID })
        }
        switch CloseShortcutPolicy.action(
            hasWorkspace: focusedWorkspace != nil,
            hasActiveFileTab: activeFile != nil,
            hasActiveHerdrTab: focusedTab?.id != nil,
            tabCount: unifiedTabs.count
        ) {
        case .closeFile:
            guard let tab = activeFile else {
                interactionNotice = "The active file tab could not be resolved. Nothing was closed."
                return
            }
            HideLaunchTrace.mark("tab.close_shortcut.file", detail: tab.id)
            closeFileTab(tab)
        case .closeHerdr:
            guard let tab = focusedTab, let tabID = tab.id else {
                interactionNotice = "The active Herdr tab could not be resolved. Nothing was closed."
                return
            }
            HideLaunchTrace.mark("tab.close_shortcut.herdr", detail: tabID)
            requestTabClose(tab)
        case .nothingToClose:
            interactionNotice = focusedWorkspace == nil
                ? "There is no open tab to close. Create a workspace to start one."
                : "There is no open tab to close. Create a pane to start one."
            HideLaunchTrace.mark(
                "tab.close_shortcut.nothing_to_close",
                detail: focusedWorkspace == nil ? "no_workspace" : "workspace_without_tabs"
            )
        case .blocked:
            interactionNotice = "The selected workspace has tabs but none is active. Nothing was closed."
            HideLaunchTrace.mark("tab.close_shortcut.blocked", detail: "tabs_without_active_tab")
        }
    }

    func focusUnifiedTab(_ item: ShellTabItem) {
        switch item.kind {
        case .herdr(let tab): focusTab(tab)
        case .file(let tab): focusFileTab(tab)
        }
    }

    func closeUnifiedTab(_ item: ShellTabItem) {
        switch item.kind {
        case .herdr(let tab): requestTabClose(tab)
        case .file(let tab): closeFileTab(tab)
        }
    }

    private func requestTabClose(_ tab: CoreTabSnapshot) {
        guard let tabID = tab.id else {
            interactionNotice = "The selected Herdr tab has no identity. No tab was closed."
            return
        }
        let target: TabCloseTarget
        if isRemoteContext {
            guard let targetID = remote.navigation?.deviceID else {
                interactionNotice = "The selected remote tab has no target identity. No tab was closed."
                return
            }
            target = .remote(targetID: targetID, tabID: tabID)
        } else {
            target = .local(tabID: tabID)
        }
        let paneIDs = Set(tab.panes.map(\.id))
        let affectedAgents = agents.filter { paneIDs.contains($0.paneID) }
        let destructiveTargets = affectedAgents.isEmpty
            ? [DestructiveTarget(
                id: tabID,
                label: tab.label ?? tabID,
                state: "idle",
                summary: "No working or attention state is reported for this tab."
            )]
            : affectedAgents.map {
                DestructiveTarget(
                    id: $0.paneID,
                    label: $0.workspaceLabel,
                    state: $0.state,
                    summary: $0.summary
                )
            }
        let notice = ConsequencePolicy.notice(kind: .tab, targets: destructiveTargets)
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingTabCloseTarget = target
            consequenceNotice = notice
        } else {
            pendingTabCloseTarget = nil
            executeTabClose(target, confirmed: false)
            consequenceResult = "Close requested for tab \(tab.label ?? tabID)."
        }
    }

    private func toggleCurrentPaneZoom() {
        core.toggleCurrentPaneZoom()
        focus(.terminal)
    }

    private func closeCurrentPane(target closeTarget: PaneCloseTarget) {
        let paneID = closeTarget.paneID
        guard paneMetadata(for: paneID) != nil else {
            consequenceResult = "Select a terminal pane before closing."
            return
        }
        let agent = agents.first { $0.paneID == paneID }
        let target = DestructiveTarget(
            id: paneID,
            label: paneID,
            state: agent?.state ?? "idle",
            summary: agent?.summary ?? "No working or attention state is reported for this pane."
        )
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingPaneCloseTarget = closeTarget
            consequenceNotice = notice
        } else {
            pendingPaneCloseTarget = nil
            executePaneClose(closeTarget, confirmed: false)
            consequenceResult = "Close requested for idle pane \(paneID)."
        }
        focus(.terminal)
    }

    func performPaneCommand(_ command: PaneCommand) {
        // Text size is keyed by pane id and rendered by the shell, so it means
        // the same thing for a local and a remote pane and is settled before
        // the route split rather than twice inside it.
        if let direction = command.textScaleDirection {
            guard let target = textScaleTarget else {
                interactionNotice = "Select a pane before changing its text size."
                return
            }
            core.setPaneTextScale(paneID: target, direction: direction)
            return
        }
        switch paneCommandRoute {
        case .unavailable(let message):
            interactionNotice = message
            HideLaunchTrace.mark(
                "pane.command.blocked",
                detail: "command=\(command.rawValue) device_id=\(selectedDeviceID)"
            )
        case .local:
            switch command {
            case .splitRight: splitCurrentPane(.right)
            case .splitDown: splitCurrentPane(.down)
            case .toggleZoom: toggleCurrentPaneZoom()
            case .closePane:
                guard let paneID = focusedPaneID else {
                    consequenceResult = "Select a terminal pane before closing."
                    return
                }
                closeCurrentPane(target: .local(paneID: paneID))
            case .increaseTextSize, .decreaseTextSize, .resetTextSize:
                // Unreachable: `textScaleDirection` is non-nil for exactly
                // these three and the guard above has already returned.
                assertionFailure("a text size command reached the route switch")
            }
        case .remote(let paneID):
            guard let targetID = remote.navigation?.deviceID else {
                interactionNotice = "The selected remote pane has no target identity. No command was sent."
                return
            }
            switch command {
            case .splitRight:
                core.splitRemotePane(targetID: targetID, paneID: paneID, direction: .right)
            case .splitDown:
                core.splitRemotePane(targetID: targetID, paneID: paneID, direction: .down)
            case .toggleZoom:
                core.toggleRemotePaneZoom(targetID: targetID, paneID: paneID)
            case .closePane: closeCurrentPane(target: .remote(paneID: paneID))
            case .increaseTextSize, .decreaseTextSize, .resetTextSize:
                // Unreachable: `textScaleDirection` is non-nil for exactly
                // these three and the guard above has already returned.
                assertionFailure("a text size command reached the route switch")
            }
            focus(.terminal)
        }
    }

    func shortcut(for command: PaneCommand) -> PaneShortcut {
        paneShortcuts[command] ?? command.defaultShortcut
    }

    func updateShortcut(_ command: PaneCommand, raw: String) {
        switch PaneShortcutPolicy.updating(command: command, raw: raw, current: paneShortcuts) {
        case let .success(updated):
            paneShortcuts = updated
            shortcutErrors[command] = nil
            shortcutDiagnostic = "Pane menu shortcuts were updated and saved."
            core.persistUIState(
                shortcutBindings: Dictionary(uniqueKeysWithValues: updated.map {
                    ($0.key.rawValue, $0.value.canonical)
                })
            )
        case let .failure(error):
            shortcutErrors[command] = error.localizedDescription
            shortcutDiagnostic = "The invalid shortcut was not saved."
        }
    }

    /// Stores the pet's global shortcut. The registrar reports back whether
    /// macOS accepted it, so a conflict is visible rather than a shortcut
    /// that silently never fires.
    func updatePetShortcut(_ accelerator: String?) {
        core.updatePetShortcut(accelerator: accelerator, error: petShortcutRegistrar?(accelerator))
    }

    /// Set by the app delegate, which owns the Carbon registration.
    var petShortcutRegistrar: ((String?) -> String?)?

    func previewConsequence(_ kind: DestructiveTargetKind) {
        pendingPaneCloseTarget = nil
        pendingTabCloseTarget = nil
        let agents = core.snapshot?.navigator.agents ?? []
        let targets = agents.map {
            DestructiveTarget(id: $0.paneID, label: $0.workspaceLabel, state: $0.state, summary: $0.summary)
        }
        consequenceNotice = ConsequencePolicy.notice(kind: kind, targets: targets)
        consequenceResult = nil
    }

    func confirmConsequencePreview() {
        guard let consequenceNotice else { return }
        if let target = pendingTabCloseTarget {
            executeTabClose(target, confirmed: true)
            consequenceResult = "Confirmed close requested for tab \(target.tabID)."
            pendingTabCloseTarget = nil
        } else if let target = pendingPaneCloseTarget {
            let paneID = target.paneID
            executePaneClose(target, confirmed: true)
            consequenceResult = "Confirmed close requested for pane \(paneID)."
            pendingPaneCloseTarget = nil
        } else {
            consequenceResult = consequenceNotice.requiresConfirmation
                ? "Confirmation recorded for the prefixed verification preview. No user resource was changed."
                : "Idle close requires no confirmation. No user resource was changed in this preview."
        }
        self.consequenceNotice = nil
    }

    func cancelConsequencePreview() {
        consequenceResult = "Cancelled before any process or checkout was affected."
        pendingPaneCloseTarget = nil
        pendingTabCloseTarget = nil
        consequenceNotice = nil
    }

    private func executePaneClose(_ target: PaneCloseTarget, confirmed: Bool) {
        switch target {
        case .local(let paneID):
            core.closePane(paneID, confirmed: confirmed)
        case .remote(let paneID):
            guard let targetID = remote.navigation?.deviceID else {
                interactionNotice = "The selected remote pane has no target identity. No close command was sent."
                return
            }
            core.closeRemotePane(targetID: targetID, paneID: paneID, confirmed: confirmed)
        }
    }

    private func executeTabClose(_ target: TabCloseTarget, confirmed: Bool) {
        switch target {
        case .local(let tabID):
            core.closeTab(tabID, confirmed: confirmed)
        case .remote(let targetID, let tabID):
            core.closeRemoteTab(targetID: targetID, tabID: tabID, confirmed: confirmed)
        }
    }

    private static func uiStateDiagnostic(_ snapshot: CoreSnapshot?) -> String? {
        guard let diagnostic = snapshot?.status.diagnostics.last(where: {
            $0.kind == "ui_state.missing" || $0.kind == "ui_state.corrupt"
        }) else { return nil }
        return switch diagnostic.kind {
        case "ui_state.missing": "No shortcut state file was found. Default pane shortcuts are active."
        default: "The shortcut state file was corrupt. Default pane shortcuts are active."
        }
    }
}
