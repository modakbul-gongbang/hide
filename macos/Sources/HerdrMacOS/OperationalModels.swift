import AppKit
import CoreFoundation
import Foundation

enum RuntimePhase: String, Codable, Sendable {
    case idle
    case loading
    case ready
    case stale
    case unavailable
    case failed
}

/// Writes a verification receipt atomically and reports a write failure on
/// stderr, so a receipt that never lands is visible instead of silently absent.
enum VerificationReceipt {
    /// Emits one JSON record as its own stderr line.
    static func writeLine(_ data: Data) {
        FileHandle.standardError.write(data)
        FileHandle.standardError.write(Data("\n".utf8))
    }

    static func write(_ data: Data, to path: String, failureKind: String) {
        do {
            try data.write(to: URL(fileURLWithPath: path), options: .atomic)
        } catch {
            writeLine(Data("{\"kind\":\"\(failureKind)\",\"path\":\"\(path)\"}".utf8))
        }
    }
}

struct RemoteWorkspaceSummary: Decodable, Identifiable, Sendable {
    let workspaceID: String
    let label: String
    let paneCount: Int
    let activeTabID: String?

    var id: String { workspaceID }

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case label
        case paneCount = "pane_count"
        case activeTabID = "active_tab_id"
    }
}

struct RemoteFileNode: Identifiable, Hashable, Sendable {
    let path: String
    let name: String
    let isDirectory: Bool

    var id: String { path }
}

struct RemotePaneLayoutFrame: Decodable, Equatable, Sendable {
    let paneID: String
    let x: Double
    let y: Double
    let width: Double
    let height: Double

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case x
        case y
        case width
        case height
    }
}

struct RemotePaneLayoutSnapshot: Decodable, Equatable, Sendable {
    let workspaceID: String
    let tabID: String
    let focusedPaneID: String
    let zoomed: Bool
    let frames: [RemotePaneLayoutFrame]

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case tabID = "tab_id"
        case focusedPaneID = "focused_pane_id"
        case zoomed
        case frames
    }

}

struct RemoteNavigationSnapshot {
    let deviceID: String
    let targetLabel: String
    let workspaces: [CoreWorkspaceSnapshot]
    let agents: [SidebarAgent]
    let focusedWorkspaceID: String?
    let focusedCheckoutID: String?
    let focusedTabID: String?
    let focusedPaneID: String?
    let paneLayouts: [RemotePaneLayoutSnapshot]
    private let activeTabIDs: [String: String]

    var focusedPaneLayout: RemotePaneLayoutSnapshot? {
        guard let focusedTabID else { return nil }
        return paneLayouts.first(where: { $0.tabID == focusedTabID })
    }

    init(deviceID: String, targetLabel: String, core: CoreRemoteSessionSnapshot) {
        self.deviceID = deviceID
        self.targetLabel = targetLabel
        workspaces = core.workspaces
        agents = core.agents
        focusedWorkspaceID = core.focusedWorkspaceID
        focusedCheckoutID = core.focusedCheckoutID
        focusedTabID = core.focusedTabID
        focusedPaneID = core.focusedPaneID
        paneLayouts = core.paneLayouts
        activeTabIDs = core.activeTabIDs
    }

    private init(
        deviceID: String,
        targetLabel: String,
        workspaces: [CoreWorkspaceSnapshot],
        agents: [SidebarAgent],
        focusedWorkspaceID: String?,
        focusedCheckoutID: String?,
        focusedTabID: String?,
        focusedPaneID: String?,
        paneLayouts: [RemotePaneLayoutSnapshot],
        activeTabIDs: [String: String]
    ) {
        self.deviceID = deviceID
        self.targetLabel = targetLabel
        self.workspaces = workspaces
        self.agents = agents
        self.focusedWorkspaceID = focusedWorkspaceID
        self.focusedCheckoutID = focusedCheckoutID
        self.focusedTabID = focusedTabID
        self.focusedPaneID = focusedPaneID
        self.paneLayouts = paneLayouts
        self.activeTabIDs = activeTabIDs
    }

    func focused(
        workspaceID: String,
        checkoutID: String,
        tabID: String? = nil,
        paneID: String? = nil
    ) -> RemoteNavigationSnapshot {
        let selectedCheckout = workspaces
            .first(where: { $0.id == workspaceID })?
            .checkouts
            .first(where: { $0.id == checkoutID })
        let selectedTabs: [CoreTabSnapshot] = selectedCheckout?.tabs ?? []
        var selectedTab: CoreTabSnapshot?

        if let paneID {
            selectedTab = selectedTabs.first { tab in
                tab.panes.contains { pane in pane.id == paneID }
            }
        }

        if selectedTab == nil, let tabID {
            selectedTab = selectedTabs.first { tab in
                tab.id == tabID
            }
        }

        if selectedTab == nil, let activeTabID = activeTabIDs[checkoutID] {
            selectedTab = selectedTabs.first { tab in
                tab.id == activeTabID
            }
        }

        if selectedTab == nil {
            selectedTab = selectedTabs.first
        }

        let selectedPaneID: String?
        if let paneID,
           selectedTab?.panes.contains(where: { pane in pane.id == paneID }) == true {
            selectedPaneID = paneID
        } else {
            selectedPaneID = selectedTab?.panes.first?.id
        }
        return RemoteNavigationSnapshot(
            deviceID: deviceID,
            targetLabel: targetLabel,
            workspaces: workspaces,
            agents: agents,
            focusedWorkspaceID: workspaceID,
            focusedCheckoutID: checkoutID,
            focusedTabID: selectedTab?.id,
            focusedPaneID: selectedPaneID,
            paneLayouts: paneLayouts,
            activeTabIDs: activeTabIDs
        )
    }

    func focusedPane(_ paneID: String) -> RemoteNavigationSnapshot {
        RemoteNavigationSnapshot(
            deviceID: deviceID,
            targetLabel: targetLabel,
            workspaces: workspaces,
            agents: agents,
            focusedWorkspaceID: focusedWorkspaceID,
            focusedCheckoutID: focusedCheckoutID,
            focusedTabID: focusedTabID,
            focusedPaneID: paneID,
            paneLayouts: paneLayouts,
            activeTabIDs: activeTabIDs
        )
    }

    static func workspaceID(deviceID: String, remoteID: String) -> String {
        "remote:\(deviceID):workspace:\(remoteID)"
    }

    static func checkoutID(deviceID: String, remoteID: String) -> String {
        "remote:\(deviceID):checkout:\(remoteID)"
    }

}

@MainActor
final class RemoteRuntimeModel: ObservableObject {
    @Published private(set) var phase: RuntimePhase = .idle
    @Published private(set) var isRefreshing = false
    @Published private(set) var message = "No remote device has been selected yet."
    @Published private(set) var workspaces: [RemoteWorkspaceSummary] = []
    @Published private(set) var navigation: RemoteNavigationSnapshot?
    @Published private(set) var files: [RemoteFileNode] = []
    @Published private(set) var fileState = "idle"
    @Published private(set) var fileError: String?
    /// What allowing Hide's helper on this device installs and runs, while
    /// the device has no consent; the panel asks for it with this text.
    @Published private(set) var fileConsentPrompt: String?
    @Published private(set) var checkedAt = "never"
    @Published private(set) var targetLabel = ""
    private var activeTargetID: String?
    private var statusesByTarget: [String: CoreRemoteStatus] = [:]

    var statusMessage: String {
        message
    }

    func clearNavigation() {
        activeTargetID = nil
        isRefreshing = false
        navigation = nil
        workspaces = []
        files = []
        fileState = "idle"
        fileError = nil
        fileConsentPrompt = nil
        phase = .idle
        message = "No remote device has been selected yet."
    }

    func ingest(_ statuses: [CoreRemoteStatus]) {
        statusesByTarget = Dictionary(uniqueKeysWithValues: statuses.map { ($0.targetID, $0) })
        guard let activeTargetID else { return }
        guard let status = statusesByTarget[activeTargetID] else {
            // The device was removed; a projection of a target the core no
            // longer has would otherwise stay on screen as "Ready".
            clearNavigation()
            return
        }
        apply(status)
    }

    func refresh(targetID: String, label: String) {
        activeTargetID = targetID
        files = []
        fileState = "idle"
        fileError = nil
        fileConsentPrompt = nil
        targetLabel = label
        if let status = statusesByTarget[targetID] {
            apply(status)
        } else {
            isRefreshing = true
            phase = .loading
            message = "Waiting for the Rust remote session coordinator to report \(label)…"
        }
    }

    private func apply(_ status: CoreRemoteStatus) {
        isRefreshing = status.state == "not_connected"
        checkedAt = ISO8601DateFormatter().string(from: Date())

        if let session = status.session {
            var projection = RemoteNavigationSnapshot(
                deviceID: status.targetID,
                targetLabel: targetLabel,
                core: session
            )
            if let current = navigation,
               current.deviceID == status.targetID,
               let workspaceID = current.focusedWorkspaceID,
               let checkoutID = current.focusedCheckoutID {
                projection = projection.focused(
                    workspaceID: workspaceID,
                    checkoutID: checkoutID,
                    tabID: current.focusedTabID,
                    paneID: current.focusedPaneID
                )
            }
            navigation = projection
            workspaces = projection.workspaces.map { workspace in
                RemoteWorkspaceSummary(
                    workspaceID: workspace.id,
                    label: workspace.label,
                    paneCount: workspace.checkouts.flatMap { $0.tabs }.flatMap { $0.panes }.count,
                    activeTabID: workspace.checkouts.first?.tabs.first?.id
                )
            }
            let selectedRoot = projection.workspaces
                .flatMap(\.checkouts)
                .first(where: { $0.id == projection.focusedCheckoutID })?
                .path
            if status.files.rootPath == selectedRoot {
                fileState = status.files.state
                files = status.files.entries.map { entry in
                    RemoteFileNode(
                        path: entry.path,
                        name: entry.name,
                        isDirectory: entry.isDirectory
                    )
                }
                fileError = status.files.state == "unavailable"
                    ? status.files.message ?? "Remote files are unavailable."
                    : nil
                fileConsentPrompt = status.files.state == "not_allowed"
                    ? status.files.message ?? "Allow Hide's helper on this device to read its files."
                    : nil
            } else {
                fileState = "idle"
                files = []
                fileError = nil
                fileConsentPrompt = nil
            }
        }

        switch status.state {
        case "connected":
            phase = .ready
            message = workspaces.isEmpty
                ? "\(targetLabel) is connected, but no remote workspace is open."
                : "\(targetLabel) connected through the official Herdr Socket API."
            log(kind: "remote.ready")
        case "not_connected":
            phase = .loading
            message = status.message ?? "Waiting for the first remote Herdr connection attempt."
        case "stale":
            phase = .stale
            message = status.message ?? "The last remote session is visible while Herdr reconnects."
        case "disabled", "socket_missing":
            phase = .unavailable
            message = status.message ?? "Remote Herdr is unavailable."
        default:
            phase = .failed
            message = status.message ?? "Remote Herdr synchronization failed."
        }
    }

    func focus(
        workspaceID: String,
        checkoutID: String,
        tabID: String? = nil,
        paneID: String? = nil
    ) {
        navigation = navigation?.focused(
            workspaceID: workspaceID,
            checkoutID: checkoutID,
            tabID: tabID,
            paneID: paneID
        )
    }

    func focusPane(_ paneID: String) {
        navigation = navigation?.focusedPane(paneID)
    }

    private func log(kind: String) {
        let record: [String: Any] = [
            "kind": kind,
            "target": targetLabel,
            "state": phase.rawValue,
            "workspace_count": workspaces.count,
            "file_count": files.count,
            "checked_at": checkedAt,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: record) else { return }
        VerificationReceipt.writeLine(data)
    }
}

enum DestructiveTargetKind: String, Sendable {
    case pane
    case tab
    case workspace
    case worktree
}

struct DestructiveTarget: Identifiable, Equatable, Sendable {
    let id: String
    let label: String
    /// The short human word for what this target is doing right now.
    let statusLabel: String
    /// Whether stopping this target interrupts work or discards an unread
    /// result. Decided by the core, never by a list of state names here.
    let requiresCloseConfirmation: Bool
    let summary: String
    /// Whether the core needs a fresh activity status before this target can
    /// be closed safely.
    var requiresStatusCheck: Bool = false
}

struct ConsequenceNotice: Equatable, Identifiable, Sendable {
    let title: String
    let consequence: String
    let affected: [DestructiveTarget]
    let requiresConfirmation: Bool

    var id: String { title }

    /// The prompt body: the consequence, then the working panes it lists.
    var message: String {
        let listed = affected.map { "\($0.label) - \($0.statusLabel)" }
        return ([consequence] + listed).joined(separator: "\n")
    }
}

enum ConsequencePolicy {
    static func notice(kind: DestructiveTargetKind, targets: [DestructiveTarget]) -> ConsequenceNotice {
        let risky = targets.filter(\.requiresCloseConfirmation)
        switch kind {
        case .pane:
            return ConsequenceNotice(
                title: risky.isEmpty ? "Close idle pane" : "Stop the active pane?",
                consequence: risky.isEmpty
                    ? "The idle pane closes immediately. Its terminal history will no longer be available in this window."
                    : "Closing this pane terminates its running process and interrupts the listed work.",
                affected: risky.isEmpty ? targets : risky,
                requiresConfirmation: !risky.isEmpty
            )
        case .tab:
            return aggregate("Close this tab?", "Closing the tab terminates all listed working or attention panes in one operation.", targets)
        case .workspace:
            return aggregate("Close this workspace?", "Closing the workspace terminates all listed working or attention panes and closes its tabs.", targets)
        case .worktree:
            return ConsequenceNotice(
                title: "Remove this checkout from Hide?",
                consequence: "Only Hide's registration is removed. The checkout directory on disk, repository, branch, and running processes remain untouched.",
                affected: targets,
                requiresConfirmation: false
            )
        }
    }

    private static func aggregate(_ title: String, _ consequence: String, _ targets: [DestructiveTarget]) -> ConsequenceNotice {
        let affected = targets.filter(\.requiresCloseConfirmation)
        return ConsequenceNotice(
            title: title,
            consequence: consequence,
            affected: affected,
            requiresConfirmation: !affected.isEmpty
        )
    }
}
