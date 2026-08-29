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
    static let executable = "/Users/hoyeonlee/Library/pnpm/chromux"

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
        guard FileManager.default.isExecutableFile(atPath: executable) else {
            return receipt(
                phase: .unavailable,
                profile: profile,
                action: "unavailable",
                message: "chromux is not installed at the required path.",
                checkedAt: checkedAt
            )
        }
        guard pathState == "available" else {
            return receipt(
                phase: .unavailable,
                profile: profile,
                action: "unavailable",
                message: "chromux is hidden from this app process PATH. Restore /Users/hoyeonlee/Library/pnpm and retry.",
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
        FileHandle.standardError.write(data)
        FileHandle.standardError.write(Data("\n".utf8))
    }

    private func writeReceipt(_ receipt: BrowserRuntimeReceipt) {
        guard let receiptPath, let data = try? JSONEncoder().encode(receipt) else { return }
        do {
            try data.write(to: URL(fileURLWithPath: receiptPath), options: .atomic)
        } catch {
            let message = "{\"kind\":\"chromux.receipt_write_failed\",\"path\":\"\(receiptPath)\"}\n"
            FileHandle.standardError.write(Data(message.utf8))
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

private struct RemoteWorkspaceEnvelope: Decodable {
    struct Result: Decodable {
        let workspaces: [RemoteWorkspaceSummary]
    }
    let result: Result
}

@MainActor
final class RemoteRuntimeModel: ObservableObject {
    @Published private(set) var phase: RuntimePhase = .idle
    @Published private(set) var message = "Remote mini has not been checked yet."
    @Published private(set) var workspaces: [RemoteWorkspaceSummary] = []
    @Published private(set) var checkedAt = "never"
    var environmentStateProvider: ((String) -> String?)?

    func refreshMini() {
        guard environmentStateProvider?("SSH_AUTH_SOCK") == "available" else {
            phase = .unavailable
            message = "SSH_AUTH_SOCK is unavailable. Remote features are disabled; launch from a shell with the agent socket exported."
            checkedAt = ISO8601DateFormatter().string(from: Date())
            return
        }
        phase = .loading
        message = "Connecting to mini and loading remote workspaces…"
        Task {
            let result = await Task.detached {
                SafeProcess.run(
                    executable: "/usr/bin/ssh",
                    arguments: ["mini", "/Users/grab/.local/bin/herdr", "workspace", "list"]
                )
            }.value
            checkedAt = ISO8601DateFormatter().string(from: Date())
            guard result.status == 0,
                  let envelope = try? JSONDecoder().decode(RemoteWorkspaceEnvelope.self, from: result.stdout)
            else {
                phase = .failed
                let detail = String(decoding: result.stderr, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                message = detail.isEmpty ? "mini workspace list failed. Retry after checking SSH and the remote Herdr service." : detail
                log(kind: "remote.refresh_failed")
                return
            }
            workspaces = envelope.result.workspaces.sorted { lhs, rhs in
                let lhsOwned = lhs.label.hasPrefix("herdr-ide-verify-")
                let rhsOwned = rhs.label.hasPrefix("herdr-ide-verify-")
                if lhsOwned != rhsOwned { return lhsOwned }
                return lhs.label.localizedStandardCompare(rhs.label) == .orderedAscending
            }
            phase = .ready
            message = workspaces.isEmpty
                ? "mini is connected, but no remote workspace is open. Create one on mini and retry."
                : "mini connected. Remote file viewing and terminal attach are available; inline editing stays disabled."
            log(kind: "remote.ready")
        }
    }

    private func log(kind: String) {
        let record: [String: Any] = [
            "kind": kind,
            "target": "mini",
            "state": phase.rawValue,
            "workspace_count": workspaces.count,
            "checked_at": checkedAt,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: record) else { return }
        FileHandle.standardError.write(data)
        FileHandle.standardError.write(Data("\n".utf8))
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
    static func notice(kind: DestructiveTargetKind, targets: [DestructiveTarget]) -> ConsequenceNotice {
        let risky = targets.filter { target in
            ["working", "question", "approval", "error", "unseen_completion"].contains(target.state)
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
                title: "Remove this worktree checkout?",
                consequence: "The checkout directory is removed from disk. The repository and branch remain, but uncommitted files in that checkout can be lost.",
                affected: targets,
                requiresConfirmation: true
            )
        }
    }

    private static func aggregate(_ title: String, _ consequence: String, _ targets: [DestructiveTarget]) -> ConsequenceNotice {
        let affected = targets.filter { ["working", "question", "approval", "error", "unseen_completion"].contains($0.state) }
        return ConsequenceNotice(
            title: title,
            consequence: consequence,
            affected: affected,
            requiresConfirmation: !affected.isEmpty
        )
    }
}
