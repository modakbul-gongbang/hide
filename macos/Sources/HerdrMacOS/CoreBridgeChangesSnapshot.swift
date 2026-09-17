import Foundation

/// One checkout's Git working-tree state as the core read it.
struct CoreChangesSnapshot: Decodable {
    let rootPath: String?
    /// The working tree's own changes.
    let entries: [CoreChangedFile]
    /// What commits on this branch changed since `baseBranch`.
    let committed: [CoreChangedFile]
    /// What the committed group is measured against. `nil` means there is no
    /// comparison to make, so that group is not shown at all.
    let baseBranch: String?
    let selectedPath: String?
    /// Which group the selection is in. The same path can appear in both and
    /// its two diffs differ, so the group is part of the selection.
    let selectedCommitted: Bool
    let diff: CoreChangedFileDiff?
    /// Why there is nothing to list. An empty list with no reason means the
    /// checkout genuinely has no changes.
    let unavailableReason: String?

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case entries
        case committed
        case baseBranch = "base_branch"
        case selectedPath = "selected_path"
        case selectedCommitted = "selected_committed"
        case diff
        case unavailableReason = "unavailable_reason"
    }

    static let empty = CoreChangesSnapshot(
        rootPath: nil,
        entries: [],
        committed: [],
        baseBranch: nil,
        selectedPath: nil,
        selectedCommitted: false,
        diff: nil,
        unavailableReason: nil
    )

    init(
        rootPath: String?,
        entries: [CoreChangedFile],
        committed: [CoreChangedFile] = [],
        baseBranch: String? = nil,
        selectedPath: String?,
        selectedCommitted: Bool = false,
        diff: CoreChangedFileDiff?,
        unavailableReason: String?
    ) {
        self.rootPath = rootPath
        self.entries = entries
        self.committed = committed
        self.baseBranch = baseBranch
        self.selectedPath = selectedPath
        self.selectedCommitted = selectedCommitted
        self.diff = diff
        self.unavailableReason = unavailableReason
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        rootPath = try container.decodeIfPresent(String.self, forKey: .rootPath)
        entries = try container.decodeIfPresent([CoreChangedFile].self, forKey: .entries) ?? []
        committed = try container.decodeIfPresent([CoreChangedFile].self, forKey: .committed) ?? []
        baseBranch = try container.decodeIfPresent(String.self, forKey: .baseBranch)
        selectedPath = try container.decodeIfPresent(String.self, forKey: .selectedPath)
        selectedCommitted = try container.decodeIfPresent(Bool.self, forKey: .selectedCommitted) ?? false
        diff = try container.decodeIfPresent(CoreChangedFileDiff.self, forKey: .diff)
        unavailableReason = try container.decodeIfPresent(String.self, forKey: .unavailableReason)
    }
}
struct CoreChangedFile: Decodable, Identifiable, Equatable {
    let path: String
    let relativePath: String
    let previousRelativePath: String?
    let status: CoreChangedFileStatus
    /// Absent for a file git cannot count - an untracked one has no index side
    /// and a binary one has no lines - so the row shows no numbers rather than
    /// a zero that would read as "changed nothing".
    let addedLines: Int?
    let removedLines: Int?

    var id: String { path }

    /// Row identity per group. A file edited again after being committed is in
    /// both groups under one path, and the list must show it twice.
    var uncommittedRowID: String { "uncommitted:" + path }
    var committedRowID: String { "committed:" + path }

    /// The directory the row shows in the dimmer half, empty at the root.
    var directory: String {
        let components = relativePath.split(separator: "/").dropLast()
        return components.joined(separator: "/")
    }

    /// The file name the row shows in the brighter half.
    var name: String {
        String(relativePath.split(separator: "/").last ?? "")
    }

    enum CodingKeys: String, CodingKey {
        case path
        case relativePath = "relative_path"
        case previousRelativePath = "previous_relative_path"
        case status
        case addedLines = "added_lines"
        case removedLines = "removed_lines"
    }

    init(
        path: String,
        relativePath: String,
        previousRelativePath: String? = nil,
        status: CoreChangedFileStatus,
        addedLines: Int? = nil,
        removedLines: Int? = nil
    ) {
        self.path = path
        self.relativePath = relativePath
        self.previousRelativePath = previousRelativePath
        self.status = status
        self.addedLines = addedLines
        self.removedLines = removedLines
    }
}

enum CoreChangedFileStatus: String, Decodable, Equatable {
    case modified
    case added
    case deleted
    case untracked
    case renamed
    case conflict

    /// The single letter the row shows, which is how Git itself names these.
    var badge: String {
        switch self {
        case .modified: "M"
        case .added: "A"
        case .deleted: "D"
        case .untracked: "U"
        case .renamed: "R"
        case .conflict: "!"
        }
    }

    var title: String {
        switch self {
        case .modified: "Modified"
        case .added: "Added"
        case .deleted: "Deleted"
        case .untracked: "Untracked"
        case .renamed: "Renamed"
        case .conflict: "Conflict"
        }
    }
}

struct CoreChangedFileDiff: Decodable, Equatable {
    let path: String
    let text: String
    /// Why this diff is not the whole story: it was cut for size, or git
    /// would not produce it. Either way the reader is told rather than shown a
    /// short diff that looks complete.
    let notice: String?

    enum CodingKeys: String, CodingKey {
        case path
        case text
        case notice
    }
}
