import Foundation

enum CoreSessionsMode: String, Decodable, CaseIterable, Identifiable {
    case sessions
    case memory
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
}

enum CoreSessionsProviderFilter: String, Decodable, CaseIterable, Identifiable {
    case all
    case codex
    case claude
    var id: String { rawValue }
    var title: String {
        switch self {
        case .all: "All"
        case .codex: "Codex"
        case .claude: "Claude Code"
        }
    }
}

struct CoreSessionsSnapshot: Decodable, Equatable {
    let projectID: String?
    let checkoutPath: String?
    let mode: CoreSessionsMode
    let providerFilter: CoreSessionsProviderFilter
    let query: String
    let loading: Bool
    let unavailableReason: String?
    let rows: [CoreSessionRowSnapshot]
    let totalSessionCount: Int
    let memories: [CoreMemoryRowSnapshot]
    let memoryEnabled: Bool
    let memoryDisclosureAccepted: Bool
    let memoryActiveCount: Int
    let memoryConflictCount: Int
    let memoryCapacityReached: Bool
    let analysis: CoreMemoryAnalysisSnapshot
    let notice: CoreMemoryNoticeSnapshot?
    let thisTurnMemoryIDs: [String]

    enum CodingKeys: String, CodingKey {
        case mode, query, loading, rows, memories, analysis, notice
        case projectID = "project_id"
        case checkoutPath = "checkout_path"
        case providerFilter = "provider_filter"
        case unavailableReason = "unavailable_reason"
        case totalSessionCount = "total_session_count"
        case memoryEnabled = "memory_enabled"
        case memoryDisclosureAccepted = "memory_disclosure_accepted"
        case memoryActiveCount = "memory_active_count"
        case memoryConflictCount = "memory_conflict_count"
        case memoryCapacityReached = "memory_capacity_reached"
        case thisTurnMemoryIDs = "this_turn_memory_ids"
    }

    static let empty = CoreSessionsSnapshot(
        projectID: nil,
        checkoutPath: nil,
        mode: .sessions,
        providerFilter: .all,
        query: "",
        loading: false,
        unavailableReason: nil,
        rows: [],
        totalSessionCount: 0,
        memories: [],
        memoryEnabled: false,
        memoryDisclosureAccepted: false,
        memoryActiveCount: 0,
        memoryConflictCount: 0,
        memoryCapacityReached: false,
        analysis: .idle,
        notice: nil,
        thisTurnMemoryIDs: []
    )
}

struct CoreSessionRowSnapshot: Decodable, Equatable, Identifiable {
    let id: String
    let provider: String
    let providerLabel: String
    let locator: String
    let checkoutPath: String
    let firstHumanRequest: String?
    let startedAtUnixMS: UInt64?
    let updatedAtUnixMS: UInt64
    let title: String?
    let unavailableReason: String?

    enum CodingKeys: String, CodingKey {
        case id, provider, locator, title
        case providerLabel = "provider_label"
        case checkoutPath = "checkout_path"
        case firstHumanRequest = "first_human_request"
        case startedAtUnixMS = "started_at_unix_ms"
        case updatedAtUnixMS = "updated_at_unix_ms"
        case unavailableReason = "unavailable_reason"
    }
}

struct CoreMemoryRowSnapshot: Decodable, Equatable, Identifiable {
    let id: String
    let body: String
    let lifecycle: String
    let revision: UInt64
    let sourceCount: Int
    let providedSessionCount: Int
    let updatedAtUnixMS: UInt64

    enum CodingKeys: String, CodingKey {
        case id, body, lifecycle, revision
        case sourceCount = "source_count"
        case providedSessionCount = "provided_session_count"
        case updatedAtUnixMS = "updated_at_unix_ms"
    }
}

struct CoreMemoryAnalysisSnapshot: Decodable, Equatable {
    let state: String
    let discovered: Int
    let analyzed: Int
    let failed: Int
    let message: String?
    let action: String?

    static let idle = CoreMemoryAnalysisSnapshot(
        state: "idle", discovered: 0, analyzed: 0, failed: 0, message: nil, action: nil
    )
}

struct CoreMemoryNoticeSnapshot: Decodable, Equatable {
    let message: String
    let undoBatchID: String?

    enum CodingKeys: String, CodingKey {
        case message
        case undoBatchID = "undo_batch_id"
    }
}
