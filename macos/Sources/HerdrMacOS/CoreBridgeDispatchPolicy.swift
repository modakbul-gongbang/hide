import Foundation

struct HerdrProtocolMismatchDetails: Equatable, Identifiable {
    enum Recovery: Equatable {
        case restartBundledHerdr
        case updateHide
        case reviewDiagnostics
    }

    let expectedProtocol: UInt64?
    let receivedProtocol: UInt64?
    let expectedVersion: String?
    let receivedVersion: String?
    let hideVersion: String?

    var id: String {
        [expectedProtocol.map(String.init), receivedProtocol.map(String.init), expectedVersion, receivedVersion]
            .map { $0 ?? "unknown" }
            .joined(separator: ":")
    }

    var recovery: Recovery {
        guard let expectedProtocol, let receivedProtocol else { return .reviewDiagnostics }
        if receivedProtocol < expectedProtocol { return .restartBundledHerdr }
        if receivedProtocol > expectedProtocol { return .updateHide }
        return .reviewDiagnostics
    }

    var title: String {
        switch recovery {
        case .restartBundledHerdr:
            "Restart Herdr when your work is safe"
        case .updateHide:
            "Hide needs an update"
        case .reviewDiagnostics:
            "Hide and Herdr aren’t compatible"
        }
    }

    var message: String {
        switch recovery {
        case .restartBundledHerdr:
            if let expectedProtocol, let receivedProtocol {
                "The running Herdr uses protocol \(receivedProtocol), but Hide requires protocol \(expectedProtocol). When your current work is safe, stop the Herdr session and reopen Hide. Hide will start its compatible bundled Herdr. No workspace or agent was created."
            } else {
                "When your current work is safe, stop the Herdr session and reopen Hide. Hide will start its compatible bundled Herdr. No workspace or agent was created."
            }
        case .updateHide:
            if let expectedProtocol, let receivedProtocol {
                "The running Herdr uses protocol \(receivedProtocol), but this Hide supports protocol \(expectedProtocol). Update Hide to a compatible release, then try again. No workspace or agent was created."
            } else {
                "The running Herdr is newer than this version of Hide. Update Hide to a compatible release, then try again. No workspace or agent was created."
            }
        case .reviewDiagnostics:
            "Hide and the running Herdr use incompatible protocols. Copy the diagnostics before reporting the issue. No workspace or agent was created."
        }
    }

    var primaryActionLabel: String? {
        switch recovery {
        case .restartBundledHerdr:
            "Open Restart Guide"
        case .updateHide:
            "Open Hide Releases"
        case .reviewDiagnostics:
            nil
        }
    }

    var diagnostics: String {
        var lines = ["Error code: protocol_mismatch"]
        if let hideVersion, !hideVersion.isEmpty {
            lines.append("Hide version: \(hideVersion)")
        }
        if let expectedVersion, !expectedVersion.isEmpty {
            lines.append("Required Herdr version: \(expectedVersion)")
        }
        if let expectedProtocol {
            lines.append("Required protocol: \(expectedProtocol)")
        }
        if let receivedVersion, !receivedVersion.isEmpty {
            lines.append("Running Herdr version: \(receivedVersion)")
        }
        if let receivedProtocol {
            lines.append("Running protocol: \(receivedProtocol)")
        }
        return lines.joined(separator: "\n")
    }
}

enum LocalHerdrMutationReadiness: Equatable {
    case connected
    case initializing(String)
    case protocolMismatch(HerdrProtocolMismatchDetails)
    case unavailable(String)

    var message: String {
        switch self {
        case .connected:
            "Connected to Herdr"
        case .initializing(let message), .unavailable(let message):
            message
        case .protocolMismatch(let details):
            details.message
        }
    }
}

enum LocalHerdrMutationPolicy {
    static func evaluate(
        runtimeSelection: HerdrRuntimeSelection?,
        status: CoreHerdrStatus?,
        startupDiagnostic: String?,
        hideVersion: String?
    ) -> LocalHerdrMutationReadiness {
        guard let runtimeSelection else {
            return .initializing(
                startupDiagnostic
                    ?? "The bundled Herdr runtime is still starting. Wait for Herdr status, then try again."
            )
        }
        guard let status else {
            return .initializing("Hide is waiting for the first Herdr status. Try again in a moment.")
        }
        if status.state == "connected" {
            return .connected
        }
        if status.state == "protocol_mismatch" {
            return .protocolMismatch(HerdrProtocolMismatchDetails(
                expectedProtocol: status.expectedProtocol,
                receivedProtocol: status.receivedProtocol,
                expectedVersion: runtimeSelection.version,
                receivedVersion: status.receivedVersion,
                hideVersion: hideVersion
            ))
        }
        return .unavailable(
            status.message
                ?? "Hide is not connected to Herdr yet. Wait for the connection, then try again."
        )
    }
}

struct CoreDispatchRoutingPolicy {
    private static let remoteTargetScopedEventKinds: Set<String> = [
        "key",
        "terminal_click",
        "terminal_resize",
        "terminal_scroll",
        "terminal_viewport",
    ]

    static func blocks(
        kind: String,
        payload: [String: Any] = [:],
        remoteDeviceID: String?
    ) -> Bool {
        guard let remoteDeviceID,
              LocalHerdrMutationDispatchPolicy.isMutation(kind: kind)
        else {
            return false
        }
        guard remoteTargetScopedEventKinds.contains(kind) else {
            return true
        }
        guard let paneID = payload["pane_id"] as? String else {
            return true
        }
        return !paneID.hasPrefix("remote:\(remoteDeviceID):pane:")
    }
}

/// Names every shell event that can ask the local Herdr session to change.
///
/// This is the last synchronous boundary before an event enters the core. A
/// caller can forget a view-level readiness check, but it still cannot send a
/// mutating request while the current core snapshot says the session is not
/// compatible. Local-only UI and file events remain available so the last
/// useful screen can still be inspected and recovered.
struct LocalHerdrMutationDispatchPolicy {
    private static let eventKinds: Set<String> = [
        "close_pane",
        "close_tab",
        "create_pane",
        "create_scratch_chat_tab",
        "create_tab",
        "create_worktree",
        "create_workspace",
        "focus_checkout",
        "focus_pane",
        "focus_tab",
        "fork_pane",
        "git_worktree_open",
        "key",
        "migrate_main_branch",
        "reconnect_pane",
        "reopen_closed",
        "remove_worktree",
        "reorder_tab",
        "resize_pane",
        "terminal_click",
        "terminal_resize",
        "terminal_scroll",
        "terminal_viewport",
        "toggle_zoom",
    ]

    static func requiresConnectedHerdr(
        kind: String,
        whenDeviceIsRemote: Bool = false
    ) -> Bool {
        !whenDeviceIsRemote && isMutation(kind: kind)
    }

    static func isMutation(kind: String) -> Bool {
        eventKinds.contains(kind)
    }
}

/// Whether a typed core event entered the runtime.
///
/// Most callers only need fire-and-forget dispatch. Pane relationship Open is
/// different: B24 keeps its pending control on screen when the event could not
/// even enter the core, so that caller needs the synchronous admission answer
/// without scraping the app-wide `bridgeError` string.
enum CoreDispatchOutcome: Equatable {
    case accepted
    case rejected(String)
}
