import Foundation

struct CoreEditorSnapshot: Decodable {
    let tabs: [CoreEditorTabSnapshot]
    let activeTabID: String?
    let document: CoreEditorDocumentSnapshot?
    let archiveDetail: CoreArchiveDetailSnapshot?

    var path: String? { document?.path }
    var language: String? { document?.language }
    var documentKind: CoreDocumentKind? { document?.documentKind }
    var contentsUTF8: String? { document?.contentsUTF8 }
    var openedModifiedAt: UInt64? { document?.openedModifiedAt }
    var dirty: Bool { document?.dirty ?? false }
    var readonlyReason: String? { document?.readonlyReason }
    var conflict: CoreEditorConflict? { document?.conflict }

    enum CodingKeys: String, CodingKey {
        case tabs
        case activeTabID = "active_tab_id"
        case document
        case archiveDetail = "archive_detail"
    }
}

enum CoreEditorTabKind: String, Decodable, Equatable, CaseIterable {
    case file
    case diff
    case session
    case memory
}

struct CoreArchiveDetailSnapshot: Decodable, Equatable {
    let id: String
    let kind: String
    let title: String
    let provider: String?
    let unavailableReason: String?
    let events: [CoreArchiveEventSnapshot]
    let memory: CoreMemoryDetailSnapshot?

    enum CodingKeys: String, CodingKey {
        case id, kind, title, provider, events, memory
        case unavailableReason = "unavailable_reason"
    }
}

struct CoreArchiveEventSnapshot: Decodable, Equatable, Identifiable {
    var id: String { "\(atUnixMS):\(role):\(kind)" }
    let role: String
    let kind: String
    let atUnixMS: UInt64
    let text: String
    let memoryAttachedCount: Int?
    let memoryAttachedItemIDs: [String]?

    enum CodingKeys: String, CodingKey {
        case role, kind, text
        case atUnixMS = "at_unix_ms"
        case memoryAttachedCount = "memory_attached_count"
        case memoryAttachedItemIDs = "memory_attached_item_ids"
    }
}

struct CoreMemoryDetailSnapshot: Decodable, Equatable {
    let id: String
    let body: String
    let lifecycle: String
    let revision: UInt64
    let sourceCount: Int
    let providedSessionCount: Int
    let learnedAtUnixMS: UInt64
    let conflictExistingID: String?
    let conflictCandidateID: String?
    let sources: [CoreMemorySourceSnapshot]
    let revisions: [CoreMemoryRevisionSnapshot]

    enum CodingKeys: String, CodingKey {
        case id, body, lifecycle, revision, sources, revisions
        case sourceCount = "source_count"
        case providedSessionCount = "provided_session_count"
        case learnedAtUnixMS = "learned_at_unix_ms"
        case conflictExistingID = "conflict_existing_id"
        case conflictCandidateID = "conflict_candidate_id"
    }
}

struct CoreMemorySourceSnapshot: Decodable, Equatable, Identifiable {
    var id: String { "\(provider):\(sessionID):\(eventOffset)" }
    let provider: String
    let sessionID: String
    let eventOffset: UInt64
    let available: Bool

    enum CodingKeys: String, CodingKey {
        case provider, available
        case sessionID = "session_id"
        case eventOffset = "event_offset"
    }
}

struct CoreMemoryRevisionSnapshot: Decodable, Equatable, Identifiable {
    var id: UInt64 { revision }
    let revision: UInt64
    let body: String
    let lifecycle: String
    let createdAtUnixMS: UInt64

    enum CodingKeys: String, CodingKey {
        case revision, body, lifecycle
        case createdAtUnixMS = "created_at_unix_ms"
    }
}

struct CoreEditorTabSnapshot: Decodable, Identifiable, Equatable {
    let id: String
    let workspaceID: String
    let checkoutID: String
    let path: String
    let label: String
    let kind: CoreEditorTabKind
    let diffCommitted: Bool?
    var markdownLive: Bool? = nil
    var wrap: Bool? = nil
    let dirty: Bool
    /// The checkout's replaceable preview tab (PRD editor-preview-tab D-07).
    var preview: Bool = false

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceID = "workspace_id"
        case checkoutID = "checkout_id"
        case path
        case label
        case kind
        case markdownLive = "markdown_live"
        case wrap
        case diffCommitted = "diff_committed"
        case dirty
        case preview
    }
}

/// The core's verdict on what an open file is. The shell draws one view per
/// case and decides nothing from the file's name or bytes itself.
enum CoreDocumentKind: String, Decodable, Equatable, CaseIterable {
    case text
    case markdown
    case image
    case pdf
    case binary
}

struct CoreEditorDocumentSnapshot: Decodable {
    let path: String
    let language: String?
    let documentKind: CoreDocumentKind
    let contentsUTF8: String?
    let openedModifiedAt: UInt64?
    let dirty: Bool
    let readonlyReason: String?
    let conflict: CoreEditorConflict?

    enum CodingKeys: String, CodingKey {
        case path
        case language
        case documentKind = "document_kind"
        case contentsUTF8 = "contents_utf8"
        case openedModifiedAt = "opened_modified_at_unix_ms"
        case dirty
        case readonlyReason = "readonly_reason"
        case conflict
    }
}

struct CoreEditorConflict: Decodable {
    let diskModifiedAt: UInt64
    let openedModifiedAt: UInt64

    enum CodingKeys: String, CodingKey {
        case diskModifiedAt = "disk_modified_at_unix_ms"
        case openedModifiedAt = "opened_modified_at_unix_ms"
    }
}
