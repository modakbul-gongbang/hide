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

private struct RemoteWorktreeWire: Decodable, Sendable {
    let checkoutPath: String?

    enum CodingKeys: String, CodingKey {
        case checkoutPath = "checkout_path"
    }
}

private struct RemoteWorkspaceWire: Decodable, Sendable {
    let workspaceID: String
    let label: String
    let activeTabID: String?
    let paneCount: Int
    let tabCount: Int
    let worktree: RemoteWorktreeWire?

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case label
        case activeTabID = "active_tab_id"
        case paneCount = "pane_count"
        case tabCount = "tab_count"
        case worktree
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        workspaceID = try container.decode(String.self, forKey: .workspaceID)
        label = try container.decode(String.self, forKey: .label)
        activeTabID = try container.decodeIfPresent(String.self, forKey: .activeTabID)
        paneCount = try container.decodeIfPresent(Int.self, forKey: .paneCount) ?? 0
        tabCount = try container.decodeIfPresent(Int.self, forKey: .tabCount) ?? 0
        worktree = try container.decodeIfPresent(RemoteWorktreeWire.self, forKey: .worktree)
    }
}

private struct RemoteTabWire: Decodable, Sendable {
    let tabID: String
    let workspaceID: String
    let label: String
    let paneCount: Int

    enum CodingKeys: String, CodingKey {
        case tabID = "tab_id"
        case workspaceID = "workspace_id"
        case label
        case paneCount = "pane_count"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        tabID = try container.decode(String.self, forKey: .tabID)
        workspaceID = try container.decode(String.self, forKey: .workspaceID)
        label = try container.decodeIfPresent(String.self, forKey: .label) ?? "Tab"
        paneCount = try container.decodeIfPresent(Int.self, forKey: .paneCount) ?? 0
    }
}

private struct RemotePaneWire: Decodable, Sendable {
    let paneID: String
    let workspaceID: String
    let tabID: String
    let cwd: String
    let terminalTitle: String?
    let terminalTitleStripped: String?

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case workspaceID = "workspace_id"
        case tabID = "tab_id"
        case cwd
        case terminalTitle = "terminal_title"
        case terminalTitleStripped = "terminal_title_stripped"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        paneID = try container.decode(String.self, forKey: .paneID)
        workspaceID = try container.decode(String.self, forKey: .workspaceID)
        tabID = try container.decode(String.self, forKey: .tabID)
        cwd = try container.decodeIfPresent(String.self, forKey: .cwd) ?? ""
        terminalTitle = try container.decodeIfPresent(String.self, forKey: .terminalTitle)
        terminalTitleStripped = try container.decodeIfPresent(String.self, forKey: .terminalTitleStripped)
    }
}

private struct RemoteLayoutRectWire: Decodable, Sendable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

private struct RemoteLayoutPaneWire: Decodable, Sendable {
    let paneID: String
    let rect: RemoteLayoutRectWire

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case rect
    }
}

private struct RemoteLayoutWire: Decodable, Sendable {
    let workspaceID: String
    let tabID: String
    let focusedPaneID: String
    let zoomed: Bool
    let area: RemoteLayoutRectWire
    let panes: [RemoteLayoutPaneWire]

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case tabID = "tab_id"
        case focusedPaneID = "focused_pane_id"
        case zoomed
        case area
        case panes
    }
}

private struct RemoteAgentWire: Decodable, Sendable {
    let agent: String
    let agentStatus: String
    let paneID: String
    let workspaceID: String
    let tokens: [String: String]

    enum CodingKeys: String, CodingKey {
        case agent
        case agentStatus = "agent_status"
        case paneID = "pane_id"
        case workspaceID = "workspace_id"
        case tokens
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        agent = try container.decodeIfPresent(String.self, forKey: .agent) ?? "agent"
        agentStatus = try container.decodeIfPresent(String.self, forKey: .agentStatus) ?? "unknown"
        paneID = try container.decode(String.self, forKey: .paneID)
        workspaceID = try container.decode(String.self, forKey: .workspaceID)
        tokens = try container.decodeIfPresent([String: String].self, forKey: .tokens) ?? [:]
    }
}

fileprivate struct RemoteSnapshotWire: Decodable, Sendable {
    let focusedWorkspaceID: String?
    let focusedTabID: String?
    let focusedPaneID: String?
    let workspaces: [RemoteWorkspaceWire]
    let tabs: [RemoteTabWire]
    let panes: [RemotePaneWire]
    let layouts: [RemoteLayoutWire]
    let agents: [RemoteAgentWire]

    enum CodingKeys: String, CodingKey {
        case focusedWorkspaceID = "focused_workspace_id"
        case focusedTabID = "focused_tab_id"
        case focusedPaneID = "focused_pane_id"
        case workspaces
        case tabs
        case panes
        case layouts
        case agents
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        focusedWorkspaceID = try container.decodeIfPresent(String.self, forKey: .focusedWorkspaceID)
        focusedTabID = try container.decodeIfPresent(String.self, forKey: .focusedTabID)
        focusedPaneID = try container.decodeIfPresent(String.self, forKey: .focusedPaneID)
        workspaces = try container.decodeIfPresent([RemoteWorkspaceWire].self, forKey: .workspaces) ?? []
        tabs = try container.decodeIfPresent([RemoteTabWire].self, forKey: .tabs) ?? []
        panes = try container.decodeIfPresent([RemotePaneWire].self, forKey: .panes) ?? []
        layouts = try container.decodeIfPresent([RemoteLayoutWire].self, forKey: .layouts) ?? []
        agents = try container.decodeIfPresent([RemoteAgentWire].self, forKey: .agents) ?? []
    }
}

private struct RemoteSnapshotEnvelope: Decodable, Sendable {
    struct Result: Decodable, Sendable {
        let snapshot: RemoteSnapshotWire
    }

    let result: Result
}

enum RemoteSnapshotProjection {
    static func decode(
        _ data: Data,
        deviceID: String,
        targetLabel: String
    ) throws -> RemoteNavigationSnapshot {
        let envelope = try JSONDecoder().decode(RemoteSnapshotEnvelope.self, from: data)
        return RemoteNavigationSnapshot(
            deviceID: deviceID,
            targetLabel: targetLabel,
            wire: envelope.result.snapshot
        )
    }
}

struct RemoteFileNode: Identifiable, Hashable, Sendable {
    let path: String
    let isDirectory: Bool

    var id: String { path }
    var name: String { URL(fileURLWithPath: path).lastPathComponent }
}

struct RemotePaneLayoutFrame: Equatable, Sendable {
    let paneID: String
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

struct RemotePaneLayoutSnapshot: Equatable, Sendable {
    let workspaceID: String
    let tabID: String
    let focusedPaneID: String
    let zoomed: Bool
    let frames: [RemotePaneLayoutFrame]

    fileprivate init(wire: RemoteLayoutWire) {
        workspaceID = wire.workspaceID
        tabID = wire.tabID
        focusedPaneID = wire.focusedPaneID
        zoomed = wire.zoomed
        guard wire.area.width > 0, wire.area.height > 0 else {
            frames = []
            return
        }
        frames = wire.panes.map { pane in
            RemotePaneLayoutFrame(
                paneID: pane.paneID,
                x: (pane.rect.x - wire.area.x) / wire.area.width,
                y: (pane.rect.y - wire.area.y) / wire.area.height,
                width: pane.rect.width / wire.area.width,
                height: pane.rect.height / wire.area.height
            )
        }
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

    fileprivate init(deviceID: String, targetLabel: String, wire: RemoteSnapshotWire) {
        self.deviceID = deviceID
        self.targetLabel = targetLabel

        let labelsByWorkspace = Dictionary(uniqueKeysWithValues: wire.workspaces.map {
            ($0.workspaceID, $0.label)
        })
        var projectedWorkspaces: [CoreWorkspaceSnapshot] = []
        for workspace in wire.workspaces {
            let workspaceID = Self.workspaceID(deviceID: deviceID, remoteID: workspace.workspaceID)
            let workspacePanes = wire.panes.filter { $0.workspaceID == workspace.workspaceID }
            let workspaceTabs = wire.tabs.filter { $0.workspaceID == workspace.workspaceID }
            let tabs = workspaceTabs.map { tab in
                let panes = wire.panes
                    .filter { $0.tabID == tab.tabID && $0.workspaceID == workspace.workspaceID }
                    .map { pane in
                        CorePaneSnapshot(
                            id: pane.paneID,
                            label: pane.terminalTitleStripped ?? pane.terminalTitle ?? pane.paneID,
                            cwd: pane.cwd,
                            state: "attached",
                            summary: nil,
                            activityAt: nil
                        )
                    }
                return CoreTabSnapshot(
                    id: tab.tabID,
                    workspaceID: workspaceID,
                    checkoutID: Self.checkoutID(deviceID: deviceID, remoteID: workspace.workspaceID),
                    label: tab.label,
                    empty: panes.isEmpty,
                    panes: panes
                )
            }
            let path = workspacePanes.first(where: { !$0.cwd.isEmpty })?.cwd
                ?? workspace.worktree?.checkoutPath
                ?? ""
            let checkoutID = Self.checkoutID(deviceID: deviceID, remoteID: workspace.workspaceID)
            let checkout = CoreCheckoutSnapshot(
                id: checkoutID,
                workspaceID: workspaceID,
                label: workspace.label,
                path: path,
                branch: nil,
                isWorktree: workspace.worktree != nil,
                exists: true,
                temporary: false,
                tabs: tabs
            )
            projectedWorkspaces.append(CoreWorkspaceSnapshot(
                id: workspaceID,
                label: workspace.label,
                path: path,
                remoteTargetID: deviceID,
                expanded: true,
                deviceID: deviceID,
                repoName: workspace.label,
                isGit: workspace.worktree != nil,
                defaultBranch: nil,
                registered: true,
                temporary: false,
                checkouts: [checkout]
            ))
        }
        workspaces = projectedWorkspaces.sorted { lhs, rhs in
            lhs.label.localizedStandardCompare(rhs.label) == .orderedAscending
        }
        activeTabIDs = Dictionary(uniqueKeysWithValues: wire.workspaces.compactMap { workspace in
            guard let activeTabID = workspace.activeTabID else { return nil }
            return (
                Self.workspaceID(deviceID: deviceID, remoteID: workspace.workspaceID),
                activeTabID
            )
        })
        agents = wire.agents.map { agent in
            let tokens = agent.tokens
            return SidebarAgent(
                id: "remote:\(deviceID):\(agent.paneID)",
                paneID: agent.paneID,
                workspaceLabel: labelsByWorkspace[agent.workspaceID] ?? agent.workspaceID,
                agentKind: agent.agent,
                state: agent.agentStatus,
                symbol: tokens["agent_\(agent.agent)"] ?? "?",
                summary: tokens["summary"] ?? tokens["terminal_title"] ?? "Remote agent",
                elapsed: tokens["elapsed"] ?? "",
                sortRank: tokens["sort_rank"] ?? "99",
                activity: tokens["summary"] ?? "",
                ambient: nil
            )
        }
        focusedWorkspaceID = wire.focusedWorkspaceID.map {
            Self.workspaceID(deviceID: deviceID, remoteID: $0)
        }
        focusedCheckoutID = wire.focusedWorkspaceID.map {
            Self.checkoutID(deviceID: deviceID, remoteID: $0)
        }
        focusedTabID = wire.focusedTabID
        focusedPaneID = wire.focusedPaneID
        paneLayouts = wire.layouts.map(RemotePaneLayoutSnapshot.init(wire:))
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
        let selectedTab = paneID.flatMap { paneID in
            selectedCheckout?.tabs.first(where: { tab in
                tab.panes.contains(where: { $0.id == paneID })
            })
        }
            ?? tabID.flatMap { tabID in
                selectedCheckout?.tabs.first(where: { $0.id == tabID })
            }
            ??
            selectedCheckout?.tabs
            .first(where: { $0.id == activeTabIDs[workspaceID] })
            ?? selectedCheckout?.tabs.first
        let selectedPaneID = paneID.flatMap { paneID in
            selectedTab?.panes.contains(where: { $0.id == paneID }) == true ? paneID : nil
        } ?? selectedTab?.panes.first?.id
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

    func remoteWorkspaceID(for projectedID: String) -> String? {
        let prefix = "remote:\(deviceID):workspace:"
        guard projectedID.hasPrefix(prefix) else { return nil }
        let remoteID = String(projectedID.dropFirst(prefix.count))
        return remoteID.isEmpty ? nil : remoteID
    }
}

enum RemoteTerminalCommand: Sendable {
    case split(paneID: String, direction: PaneSplitDirection)
    case toggleZoom(paneID: String)
    case close(paneID: String)
    case createTab(workspaceID: String, cwd: String, label: String)
    case focusWorkspace(workspaceID: String)
    case focusTab(tabID: String)
    case focusAgent(paneID: String)

    var shellCommand: String {
        switch self {
        case .split(let paneID, let direction):
            "herdr pane split \(RemoteShellCommand.quote(paneID)) --direction \(direction.rawValue)"
        case .toggleZoom(let paneID):
            "herdr pane zoom \(RemoteShellCommand.quote(paneID)) --toggle"
        case .close(let paneID):
            "herdr pane close \(RemoteShellCommand.quote(paneID))"
        case .createTab(let workspaceID, let cwd, let label):
            "herdr tab create --workspace \(RemoteShellCommand.quote(workspaceID)) --cwd \(RemoteShellCommand.quote(cwd)) --label \(RemoteShellCommand.quote(label)) --focus"
        case .focusWorkspace(let workspaceID):
            "herdr workspace focus \(RemoteShellCommand.quote(workspaceID))"
        case .focusTab(let tabID):
            "herdr tab focus \(RemoteShellCommand.quote(tabID))"
        case .focusAgent(let paneID):
            "herdr agent focus \(RemoteShellCommand.quote(paneID))"
        }
    }

    var successMessage: String {
        switch self {
        case .split(_, let direction): "Remote pane split \(direction.rawValue)."
        case .toggleZoom: "Remote pane zoom toggled."
        case .close: "Remote pane closed."
        case .createTab: "Remote tab created."
        case .focusWorkspace: "Remote workspace focused."
        case .focusTab: "Remote tab focused."
        case .focusAgent: "Remote agent pane focused."
        }
    }

    var logKind: String {
        switch self {
        case .split: "split"
        case .toggleZoom: "zoom"
        case .close: "close"
        case .createTab: "tab_create"
        case .focusWorkspace: "workspace_focus"
        case .focusTab: "tab_focus"
        case .focusAgent: "agent_focus"
        }
    }

    var requiresSnapshotRefresh: Bool {
        switch self {
        case .split, .toggleZoom, .close, .createTab: true
        case .focusWorkspace, .focusTab, .focusAgent: false
        }
    }
}

private struct RemoteProbeResult: Sendable {
    let version: ProcessReceipt
    let snapshot: ProcessReceipt?
}

@MainActor
final class RemoteRuntimeModel: ObservableObject {
    @Published private(set) var phase: RuntimePhase = .idle
    @Published private(set) var isRefreshing = false
    @Published private(set) var message = "Remote mini has not been checked yet."
    @Published private(set) var workspaces: [RemoteWorkspaceSummary] = []
    @Published private(set) var navigation: RemoteNavigationSnapshot?
    @Published private(set) var files: [RemoteFileNode] = []
    @Published private(set) var fileError: String?
    @Published private(set) var attachError: String?
    @Published private(set) var actionError: String?
    @Published private(set) var checkedAt = "never"
    @Published private(set) var targetLabel = "mini"
    private(set) var sshAlias = "mini"
    private var refreshGeneration = UUID()
    private var loadedFilePath: String?
    private var pendingTerminalPaths: Set<String> = []
    var environmentStateProvider: ((String) -> String?)?
    var onActionFailure: ((String) -> Void)?

    var statusMessage: String {
        actionError ?? attachError ?? message
    }

    func refreshMini() {
        refresh(targetID: "mini", label: "mini", sshAlias: "mini")
    }

    func clearNavigation() {
        refreshGeneration = UUID()
        isRefreshing = false
        navigation = nil
        workspaces = []
        files = []
        fileError = nil
        attachError = nil
        actionError = nil
        loadedFilePath = nil
        phase = .idle
        message = "Remote mini has not been checked yet."
    }

    func refresh(targetID: String, label: String, sshAlias: String) {
        let requestID = UUID()
        refreshGeneration = requestID
        fileError = nil
        attachError = nil
        actionError = nil
        isRefreshing = true
        targetLabel = label
        self.sshAlias = sshAlias
        guard environmentStateProvider?("SSH_AUTH_SOCK") == "available" else {
            finishRefreshFailure(
                "SSH_AUTH_SOCK is unavailable. Remote features are disabled; launch from a shell with the agent socket exported.",
                phaseIfEmpty: .unavailable,
                logKind: "remote.refresh_unavailable"
            )
            checkedAt = ISO8601DateFormatter().string(from: Date())
            return
        }
        if navigation == nil {
            phase = .loading
            message = "Connecting to \(label) and loading remote workspaces…"
        } else {
            // Keep the last authoritative projection interactive while its
            // replacement is fetched. Clearing it dismantles terminal hosts,
            // kills their SSH children, and turns a refresh into a reconnect.
            phase = .ready
            message = "Refreshing \(label) while the current remote session stays attached…"
        }
        Task {
            let probe = await Task.detached { () -> RemoteProbeResult in
                let version = SafeProcess.run(
                    executable: "/usr/bin/ssh",
                    arguments: [sshAlias, RemoteShellCommand.loginShell("herdr --version")]
                )
                guard version.status == 0 else {
                    return RemoteProbeResult(version: version, snapshot: nil)
                }
                return RemoteProbeResult(
                    version: version,
                    snapshot: SafeProcess.run(
                        executable: "/usr/bin/ssh",
                        arguments: [sshAlias, RemoteShellCommand.loginShell("herdr api snapshot")]
                    )
                )
            }.value
            guard refreshGeneration == requestID else { return }
            isRefreshing = false
            checkedAt = ISO8601DateFormatter().string(from: Date())
            guard probe.version.status == 0 else {
                finishRefreshFailure(
                    remoteFailure(probe.version, label: label),
                    phaseIfEmpty: .failed,
                    logKind: "remote.refresh_failed"
                )
                return
            }
            guard let remoteVersion = version(from: probe.version.stdout) else {
                finishRefreshFailure(
                    "The Herdr version response from \(label) was not readable. Retry after checking the remote installation.",
                    phaseIfEmpty: .failed,
                    logKind: "remote.refresh_failed"
                )
                return
            }
            if compare(remoteVersion, with: HideRuntimeEnvironment.bundledVersion) == .orderedAscending {
                finishRefreshFailure(
                    "Herdr \(remoteVersion) on \(label) is below hide's supported \(HideRuntimeEnvironment.bundledVersion). Upgrade the remote Herdr installation, then retry. Hide does not install or upgrade it.",
                    phaseIfEmpty: .stale,
                    logKind: "remote.stale"
                )
                return
            }
            guard let snapshot = probe.snapshot else {
                finishRefreshFailure(
                    "The remote snapshot from \(label) was not available. Retry after checking SSH and the remote Herdr service.",
                    phaseIfEmpty: .failed,
                    logKind: "remote.refresh_failed"
                )
                return
            }
            guard snapshot.status == 0,
                  let envelope = try? JSONDecoder().decode(RemoteSnapshotEnvelope.self, from: snapshot.stdout)
            else {
                let failure = snapshot.status == 0
                    ? "The remote snapshot from \(label) was malformed. Retry after checking the remote Herdr service."
                    : remoteFailure(snapshot, label: label)
                finishRefreshFailure(
                    failure,
                    phaseIfEmpty: .failed,
                    logKind: "remote.refresh_failed"
                )
                return
            }
            let wire = envelope.result.snapshot
            let projection = RemoteNavigationSnapshot(deviceID: targetID, targetLabel: label, wire: wire)
            navigation = projection
            workspaces = projection.workspaces.map { workspace in
                RemoteWorkspaceSummary(
                    workspaceID: workspace.id,
                    label: workspace.label,
                    paneCount: workspace.checkouts.flatMap { $0.tabs }.flatMap { $0.panes }.count,
                    activeTabID: workspace.checkouts.first?.tabs.first?.id
                )
            }
            phase = .ready
            actionError = nil
            message = workspaces.isEmpty
                ? "\(label) is connected, but no remote workspace is open. Create one on \(label) and retry."
                : "\(label) connected. Remote workspaces, file trees, and terminal panes are attached; inline editing stays disabled."
            log(kind: "remote.ready")
        }
    }

    private func finishRefreshFailure(
        _ failure: String,
        phaseIfEmpty: RuntimePhase,
        logKind: String
    ) {
        isRefreshing = false
        if navigation == nil {
            phase = phaseIfEmpty
        } else {
            phase = .ready
        }
        actionError = failure
        message = failure
        log(kind: logKind)
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

    func focusWorkspace(projectedID: String) {
        guard let workspaceID = navigation?.remoteWorkspaceID(for: projectedID) else {
            reportActionFailure(
                "The selected workspace does not belong to the connected remote device. No focus command was sent.",
                kind: "workspace_focus_mismatch"
            )
            return
        }
        perform(.focusWorkspace(workspaceID: workspaceID))
    }

    func startTerminal(checkout: CoreCheckoutSnapshot) {
        guard pendingTerminalPaths.insert(checkout.path).inserted else {
            reportActionFailure(
                "A remote terminal is already starting for this checkout.",
                kind: "tab_create_duplicate"
            )
            return
        }
        guard let command = createTabCommand(checkout: checkout, label: "hide \(checkout.label)") else {
            pendingTerminalPaths.remove(checkout.path)
            return
        }
        perform(command, pendingTerminalPath: checkout.path)
    }

    func createTab(checkout: CoreCheckoutSnapshot) {
        guard let command = createTabCommand(checkout: checkout, label: "New tab") else { return }
        perform(command)
    }

    func perform(_ command: RemoteTerminalCommand) {
        perform(command, pendingTerminalPath: nil)
    }

    func loadFiles(path: String) {
        guard phase == .ready, !path.isEmpty, path != loadedFilePath else { return }
        loadedFilePath = path
        fileError = nil
        let requestID = refreshGeneration
        let alias = sshAlias
        Task {
            let result = await Task.detached {
                let quotedPath = RemoteShellCommand.quote(path)
                let command = "find \(quotedPath) -mindepth 1 -maxdepth 1 -type d -exec printf 'D\\t%s\\n' {} \\; ; find \(quotedPath) -mindepth 1 -maxdepth 1 -type f -exec printf 'F\\t%s\\n' {} \\;"
                return SafeProcess.run(
                    executable: "/usr/bin/ssh",
                    arguments: [alias, RemoteShellCommand.loginShell(command)]
                )
            }.value
            guard refreshGeneration == requestID else { return }
            guard result.status == 0 else {
                fileError = remoteFailure(result, label: targetLabel)
                files = []
                log(kind: "remote.files_failed")
                return
            }
            files = String(decoding: result.stdout, as: UTF8.self)
                .split(whereSeparator: \.isNewline)
                .compactMap { line in
                    let pieces = line.split(separator: "\t", maxSplits: 1).map(String.init)
                    guard pieces.count == 2 else { return nil }
                    return RemoteFileNode(path: pieces[1], isDirectory: pieces[0] == "D")
                }
                .sorted { lhs, rhs in
                    if lhs.isDirectory != rhs.isDirectory { return lhs.isDirectory }
                    return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
                }
            log(kind: "remote.files_ready")
        }
    }

    func recordAttachFailure(paneID: String, exitCode: Int32?) {
        let suffix = exitCode.map { " (exit \($0))" } ?? ""
        attachError = "Remote terminal initialization failed for \(paneID)\(suffix). Check the SSH PTY/TERM and remote Herdr session, then retry."
        log(kind: "remote.terminal_attach_failed")
    }

    private func createTabCommand(
        checkout: CoreCheckoutSnapshot,
        label: String
    ) -> RemoteTerminalCommand? {
        guard phase == .ready else {
            reportActionFailure(
                "Remote Herdr is not ready. Retry the connection before creating a tab.",
                kind: "tab_create_not_ready"
            )
            return nil
        }
        guard !checkout.path.isEmpty else {
            reportActionFailure(
                "The remote checkout path is unavailable, so Hide cannot create a tab there.",
                kind: "tab_create_path_missing"
            )
            return nil
        }
        guard let workspaceID = navigation?.remoteWorkspaceID(for: checkout.workspaceID) else {
            reportActionFailure(
                "The selected checkout does not belong to the connected remote workspace. No tab was created.",
                kind: "tab_create_workspace_mismatch"
            )
            return nil
        }
        return .createTab(workspaceID: workspaceID, cwd: checkout.path, label: label)
    }

    private func perform(
        _ command: RemoteTerminalCommand,
        pendingTerminalPath: String?
    ) {
        guard phase == .ready else {
            if let pendingTerminalPath { pendingTerminalPaths.remove(pendingTerminalPath) }
            reportActionFailure(
                "Remote Herdr is not ready. No command was sent to either device.",
                kind: "command_not_ready"
            )
            return
        }
        guard let targetID = navigation?.deviceID else {
            if let pendingTerminalPath { pendingTerminalPaths.remove(pendingTerminalPath) }
            reportActionFailure(
                "The selected remote device has no navigation snapshot. No command was sent to either device.",
                kind: "command_navigation_missing"
            )
            return
        }
        actionError = nil
        let requestID = refreshGeneration
        let alias = sshAlias
        let label = targetLabel
        let shellCommand = command.shellCommand
        Task {
            let result = await Task.detached {
                SafeProcess.run(
                    executable: "/usr/bin/ssh",
                    arguments: [alias, RemoteShellCommand.loginShell(shellCommand)]
                )
            }.value
            if let pendingTerminalPath { pendingTerminalPaths.remove(pendingTerminalPath) }
            guard refreshGeneration == requestID else { return }
            guard result.status == 0 else {
                reportActionFailure(
                    remoteFailure(result, label: label),
                    kind: "command_\(command.logKind)_failed"
                )
                return
            }
            actionError = nil
            log(kind: "remote.command_\(command.logKind)_ready")
            if command.requiresSnapshotRefresh {
                message = "\(command.successMessage) Refreshing \(label)."
                refresh(targetID: targetID, label: label, sshAlias: alias)
            } else {
                message = command.successMessage
            }
        }
    }

    private func reportActionFailure(_ failure: String, kind: String) {
        actionError = failure
        message = failure
        onActionFailure?(failure)
        log(kind: "remote.\(kind)")
    }

    private func version(from data: Data) -> String? {
        let line = String(decoding: data, as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return line.split(separator: " ").last.map(String.init)
    }

    private func compare(_ left: String, with right: String) -> ComparisonResult {
        let leftParts = left.split(separator: ".").compactMap { Int($0) }
        let rightParts = right.split(separator: ".").compactMap { Int($0) }
        for index in 0 ..< max(leftParts.count, rightParts.count) {
            let leftPart = index < leftParts.count ? leftParts[index] : 0
            let rightPart = index < rightParts.count ? rightParts[index] : 0
            if leftPart != rightPart {
                return leftPart < rightPart ? .orderedAscending : .orderedDescending
            }
        }
        return .orderedSame
    }

    private func remoteFailure(_ result: ProcessReceipt, label: String) -> String {
        let detail = String(decoding: result.stderr, as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let normalized = detail.lowercased()
        if normalized.contains("command not found") || normalized.contains("no such file") {
            return "Herdr is not installed on \(label). Run `curl -fsSL https://herdr.dev/install.sh | sh` or `brew install herdr` there. Hide does not install it."
        }
        if normalized.contains("permission denied") || normalized.contains("publickey") {
            return "SSH authentication failed for \(label). Check the existing ssh-agent and SSH config; Hide does not collect credentials."
        }
        if normalized.contains("host key verification failed") {
            return "SSH host-key verification failed for \(label). Verify the host in known_hosts, then retry."
        }
        if normalized.contains("could not resolve") || normalized.contains("connection refused") || normalized.contains("no route") {
            return "SSH could not reach \(label). Check the alias and host availability, then retry."
        }
        return detail.isEmpty ? "\(label) remote Herdr operation failed. Retry after checking SSH and the remote Herdr service." : detail
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

enum RemoteShellCommand {
    static func quote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    /// SSH joins its trailing arguments into one remote command. Keep the
    /// complete login-shell invocation in one argument so `-c` receives the
    /// entire Herdr command instead of treating its words as `$0`, `$1`, ... .
    static func loginShell(_ command: String) -> String {
        "zsh -ilc \(quote(command))"
    }

    /// Keep remote Herdr diagnostics out of the terminal surface.
    ///
    /// A pane attach failure can include a Rust panic when the remote shell
    /// has no usable PTY or TERM. The failure remains observable through the
    /// exit code and the normalized sentence below, without leaking an
    /// implementation traceback into the product UI.
    static func attach(paneID: String) -> String {
        let command = "herdr pane attach \(quote(paneID)) 2>/dev/null; status=$?; if [ \"$status\" -ne 0 ]; then printf '\\r\\nHide: remote terminal initialization failed on mini (exit %s). Check SSH PTY/TERM and the remote Herdr session, then retry.\\r\\n' \"$status\"; exit \"$status\"; fi"
        return command
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
