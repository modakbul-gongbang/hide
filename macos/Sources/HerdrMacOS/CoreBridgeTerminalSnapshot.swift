import Foundation

struct CoreTerminalSnapshot: Decodable {
    let paneID: String?
    let closed: Bool
    let exitCode: Int32?
    let panes: [CoreTerminalPaneSnapshot]

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case closed
        case exitCode = "exit_code"
        case panes
    }
}
struct CoreTerminalChunk: Decodable {
    let paneID: String
    let sequence: UInt64
    let bytesBase64: String
    let frame: CoreTerminalFrame?
    let inputSent: CoreTerminalInputSent?

    enum CodingKeys: String, CodingKey {
        case frame
        case inputSent = "input_sent"
        case paneID = "pane_id"
        case sequence
        case bytesBase64 = "bytes_base64"
    }
}

struct CoreTerminalFrame: Decodable {
    let width: Int
    let height: Int
    let full: Bool
}

struct CoreTerminalInputSent: Decodable {
    let id: UInt64
    let milliseconds: Double
    let outcome: String
}

struct CoreTerminalPaneSnapshot: Decodable, Identifiable {
    var id: String { paneID }
    let paneID: String
    let closed: Bool
    let exitCode: Int32?
    let transportState: String
    let transportMessage: String?
    let transportGeneration: UInt64
    let transportLastAttemptAtUnixMS: UInt64?
    let transportAttempt: UInt64
    let transportExitCategory: String?
    let transportRetryDecision: String

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case closed
        case exitCode = "exit_code"
        case transportState = "transport_state"
        case transportMessage = "transport_message"
        case transportGeneration = "transport_generation"
        case transportLastAttemptAtUnixMS = "transport_last_attempt_at_unix_ms"
        case transportAttempt = "transport_attempt"
        case transportExitCategory = "transport_exit_category"
        case transportRetryDecision = "transport_retry_decision"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        paneID = try container.decode(String.self, forKey: .paneID)
        closed = try container.decodeIfPresent(Bool.self, forKey: .closed) ?? false
        exitCode = try container.decodeIfPresent(Int32.self, forKey: .exitCode)
        transportState = try container.decodeIfPresent(String.self, forKey: .transportState) ?? "idle"
        transportMessage = try container.decodeIfPresent(String.self, forKey: .transportMessage)
        transportGeneration = try container.decodeIfPresent(UInt64.self, forKey: .transportGeneration) ?? 0
        transportAttempt = try container.decodeIfPresent(UInt64.self, forKey: .transportAttempt) ?? 0
        transportLastAttemptAtUnixMS = try container.decodeIfPresent(UInt64.self, forKey: .transportLastAttemptAtUnixMS)
        transportExitCategory = try container.decodeIfPresent(String.self, forKey: .transportExitCategory)
        transportRetryDecision = try container.decodeIfPresent(
            String.self,
            forKey: .transportRetryDecision
        ) ?? "none"
    }
}
