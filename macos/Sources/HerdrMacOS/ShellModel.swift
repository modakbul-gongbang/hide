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
