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

struct BrowserRuntimeReceipt: Codable, Sendable {
    let phase: RuntimePhase
    let profile: String
    let action: String
    let message: String
    let pid: Int32?
    let port: Int?
    let currentURL: String?
    let currentTitle: String?
    let checkedAt: String
    var focusRequested = false
    var focusActivationAccepted: Bool?
    var focusObserved: Bool?
}

private struct ChromuxProcessList: Decodable {
    let profiles: [ChromuxProfile]
}

private struct ChromuxProfile: Decodable {
    let profile: String
    let port: Int?
    let pid: Int32?
    let status: String
    let daemon: String?
}

struct ChromuxTab: Decodable {
    let url: String?
    let title: String?
    let type: String?

    private enum CodingKeys: String, CodingKey {
        case url
        case title
        case type
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        url = try container.decodeIfPresent(String.self, forKey: .url)
        type = try container.decodeIfPresent(String.self, forKey: .type)
        title = try container.decodeIfPresent(String.self, forKey: .title).map { encodedTitle in
            guard encodedTitle.contains("&"),
                  let decodedTitle = CFXMLCreateStringByUnescapingEntities(
                      kCFAllocatorDefault,
                      encodedTitle as CFString,
                      nil
                  )
            else { return encodedTitle }
            return decodedTitle as String
        }
    }
}

enum ChromuxTabDecoder {
    static func decode(_ data: Data) throws -> [ChromuxTab] {
        try JSONDecoder().decode([ChromuxTab].self, from: data)
    }
}

private struct ProcessReceipt: Sendable {
    let status: Int32
    let stdout: Data
    let stderr: Data
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

enum SafeProcess {
    fileprivate static func run(executable: String, arguments: [String]) -> ProcessReceipt {
        let process = Process()
        let output = Pipe()
        let error = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardOutput = output
        process.standardError = error
        do {
            try process.run()
        } catch {
            return ProcessReceipt(
                status: 127,
                stdout: Data(),
                stderr: Data(error.localizedDescription.utf8)
            )
        }
        process.waitUntilExit()
        return ProcessReceipt(
            status: process.terminationStatus,
            stdout: output.fileHandleForReading.readDataToEndOfFile(),
            stderr: error.fileHandleForReading.readDataToEndOfFile()
        )
    }
}

enum ChromuxExecutor {
    static func inspectAndOpen(
        profile: String,
        shouldOpen: Bool,
        pathState: String,
        endpointPortOverride: Int? = nil
    ) async -> BrowserRuntimeReceipt {
        let checkedAt = ISO8601DateFormatter().string(from: Date())
        guard profile == "default" || profile == "herdr-ide-verify-absent" else {
            return receipt(
                phase: .unavailable,
                profile: profile,
                action: "refused",
                message: "Only the default profile and the verification-only absent profile are allowed.",
                checkedAt: checkedAt
            )
        }
        guard pathState == "available" else {
            return receipt(
                phase: .unavailable,
                profile: profile,
                action: "unavailable",
                message: "chromux is hidden from this app process PATH. Restore the login-shell PATH and retry.",
                checkedAt: checkedAt
            )
        }
        guard let executable = HideRuntimeEnvironment.resolveExecutable(named: "chromux") else {
            return receipt(
                phase: .unavailable,
                profile: profile,
                action: "unavailable",
                message: "chromux is not installed in the login-shell executable PATH.",
                checkedAt: checkedAt
            )
        }

        let first = SafeProcess.run(executable: executable, arguments: ["ps", "--json"])
        guard first.status == 0 else {
            return receipt(
                phase: .stale,
                profile: profile,
                action: "status-failed",
                message: visibleFailure(first, fallback: "chromux status failed."),
                checkedAt: checkedAt
            )
        }
        guard let list = try? JSONDecoder().decode(ChromuxProcessList.self, from: first.stdout) else {
            return receipt(
                phase: .stale,
                profile: profile,
                action: "status-invalid",
                message: "chromux ps returned an unreadable contract.",
                checkedAt: checkedAt
            )
        }
        if profile == "herdr-ide-verify-absent",
           !list.profiles.contains(where: { $0.profile == profile }) {
            return receipt(
                phase: .unavailable,
                profile: profile,
                action: "missing-profile",
                message: "Profile herdr-ide-verify-absent does not exist. Create it explicitly with chromux profile new if it is ever needed.",
                checkedAt: checkedAt
            )
        }

        var selected = list.profiles.first(where: { $0.profile == profile })
        var action = "status"
        if shouldOpen {
            if selected?.status == "running" {
                action = "reuse"
            } else if profile == "default" {
                let launch = SafeProcess.run(executable: executable, arguments: ["launch", "default"])
                guard launch.status == 0 else {
                    return receipt(
                        phase: .failed,
                        profile: profile,
                        action: "launch-failed",
                        message: visibleFailure(launch, fallback: "chromux launch default failed."),
                        checkedAt: checkedAt
                    )
                }
                action = "launch"
                let refreshed = SafeProcess.run(executable: executable, arguments: ["ps", "--json"])
                if refreshed.status == 0,
                   let refreshedList = try? JSONDecoder().decode(ChromuxProcessList.self, from: refreshed.stdout) {
                    selected = refreshedList.profiles.first(where: { $0.profile == profile })
                }
            } else {
                return receipt(
                    phase: .unavailable,
                    profile: profile,
                    action: "missing-profile",
                    message: "The requested profile is absent and was not created.",
                    checkedAt: checkedAt
                )
            }
        }

        guard let selected, selected.status == "running", let port = selected.port else {
            return receipt(
                phase: .stale,
                profile: profile,
                action: action,
                message: "The profile is known but its browser endpoint is not running.",
                pid: selected?.pid,
                port: selected?.port,
                checkedAt: checkedAt
            )
        }
        guard selected.daemon == nil || selected.daemon == "ok" || selected.daemon == "idle" else {
            return receipt(
                phase: .stale,
                profile: profile,
                action: action,
                message: "The browser is running but the chromux daemon is not healthy.",
                pid: selected.pid,
                port: port,
                checkedAt: checkedAt
            )
        }

        let endpointPort = endpointPortOverride ?? port
        let tabStatus = await readCurrentTab(port: endpointPort)
        guard tabStatus.reachable else {
            return receipt(
                phase: .stale,
                profile: profile,
                action: action,
                message: "The chromux endpoint did not respond. Browser status is stale; retry without stopping the live daemon.",
                pid: selected.pid,
                port: endpointPort,
                checkedAt: checkedAt
            )
        }
        return BrowserRuntimeReceipt(
            phase: .ready,
            profile: profile,
            action: action,
            message: tabStatus.tab == nil ? "Browser ready. No open page target was reported." : "Browser ready and current tab status loaded.",
            pid: selected.pid,
            port: port,
            currentURL: tabStatus.tab?.url,
            currentTitle: tabStatus.tab?.title,
            checkedAt: checkedAt
        )
    }

    private static func readCurrentTab(port: Int) async -> (reachable: Bool, tab: ChromuxTab?) {
        guard let url = URL(string: "http://127.0.0.1:\(port)/json/list") else { return (false, nil) }
        var request = URLRequest(url: url)
        request.timeoutInterval = 2
        guard let (data, _) = try? await URLSession.shared.data(for: request),
              let tabs = try? ChromuxTabDecoder.decode(data)
        else { return (false, nil) }
        return (true, tabs.first(where: { $0.type == "page" }) ?? tabs.first)
    }

    private static func visibleFailure(_ process: ProcessReceipt, fallback: String) -> String {
        let error = String(decoding: process.stderr, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return error.isEmpty ? fallback : error
    }

    private static func receipt(
        phase: RuntimePhase,
        profile: String,
        action: String,
        message: String,
        pid: Int32? = nil,
        port: Int? = nil,
        checkedAt: String
    ) -> BrowserRuntimeReceipt {
        BrowserRuntimeReceipt(
            phase: phase,
            profile: profile,
            action: action,
            message: message,
            pid: pid,
            port: port,
            currentURL: nil,
            currentTitle: nil,
            checkedAt: checkedAt
        )
    }
}

@MainActor
final class BrowserRuntimeModel: ObservableObject {
    @Published private(set) var receipt = BrowserRuntimeReceipt(
        phase: .idle,
        profile: "default",
        action: "not-checked",
        message: "Browser status has not been checked yet.",
        pid: nil,
        port: nil,
        currentURL: nil,
        currentTitle: nil,
        checkedAt: "never"
    )

    let profile: String
    var onReceipt: ((BrowserRuntimeReceipt) -> Void)?
    var environmentStateProvider: ((String) -> String?)?
    private let receiptPath: String?
    private let endpointPortOverride: Int?

    init(arguments: [String] = CommandLine.arguments) {
        #if DEBUG
        profile = LaunchArguments.value("--verification-chromux-profile", in: arguments) ?? "default"
        #else
        profile = "default"
        #endif
        receiptPath = LaunchArguments.value("--verification-browser-receipt", in: arguments)
        #if DEBUG
        endpointPortOverride = LaunchArguments.value("--verification-chromux-port", in: arguments).flatMap(Int.init)
        #else
        endpointPortOverride = nil
        #endif
    }

    func refresh() {
        execute(shouldOpen: false)
    }

    func openOrFocus() {
        execute(shouldOpen: true)
    }

    private func execute(shouldOpen: Bool) {
        receipt = BrowserRuntimeReceipt(
            phase: .loading,
            profile: profile,
            action: shouldOpen ? "opening" : "checking",
            message: shouldOpen ? "Opening or reusing the default Chrome profile…" : "Checking chromux status…",
            pid: nil,
            port: nil,
            currentURL: nil,
            currentTitle: nil,
            checkedAt: ISO8601DateFormatter().string(from: Date())
        )
        Task {
            var result = await ChromuxExecutor.inspectAndOpen(
                profile: profile,
                shouldOpen: shouldOpen,
                pathState: environmentStateProvider?("PATH") ?? "absent",
                endpointPortOverride: endpointPortOverride
            )
            if shouldOpen, result.phase == .ready, let pid = result.pid {
                result.focusRequested = true
                result.focusActivationAccepted = NSRunningApplication(processIdentifier: pid)?
                    .activate(options: [.activateAllWindows]) ?? false
                try? await Task.sleep(for: .milliseconds(250))
                result.focusObserved = NSWorkspace.shared.frontmostApplication?.processIdentifier == pid
            }
            receipt = result
            onReceipt?(result)
            writeReceipt(result)
            Self.log(result)
        }
    }

    private static func log(_ receipt: BrowserRuntimeReceipt) {
        guard let data = try? JSONEncoder().encode(receipt) else { return }
        VerificationReceipt.writeLine(data)
    }

    private func writeReceipt(_ receipt: BrowserRuntimeReceipt) {
        guard let receiptPath, let data = try? JSONEncoder().encode(receipt) else { return }
        VerificationReceipt.write(data, to: receiptPath, failureKind: "chromux.receipt_write_failed")
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
    let sizeBytes: UInt64

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

        if selectedTab == nil, let activeTabID = activeTabIDs[workspaceID] {
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
    @Published private(set) var message = "Remote mini has not been checked yet."
    @Published private(set) var workspaces: [RemoteWorkspaceSummary] = []
    @Published private(set) var navigation: RemoteNavigationSnapshot?
    @Published private(set) var files: [RemoteFileNode] = []
    @Published private(set) var fileState = "idle"
    @Published private(set) var fileError: String?
    @Published private(set) var checkedAt = "never"
    @Published private(set) var targetLabel = "mini"
    private var activeTargetID: String?
    private var statusesByTarget: [String: CoreRemoteStatus] = [:]

    var statusMessage: String {
        message
    }

    func refreshMini() {
        refresh(targetID: "mini", label: "mini")
    }

    func clearNavigation() {
        activeTargetID = nil
        isRefreshing = false
        navigation = nil
        workspaces = []
        files = []
        fileState = "idle"
        fileError = nil
        phase = .idle
        message = "Remote mini has not been checked yet."
    }

    func ingest(_ statuses: [CoreRemoteStatus]) {
        statusesByTarget = Dictionary(uniqueKeysWithValues: statuses.map { ($0.targetID, $0) })
        guard let activeTargetID,
              let status = statusesByTarget[activeTargetID]
        else { return }
        apply(status)
    }

    func refresh(targetID: String, label: String) {
        activeTargetID = targetID
        files = []
        fileState = "idle"
        fileError = nil
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
                        isDirectory: entry.isDirectory,
                        sizeBytes: entry.sizeBytes
                    )
                }
                fileError = status.files.state == "unavailable"
                    ? status.files.message ?? "Remote files are unavailable."
                    : nil
            } else {
                fileState = "idle"
                files = []
                fileError = nil
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
    let state: String
    let summary: String
}

struct ConsequenceNotice: Equatable, Identifiable, Sendable {
    let title: String
    let consequence: String
    let affected: [DestructiveTarget]
    let requiresConfirmation: Bool

    var id: String { title }
}

enum ConsequencePolicy {
    /// The pane states that make a close destructive. `close_pane` in the core
    /// requires `confirmed: true` for exactly this set, so the two must agree.
    private static let attentionStates: Set<String> = [
        "working", "question", "approval", "error", "unseen_completion",
    ]

    static func notice(kind: DestructiveTargetKind, targets: [DestructiveTarget]) -> ConsequenceNotice {
        let risky = targets.filter { target in
            attentionStates.contains(target.state)
        }
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
        let affected = targets.filter { attentionStates.contains($0.state) }
        return ConsequenceNotice(
            title: title,
            consequence: consequence,
            affected: affected,
            requiresConfirmation: !affected.isEmpty
        )
    }
}
