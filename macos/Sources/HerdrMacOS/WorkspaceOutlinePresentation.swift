import Foundation

/// One Explorer row's fixed Git slot. The value is derived once from the
/// root-scoped Changes snapshot, never from the lazily loaded outline nodes.
struct WorkspaceGitDecoration: Equatable {
    let badge: String
    let title: String
    let status: CoreChangedFileStatus
}

struct WorkspaceGitDecorations: Equatable {
    let rootPath: String
    let entries: [CoreChangedFile]
    private let fileStatuses: [String: CoreChangedFileStatus]
    private let directoryStatuses: [String: CoreChangedFileStatus]

    init(rootPath: String, entries: [CoreChangedFile]) {
        self.rootPath = rootPath
        self.entries = entries

        var fileStatuses: [String: CoreChangedFileStatus] = [:]
        var directoryStatuses: [String: CoreChangedFileStatus] = [:]
        for entry in entries {
            fileStatuses[entry.relativePath] = Self.higherRisk(
                fileStatuses[entry.relativePath],
                entry.status
            )

            var parent = (entry.relativePath as NSString).deletingLastPathComponent
            while !parent.isEmpty && parent != "." {
                directoryStatuses[parent] = Self.higherRisk(
                    directoryStatuses[parent],
                    entry.status
                )
                parent = (parent as NSString).deletingLastPathComponent
            }
            directoryStatuses["."] = Self.higherRisk(directoryStatuses["."], entry.status)
        }
        self.fileStatuses = fileStatuses
        self.directoryStatuses = directoryStatuses
    }

    func decoration(for path: String, isDirectory: Bool) -> WorkspaceGitDecoration? {
        let relative = WorkspaceOutlinePathPresentation.relativePath(path, root: rootPath)
        let status = isDirectory ? directoryStatuses[relative] : fileStatuses[relative]
        guard let status else { return nil }
        return WorkspaceGitDecoration(
            badge: isDirectory ? "●" : status.badge,
            title: isDirectory ? "Contains changed files; highest priority is \(status.title.lowercased())" : status.title,
            status: status
        )
    }

    private static func higherRisk(
        _ current: CoreChangedFileStatus?,
        _ candidate: CoreChangedFileStatus
    ) -> CoreChangedFileStatus {
        guard let current else { return candidate }
        return priority(candidate) < priority(current) ? candidate : current
    }

    private static func priority(_ status: CoreChangedFileStatus) -> Int {
        switch status {
        case .conflict: 0
        case .deleted: 1
        case .renamed: 2
        case .modified: 3
        case .added: 4
        case .untracked: 5
        }
    }
}

/// The core's most recent explorer filesystem change and how far it got.
///
/// The tree reads `finished` to reload the parents of `path` and
/// `destination`, and `failed` to draw `message` under the row the change
/// started from. `id` is what keeps a settled slot from being acted on twice.
struct CoreExplorerOperation: Decodable, Equatable, Sendable {
    let id: UInt64
    let kind: String
    let phase: String
    let path: String
    let destination: String
    let message: String?

    var isSettled: Bool { phase != "working" }
}

/// What a right-click on the tree offers, in the order VS Code offers it.
enum WorkspaceOutlineMenuItem: Equatable {
    case newFile
    case newFolder
    case openWithDefaultApp
    case openInBrowserPane
    case revealInFinder
    case copyPath
    case copyRelativePath
    case rename
    case delete
    case separator

    var title: String {
        switch self {
        case .newFile: "New File"
        case .newFolder: "New Folder"
        case .openWithDefaultApp: "Open with Default App"
        case .openInBrowserPane: "Open in Browser Pane"
        case .revealInFinder: "Reveal in Finder"
        case .copyPath: "Copy Path"
        case .copyRelativePath: "Copy Relative Path"
        case .rename: "Rename"
        case .delete: "Delete"
        case .separator: ""
        }
    }

    /// The catalog chord the item also answers to, drawn beside its title.
    /// Only Delete has one: the chord is bound inside the tree, and the
    /// catalog declares it so nothing else can claim it (D-04).
    var command: ShellMenuCommand? {
        switch self {
        case .delete: .moveToTrash
        default: nil
        }
    }
}

enum WorkspaceOutlineMenuTarget: Equatable {
    /// A file or folder row. A file's creation items act on its parent.
    case item(isDirectory: Bool)
    /// The space below the last row, which stands for the root.
    case emptyArea
}

/// Whether Open in Browser Pane can act right now, and if not the one
/// reason its tooltip gives (D-08). The item stays in the menu either way,
/// so the operator learns what to fix rather than wondering where it went.
enum BrowserPaneOpenAvailability: Equatable {
    case available
    case unavailable(String)

    var reason: String? {
        switch self {
        case .available: nil
        case .unavailable(let reason): reason
        }
    }
}

/// Decides the menu from what was clicked and where the tree lives.
///
/// A remote tree is read-only, so it offers only the two copies; the empty
/// area has no item to reveal, copy, rename or delete, so it offers only
/// creation. A file row alone carries the two open items, between creation
/// and reveal as VS Code orders them; a folder's "open" is already Reveal in
/// Finder (D-04). Delete is last and behind its own separator, as VS Code
/// has it, so the one destructive item is not a neighbour of Rename.
enum WorkspaceOutlineMenuPresentation {
    static func items(for target: WorkspaceOutlineMenuTarget, isRemote: Bool) -> [WorkspaceOutlineMenuItem] {
        if isRemote {
            return [.copyPath, .copyRelativePath]
        }
        switch target {
        case .emptyArea:
            return [.newFile, .newFolder]
        case .item(let isDirectory):
            return [
                .newFile, .newFolder,
                .separator,
            ] + (isDirectory ? [] : [.openWithDefaultApp, .openInBrowserPane, .separator]) + [
                .revealInFinder, .copyPath, .copyRelativePath,
                .separator,
                .rename,
                .separator,
                .delete,
            ]
        }
    }

    /// The conditions Open in Browser Pane needs, each read from the shell's
    /// snapshot-backed state by the caller, so the decision is a value.
    struct BrowserPaneConditions: Equatable {
        var isRemote: Bool
        var nodeOnPath: Bool
        var herdrConnected: Bool
        var hasFocusedPane: Bool
        /// The same file is already being opened (D-09: one request per file).
        var opening: Bool
    }

    /// One reason at a time, in the order the operator can act on it: an open
    /// already in flight finishes on its own, a missing tool is fixed once,
    /// and the connection and the pane come back with the session.
    static func browserPaneAvailability(_ conditions: BrowserPaneConditions) -> BrowserPaneOpenAvailability {
        if conditions.isRemote { return .unavailable("Remote files open on their device") }
        if conditions.opening { return .unavailable("Opening…") }
        if !conditions.nodeOnPath { return .unavailable("Node.js is not on PATH") }
        if !conditions.herdrConnected { return .unavailable("Not connected to Herdr") }
        if !conditions.hasFocusedPane { return .unavailable("No focused pane to open beside") }
        return .available
    }
}

/// The confirmation the tree asks for before an item goes to the Trash,
/// and everything the shell needs to act on the answer.
///
/// The prompt is a value the tree hands to the shell model, which shows it
/// through `.alert(item:)` as the workspace removal prompt is shown. Nothing
/// reaches the core without this value passing through the modal: the
/// tree's two entry points, the menu item and ⌘⌫, both end here (D-03).
struct WorkspaceOutlineTrashPrompt: Identifiable, Equatable, Sendable {
    let root: URL
    let path: URL
    let isDirectory: Bool
    /// The row selected once the item is gone (D-05).
    let selectAfter: URL
    /// The item's inode when the prompt was built, so the core moves the
    /// item the modal named and refuses one that replaced it at the same
    /// path while the modal was open (D-03). Nil when it could not be read;
    /// the core then checks existence only.
    let inode: UInt64?

    var id: String { path.path }
    var name: String { path.lastPathComponent }

    var title: String { "Move '\(name)' to Trash?" }

    var message: String {
        isDirectory
            ? "This folder and everything in it will move to the Trash. You can restore it from Finder."
            : "You can restore it from Finder."
    }

    static let confirmTitle = "Move to Trash"
}

/// Decides where the selection goes once a row leaves the tree.
enum WorkspaceOutlineSelectionPolicy {
    /// The next sibling, else the previous one, else the parent (D-05).
    /// `siblings` is the parent's entries in row order and `removed` the
    /// one leaving; a `removed` not among them means the tree and the
    /// request disagree, so the parent is the only honest answer.
    static func selectionAfterRemoving(_ removed: String, from siblings: [String], parent: String) -> String {
        guard let index = siblings.firstIndex(of: removed) else { return parent }
        if index + 1 < siblings.count { return siblings[index + 1] }
        if index > 0 { return siblings[index - 1] }
        return parent
    }
}

/// Judges a typed name before it is sent to the core.
///
/// The core refuses the same things and more, but it answers after a round
/// trip; the tree already holds the folder's entries, so the three refusals
/// the operator can cause with the keyboard are answered under the field at
/// once. An unchanged rename is neither accepted nor refused: nothing
/// happens and the field closes.
enum WorkspaceOutlineNamePolicy {
    enum Verdict: Equatable {
        case accepted
        case unchanged
        case rejected(String)
    }

    static func verdict(name: String, siblings: Set<String>, current: String? = nil) -> Verdict {
        if name.isEmpty {
            return .rejected("A name is required")
        }
        if name.contains("/") {
            return .rejected("A name cannot contain /")
        }
        if name == "." || name == ".." {
            return .rejected("\(name) is not a valid name")
        }
        if let current, name == current {
            return .unchanged
        }
        if siblings.contains(name) {
            return .rejected("\(name) already exists here")
        }
        return .accepted
    }
}

enum WorkspaceOutlinePathPresentation {
    /// The path relative to the tree's root; the root itself is `.`, and a
    /// path outside the root is returned whole rather than guessed at.
    static func relativePath(_ path: String, root: String) -> String {
        if path == root { return "." }
        let prefix = root.hasSuffix("/") ? root : root + "/"
        guard path.hasPrefix(prefix) else { return path }
        return String(path.dropFirst(prefix.count))
    }

    static func parentPath(_ path: String) -> String {
        (path as NSString).deletingLastPathComponent
    }
}

/// Decides where a dragged item would land, or that it would not.
enum WorkspaceOutlineDropPolicy {
    struct Target: Equatable {
        let path: String
        let isDirectory: Bool
    }

    /// The folder a drop delivers `source` into, or `nil` when the drop
    /// changes nothing: the same parent, the item itself, or a folder inside
    /// the item. A folder receives; a file stands for its parent; the empty
    /// area stands for the root.
    static func destinationDirectory(source: String, over target: Target?, root: String) -> String? {
        let destination: String
        switch target {
        case nil:
            destination = root
        case .some(let target) where target.isDirectory:
            destination = target.path
        case .some(let target):
            destination = WorkspaceOutlinePathPresentation.parentPath(target.path)
        }
        if destination == WorkspaceOutlinePathPresentation.parentPath(source) { return nil }
        if destination == source || destination.hasPrefix(source + "/") { return nil }
        return destination
    }
}
