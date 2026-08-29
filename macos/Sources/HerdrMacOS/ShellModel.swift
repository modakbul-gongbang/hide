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

@MainActor
final class ShellModel: ObservableObject {
    @Published var activeSurface: ShellSurface = .terminal
    @Published var consequenceNotice: ConsequenceNotice?
    @Published var consequenceResult: String?
    let core: CoreBridge
    let browser: BrowserRuntimeModel
    let remote: RemoteRuntimeModel
    private var coreSubscription: AnyCancellable?
    private var browserSubscription: AnyCancellable?
    private var remoteSubscription: AnyCancellable?

    init(
        core: CoreBridge = CoreBridge(),
        browser: BrowserRuntimeModel = BrowserRuntimeModel(),
        remote: RemoteRuntimeModel = RemoteRuntimeModel()
    ) {
        self.core = core
        self.browser = browser
        self.remote = remote
        coreSubscription = core.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        browserSubscription = browser.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        remoteSubscription = remote.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        browser.onReceipt = { [weak core] receipt in
            core?.recordBrowserStatus(receipt)
        }
    }

    func focus(_ surface: ShellSurface) {
        activeSurface = surface
    }

    func previewConsequence(_ kind: DestructiveTargetKind) {
        let agents = core.snapshot?.navigator.agents ?? []
        let targets = agents.map {
            DestructiveTarget(id: $0.paneID, label: $0.workspaceLabel, state: $0.state, summary: $0.summary)
        }
        consequenceNotice = ConsequencePolicy.notice(kind: kind, targets: targets)
        consequenceResult = nil
    }

    func confirmConsequencePreview() {
        guard let consequenceNotice else { return }
        consequenceResult = consequenceNotice.requiresConfirmation
            ? "Confirmation recorded for the prefixed verification preview. No user resource was changed."
            : "Idle close requires no confirmation. No user resource was changed in this preview."
        self.consequenceNotice = nil
    }

    func cancelConsequencePreview() {
        consequenceResult = "Cancelled before any process or checkout was affected."
        consequenceNotice = nil
    }
}
