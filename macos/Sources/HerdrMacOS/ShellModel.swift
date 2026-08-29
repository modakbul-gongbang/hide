import Combine
import Foundation

enum ShellSurface: String, CaseIterable, Hashable {
    case agents
    case terminal
    case workbench

    var title: String {
        switch self {
        case .agents:
            "Agents"
        case .terminal:
            "Terminal"
        case .workbench:
            "Workbench"
        }
    }
}

enum PaneSplitDirection: String, Decodable, Equatable {
    case right
    case down
}

@MainActor
final class ShellModel: ObservableObject {
    @Published var activeSurface: ShellSurface = .terminal
    @Published var consequenceNotice: ConsequenceNotice?
    @Published var consequenceResult: String?
    @Published var showNewWorkspace = false
    @Published var showNewAgent = false
    @Published var showSearch = false
    @Published var showSettings = false
    @Published var workspaceToRemove: CoreWorkspaceSnapshot?
    @Published var interactionNotice: String?
    @Published var selectedAgentKind = "claude"
    @Published var selectedAgentCheckoutID: String?
    @Published var selectedAgentDeviceID = "local"
    @Published var agentBypassWarnings = false
    @Published private(set) var paneShortcuts: [PaneCommand: PaneShortcut]
    @Published private(set) var shortcutErrors: [PaneCommand: String] = [:]
    @Published private(set) var shortcutDiagnostic: String?
    let core: CoreBridge
    let browser: BrowserRuntimeModel
    let remote: RemoteRuntimeModel
    private var coreSubscription: AnyCancellable?
    private var browserSubscription: AnyCancellable?
    private var remoteSubscription: AnyCancellable?
    private var pendingPaneCloseID: String?
    private var lastRemoteDevice: CoreDeviceSnapshot?

    init(
        core: CoreBridge = CoreBridge(),
        browser: BrowserRuntimeModel = BrowserRuntimeModel(),
        remote: RemoteRuntimeModel = RemoteRuntimeModel()
    ) {
        self.core = core
        self.browser = browser
        self.remote = remote
        agentBypassWarnings = core.snapshot?.uiState.bypassWarnings ?? false
        let shortcutResolution = PaneShortcutPolicy.resolve(
            stored: core.snapshot?.uiState.shortcutBindings ?? [:]
        )
        paneShortcuts = shortcutResolution.bindings
        shortcutDiagnostic = shortcutResolution.diagnostic ?? Self.uiStateDiagnostic(core.snapshot)
        coreSubscription = core.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
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
        remote.environmentStateProvider = { [weak core] key in
            core?.environmentState(for: key)
        }
        browser.onReceipt = { [weak core] receipt in
            core?.recordBrowserStatus(receipt)
        }
    }

    var workspaces: [CoreWorkspaceSnapshot] {
        core.snapshot?.navigator.workspaces ?? []
    }

    var devices: [CoreDeviceSnapshot] {
        core.snapshot?.navigator.devices ?? []
    }

    var agents: [SidebarAgent] {
        core.snapshot?.navigator.agents ?? []
    }

    var focusedWorkspace: CoreWorkspaceSnapshot? {
        guard let id = core.snapshot?.navigator.focusedWorkspaceID else {
            return workspaces.first
        }
        return workspaces.first(where: { $0.id == id }) ?? workspaces.first
    }

    var focusedCheckout: CoreCheckoutSnapshot? {
        if let id = core.snapshot?.navigator.focusedCheckoutID,
           let checkout = workspaces.lazy.compactMap({ workspace in
               workspace.checkouts.first(where: { $0.id == id })
           }).first {
            return checkout
        }
        return focusedWorkspace?.checkouts.first
    }

    var focusedTabs: [CoreTabSnapshot] {
        focusedCheckout?.tabs ?? []
    }

    var focusedPath: URL? {
        guard let path = core.snapshot?.navigator.rootPath else { return nil }
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    var herdrIsConnected: Bool {
        core.snapshot?.status.herdr.state == "connected"
    }

    func openNewWorkspace() {
        showNewWorkspace = true
        interactionNotice = nil
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

    func selectCheckout(_ checkout: CoreCheckoutSnapshot) {
        core.focusCheckout(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
        focus(.terminal)
    }

    func selectDevice(_ device: CoreDeviceSnapshot) {
        core.focusDevice(device.id)
        guard device.kind == "remote", let alias = device.sshAlias else { return }
        lastRemoteDevice = device
        remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
    }

    func selectAgent(_ agent: SidebarAgent) {
        guard let workspace = workspaces.first(where: { $0.label == agent.workspaceLabel }),
              let checkout = workspace.checkouts.first(where: { checkout in
                  checkout.tabs.contains(where: { tab in tab.panes.contains(where: { $0.id == agent.paneID }) })
              }) ?? workspace.checkouts.first
        else {
            interactionNotice = "The selected agent is not attached to a known checkout."
            return
        }
        core.focusCheckout(workspaceID: workspace.id, checkoutID: checkout.id)
        core.focusPane(agent.paneID)
        focus(.terminal)
    }

    func addWorkspace(path: URL, label: String, initializeGit: Bool) {
        guard path.isFileURL else {
            interactionNotice = "Choose a local folder before adding the workspace."
            return
        }
        core.createWorkspace(path: path, label: label, initializeGit: initializeGit)
        showNewWorkspace = false
        interactionNotice = "Workspace registration requested. Files stay where they are."
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

    func addTab() {
        guard let workspace = focusedWorkspace else {
            interactionNotice = "Create or register a workspace before adding a tab."
            return
        }
        core.createTab(workspaceID: workspace.id, checkoutID: focusedCheckout?.id)
    }

    func startAgent() {
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
            bypassWarnings: agentBypassWarnings
        )
        showNewAgent = false
        interactionNotice = "Agent start requested. Hide does not handle credentials."
    }

    func updatePreferences(accentHex: String? = nil, fontSize: Double? = nil, bypassWarnings: Bool? = nil) {
        core.persistUIState(
            accentHex: accentHex,
            fontSize: fontSize,
            bypassWarnings: bypassWarnings
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
        if let alias = device.sshAlias {
            remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
        }
        interactionNotice = "Connection test requested for \(device.label). Authentication remains owned by SSH."
    }

    func retryRemote() {
        guard let device = lastRemoteDevice ?? devices.first(where: { $0.kind == "remote" }),
              let alias = device.sshAlias else {
            interactionNotice = "Add an SSH device before retrying a remote connection."
            return
        }
        remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
    }

    var selectedAgentCheckout: CoreCheckoutSnapshot? {
        guard let selectedAgentCheckoutID else { return focusedCheckout }
        return workspaces.lazy.compactMap { workspace in
            workspace.checkouts.first(where: { $0.id == selectedAgentCheckoutID })
        }.first
    }

    func focus(_ surface: ShellSurface) {
        activeSurface = surface
    }

    func splitCurrentPane(_ direction: PaneSplitDirection) {
        core.splitCurrentPane(direction: direction)
        focus(.terminal)
    }

    func toggleCurrentPaneZoom() {
        core.toggleCurrentPaneZoom()
        focus(.terminal)
    }

    func closeCurrentPane() {
        guard let paneID = core.snapshot?.terminal.paneID else {
            consequenceResult = "Select a terminal pane before closing."
            return
        }
        let agent = core.snapshot?.navigator.agents.first { $0.paneID == paneID }
        let target = DestructiveTarget(
            id: paneID,
            label: paneID,
            state: agent?.state ?? "idle",
            summary: agent?.summary ?? "No working or attention state is reported for this pane."
        )
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingPaneCloseID = paneID
            consequenceNotice = notice
        } else {
            pendingPaneCloseID = nil
            core.closePane(paneID, confirmed: false)
            consequenceResult = "Close requested for idle pane \(paneID)."
        }
        focus(.terminal)
    }

    func performPaneCommand(_ command: PaneCommand) {
        switch command {
        case .splitRight: splitCurrentPane(.right)
        case .splitDown: splitCurrentPane(.down)
        case .toggleZoom: toggleCurrentPaneZoom()
        case .closePane: closeCurrentPane()
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
        pendingPaneCloseID = nil
        let agents = core.snapshot?.navigator.agents ?? []
        let targets = agents.map {
            DestructiveTarget(id: $0.paneID, label: $0.workspaceLabel, state: $0.state, summary: $0.summary)
        }
        consequenceNotice = ConsequencePolicy.notice(kind: kind, targets: targets)
        consequenceResult = nil
    }

    func confirmConsequencePreview() {
        guard let consequenceNotice else { return }
        if let paneID = pendingPaneCloseID {
            core.closePane(paneID, confirmed: true)
            consequenceResult = "Confirmed close requested for pane \(paneID)."
            pendingPaneCloseID = nil
        } else {
            consequenceResult = consequenceNotice.requiresConfirmation
                ? "Confirmation recorded for the prefixed verification preview. No user resource was changed."
                : "Idle close requires no confirmation. No user resource was changed in this preview."
        }
        self.consequenceNotice = nil
    }

    func cancelConsequencePreview() {
        consequenceResult = "Cancelled before any process or checkout was affected."
        pendingPaneCloseID = nil
        consequenceNotice = nil
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
