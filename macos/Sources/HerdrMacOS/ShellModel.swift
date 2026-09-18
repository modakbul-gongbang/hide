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

/// The Agents list is intentionally narrower than global search and project
/// overview. It is session-local: every launch starts with the work the
/// operator currently owns, while All remains one reversible click away.
enum AgentListScope: String, CaseIterable, Hashable, Identifiable {
    case mine
    case all

    var id: String { rawValue }
    var title: String { self == .mine ? "My Work" : "All" }
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
    case closePane
    case closeHerdr
    case nothingToClose
    case blocked
}

enum CloseShortcutPolicy {
    /// Closing the last tab used to close the window, which is how the
    /// operator lost the whole application by pressing `⌘W` one time too many.
    /// The close shortcut closes what the operator is looking at: a file tab,
    /// or the focused terminal pane. Closing a whole Herdr tab from ⌘W took
    /// every pane in it at once, which read as one pane dragging the others
    /// down; a tab now closes from its own close control, and closing the
    /// last pane in a tab closes the tab through Herdr anyway. When nothing
    /// is left it says so. Closing the window stays on the window's own close
    /// control, where the operator means it.
    static func action(
        hasWorkspace: Bool,
        hasActiveFileTab: Bool,
        hasActiveHerdrTab: Bool,
        hasFocusedPane: Bool = false,
        tabCount: Int
    ) -> CloseShortcutAction {
        if hasActiveFileTab { return .closeFile }
        if !hasWorkspace { return .nothingToClose }
        if hasActiveHerdrTab && hasFocusedPane { return .closePane }
        if hasActiveHerdrTab { return .closeHerdr }
        return tabCount == 0 ? .nothingToClose : .blocked
    }
}


enum HerdrStatusPresentation {
    /// The one line the status bar shows. A launch that cannot proceed says
    /// so first; after that, while Herdr is not connected, the reason the core
    /// gives outranks the last action's error, because no action can succeed
    /// until the connection does and the mismatch it names is what to fix.
    static func localMessage(
        startupDiagnostic: String?,
        bridgeError: String?,
        state: String?,
        providerMessage: String?
    ) -> String {
        if let startupDiagnostic {
            return startupDiagnostic
        }
        if state != "connected", let providerMessage {
            return providerMessage
        }
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
    @Published var agentListScope: AgentListScope = .mine
    @Published var consequenceNotice: ConsequenceNotice?
    @Published var consequenceResult: String?
    @Published var showSearch = false { didSet { refreshHintSheetState() } }
    @Published var showFileSearch = false { didSet { refreshHintSheetState() } }
    @Published var showSettings = false { didSet { refreshHintSheetState() } }
    /// Which Settings tab the sheet opens on. Only the verification fixture
    /// sets it; the sheet opens on the first tab otherwise.
    var settingsInitialTab: HideSettingsTab = .general
    @Published var showPetDashboard = false
    let recentNavigation = RecentNavigationPresentation()
    private(set) var projectSwitcherCycle: ProjectSwitcherCycle? {
        get { recentNavigation.projectCycle }
        set { recentNavigation.projectCycle = newValue }
    }
    private(set) var tabSwitcherCycle: TabSwitcherCycle? {
        get { recentNavigation.tabCycle }
        set { recentNavigation.tabCycle = newValue }
    }
    @Published var workspaceToRemove: CoreWorkspaceSnapshot?
    /// The Explorer item waiting for the Move to Trash modal's answer.
    @Published var explorerTrashPrompt: WorkspaceOutlineTrashPrompt?
    @Published var worktreeToDelete: CoreGitWorktree?
    @Published var deleteWorktreeBranch = false
    private var handledRemovalIDs: Set<UInt64> = []
    @Published var worktreeWorkspace: CoreWorkspaceSnapshot?
    @Published var worktreeDraft = WorktreeSheetDraft()
    @Published private(set) var worktreeError: String?
    @Published var branchMigration: BranchMigrationRequest?
    private var handledTaskOperationIDs: Set<UInt64> = []
    @Published var interactionNotice: String?
    @Published private(set) var interactionStatusRefreshAvailable = false
    @Published var herdrProtocolMismatch: HerdrProtocolMismatchDetails?
    /// When the last reported fork failure happened, so one failure is raised
    /// once rather than on every snapshot that still carries it.
    /// Panes with a fork in flight, which is what puts "forking…" in the
    /// header and what a failure is attributed to.
    @Published private(set) var panesForking: Set<String> = []

    /// Relationship Open and parent Return share this target-scoped outcome.
    /// Generic bridge errors remain available to the status bar, but are not
    /// used as the control's pending/failure contract (PRD B24).
    @Published private(set) var paneSelectionOperation: PaneSelectionOperation?

    /// A failure that belongs to one pane, keyed by that pane.
    @Published private(set) var paneNotices: [String: String] = [:]
    @Published private(set) var reopenPaneNotices: [String: String] = [:]
    @Published private(set) var reopenTabNotice: String?
    @Published private(set) var panesReopening: Set<String> = []

    private var lastReportedForkFailure: UInt64?
    @Published private(set) var paneShortcuts: [PaneCommand: PaneShortcut]
    @Published private(set) var shortcutErrors: [PaneCommand: String] = [:]
    @Published private(set) var shortcutDiagnostic: String?
    @Published private(set) var checkoutStartState: CheckoutStartState = .idle
    @Published private(set) var shortcutHintState = HideHintState()

    let core: CoreBridge
    let browser: BrowserRuntimeModel
    let remote: RemoteRuntimeModel
    private var coreSubscription: AnyCancellable?
    private var browserSubscription: AnyCancellable?
    private var remoteSubscription: AnyCancellable?
    private var pendingPaneCloseTarget: PaneCloseTarget?
    private var pendingTabCloseTarget: TabCloseTarget?
    private var pendingCloseScopePaneIDs: [String]?
    private var pendingCloseApprovalFingerprint: String?
    private var lastRemoteDevice: CoreDeviceSnapshot?
    @Published private(set) var activeRemoteDevice: CoreDeviceSnapshot?
    private var pendingCheckoutStarts: Set<String> = []
    private var projectMRU = ProjectMRU()
    private(set) var recentProjects: [String: RecentProject] = [:]
    private(set) var recentSurfaces: [String: RecentSurface] = [:]
    private var observedNavigationRevision: UInt64?
    private var observedNavigationDevice: String?
    private var currentProjectID: String?
    private var currentSurfaceID: String?
    private var shortcutHintTask: Task<Void, Never>?
    private var surfaceMRU = SurfaceMRU()

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
        remote.ingest(core.snapshot?.status.remote ?? [])
        observeNavigation(in: core.snapshot)
        observeRecentClosed(in: core.snapshot)
        coreSubscription = core.$snapshot.sink { [weak self] snapshot in
            guard let self else { return }
            self.observeWorktreeRemoval(in: snapshot)
            self.observeTaskOperation(in: snapshot)
            self.observeForkFailure(in: snapshot)
            self.observeRecentClosed(in: snapshot)
            self.settlePaneSelection(in: snapshot)
            self.remote.ingest(snapshot?.status.remote ?? [])
            self.observeNavigation(in: snapshot)
            self.settleCheckoutStart()
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
        core.localHerdrMutationRejectionHandler = { [weak self] readiness in
            self?.presentLocalHerdrMutationRejection(readiness)
        }
        #if DEBUG
        // Replay a supplied remote projection without asking an SSH target to
        // connect. The fixture uses the same navigation and file presentation.
        if CommandLine.arguments.contains("--verification-ui-fixture"),
           CommandLine.arguments.contains("--verification-remote-preview"),
           let device = core.snapshot?.navigator.devices.first(where: { $0.kind == "remote" }) {
            activeRemoteDevice = device
            remote.refresh(targetID: device.id, label: device.label)
        }
        #endif
    }

    private func observeRecentClosed(in snapshot: CoreSnapshot?) {
        let state = snapshot?.recentClosed ?? .empty
        let visiblePaneIDs = Set(
            (snapshot?.navigator.workspaces ?? [])
                .flatMap(\.checkouts)
                .flatMap(\.tabs)
                .flatMap(\.panes)
                .map(\.id)
        )
        reopenPaneNotices = Dictionary(
            state.notices.compactMap { notice in
                notice.paneID.map { ($0, notice.message) }
            },
            uniquingKeysWith: { _, newest in newest }
        )
        reopenTabNotice = state.notices.last(where: { notice in
            guard let paneID = notice.paneID else { return true }
            return !visiblePaneIDs.contains(paneID)
        })?.message
        panesReopening = state.restoring
            ? Set(state.notices.compactMap(\.paneID))
            : []
    }

    /// A fork that failed has to say so where the operator is looking, and
    /// nowhere else.
    ///
    /// Success needs nothing here: the forked pane appears with its fork mark,
    /// which is the result itself. Failure has no such evidence - the header
    /// suffix simply stops - so the reason is put on the pane that was forked,
    /// in the pane's own notice row. It does not go in a dialog: a fork the
    /// operator can simply try again is not worth a box to dismiss first, and
    /// the reason has to stay readable while they try.
    ///
    /// The pane is found by looking for one this shell has a fork in flight
    /// for inside the core's message, rather than by parsing the message's
    /// shape; `ForkFailurePresentation` carries the matching rule. When no
    /// pane matches, the reason still leaves the process through the trace and
    /// the core's own stderr diagnostic.
    private func observeForkFailure(in snapshot: CoreSnapshot?) {
        clearSettledForks(in: snapshot)
        guard let error = snapshot?.status.lastError,
              error.kind == "pane.fork_failed",
              lastReportedForkFailure != error.occurredAt
        else { return }
        lastReportedForkFailure = error.occurredAt
        HideLaunchTrace.mark("pane.fork.failed", detail: error.message)
        guard let owner = ForkFailurePresentation.owner(of: error.message, amongst: panesForking)
        else { return }
        panesForking.remove(owner)
        paneNotices[owner] = error.message
    }

    /// Drops the in-flight mark from a pane whose fork has arrived. Herdr's
    /// own lineage on the new pane is the evidence, so a fork that landed
    /// while this shell was not looking settles the same way.
    private func clearSettledForks(in snapshot: CoreSnapshot?) {
        guard !panesForking.isEmpty else { return }
        let arrived = Set(
            (snapshot?.navigator.workspaces ?? [])
                .flatMap(\.checkouts)
                .flatMap(\.tabs)
                .flatMap(\.panes)
                .compactMap { PaneHeaderControls.forkMark($0.fork) }
        )
        panesForking.subtract(arrived)
    }

    /// What a pane is doing right now, shown after its name in the header.
    func paneActivity(for paneID: String) -> String {
        if panesReopening.contains(paneID) { return " · reopening…" }
        if let operation = asyncOperation(targetID: paneID) {
            return asyncOperationActivity(operation)
        }
        return panesForking.contains(paneID) ? " · forking…" : ""
    }

    /// A failure that belongs to one pane, shown in that pane rather than in
    /// a dialog.
    func paneNotice(for paneID: String) -> String? {
        reopenPaneNotices[paneID] ?? asyncOperationNotice(asyncOperation(targetID: paneID)) ?? paneNotices[paneID]
    }

    var canReopenClosed: Bool {
        guard let recent = core.snapshot?.recentClosed else { return false }
        return recent.canReopen
    }

    func reopenClosed() {
        guard canReopenClosed else { return }
        core.reopenClosed()
    }

    private var asyncOperations: [CoreAsyncOperation] {
        core.snapshot?.status.asyncOperations ?? []
    }

    private func asyncOperation(targetID: String) -> CoreAsyncOperation? {
        asyncOperations.last(where: { $0.targetID == targetID })
    }

    private func asyncOperationActivity(_ operation: CoreAsyncOperation) -> String {
        switch operation.phase {
        case "unknown":
            let closeStatusCheckInFlight = core.snapshot?.recentClosed.pending.contains {
                $0.key == operation.id && $0.checking
            } == true
            return closeStatusCheckInFlight ? " · checking…" : " · status check needed"
        case "failed", "refused":
            return ""
        default:
            switch operation.kind {
            case "pane.close", "tab.close": return " · closing…"
            case "pane.split": return " · splitting…"
            case "pane.zoom": return " · zooming…"
            case "pane.resize": return " · resizing…"
            case "tab.move": return " · moving…"
            default: return " · working…"
            }
        }
    }

    private func asyncOperationNotice(_ operation: CoreAsyncOperation?) -> String? {
        guard let operation,
              ["failed", "refused", "unknown"].contains(operation.phase)
        else { return nil }
        return operation.message
    }

    private func asyncIdentityIDs(for tab: ShellTabItem) -> Set<String> {
        var ids = Set([tab.id])
        if case .herdr(let coreTab) = tab.kind, let sourceID = coreTab.id {
            ids.insert(sourceID)
        }
        return ids
    }

    private func tabAsyncOperation(for tab: ShellTabItem) -> CoreAsyncOperation? {
        let tabIDs = asyncIdentityIDs(for: tab)
        return asyncOperations.last(where: { operation in
            tabIDs.contains(operation.targetID) || tabIDs.contains(operation.scopeID)
        })
    }

    func tabActivity(for tab: ShellTabItem) -> String {
        guard let operation = tabAsyncOperation(for: tab) else { return "" }
        return asyncOperationActivity(operation)
    }

    func tabNotice(for tab: ShellTabItem) -> String? {
        asyncOperationNotice(tabAsyncOperation(for: tab))
    }

    var asyncTabNotice: String? {
        guard let operation = asyncOperations.reversed().first(where: { operation in
            ["failed", "refused", "unknown"].contains(operation.phase)
                && unifiedTabs.contains { tab in
                    let tabIDs = asyncIdentityIDs(for: tab)
                    return tabIDs.contains(operation.targetID) || tabIDs.contains(operation.scopeID)
                }
        }) else { return nil }
        return operation.message
    }

    private var pendingClose: CoreRecentClosedPending? {
        core.snapshot?.recentClosed.pending.last
    }

    var pendingCloseStatusChecking: Bool {
        pendingClose?.checking == true
    }

    var pendingCloseNeedsStatusCheck: Bool {
        pendingClose?.phase == "unknown"
    }

    var pendingCloseNotice: String? {
        guard let pendingClose else { return nil }
        if pendingClose.checking { return "Checking whether the close completed…" }
        if let message = pendingClose.message { return message }
        switch pendingClose.phase {
        case "preparing", "capturing": return "Preparing close…"
        case "transmitting": return "Closing…"
        case "awaiting_topology": return "Confirming close…"
        case "unknown": return "Close result needs checking."
        default: return nil
        }
    }

    func checkLatestCloseStatus() {
        guard let pendingClose, pendingCloseNeedsStatusCheck, !pendingCloseStatusChecking else { return }
        core.checkCloseStatus(pendingClose.key)
    }

    func refreshInteractionStatus() {
        guard interactionStatusRefreshAvailable else { return }
        interactionStatusRefreshAvailable = false
        interactionNotice = nil
        core.refreshStatus()
    }

    var workspaces: [CoreWorkspaceSnapshot] {
        if isRemoteContext {
            return remote.navigation?.workspaces ?? []
        }
        return core.snapshot?.navigator.workspaces ?? []
    }

    /// Core-owned project folds that have rows in the current navigation
    /// context. Remote navigation remains unchanged until its wire exposes the
    /// same metadata.
    var inactiveProjectGroups: [CoreInactiveProjectGroupSnapshot] {
        guard !isRemoteContext else { return [] }
        let available = Set(workspaces.map(\.id))
        return (core.snapshot?.navigator.inactiveProjects ?? []).filter { group in
            group.projectIDs.contains(where: available.contains)
        }
    }

    var sidebarProjectRows: [SidebarProjectRow] {
        SidebarInactiveProjection.projectRows(workspaces, groups: inactiveProjectGroups)
    }

    /// Project rows in exactly the order the Projects view draws them.
    var sidebarVisibleWorkspaces: [CoreWorkspaceSnapshot] {
        sidebarProjectRows.compactMap { row in
            guard case .workspace(let workspace, _) = row else { return nil }
            return workspace
        }
    }

    func activeCheckouts(in workspace: CoreWorkspaceSnapshot) -> [CoreCheckoutSnapshot] {
        SidebarInactiveProjection.activeCheckouts(in: workspace)
    }

    func inactiveCheckouts(in workspace: CoreWorkspaceSnapshot) -> [CoreCheckoutSnapshot] {
        SidebarInactiveProjection.inactiveCheckouts(in: workspace)
    }

    func sidebarVisibleCheckouts(in workspace: CoreWorkspaceSnapshot) -> [CoreCheckoutSnapshot] {
        activeCheckouts(in: workspace)
            + (workspace.inactiveCheckouts.expanded ? inactiveCheckouts(in: workspace) : [])
    }

    var devices: [CoreDeviceSnapshot] {
        core.snapshot?.navigator.devices ?? []
    }

    /// What the Settings diagnosis draws. The core resolves every judgement;
    /// this is the read.
    var agentHooks: CoreAgentHooks {
        core.snapshot?.status.agentHooks ?? CoreAgentHooks()
    }

    /// The Background AI group's state. The core resolves the choice, each
    /// provider's availability and its model list; this is the read.
    var backgroundAI: CoreBackgroundAI {
        core.snapshot?.status.backgroundAI ?? CoreBackgroundAI()
    }

    var agentsConnected: Bool {
        if let device = activeRemoteDevice {
            return core.snapshot?.status.remote.first { $0.targetID == device.id }?.state == "connected"
        }
        return core.snapshot?.status.herdr.state == "connected"
    }

    var agents: [SidebarAgent] {
        if isRemoteContext {
            return remote.navigation?.agents ?? []
        }
        return core.snapshot?.navigator.agents ?? []
    }

    var visibleAgentList: [SidebarAgent] {
        SidebarGrouping.visibleAgents(agents, scope: agentListScope)
    }

    /// The Needs You and Done sections the Projects view draws above the tree.
    var raisedAgentSections: [AgentGroupSection] {
        SidebarGrouping.raised(agents)
    }

    /// Every non-empty group in group order, for the Agents view.
    var agentSections: [AgentGroupSection] {
        SidebarGrouping.sections(visibleAgentList)
    }

    /// The rows the Projects view has already drawn at the top, so the tree
    /// below does not repeat them.
    var raisedAgents: [SidebarAgent] {
        raisedAgentSections.flatMap(\.agents)
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
        var paneIDs = isRemoteContext
            ? Dictionary(uniqueKeysWithValues: (remote.navigation?.paneLayouts ?? []).map { ($0.tabID, $0.focusedPaneID) })
            : Dictionary(uniqueKeysWithValues: (core.snapshot?.paneLayouts ?? []).map { ($0.tabID, $0.focusedPaneID) })
        // Core-owned focus responds immediately while Herdr confirms the layout.
        if let tabID = focusedTab?.id, let paneID = focusedPaneID { paneIDs[tabID] = paneID }
        return ShellTabStrip.items(
            strip: checkout.strip,
            herdrTabs: checkout.tabs,
            editorTabs: core.snapshot?.editor.tabs ?? [],
            activeHerdrTabID: focusedTab?.id,
            activeFileTabID: activeFileID,
            focusedPaneIDsByTab: paneIDs,
            agents: agents
        )
    }

    /// The active tab is the one the core names, and the core takes that name
    /// from Herdr. A remote context browses its own selection, so it keeps its
    /// navigation state. Neither falls back to the leftmost tab: reading
    /// position as focus is what let a reordered strip look like a tab switch.
    var focusedTab: CoreTabSnapshot? {
        guard let checkout = focusedCheckout else { return nil }
        let activeTabID = isRemoteContext
            ? remote.navigation?.focusedTabID
            : checkout.activeTabID
        guard let activeTabID else { return nil }
        return checkout.tabs.first(where: { $0.id == activeTabID })
    }

    var focusedPanes: [CorePaneSnapshot] {
        focusedTab?.panes ?? []
    }

    /// A remote target draws one canvas for the tab it is showing. Only the
    /// local surface keeps a canvas per visited tab, because only local panes
    /// are attached through this process.
    var remotePaneGridItems: [PaneGridItem] {
        guard isRemoteContext,
              paneProjectionNotice == nil,
              let layout = remote.navigation?.focusedPaneLayout
        else { return [] }
        return PaneGridPresentation.items(
            remoteLayout: layout,
            focusedPaneID: focusedPaneID
        )
    }

    /// One canvas per tab the operator has already opened in this checkout.
    /// The rule itself lives in `PaneGridPresentation`; this only reads the
    /// snapshot it needs.
    var retainedTabCanvases: [RetainedTabCanvas] {
        guard !isRemoteContext, let checkout = focusedCheckout else { return [] }
        return PaneGridPresentation.retainedCanvases(
            tabIDs: checkout.tabs.compactMap(\.id),
            layouts: core.snapshot?.paneLayouts ?? [],
            // A pane whose session the core released keeps its projection
            // entry so its state is readable, but it has no live stream, so
            // its canvas goes with the session and the tab redraws from
            // Herdr's own frame when it is shown again.
            attachedPaneIDs: Set(
                (core.snapshot?.terminal.panes ?? [])
                    .filter { $0.transportState != "released" }
                    .map(\.paneID)
            ),
            visibleTabID: focusedTab?.id,
            visibleFocusedPaneID: focusedPaneID
        )
    }

    func resizePane(_ paneID: String, direction: PaneResizeDirection, amount: Double) {
        guard !isRemoteContext else {
            interactionNotice = "Remote pane resizing is not available from this Mac."
            return
        }
        core.resizePane(paneID, direction: direction, amount: amount)
    }

    /// The layout of the tab the canvas is showing. Every tab's layout is in
    /// the snapshot, so this is a lookup by tab id and never empties to mark
    /// a switch in progress.
    var focusedPaneLayout: CorePaneLayoutSnapshot? {
        guard !isRemoteContext, let tabID = focusedTab?.id else { return nil }
        return core.snapshot?.paneLayouts.first(where: { $0.tabID == tabID })
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
            // A terminal the shell just launched is expected to be missing
            // from the checkout until the next sync lands it. The empty
            // state already says "starting"; a projection error on top of
            // it would announce a failure that is not one.
            switch checkoutStartState {
            case .starting, .started: return nil
            case .idle, .failed: return localProjectionNotice
            }
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
        // The core owns the focused pane, so its own field is the answer. The
        // layout's focused pane is Herdr's last word on the same question and
        // stands in only before the core has one, because a click has to move
        // the ring on its own frame rather than on Herdr's confirming event.
        return core.snapshot?.terminal.paneID
            ?? focusedPaneLayout?.focusedPaneID
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

    /// The canonical pane name already used by the focused header. Lineage
    /// payloads can carry a stale agent spawn name, while this snapshot carries
    /// the live user-facing pane label. Relationship surfaces resolve through
    /// this one ladder so Return always names the pane it will actually select.
    func paneIdentity(for paneID: String) -> String? {
        guard let pane = paneMetadata(for: paneID) else { return nil }
        let agent = agents.first { $0.paneID == paneID }
        return PaneHeaderPresentation.title(
            herdrLabel: pane.herdrLabel,
            agentSummary: agent?.identityLabel ?? pane.identityLabel,
            terminalTitle: pane.terminalTitle,
            workspaceLabel: pane.workspaceLabel,
            paneID: pane.id
        )
    }

    func resolvedLineagePath(for pane: CorePaneSnapshot) -> [CoreLineageStep] {
        PaneLineagePresentation.resolvingLivePaneLabels(in: pane.lineagePath) {
            paneIdentity(for: $0)
        }
    }

    func paneStatus(for paneID: String) -> String {
        if isRemoteContext {
            return paneMetadata(for: paneID)?.statusLabel ?? "Unavailable"
        }
        guard let pane = core.snapshot?.terminal.panes.first(where: { $0.paneID == paneID }) else {
            return "idle"
        }
        return pane.closed ? "closed" : pane.transportState
    }

    func paneTransportMessage(for paneID: String) -> String? {
        guard let pane = core.snapshot?.terminal.panes.first(where: { $0.paneID == paneID }),
              let message = pane.transportMessage else { return nil }
        guard let timestamp = pane.transportLastAttemptAtUnixMS else { return message }
        let date = Date(timeIntervalSince1970: Double(timestamp) / 1000)
        return "\(message) Last attempt: \(date.formatted(date: .omitted, time: .standard))."
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
            core.focusPane(paneID, origin: .operatorChoice)
            HideLaunchTrace.mark("pane.selection", detail: "local_\(paneID)")
        }
        focus(.terminal)
    }

    /// Opens one pane from an explicit relationship action.
    ///
    /// Row selection remains inspection-only. This method is called only by
    /// child Open/Return intents, owns their pending state, and waits for the
    /// Herdr layout to confirm focus instead of treating the core's immediate
    /// projection as success (PRD B23, B24).
    func requestPaneSelection(from sourcePaneID: String, to targetPaneID: String) {
        let availablePaneIDs = Set(allPaneMetadata().map(\.id))
        let targetLabel = paneIdentity(for: targetPaneID) ?? targetPaneID
        let decision = PaneSelectionPolicy.start(
            current: paneSelectionOperation,
            sourcePaneID: sourcePaneID,
            targetPaneID: targetPaneID,
            targetLabel: targetLabel,
            availablePaneIDs: availablePaneIDs
        )
        switch decision {
        case .unchanged:
            return
        case .failed(let operation):
            paneSelectionOperation = operation
            return
        case .dispatch(let operation):
            paneSelectionOperation = operation
        }
        guard let operation = paneSelectionOperation else { return }

        if isRemoteContext {
            guard let workspace = focusedWorkspace,
                  let checkout = focusedCheckout,
                  let targetID = remote.navigation?.deviceID
            else {
                failPaneSelection(
                    "The remote pane cannot be opened because its workspace selection is unavailable."
                )
                return
            }
            remote.focus(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                paneID: targetPaneID
            )
            switch core.focusRemotePane(
                targetID: targetID,
                paneID: targetPaneID,
                requestID: operation.requestID
            ) {
            case .accepted:
                focus(.terminal)
            case .rejected(let reason):
                failPaneSelection("Could not open \(targetLabel): \(reason)")
            }
            return
        }

        switch core.focusPane(
            targetPaneID,
            origin: .operatorChoice,
            requestID: operation.requestID
        ) {
        case .accepted:
            focus(.terminal)
        case .rejected(let reason):
            failPaneSelection("Could not open \(targetLabel): \(reason)")
        }
    }

    private func settlePaneSelection(in snapshot: CoreSnapshot?) {
        guard let operation = paneSelectionOperation, operation.isPending else { return }
        switch PaneSelectionPolicy.resolve(
            operation,
            outcome: snapshot?.status.paneFocusRequest
        ) {
        case .pending:
            return
        case .succeeded:
            paneSelectionOperation = nil
        case .failed(let reason, let retryable):
            failPaneSelection(reason, retryable: retryable)
        }
    }

    private func failPaneSelection(_ reason: String, retryable: Bool = true) {
        guard let current = paneSelectionOperation else { return }
        paneSelectionOperation = PaneSelectionOperation(
            requestID: current.requestID,
            sourcePaneID: current.sourcePaneID,
            targetPaneID: current.targetPaneID,
            targetLabel: current.targetLabel,
            phase: .failed(reason: reason, retryable: retryable)
        )
    }

    func retryPaneSelection() {
        guard let operation = paneSelectionOperation,
              case .failed(_, let retryable) = operation.phase,
              retryable
        else { return }
        requestPaneSelection(
            from: operation.sourcePaneID,
            to: operation.targetPaneID
        )
    }

    private func allPaneMetadata() -> [CorePaneSnapshot] {
        Array(workspaces.lazy.flatMap(\.checkouts).flatMap(\.tabs).flatMap(\.panes))
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
            checkoutRoot: focusedCheckout.map { URL(fileURLWithPath: $0.path, isDirectory: true) },
            checkouts: registeredCheckouts
        ) {
        case .web(let url):
            ExternalBrowser.open(url) { [weak self] message in
                self?.interactionNotice = message
                HideLaunchTrace.mark("terminal.link.failed", detail: "external_open")
            }
            interactionNotice = nil
            HideLaunchTrace.mark("terminal.link.opened", detail: "external")
        case .path(let route):
            open(route)
        case .unresolved(let message):
            // A click that resolves to nothing does nothing. Detection is a
            // guess made over arbitrary terminal output, so a wrong guess is
            // ordinary, and a modal makes the operator dismiss a dialog for a
            // mis-click they can simply repeat elsewhere. The reason still
            // leaves the process through the trace, so the case stays
            // answerable without putting it on screen.
            HideLaunchTrace.mark("terminal.link.failed", detail: message)
        }
    }

    /// Every checkout this Mac has registered, which is what "inside the
    /// scope" means for a clicked path (A3). A remote checkout is not one:
    /// its paths name files on the other machine.
    var registeredCheckouts: [TerminalLinkCheckout] {
        (core.snapshot?.navigator.workspaces ?? [])
            .filter { $0.remoteTargetID == nil }
            .flatMap(\.checkouts)
            .map { TerminalLinkCheckout(id: $0.id, workspaceID: $0.workspaceID, path: $0.path) }
    }

    /// Takes one of the five path branches.
    ///
    /// A checkout path becomes one core event that settles the whole screen; a
    /// path outside every checkout is handed to macOS. Either way the branch
    /// and its outcome reach the trace, so a click is answerable from outside
    /// the process.
    private func open(_ route: TerminalPathRoute) {
        switch route {
        case .checkoutFile(let url, let checkout), .checkoutFolder(let url, let checkout):
            core.revealPath(
                url,
                workspaceID: checkout.workspaceID,
                checkoutID: checkout.id,
                isDirectory: route.namesAFolder
            )
            // The core opens the panel in the same event; reaching for it here
            // would send a second, stale UI-state save that undid the reveal.
            activeSurface = .rightPanel
            interactionNotice = nil
        case .externalFile(let url), .externalFolder(let url):
            ExternalFileOpener.open(url) { [weak self] message in
                self?.interactionNotice = message
                HideLaunchTrace.mark("terminal.link.failed", detail: "external_open")
            }
            interactionNotice = nil
        case .externalReveal(let url):
            // Opening this would run it. Finder selects it instead (A1).
            ExternalFileOpener.reveal(url)
            interactionNotice = nil
        }
        HideLaunchTrace.mark("terminal.link.opened", detail: route.traceName)
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
            core.focusPane(agent.paneID, origin: .operatorChoice)
        }
        focus(.terminal)
    }

    /// ⌥1…⌥9 select the nth agent in the same order the sidebar lists them.
    /// The agents ⌥1-⌥9 reach, in the order the visible sidebar view lists
    /// them: the whole agent list in the Agents view, the raised rows and expanded
    /// lineage trees in the Projects view.
    var shortcutAgents: [SidebarAgent] {
        let expandedWorkspaces = sidebarVisibleWorkspaces.filter(\.expanded)
        let visibleCheckouts = expandedWorkspaces.flatMap { sidebarVisibleCheckouts(in: $0) }
        return AgentShortcutNumbering.candidates(
            for: sidebarContent,
            agents: sidebarContent == .agents ? visibleAgentList : agents,
            visibleCheckoutIDs: visibleCheckouts.map(\.id),
            collapsedCheckoutIDs: Set(core.snapshot?.uiState.collapsedCheckoutIDs ?? []),
            ownedPaneIDsByCheckout: Dictionary(uniqueKeysWithValues:
                visibleCheckouts.map {
                    ($0.id, Set($0.tabs.flatMap(\.panes).map(\.id)))
                })
        )
    }

    /// A number past the end of the list is a miss, not an error: the user is
    /// reaching for a slot that is simply empty right now.
    func selectAgent(shortcutNumber: Int) {
        guard let agent = AgentShortcutNumbering.agent(atNumber: shortcutNumber, in: shortcutAgents)
        else {
            return
        }
        selectAgent(agent)
    }

    func agentShortcutNumber(paneID: String) -> Int? {
        AgentShortcutNumbering.number(ofPaneID: paneID, in: shortcutAgents)
    }

    private var hintSuppressingSheetVisibility: [Bool] {
        [showSearch, showFileSearch, showSettings]
    }

    var hintSheetPresented: Bool {
        hintSuppressingSheetVisibility.contains(true)
    }

    func setShortcutModifiersHeld(_ modifiers: Set<PaneShortcut.Modifier>) {
        guard !hintSheetPresented else { clearShortcutHints(); return }
        shortcutHintState.update(modifiers, at: ProcessInfo.processInfo.systemUptime)
        shortcutHintTask?.cancel()
        shortcutHintTask = nil
        guard let deadline = shortcutHintState.deadline else { return }
        let delay = max(0, deadline - ProcessInfo.processInfo.systemUptime)
        shortcutHintTask = Task { [weak self] in
            do { try await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000)) }
            catch { return } // Cancellation is the release/replacement path.
            guard let self, !Task.isCancelled, !self.hintSheetPresented else { return }
            self.shortcutHintState.advance(to: ProcessInfo.processInfo.systemUptime)
        }
    }

    func clearShortcutHints() {
        shortcutHintTask?.cancel()
        shortcutHintTask = nil
        shortcutHintState.clear()
    }

    private func refreshHintSheetState() {
        clearShortcutHints()
        guard !hintSheetPresented else { return }
        setShortcutModifiersHeld(HideHintState.modifiers(in: NSEvent.modifierFlags))
    }

    func loadRemoteFiles(path: String) {
        guard isRemoteContext,
              remote.phase == .ready,
              let targetID = remote.navigation?.deviceID,
              !path.isEmpty
        else { return }
        core.listRemoteFiles(targetID: targetID, rootPath: path)
    }

    func recentProjectDetail(_ id: String) -> String {
        guard let surface = recentSurfaces(inProject: id).first else { return "No open tabs" }
        return "\(surface.checkoutLabel) · \(surface.item.label)"
    }

    /// The project switcher still restores one project's own last surface, and
    /// that order is the global one narrowed to the surfaces of that project.
    private func recentSurfaces(inProject projectID: String) -> [RecentSurface] {
        surfaceMRU.surfaceIDs.compactMap { recentSurfaces[$0] }.filter { $0.projectID == projectID }
    }

    func beginOrAdvanceProjectSwitcher() { stepProjectSwitcher(.forward) }
    func beginOrRetreatProjectSwitcher() { stepProjectSwitcher(.backward) }

    private func stepProjectSwitcher(_ direction: RecentSwitcherDirection) {
        cancelTabSwitcher()
        if var cycle = projectSwitcherCycle {
            if direction == .forward { cycle.advance() } else { cycle.retreat() }
            if cycle.selectedProjectID != projectSwitcherCycle?.selectedProjectID { projectSwitcherCycle = cycle }
            return
        }
        guard let cycle = ProjectSwitcherCycle(
            originalProjectID: currentProjectID, projectIDs: projectMRU.projectIDs, direction: direction
        ) else {
            // An empty or single-item history is an ordinary no-op.
            return
        }
        projectSwitcherCycle = cycle
    }

    func commitProjectSwitcher() {
        guard let cycle = projectSwitcherCycle else { return }
        cancelProjectSwitcher()
        guard let project = recentProjects[cycle.selectedProjectID] else {
            HideLaunchTrace.mark("navigation.project.commit_recovered", detail: "reason=project_removed outcome=selection_kept")
            return
        }
        guard let surface = recentSurfaces(inProject: project.id).first else {
            // An empty project remains navigable without inventing a terminal.
            guard let checkout = project.workspace.checkouts.first else {
                HideLaunchTrace.mark("navigation.project.commit_recovered", detail: "reason=workspace_removed outcome=selection_kept")
                return
            }
            guard selectNavigationDevice(project.deviceID) else { return }
            if project.deviceID == "local" {
                core.focusCheckout(workspaceID: project.workspace.id, checkoutID: checkout.id)
            } else {
                remote.focus(workspaceID: project.workspace.id, checkoutID: checkout.id)
                core.focusRemoteWorkspace(targetID: project.deviceID, workspaceID: project.workspace.id)
            }
            return
        }
        focusRecentSurface(surface)
    }

    func cancelProjectSwitcher() {
        if projectSwitcherCycle != nil { projectSwitcherCycle = nil }
    }

    func beginOrAdvanceTabSwitcher() { stepTabSwitcher(.forward) }
    func beginOrRetreatTabSwitcher() { stepTabSwitcher(.backward) }

    private func stepTabSwitcher(_ direction: RecentSwitcherDirection) {
        cancelProjectSwitcher()
        if var cycle = tabSwitcherCycle {
            if direction == .forward { cycle.advance() } else { cycle.retreat() }
            if cycle.selectedTabID != tabSwitcherCycle?.selectedTabID { tabSwitcherCycle = cycle }
            return
        }
        guard let cycle = TabSwitcherCycle(originalTabID: currentSurfaceID,
            tabIDs: surfaceMRU.surfaceIDs, direction: direction) else {
            // Keep the current surface without interrupting keyboard input.
            return
        }
        tabSwitcherCycle = cycle
    }

    func commitTabSwitcher() {
        guard let cycle = tabSwitcherCycle else { return }
        cancelTabSwitcher()
        guard let surface = recentSurfaces[cycle.selectedTabID] else {
            HideLaunchTrace.mark("navigation.tab.commit_recovered", detail: "reason=surface_removed outcome=selection_kept")
            return
        }
        focusRecentSurface(surface)
    }

    func cancelTabSwitcher() {
        if tabSwitcherCycle != nil { tabSwitcherCycle = nil }
    }

    private func reportNavigationNotice(_ message: String) {
        if interactionNotice != message { interactionNotice = message }
    }

    private func selectNavigationDevice(_ deviceID: String) -> Bool {
        let selected = activeRemoteDevice?.id ?? "local"
        guard selected != deviceID else { return true }
        guard let device = devices.first(where: { $0.id == deviceID }) else {
            reportNavigationNotice("The selected project's device is unavailable. Selection was kept.")
            return false
        }
        selectDevice(device)
        return (activeRemoteDevice?.id ?? "local") == deviceID
    }

    private func focusRecentSurface(_ surface: RecentSurface) {
        guard surface.id != currentSurfaceID else { return }
        guard selectNavigationDevice(surface.deviceID) else { return }
        switch surface.item.kind {
        case .herdr(let tab):
            guard let tabID = tab.id else {
                reportNavigationNotice("The selected tab has no runtime identity. Selection was kept.")
                return
            }
            if surface.deviceID == "local" {
                core.focusTab(workspaceID: surface.workspaceID, checkoutID: surface.checkoutID, tabID: tabID)
            } else {
                remote.focus(workspaceID: surface.workspaceID, checkoutID: surface.checkoutID,
                    tabID: tabID, paneID: tab.panes.first?.id)
                core.focusRemoteTab(targetID: surface.deviceID, tabID: tabID)
            }
        case .editor(let tab): core.focusFileTab(tab.id)
        }
    }

    /// Consume the incoming snapshot, not CoreBridge.snapshot: Published sends
    /// its new value before assigning the property. Reading the property here
    /// would record the previous frame's focus and miss the last visit.
    private func observeNavigation(in snapshot: CoreSnapshot?) {
        guard let snapshot else { return }
        let selectedDevice = activeRemoteDevice?.id ?? "local"
        guard snapshot.navigationRevision != observedNavigationRevision || selectedDevice != observedNavigationDevice else { return }
        observedNavigationRevision = snapshot.navigationRevision
        observedNavigationDevice = selectedDevice
        var projects: [String: RecentProject] = [:]
        var surfaces: [String: RecentSurface] = [:]
        var projectOrder: [String] = []
        var availableSurfaces: [String] = []
        currentProjectID = nil
        currentSurfaceID = nil
        var contexts: [(String, [CoreWorkspaceSnapshot], String?, String?, String?, [CoreEditorTabSnapshot],
                        [String: String], [SidebarAgent], String?)] = [
            ("local", snapshot.navigator.workspaces, snapshot.navigator.focusedWorkspaceID,
             snapshot.navigator.focusedCheckoutID, snapshot.editor.activeTabID, snapshot.editor.tabs,
             Dictionary(uniqueKeysWithValues: snapshot.paneLayouts.map { ($0.tabID, $0.focusedPaneID) }),
             snapshot.navigator.agents, snapshot.focusedPaneID)
        ]
        for status in snapshot.status.remote {
            guard let session = status.session else { continue }
            let navigation = remote.navigation?.deviceID == status.targetID ? remote.navigation : nil
            contexts.append((status.targetID, session.workspaces,
                navigation?.focusedWorkspaceID ?? session.focusedWorkspaceID,
                navigation?.focusedCheckoutID ?? session.focusedCheckoutID, nil, [],
                Dictionary(uniqueKeysWithValues: (navigation?.paneLayouts ?? session.paneLayouts).map { ($0.tabID, $0.focusedPaneID) }),
                navigation?.agents ?? session.agents, navigation?.focusedPaneID ?? session.focusedPaneID))
        }
        for (deviceID, workspaces, focusedWorkspaceID, focusedCheckoutID, activeFileID, editorTabs,
             layoutPaneIDs, contextAgents, focusedPaneID) in contexts {
            var paneIDs = layoutPaneIDs
            let agentsByPane = Dictionary(uniqueKeysWithValues: contextAgents.map { ($0.paneID, $0) })
            for workspace in workspaces {
                let projectID = "\(deviceID):\(workspace.id)"
                projects[projectID] = RecentProject(id: projectID, deviceID: deviceID, workspace: workspace)
                projectOrder.append(projectID)
                let selected = deviceID == selectedDevice && workspace.id == focusedWorkspaceID
                if selected { currentProjectID = projectID }
                for checkout in workspace.checkouts {
                    let isFocused = selected && checkout.id == focusedCheckoutID
                    let activeTabID = deviceID != "local" && isFocused
                        ? remote.navigation?.focusedTabID : checkout.activeTabID
                    // Match the tab strip: core-owned focus wins while its
                    // layout confirmation is pending. Index agents once per
                    // device, then pass only this checkout's panes.
                    if isFocused, let activeTabID, let focusedPaneID { paneIDs[activeTabID] = focusedPaneID }
                    let checkoutAgents = checkout.tabs.flatMap(\.panes).compactMap { agentsByPane[$0.id] }
                    let tabs = ShellTabStrip.items(strip: checkout.strip, herdrTabs: checkout.tabs,
                        editorTabs: editorTabs, activeHerdrTabID: activeTabID,
                        activeFileTabID: activeFileID, focusedPaneIDsByTab: paneIDs, agents: checkoutAgents)
                    if tabs.count != checkout.strip.count {
                        HideLaunchTrace.mark("navigation.tabs.reconciled", detail: "reason=missing_content removed_count=\(checkout.strip.count - tabs.count) revision=\(snapshot.navigationRevision)")
                    }
                    for item in tabs {
                        let id = "\(projectID):\(checkout.id):\(item.id)"
                        availableSurfaces.append(id)
                        surfaces[id] = RecentSurface(id: id, projectID: projectID, projectLabel: workspace.label,
                            deviceID: deviceID, workspaceID: workspace.id, checkoutID: checkout.id,
                            checkoutLabel: checkout.label, item: item)
                        if isFocused && item.active { currentSurfaceID = id }
                    }
                }
            }
        }
        recentProjects = projects
        recentSurfaces = surfaces
        surfaceMRU.observe(focusedSurfaceID: currentSurfaceID, availableSurfaceIDs: availableSurfaces)
        projectMRU.observe(focusedProjectID: currentProjectID, availableProjectIDs: projectOrder)
        if var cycle = projectSwitcherCycle {
            let original = cycle
            if !cycle.reconcile(available: Set(projectOrder)) {
                cancelProjectSwitcher()
                HideLaunchTrace.mark("navigation.project.cycle_cancelled", detail: "reason=all_removed outcome=selection_kept revision=\(snapshot.navigationRevision)")
            } else if original != cycle {
                projectSwitcherCycle = cycle
                HideLaunchTrace.mark("navigation.projects.reconciled", detail: "removed_count=\(original.projectIDs.count - cycle.projectIDs.count) revision=\(snapshot.navigationRevision)")
            }
        }
        if var cycle = tabSwitcherCycle {
            let original = cycle
            if !cycle.reconcile(available: Set(surfaceMRU.surfaceIDs)) {
                cancelTabSwitcher()
                HideLaunchTrace.mark("navigation.tab.cycle_cancelled", detail: "reason=all_removed outcome=selection_kept revision=\(snapshot.navigationRevision)")
            } else if original != cycle {
                tabSwitcherCycle = cycle
                HideLaunchTrace.mark("navigation.tabs.reconciled", detail: "reason=removed removed_count=\(original.tabIDs.count - cycle.tabIDs.count) revision=\(snapshot.navigationRevision)")
            }
        }
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

    func toggleInactiveCheckouts(in workspace: CoreWorkspaceSnapshot) {
        core.toggleInactiveCheckouts(projectPath: workspace.path)
    }

    func toggleInactiveProjects(in group: CoreInactiveProjectGroupSnapshot) {
        core.toggleInactiveProjects(deviceID: group.deviceID)
    }

    func isCheckoutExpanded(_ checkout: CoreCheckoutSnapshot) -> Bool {
        !(core.snapshot?.uiState.collapsedCheckoutIDs.contains(checkout.id) ?? false)
    }

    func toggleCheckoutExpansion(_ checkout: CoreCheckoutSnapshot) {
        var collapsed = Set(core.snapshot?.uiState.collapsedCheckoutIDs ?? [])
        if !collapsed.insert(checkout.id).inserted {
            collapsed.remove(checkout.id)
        }
        core.persistUIState(collapsedCheckoutIDs: collapsed.sorted())
    }

    func requestRemoveWorkspace(_ workspace: CoreWorkspaceSnapshot) {
        workspaceToRemove = workspace
    }

    func confirmRemoveWorkspace() {
        guard let workspace = workspaceToRemove else { return }
        core.removeWorkspace(workspace.id)
        workspaceToRemove = nil
    }

    func requestExplorerTrash(_ prompt: WorkspaceOutlineTrashPrompt) {
        explorerTrashPrompt = prompt
    }

    /// The one call that sends `path_trash`. Cancel and Esc clear the
    /// prompt through the alert binding and send nothing (D-03).
    func confirmExplorerTrash() {
        guard let prompt = explorerTrashPrompt else { return }
        core.trashPath(root: prompt.root, path: prompt.path, selectAfter: prompt.selectAfter, inode: prompt.inode)
        explorerTrashPrompt = nil
    }

    func worktree(for path: String) -> CoreGitWorktree? {
        core.snapshot?.gitWorktrees?.worktrees.first { $0.path == path }
    }

    func requestDeleteWorktree(_ checkout: CoreCheckoutSnapshot) {
        guard let worktree = checkout.worktree ?? worktree(for: checkout.path) else {
            interactionNotice = "Worktree status is not available yet. Refresh Git and try again."
            return
        }
        requestDeleteWorktree(worktree)
    }

    func requestDeleteWorktree(_ worktree: CoreGitWorktree) {
        guard worktree.deletionGate.blockedReason == nil else {
            interactionNotice = worktree.deletionGate.blockedReason
            return
        }
        deleteWorktreeBranch = false
        worktreeToDelete = worktree
    }

    func confirmDeleteWorktree() {
        guard let worktree = worktreeToDelete else { return }
        core.dispatch(kind: "remove_worktree", payload: [
            "checkout_path": worktree.path,
            "delete_branch": deleteWorktreeBranch && worktree.deletionGate.canDeleteBranch,
        ])
        worktreeToDelete = nil
    }

    var worktreeBranches: [String] {
        guard let workspace = worktreeWorkspace else { return [] }
        return ProjectBaseBranchPolicy.orderedBranches(
            workspace.branches,
            selectedBase: baseBranch(for: workspace)
        )
    }

    var worktreeCanSubmit: Bool {
        worktreeDraft.canSubmit && !worktreeBranches.isEmpty
            && core.snapshot?.taskOperation?.phase != "working"
    }

    func requestNewWorktree(_ workspace: CoreWorkspaceSnapshot) {
        worktreeWorkspace = workspace
        worktreeDraft.reset(
            branches: workspace.branches,
            preferredBase: baseBranch(for: workspace)
        )
        worktreeError = nil
    }

    func cancelNewWorktree() {
        guard core.snapshot?.taskOperation?.phase != "working" else { return }
        worktreeWorkspace = nil
        worktreeDraft = WorktreeSheetDraft()
        worktreeError = nil
    }

    func submitNewWorktree() {
        guard let workspace = worktreeWorkspace, worktreeCanSubmit else { return }
        worktreeError = nil
        core.dispatch(kind: "create_worktree", payload: [
            "repository_root": workspace.path,
            "branch": worktreeDraft.branch.trimmingCharacters(in: .whitespacesAndNewlines),
            "base_branch": worktreeDraft.baseBranch.map { $0 as Any } ?? NSNull(),
            "agent_kind": worktreeDraft.agent.map { $0.rawValue as Any } ?? NSNull(),
        ])
    }

    func requestBranchMigration(workspace: CoreWorkspaceSnapshot, checkout: CoreCheckoutSnapshot) {
        guard let branch = checkout.branch,
              let base = baseBranch(for: workspace),
              branch != base
        else { return }
        branchMigration = BranchMigrationRequest(
            repositoryRoot: workspace.path,
            branch: branch,
            baseBranch: base
        )
    }

    func baseBranch(for workspace: CoreWorkspaceSnapshot) -> String? {
        ProjectBaseBranchPolicy.selected(
            projectPath: workspace.path,
            defaultBranch: workspace.defaultBranch,
            overrides: core.snapshot?.uiState.projectBaseBranches ?? [:]
        )
    }

    func setBaseBranch(_ checkout: CoreCheckoutSnapshot, in workspace: CoreWorkspaceSnapshot) {
        guard let branch = checkout.branch else {
            interactionNotice = "A detached worktree cannot be the project base branch."
            return
        }
        core.dispatch(kind: "git_worktree_set_base", payload: [
            "repository_root": workspace.path,
            "branch": branch,
        ])
    }

    func copyCheckoutPath(_ checkout: CoreCheckoutSnapshot) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(checkout.path, forType: .string)
    }

    func revealCheckout(_ checkout: CoreCheckoutSnapshot) {
        ExternalFileOpener.reveal(URL(fileURLWithPath: checkout.path))
    }

    func openCheckoutInDefaultEditor(_ checkout: CoreCheckoutSnapshot) {
        ExternalFileOpener.openInDefaultEditor(URL(fileURLWithPath: checkout.path)) { [weak self] message in
            self?.interactionNotice = message
        }
    }

    func confirmBranchMigration() {
        guard let request = branchMigration else { return }
        branchMigration = nil
        core.dispatch(kind: "migrate_main_branch", payload: [
            "repository_root": request.repositoryRoot,
            "base_branch": request.baseBranch,
        ])
    }

    private func observeTaskOperation(in snapshot: CoreSnapshot?) {
        guard let operation = snapshot?.taskOperation,
              operation.phase != "working",
              handledTaskOperationIDs.insert(operation.id).inserted
        else { return }
        defer {
            core.dispatch(kind: "task_operation_ack", payload: ["id": operation.id])
        }
        if operation.phase == "failed" {
            let message = WorktreeSubmissionPresentation.oneLine(
                operation.message ?? "The operation failed."
            )
            if operation.kind == "worktree_create" {
                worktreeError = message
            } else {
                interactionNotice = message
            }
            return
        }
        guard let paneID = operation.paneID, let path = operation.path else {
            let message = "The operation completed without a pane or path."
            if operation.kind == "worktree_create" {
                worktreeError = message
            } else {
                interactionNotice = message
            }
            return
        }
        if operation.kind == "worktree_create" {
            worktreeWorkspace = nil
            worktreeDraft = WorktreeSheetDraft()
            if let raw = operation.agentKind, let provider = AgentProvider(rawValue: raw) {
                guard requireLocalHerdrMutationReadiness() else { return }
                core.startAgentInCreatedPane(
                    paneID: paneID,
                    path: path,
                    provider: provider
                ) { [weak self] result in
                    if !result.succeeded { self?.interactionNotice = result.message }
                }
            }
        }
    }

    private func observeWorktreeRemoval(in snapshot: CoreSnapshot?) {
        guard let removal = snapshot?.worktreeRemoval else { return }
        if removal.phase == "failed", !handledRemovalIDs.contains(removal.id) {
            handledRemovalIDs.insert(removal.id)
            interactionNotice = removal.message ?? "Worktree removal failed."
        }
        guard removal.phase == "ready", handledRemovalIDs.insert(removal.id).inserted else { return }
        Task { @MainActor [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                GitWorktreeRemover.remove(repositoryRoot: removal.repositoryRoot,
                    path: removal.checkoutPath,
                    expectedHeadSHA: removal.expectedHeadSHA,
                    expectedBranch: removal.expectedBranch,
                    protectedBaseBranch: removal.protectedBaseBranch,
                    branch: removal.deleteBranch ? removal.branch : nil)
            }.value
            guard let self else { return }
            core.dispatch(kind: "worktree_removal_finished", payload: [
                "id": removal.id, "removed": result.succeeded, "message": result.message,
            ])
            interactionNotice = result.message
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
        let label = checkout.nextTabLabel
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

    func focusEditorTab(_ tab: CoreEditorTabSnapshot) {
        guard !isRemoteContext else {
            interactionNotice = "Remote file tabs are not available in the current read-only contract."
            return
        }
        core.focusFileTab(tab.id)
    }

    func closeEditorTab(_ tab: CoreEditorTabSnapshot) {
        core.closeFileTab(tab.id)
    }

    func updatePreferences(accentHex: String? = nil, fontSize: Double? = nil) {
        core.persistUIState(
            accentHex: accentHex,
            fontSize: fontSize
        )
    }

    func clearInteractionNotice() {
        interactionNotice = nil
        interactionStatusRefreshAvailable = false
    }

    func dismissHerdrProtocolMismatch() {
        herdrProtocolMismatch = nil
    }

    func openHideReleases() {
        herdrProtocolMismatch = nil
        guard let url = URL(string: "https://github.com/modakbul-gongbang/hide/releases") else {
            interactionNotice = "Hide could not form the Releases address."
            return
        }
        ExternalBrowser.open(url) { [weak self] message in
            self?.interactionNotice = message
        }
    }

    func openHerdrRestartGuide() {
        herdrProtocolMismatch = nil
        guard let url = URL(
            string: "https://github.com/modakbul-gongbang/hide/blob/main/docs/INSTALL.md#hide-says-the-running-herdr-speaks-another-protocol"
        ) else {
            interactionNotice = "Hide could not form the restart guide address."
            return
        }
        ExternalBrowser.open(url) { [weak self] message in
            self?.interactionNotice = message
        }
    }

    func openHerdrProtocolRecovery(_ details: HerdrProtocolMismatchDetails) {
        switch details.recovery {
        case .restartBundledHerdr:
            openHerdrRestartGuide()
        case .updateHide:
            openHideReleases()
        case .reviewDiagnostics:
            break
        }
    }

    func copyHerdrProtocolDiagnostics(_ details: HerdrProtocolMismatchDetails) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(details.diagnostics, forType: .string)
        herdrProtocolMismatch = nil
    }

    @discardableResult
    private func requireLocalHerdrMutationReadiness() -> Bool {
        let readiness = core.localHerdrMutationReadiness
        guard case .connected = readiness else {
            presentLocalHerdrMutationRejection(readiness)
            return false
        }
        return true
    }

    private func presentLocalHerdrMutationRejection(
        _ readiness: LocalHerdrMutationReadiness
    ) {
        switch readiness {
        case .connected:
            return
        case .protocolMismatch(let details):
            interactionNotice = nil
            herdrProtocolMismatch = details
        case .initializing(let message), .unavailable(let message):
            interactionNotice = message
        }
    }

    func addDevice(label: String, alias: String) {
        let trimmedAlias = alias.trimmingCharacters(in: .whitespacesAndNewlines)
        let id = "device:\(trimmedAlias.lowercased().replacingOccurrences(of: "[^a-z0-9]+", with: "-", options: .regularExpression))"
        core.registerDevice(
            id: id,
            label: label.trimmingCharacters(in: .whitespacesAndNewlines),
            sshAlias: trimmedAlias
        )
    }

    func removeDevice(_ device: CoreDeviceSnapshot) {
        core.removeDevice(device.id)
    }

    func testDevice(_ device: CoreDeviceSnapshot) {
        core.testDevice(device.id)
        lastRemoteDevice = device
        if device.sshAlias != nil {
            remote.refresh(targetID: device.id, label: device.label)
        }
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
        guard requireLocalHerdrMutationReadiness() else {
            checkoutStartState = .failed(core.localHerdrMutationReadiness.message)
            return
        }
        guard pendingCheckoutStarts.insert(checkout.id).inserted else {
            checkoutStartState = .starting
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

    /// A launched terminal is "starting" only until its pane is in the
    /// checkout. Leaving the state at `.started` afterwards made the empty
    /// state read "Terminal is starting" the next time that checkout had no
    /// panes, which is exactly when the user needs the start control instead.
    private func settleCheckoutStart() {
        guard case .started = checkoutStartState, !focusedPanes.isEmpty else { return }
        checkoutStartState = .idle
    }

    var leftSidebarVisible: Bool {
        core.snapshot?.uiState.leftSidebarVisible ?? true
    }

    var rightPanelVisible: Bool {
        core.snapshot?.uiState.rightPanelVisible ?? true
    }

    var rightPanelSection: RightPanelSection {
        core.snapshot?.uiState.rightPanelSection ?? .overview
    }

    func selectRightPanelSection(_ section: RightPanelSection) {
        guard section != rightPanelSection else { return }
        core.persistUIState(rightPanelSection: section)
    }

    var changes: CoreChangesSnapshot {
        core.snapshot?.changes ?? .empty
    }

    func selectChangedFile(_ path: String?, committed: Bool = false) {
        core.selectChangedFile(path: path, committed: committed)
    }

    /// What the summary card shows beyond the checkout row's own facts.
    var card: CoreCheckoutCard {
        core.snapshot?.card ?? .empty
    }

    func conversationSessionID(for paneID: String) -> String? {
        card.panes.first(where: { $0.paneID == paneID })?.sessionID
    }

    /// The agents in a checkout, tolerating no selection so the card can ask
    /// without unwrapping first.
    func agents(in checkout: CoreCheckoutSnapshot?) -> [SidebarAgent] {
        guard let checkout else { return [] }
        return SidebarGrouping.agents(agents, in: checkout)
    }

    /// The listening ports attributed to panes in the selected checkout,
    /// de-duplicated: two panes in one directory are one server, not two.
    var portsInFocusedCheckout: [UInt16] {
        guard let checkout = focusedCheckout else { return [] }
        var seen = Set<UInt16>()
        return checkout.tabs
            .flatMap(\.panes)
            .flatMap(\.ports)
            .filter { seen.insert($0).inserted }
            .sorted()
    }

    /// The card's refresh button: read the pull request, the worktree counts,
    /// and the size again, now.
    func requestGithubStatus(_ workspace: CoreWorkspaceSnapshot, refresh: Bool = false) {
        guard workspace.isGit, workspace.remoteTargetID == nil else { return }
        core.dispatch(kind: "github_request", payload: ["workspace_id": workspace.id, "refresh": refresh])
    }

    func refreshCheckoutCard() {
        core.refreshCheckoutCard()
    }

    /// Opens a pull request outside Hide, through the same routing a terminal
    /// link uses.
    func openPullRequest(_ pullRequest: CorePullRequest) {
        guard let url = URL(string: pullRequest.url) else {
            interactionNotice = "Pull request \(pullRequest.number) has no address that can be opened."
            return
        }
        ExternalBrowser.open(url) { [weak self] message in
            self?.interactionNotice = message
        }
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
        if !isRemoteContext,
           core.snapshot?.editor.activeTabID == nil,
           let paneID = focusedPaneID,
           isConversation(for: paneID),
           canShowConversation(for: paneID)
        {
            if let textView = NSApp.keyWindow?.contentView?.firstDescendantFindableTextView() {
                NSApp.keyWindow?.makeFirstResponder(textView)
            }
            FindResponderAction.send(.showFindInterface)
            return
        }
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

    /// What the zoom chords act on: whatever the user is actually looking at.
    /// The editor overlays the terminal surface whenever a file tab is open,
    /// so an open tab means the editor is what is on screen.
    enum TextScaleTarget: Equatable {
        case editor
        case pane(String)
    }

    var textScaleTarget: TextScaleTarget? {
        if !isRemoteContext, core.snapshot?.editor.activeTabID != nil {
            return .editor
        }
        return focusedPaneID.map(TextScaleTarget.pane)
    }

    /// A pane the user has never zoomed is absent from the map, which reads as
    /// the default rather than as a missing value.
    func textScale(for paneID: String) -> CGFloat {
        CGFloat(core.snapshot?.uiState.paneTextScales[paneID] ?? 1)
    }

    func canShowConversation(for paneID: String) -> Bool {
        guard !isRemoteContext else { return false }
        guard let agent = agents.first(where: { $0.paneID == paneID }) else { return false }
        guard ConversationProvider(agentKind: agent.agentKind) != nil else { return false }
        guard let sessionID = conversationSessionID(for: paneID) else { return false }
        return ConversationReader.isSafeSessionIdentifier(
            sessionID.trimmingCharacters(in: .whitespacesAndNewlines)
        )
    }

    func isConversation(for paneID: String) -> Bool {
        core.snapshot?.uiState.conversationPaneIDs.contains(paneID) == true
    }

    var editorTextScale: CGFloat { CGFloat(core.snapshot?.uiState.editorTextScale ?? 1) }

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
        let focusedPane = focusedPaneID.flatMap { paneID in
            focusedPanes.first(where: { $0.id == paneID })
        }
        switch CloseShortcutPolicy.action(
            hasWorkspace: focusedWorkspace != nil,
            hasActiveFileTab: activeFile != nil,
            hasActiveHerdrTab: focusedTab?.id != nil,
            hasFocusedPane: focusedPane != nil,
            tabCount: unifiedTabs.count
        ) {
        case .closePane:
            guard let pane = focusedPane else {
                interactionNotice = "The focused pane could not be resolved. Nothing was closed."
                return
            }
            HideLaunchTrace.mark("tab.close_shortcut.pane", detail: pane.id)
            closePaneFromHeader(pane.id)
        case .closeFile:
            guard let tab = activeFile else {
                interactionNotice = "The active file tab could not be resolved. Nothing was closed."
                return
            }
            HideLaunchTrace.mark("tab.close_shortcut.file", detail: tab.id)
            closeEditorTab(tab)
        case .closeHerdr:
            guard let tab = focusedTab, let tabID = tab.id else {
                interactionNotice = "The active Herdr tab could not be resolved. Nothing was closed."
                return
            }
            HideLaunchTrace.mark("tab.close_shortcut.herdr", detail: tabID)
            requestTabClose(tab)
        case .nothingToClose:
            HideLaunchTrace.mark(
                "tab.close_shortcut.nothing_to_close",
                detail: focusedWorkspace == nil ? "no_workspace" : "workspace_without_tabs"
            )
        case .blocked:
            interactionNotice = "The selected workspace has tabs but none is active. Nothing was closed."
            HideLaunchTrace.mark("tab.close_shortcut.blocked", detail: "tabs_without_active_tab")
        }
    }

    /// Selecting the tab that is already active is a no-op, not a re-focus:
    /// re-sending the focus command makes the core hand focus to that tab's
    /// first pane, so the chord moved the caret off whatever pane the user was
    /// working in. Reaching for the tab you are already on must change
    /// nothing.
    func focusUnifiedTab(_ item: ShellTabItem) {
        guard !item.active else { return }
        switch item.kind {
        case .herdr(let tab): focusTab(tab)
        case .editor(let tab): focusEditorTab(tab)
        }
    }

    /// ⌘1…⌘9 select the nth tab in the strip, in the order the strip draws
    /// it. A number past the end is a miss, not an error: the user reached
    /// for a slot that is simply empty right now.
    func selectTab(shortcutNumber: Int) {
        guard let tab = TabShortcutNumbering.tab(atNumber: shortcutNumber, in: unifiedTabs)
        else { return }
        focusUnifiedTab(tab)
    }

    func tabShortcutNumber(tabID: String) -> Int? {
        TabShortcutNumbering.number(ofTabID: tabID, in: unifiedTabs)
    }

    /// Reports a tab the operator dropped at a new place in the strip.
    ///
    /// The shell does not reorder anything itself. It says which entry was
    /// dropped where, and the core decides whether that is a slot it owns or
    /// an order it has to ask Herdr for. The strip redraws from the next
    /// snapshot either way.
    func reorderUnifiedTab(_ item: ShellTabItem, to index: Int) {
        guard let workspace = focusedWorkspace, let checkout = focusedCheckout else {
            interactionNotice = "The dragged tab has no routable workspace context. No tab was moved."
            return
        }
        guard !isRemoteContext else {
            interactionNotice = "A remote target's tab order is Herdr's alone. No tab was moved."
            return
        }
        let tabs = unifiedTabs
        guard let from = tabs.firstIndex(where: { $0.id == item.id }),
              tabs.indices.contains(index),
              from != index
        else { return }
        core.reorderTab(
            workspaceID: workspace.id,
            checkoutID: checkout.id,
            tabID: item.id,
            toIndex: index
        )
    }

    func closeUnifiedTab(_ item: ShellTabItem) {
        switch item.kind {
        case .herdr(let tab): requestTabClose(tab)
        case .editor(let tab): closeEditorTab(tab)
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
        let destructiveTargets = tab.panes.map { destructiveTarget(for: $0) }
        if let unknownTarget = destructiveTargets.first(where: \.requiresStatusCheck) {
            pendingTabCloseTarget = nil
            pendingCloseScopePaneIDs = nil
            pendingCloseApprovalFingerprint = nil
            consequenceNotice = nil
            presentActivityStatusUnknown(label: unknownTarget.label)
            return
        }
        let notice = ConsequencePolicy.notice(kind: .tab, targets: destructiveTargets)
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingTabCloseTarget = target
            pendingCloseScopePaneIDs = tab.panes.map(\.id).sorted()
            pendingCloseApprovalFingerprint = closeApprovalFingerprint(
                paneIDs: pendingCloseScopePaneIDs ?? []
            )
            consequenceNotice = notice
        } else {
            pendingTabCloseTarget = nil
            pendingCloseScopePaneIDs = nil
            pendingCloseApprovalFingerprint = nil
            executeTabClose(target, confirmed: false)
            consequenceResult = "Close requested for tab \(tab.label ?? tabID)."
        }
    }

    private func toggleCurrentPaneZoom() {
        core.toggleCurrentPaneZoom()
        focus(.terminal)
    }

    func togglePaneZoom(_ paneID: String) {
        core.togglePaneZoom(paneID)
        focus(.terminal)
    }

    func toggleConversation(_ paneID: String) {
        guard canShowConversation(for: paneID) else {
            interactionNotice = "Conversation view is available only for local Claude and Codex agent panes."
            return
        }
        core.toggleConversation(paneID)
        focus(.terminal)
    }

    /// Opens a port a server in this pane's directory is listening on.
    ///
    /// Chrome first, the default browser when Chrome is absent, and a stated
    /// reason when neither takes it - the same routing a terminal link uses,
    /// because it is the same question.
    func openPanePort(_ port: UInt16) {
        guard let url = PaneHeaderControls.portURL(port) else {
            interactionNotice = "Port \(port) does not form an address that can be opened."
            return
        }
        ExternalBrowser.open(url) { [weak self] message in
            self?.interactionNotice = message
        }
    }

    /// Closes one pane from its own header.
    ///
    /// This routes through the same consequence flow the close command uses, so
    /// a pane whose agent is working states that before it goes rather than
    /// being discarded by a control that happens to sit closer to the cursor.
    func closePaneFromHeader(_ paneID: String) {
        closeCurrentPane(target: isRemoteContext ? .remote(paneID: paneID) : .local(paneID: paneID))
    }

    /// Whether this pane's header offers a fork.
    ///
    /// The core decides whether the agent can be forked at all; what is added
    /// here is that a remote pane has no fork route, so the control is withheld
    /// rather than dispatching an event the core would block anyway.
    func canForkPane(_ pane: CorePaneSnapshot) -> Bool {
        !isRemoteContext && PaneHeaderControls.showsFork(pane.fork)
    }

    /// Forks one pane into a sibling carrying its conversation forward.
    ///
    /// The wait is real - the agent has to start before the pane appears - and
    /// it is shown where the split already shows its own wait: as a suffix on
    /// that pane's header. A modal was the wrong shape for it, because it made
    /// the operator dismiss a dialog to see the result it was covering, and it
    /// said the same thing whether the fork then worked or failed.
    func forkPaneFromHeader(_ paneID: String) {
        guard requireLocalHerdrMutationReadiness() else { return }
        paneNotices[paneID] = nil
        guard let pane = paneMetadata(for: paneID), canForkPane(pane) else {
            paneNotices[paneID] = "This pane has no agent session that can be forked."
            return
        }
        panesForking.insert(paneID)
        core.forkPane(paneID)
    }

    private func destructiveTarget(for pane: CorePaneSnapshot) -> DestructiveTarget {
        let agent = agents.first { $0.paneID == pane.id }
        let contentConsequence = pane.content.closeConsequence
        return DestructiveTarget(
            id: pane.id,
            label: pane.herdrLabel ?? pane.id,
            statusLabel: agent?.statusLabel ?? "Idle",
            requiresCloseConfirmation: contentConsequence != nil || (agent?.requiresCloseConfirmation ?? false),
            summary: contentConsequence ?? agent?.detail ?? "No working or attention state is reported for this pane.",
            requiresStatusCheck: pane.requiresCloseStatusCheck || (agent?.requiresCloseStatusCheck ?? false),
            contentConsequence: contentConsequence
        )
    }

    private func presentActivityStatusUnknown(label: String) {
        interactionStatusRefreshAvailable = true
        interactionNotice = "Activity status for \(label) is unknown. Check status before closing."
    }

    private func closeCurrentPane(target closeTarget: PaneCloseTarget) {
        if case .local = closeTarget {
            guard requireLocalHerdrMutationReadiness() else { return }
        }
        let paneID = closeTarget.paneID
        guard let pane = paneMetadata(for: paneID) else {
            consequenceResult = "Select a pane before closing."
            return
        }
        let target = destructiveTarget(for: pane)
        if target.requiresStatusCheck {
            pendingPaneCloseTarget = nil
            pendingCloseScopePaneIDs = nil
            pendingCloseApprovalFingerprint = nil
            consequenceNotice = nil
            presentActivityStatusUnknown(label: target.label)
            return
        }
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingPaneCloseTarget = closeTarget
            pendingCloseScopePaneIDs = closeScopePaneIDs(for: paneID)
            pendingCloseApprovalFingerprint = closeApprovalFingerprint(
                paneIDs: pendingCloseScopePaneIDs ?? []
            )
            consequenceNotice = notice
        } else {
            pendingPaneCloseTarget = nil
            pendingCloseScopePaneIDs = nil
            pendingCloseApprovalFingerprint = nil
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
            switch textScaleTarget {
            case .none:
                interactionNotice = "Select a pane before changing its text size."
            case .editor:
                core.setEditorTextScale(direction: direction)
            case .pane(let paneID):
                core.setPaneTextScale(paneID: paneID, direction: direction)
            }
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
            case .toggleConversation:
                guard let paneID = focusedPaneID else {
                    interactionNotice = "Select an agent pane before opening its conversation."
                    return
                }
                toggleConversation(paneID)
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
            case .toggleConversation:
                interactionNotice = "Conversation view is available only for local agent panes."
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
        pendingCloseScopePaneIDs = nil
        pendingCloseApprovalFingerprint = nil
        let agents = core.snapshot?.navigator.agents ?? []
        let targets = agents.map {
            DestructiveTarget(
                id: $0.paneID,
                label: $0.identityLabel,
                statusLabel: $0.statusLabel,
                requiresCloseConfirmation: $0.requiresCloseConfirmation,
                summary: $0.detail ?? $0.statusLabel,
                requiresStatusCheck: $0.requiresCloseStatusCheck
            )
        }
        consequenceNotice = ConsequencePolicy.notice(kind: kind, targets: targets)
        consequenceResult = nil
    }

    func confirmConsequencePreview() {
        guard let consequenceNotice else { return }
        if let target = pendingTabCloseTarget {
            guard closeApprovalStillMatches(target: target) else {
                consequenceResult = nil
                interactionNotice = "The tab changed while it was waiting for confirmation. Select it and review the close again."
                pendingTabCloseTarget = nil
                pendingCloseScopePaneIDs = nil
                pendingCloseApprovalFingerprint = nil
                self.consequenceNotice = nil
                return
            }
            if case .local = target, !requireLocalHerdrMutationReadiness() {
                pendingTabCloseTarget = nil
                pendingCloseScopePaneIDs = nil
                pendingCloseApprovalFingerprint = nil
                self.consequenceNotice = nil
                return
            }
            executeTabClose(target, confirmed: true)
            consequenceResult = "Confirmed close requested for tab \(target.tabID)."
            pendingTabCloseTarget = nil
        } else if let target = pendingPaneCloseTarget {
            guard closeApprovalStillMatches(target: target) else {
                consequenceResult = nil
                interactionNotice = "The tab changed while it was waiting for confirmation. Select the pane and review the close again."
                pendingPaneCloseTarget = nil
                pendingCloseScopePaneIDs = nil
                pendingCloseApprovalFingerprint = nil
                self.consequenceNotice = nil
                return
            }
            if case .local = target, !requireLocalHerdrMutationReadiness() {
                pendingPaneCloseTarget = nil
                pendingCloseScopePaneIDs = nil
                pendingCloseApprovalFingerprint = nil
                self.consequenceNotice = nil
                return
            }
            let paneID = target.paneID
            executePaneClose(target, confirmed: true)
            consequenceResult = "Confirmed close requested for pane \(paneID)."
            pendingPaneCloseTarget = nil
        } else {
            consequenceResult = consequenceNotice.requiresConfirmation
                ? "Confirmation recorded for the prefixed verification preview. No user resource was changed."
                : "Idle close requires no confirmation. No user resource was changed in this preview."
        }
        pendingCloseScopePaneIDs = nil
        pendingCloseApprovalFingerprint = nil
        self.consequenceNotice = nil
    }

    func cancelConsequencePreview() {
        consequenceResult = "Cancelled before any process or checkout was affected."
        pendingPaneCloseTarget = nil
        pendingTabCloseTarget = nil
        pendingCloseScopePaneIDs = nil
        pendingCloseApprovalFingerprint = nil
        consequenceNotice = nil
    }

    private func closeScopePaneIDs(for paneID: String) -> [String]? {
        let tab = workspaces
            .lazy
            .flatMap(\.checkouts)
            .flatMap(\.tabs)
            .first(where: { $0.panes.contains(where: { $0.id == paneID }) })
        return tab?.panes.map(\.id).sorted()
    }

    private func closeTabScopePaneIDs(for tabID: String) -> [String]? {
        if let tab = workspaces
            .lazy
            .flatMap(\.checkouts)
            .flatMap(\.tabs)
            .first(where: { $0.id == tabID })
        {
            return tab.panes.map(\.id).sorted()
        }
        return nil
    }

    private func closeApprovalFingerprint(paneIDs: [String]) -> String {
        let panes = paneIDs.sorted().map { paneID in
            guard let pane = paneMetadata(for: paneID) else {
                return "missing|\(paneID)"
            }
            let target = destructiveTarget(for: pane)
            return [
                paneID,
                target.statusLabel,
                target.requiresCloseConfirmation ? "confirm" : "idle",
                target.requiresStatusCheck ? "unknown" : "known",
                target.summary,
            ].joined(separator: "\u{1F}")
        }
        return [focusedPaneID ?? "none", panes.joined(separator: "\u{1E}")]
            .joined(separator: "\u{1D}")
    }

    private func closeApprovalStillMatches(target: TabCloseTarget) -> Bool {
        guard let expected = pendingCloseScopePaneIDs,
              let expectedFingerprint = pendingCloseApprovalFingerprint
        else { return false }
        let current: [String]?
        switch target {
        case .local(let tabID), .remote(_, let tabID):
            current = closeTabScopePaneIDs(for: tabID)
        }
        return current == expected && closeApprovalFingerprint(paneIDs: current ?? []) == expectedFingerprint
    }

    private func closeApprovalStillMatches(target: PaneCloseTarget) -> Bool {
        guard let expected = pendingCloseScopePaneIDs,
              let expectedFingerprint = pendingCloseApprovalFingerprint,
              let current = closeScopePaneIDs(for: target.paneID)
        else { return false }
        return current == expected && closeApprovalFingerprint(paneIDs: current) == expectedFingerprint
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

/// Which pane a fork failure belongs to.
enum ForkFailurePresentation {
    /// The core names the pane in the message it raises, so the pane is found
    /// there rather than parsed out of the message's shape.
    ///
    /// Plain containment is not enough: `w1:p1` is a substring of `w1:p10`, so
    /// a failure for the second would be shown on the first. An id counts only
    /// where the message stops spelling an id, and when more than one still
    /// fits, the longest wins. Candidates are ordered before they are compared
    /// so the answer never depends on set iteration order.
    static func owner(of message: String, amongst forking: Set<String>) -> String? {
        forking
            .filter { names($0, in: message) }
            .sorted { ($0.count, $0) > ($1.count, $1) }
            .first
    }

    private static func names(_ paneID: String, in message: String) -> Bool {
        guard !paneID.isEmpty else { return false }
        var searchStart = message.startIndex
        while let found = message.range(of: paneID, range: searchStart..<message.endIndex) {
            let beforeIsIDCharacter = found.lowerBound > message.startIndex
                && isIDCharacter(message[message.index(before: found.lowerBound)])
            let afterIsIDCharacter = found.upperBound < message.endIndex
                && isIDCharacter(message[found.upperBound])
            if !beforeIsIDCharacter && !afterIsIDCharacter {
                return true
            }
            searchStart = found.upperBound
        }
        return false
    }

    private static func isIDCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || character == ":" || character == "-"
            || character == "_"
    }
}
