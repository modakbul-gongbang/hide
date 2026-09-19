import Foundation

struct CoreEditorSnapshot: Decodable {
    let tabs: [CoreEditorTabSnapshot]
    let activeTabID: String?
    let document: CoreEditorDocumentSnapshot?

    var path: String? { document?.path }
    var language: String? { document?.language }
    var contentsUTF8: String? { document?.contentsUTF8 }
    var openedModifiedAt: UInt64? { document?.openedModifiedAt }
    var dirty: Bool { document?.dirty ?? false }
    var readonlyReason: String? { document?.readonlyReason }
    var conflict: CoreEditorConflict? { document?.conflict }

    enum CodingKeys: String, CodingKey {
        case tabs
        case activeTabID = "active_tab_id"
        case document
    }
}

enum CoreEditorTabKind: String, Decodable, Equatable {
    case file
    case diff
}

struct CoreEditorTabSnapshot: Decodable, Identifiable, Equatable {
    let id: String
    let workspaceID: String
    let checkoutID: String
    let path: String
    let label: String
    let kind: CoreEditorTabKind
    let diffCommitted: Bool?
    var markdownPreview: Bool? = nil
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
        case markdownPreview = "markdown_preview"
        case wrap
        case diffCommitted = "diff_committed"
        case dirty
        case preview
    }
}

struct CoreEditorDocumentSnapshot: Decodable {
    let path: String
    let language: String?
    let contentsUTF8: String?
    let openedModifiedAt: UInt64?
    let dirty: Bool
    let readonlyReason: String?
    let conflict: CoreEditorConflict?

    enum CodingKeys: String, CodingKey {
        case path
        case language
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
