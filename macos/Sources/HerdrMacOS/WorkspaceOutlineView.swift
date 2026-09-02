import AppKit
import SwiftUI

/// What activating a row does. Opening used to be bound to selection change,
/// so walking the tree with the arrow keys opened a file tab for every row it
/// passed, and a directory's name was inert while only its disclosure triangle
/// worked. Activation is a click on the row body or `Return`; selection is
/// only a cursor.
enum WorkspaceOutlineActivation: Equatable {
    case open
    case toggle
    case none
}

enum WorkspaceOutlineActivationPolicy {
    static func activation(isDirectory: Bool, isPlaceholder: Bool) -> WorkspaceOutlineActivation {
        if isPlaceholder { return .none }
        return isDirectory ? .toggle : .open
    }
}

struct WorkspaceDirectoryEntry: Equatable, Sendable {
    let url: URL
    let isDirectory: Bool
}

enum WorkspaceDirectoryLoader {
    static let skippedNames: Set<String> = [
        ".git", ".build", "build", "target", "DerivedData",
    ]

    static func loadDirectory(at url: URL) throws -> [WorkspaceDirectoryEntry] {
        let urls = try FileManager.default.contentsOfDirectory(
            at: url,
            includingPropertiesForKeys: [.isDirectoryKey, .isRegularFileKey],
            options: []
        )
        return try urls.compactMap { child in
            guard !skippedNames.contains(child.lastPathComponent) else { return nil }
            let values = try child.resourceValues(forKeys: [.isDirectoryKey, .isRegularFileKey])
            guard values.isDirectory == true || values.isRegularFile == true else { return nil }
            return WorkspaceDirectoryEntry(url: child, isDirectory: values.isDirectory == true)
        }
        .sorted { lhs, rhs in
            if lhs.isDirectory != rhs.isDirectory { return lhs.isDirectory }
            return lhs.url.lastPathComponent.localizedStandardCompare(rhs.url.lastPathComponent) == .orderedAscending
        }
    }
}

private final class WorkspaceOutlineNode: NSObject {
    enum State {
        case unloaded
        case loading
        case loaded
        case failed(String)
    }

    let url: URL
    let isDirectory: Bool
    let placeholderMessage: String?
    var children: [WorkspaceOutlineNode] = []
    var state: State

    init(entry: WorkspaceDirectoryEntry) {
        url = entry.url
        isDirectory = entry.isDirectory
        placeholderMessage = nil
        state = entry.isDirectory ? .unloaded : .loaded
    }

    init(placeholderMessage: String, parentURL: URL) {
        url = parentURL.appendingPathComponent(".hide-placeholder")
        isDirectory = false
        self.placeholderMessage = placeholderMessage
        state = .loaded
    }

    var isPlaceholder: Bool { placeholderMessage != nil }
    var name: String { placeholderMessage ?? url.lastPathComponent }
}

private final class WorkspaceOutlineRowView: NSTableRowView {
    var level = 0 { didSet { needsDisplay = true } }
    var isHovered = false { didSet { needsDisplay = true } }
    private var trackingAreaReference: NSTrackingArea?

    override func updateTrackingAreas() {
        if let trackingAreaReference { removeTrackingArea(trackingAreaReference) }
        let tracking = NSTrackingArea(
            rect: bounds,
            options: [.activeInKeyWindow, .mouseEnteredAndExited, .inVisibleRect],
            owner: self
        )
        addTrackingArea(tracking)
        trackingAreaReference = tracking
        super.updateTrackingAreas()
    }

    override func mouseEntered(with event: NSEvent) { isHovered = true }
    override func mouseExited(with event: NSEvent) { isHovered = false }

    override func drawBackground(in dirtyRect: NSRect) {
        if isSelected {
            NSColor.white.withAlphaComponent(0.10).setFill()
            NSBezierPath(roundedRect: bounds.insetBy(dx: 4, dy: 1), xRadius: 4, yRadius: 4).fill()
        } else if isHovered {
            NSColor.white.withAlphaComponent(0.05).setFill()
            NSBezierPath(roundedRect: bounds.insetBy(dx: 4, dy: 1), xRadius: 4, yRadius: 4).fill()
        }

        guard level > 0 else { return }
        NSColor.white.withAlphaComponent(0.07).setStroke()
        for depth in 0..<level {
            let x = CGFloat(14 + depth * 8) + 0.5
            let guide = NSBezierPath()
            guide.move(to: NSPoint(x: x, y: bounds.minY))
            guide.line(to: NSPoint(x: x, y: bounds.maxY))
            guide.lineWidth = 1
            guide.stroke()
        }
    }
}

final class WorkspaceNSOutlineView: NSOutlineView {
    var onActivate: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 76 {
            onActivate?()
            return
        }
        super.keyDown(with: event)
    }
}

private final class WorkspaceOutlineScrollView: NSScrollView {
    override func layout() {
        super.layout()
        guard
            let outline = documentView as? WorkspaceNSOutlineView,
            let column = outline.tableColumns.first
        else { return }

        let viewportWidth = max(column.minWidth, contentSize.width)
        guard abs(column.width - viewportWidth) > 0.5 else { return }
        column.width = viewportWidth
    }
}

struct WorkspaceOutlineView: NSViewRepresentable {
    let rootURL: URL
    let expandedPaths: Set<String>
    let selectedPath: String?
    let fontScale: CGFloat
    let openFile: (URL) -> Void
    let updateExpandedPaths: ([String]) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(openFile: openFile, updateExpandedPaths: updateExpandedPaths)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let outline = WorkspaceNSOutlineView()
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("workspace-name"))
        column.minWidth = 120
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        outline.headerView = nil
        outline.backgroundColor = NSColor(HideTheme.panel)
        outline.selectionHighlightStyle = .none
        outline.rowHeight = 22
        outline.indentationPerLevel = 8
        outline.intercellSpacing = .zero
        outline.floatsGroupRows = false
        outline.delegate = context.coordinator
        outline.dataSource = context.coordinator
        outline.target = context.coordinator
        outline.action = #selector(Coordinator.activateClickedRow)
        outline.onActivate = { [weak coordinator = context.coordinator] in
            coordinator?.activateSelection()
        }
        outline.setAccessibilityIdentifier("explorer-file-tree")

        let scroll = WorkspaceOutlineScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = NSColor(HideTheme.panel)
        context.coordinator.attach(outline: outline)
        return scroll
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        context.coordinator.openFile = openFile
        context.coordinator.updateExpandedPaths = updateExpandedPaths
        context.coordinator.apply(
            rootURL: rootURL,
            expandedPaths: expandedPaths,
            selectedPath: selectedPath,
            fontScale: fontScale
        )
    }

    @MainActor
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
        var openFile: (URL) -> Void
        var updateExpandedPaths: ([String]) -> Void
        private weak var outline: WorkspaceNSOutlineView?
        private var rootNode: WorkspaceOutlineNode?
        private var rootPath: String?
        private var rootGeneration: UInt64 = 0
        private var desiredExpandedPaths: Set<String> = []
        private var selectedPath: String?
        /// The last path this view actually moved the selection to. The core
        /// reports the active file tab's path on every snapshot; re-asserting
        /// it on every tick would snap an arrow-key cursor back to the open
        /// file, so the selection moves only when that path changes.
        private var appliedSelectedPath: String??
        private var fontScale: CGFloat = 1
        private var suppressExpansionPersistence = false

        init(openFile: @escaping (URL) -> Void, updateExpandedPaths: @escaping ([String]) -> Void) {
            self.openFile = openFile
            self.updateExpandedPaths = updateExpandedPaths
        }

        fileprivate func attach(outline: WorkspaceNSOutlineView) {
            self.outline = outline
        }

        func apply(rootURL: URL, expandedPaths: Set<String>, selectedPath: String?, fontScale: CGFloat) {
            desiredExpandedPaths = expandedPaths
            self.selectedPath = selectedPath
            self.fontScale = fontScale
            if rootPath != rootURL.path {
                rootGeneration &+= 1
                rootPath = rootURL.path
                rootNode = WorkspaceOutlineNode(entry: .init(url: rootURL, isDirectory: true))
                appliedSelectedPath = nil
                outline?.reloadData()
                if let rootNode {
                    suppressExpansionPersistence = true
                    outline?.expandItem(rootNode)
                    suppressExpansionPersistence = false
                    loadChildren(of: rootNode)
                }
            } else {
                restoreVisibleState()
            }
        }

        func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
            guard let node = item as? WorkspaceOutlineNode else { return rootNode == nil ? 0 : 1 }
            if node.isDirectory && node.children.isEmpty {
                switch node.state {
                case .unloaded, .loading, .failed: return 1
                case .loaded: return 0
                }
            }
            return node.children.count
        }

        func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
            guard let node = item as? WorkspaceOutlineNode else { return rootNode as Any }
            if node.children.isEmpty {
                switch node.state {
                case .unloaded, .loading:
                    return WorkspaceOutlineNode(placeholderMessage: "Loading…", parentURL: node.url)
                case .failed(let message):
                    return WorkspaceOutlineNode(placeholderMessage: message, parentURL: node.url)
                case .loaded:
                    preconditionFailure("Loaded leaf requested a child")
                }
            }
            return node.children[index]
        }

        func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
            (item as? WorkspaceOutlineNode)?.isDirectory == true
        }

        func outlineView(_ outlineView: NSOutlineView, shouldExpandItem item: Any) -> Bool {
            guard let node = item as? WorkspaceOutlineNode, node.isDirectory else { return false }
            if case .failed = node.state {
                // Collapsing and expanding is an explicit retry. The previous
                // failure remains visible until the user asks to try again.
                node.state = .unloaded
            }
            loadChildren(of: node)
            return true
        }

        func outlineViewItemDidExpand(_ notification: Notification) {
            persistExpansionIfNeeded()
        }

        func outlineViewItemDidCollapse(_ notification: Notification) {
            persistExpansionIfNeeded()
            // The node graph is scoped to rootPath, so retaining loaded children caches
            // them by URL until the root changes instead of re-enumerating on re-expansion.
        }

        func outlineView(
            _ outlineView: NSOutlineView,
            viewFor tableColumn: NSTableColumn?,
            item: Any
        ) -> NSView? {
            guard let node = item as? WorkspaceOutlineNode else { return nil }
            let identifier = NSUserInterfaceItemIdentifier("workspace-outline-cell")
            let cell = (outlineView.makeView(withIdentifier: identifier, owner: self) as? NSTableCellView)
                ?? makeCell(identifier: identifier)
            configure(cell: cell, node: node)
            return cell
        }

        func outlineView(_ outlineView: NSOutlineView, rowViewForItem item: Any) -> NSTableRowView? {
            let row = WorkspaceOutlineRowView()
            row.level = max(0, outlineView.level(forItem: item))
            return row
        }

        /// A click anywhere on a row's body activates it. The disclosure
        /// triangle is its own control and consumes its own click, so a click
        /// on the triangle never reaches here and cannot toggle twice.
        @objc func activateClickedRow() {
            guard let outline, outline.clickedRow >= 0,
                  let node = outline.item(atRow: outline.clickedRow) as? WorkspaceOutlineNode
            else { return }
            activate(node)
        }

        @objc func activateSelection() {
            guard let outline, outline.selectedRow >= 0,
                  let node = outline.item(atRow: outline.selectedRow) as? WorkspaceOutlineNode
            else { return }
            activate(node)
        }

        private func activate(_ node: WorkspaceOutlineNode) {
            guard let outline else { return }
            switch WorkspaceOutlineActivationPolicy.activation(
                isDirectory: node.isDirectory,
                isPlaceholder: node.isPlaceholder
            ) {
            case .open:
                openFile(node.url)
            case .toggle:
                if outline.isItemExpanded(node) {
                    outline.collapseItem(node)
                } else {
                    outline.expandItem(node)
                }
            case .none:
                break
            }
        }

        private func makeCell(identifier: NSUserInterfaceItemIdentifier) -> NSTableCellView {
            let cell = NSTableCellView()
            cell.identifier = identifier
            let icon = NSTextField(labelWithString: "")
            icon.identifier = NSUserInterfaceItemIdentifier("icon")
            icon.translatesAutoresizingMaskIntoConstraints = false
            icon.alignment = .center
            let label = NSTextField(labelWithString: "")
            label.identifier = NSUserInterfaceItemIdentifier("label")
            label.translatesAutoresizingMaskIntoConstraints = false
            label.lineBreakMode = .byTruncatingMiddle
            cell.addSubview(icon)
            cell.addSubview(label)
            cell.textField = label
            NSLayoutConstraint.activate([
                icon.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
                icon.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
                icon.widthAnchor.constraint(equalToConstant: 16),
                label.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 5),
                label.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
                label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
            ])
            return cell
        }

        private func configure(cell: NSTableCellView, node: WorkspaceOutlineNode) {
            let label = cell.textField
            label?.stringValue = node.name
            label?.font = NSFont.systemFont(ofSize: 11 * fontScale, weight: .regular)
            label?.textColor = node.isPlaceholder ? NSColor(HideTheme.muted) : NSColor(HideTheme.primary)
            cell.setAccessibilityIdentifier("workspace-item-\(node.url.path)")
            cell.setAccessibilityLabel(node.name)

            guard let iconView = cell.subviews.first(where: { $0.identifier?.rawValue == "icon" }) as? NSTextField else {
                return
            }
            if node.isPlaceholder {
                iconView.stringValue = ""
            } else if node.isDirectory {
                let configuration = NSImage.SymbolConfiguration(
                    pointSize: 11 * fontScale,
                    weight: .regular
                )
                let attachment = NSTextAttachment()
                attachment.image = NSImage(
                    systemSymbolName: "folder",
                    accessibilityDescription: "Folder"
                )?.withSymbolConfiguration(configuration)
                attachment.bounds = NSRect(
                    x: 0,
                    y: -2,
                    width: 13 * fontScale,
                    height: 13 * fontScale
                )
                iconView.attributedStringValue = NSAttributedString(attachment: attachment)
                iconView.textColor = NSColor(HideTheme.secondary)
            } else {
                let icon = SetiFileIconCatalog.icon(for: node.url)
                if SetiIconFont.isAvailable, let font = NSFont(name: SetiIconFont.postScriptName, size: 12 * fontScale) {
                    iconView.stringValue = icon.glyph
                    iconView.font = font
                } else {
                    let configuration = NSImage.SymbolConfiguration(
                        pointSize: 11 * fontScale,
                        weight: .regular
                    )
                    let attachment = NSTextAttachment()
                    attachment.image = NSImage(
                        systemSymbolName: icon.fallbackSystemImage,
                        accessibilityDescription: node.name
                    )?.withSymbolConfiguration(configuration)
                    attachment.bounds = NSRect(
                        x: 0,
                        y: -2,
                        width: 13 * fontScale,
                        height: 13 * fontScale
                    )
                    iconView.attributedStringValue = NSAttributedString(attachment: attachment)
                }
                iconView.textColor = NSColor(HideTheme.color(for: icon.colorHex))
            }
        }

        private func loadChildren(of node: WorkspaceOutlineNode) {
            guard node.isDirectory else { return }
            guard case .unloaded = node.state else { return }
            guard let loadRoot = rootNode else { return }
            let loadGeneration = rootGeneration
            node.state = .loading
            outline?.reloadItem(node, reloadChildren: true)
            let url = node.url
            Task {
                let result = await Task.detached(priority: .userInitiated) {
                    Result { try WorkspaceDirectoryLoader.loadDirectory(at: url) }
                }.value
                guard loadGeneration == rootGeneration, loadRoot === rootNode else {
                    return
                }
                switch result {
                case .success(let entries):
                    node.children = entries.map(WorkspaceOutlineNode.init(entry:))
                    node.state = .loaded
                case .failure(let error):
                    let message = "Could not read \(url.lastPathComponent): \(error.localizedDescription)"
                    node.state = .failed(message)
                    Self.reportFailure(path: url.path, message: error.localizedDescription)
                }
                outline?.reloadItem(node, reloadChildren: true)
                restoreVisibleState()
            }
        }

        private func restoreVisibleState() {
            guard let outline, let rootNode else { return }
            suppressExpansionPersistence = true
            expandRecordedDescendants(of: rootNode, in: outline)
            applySelection(in: outline, rootNode: rootNode)
            suppressExpansionPersistence = false
        }

        /// The highlighted row follows the active file tab: it moves on open
        /// and on tab switch, and clears when the last file tab closes, which
        /// is what left a highlight standing on a file nobody had open.
        private func applySelection(in outline: NSOutlineView, rootNode: WorkspaceOutlineNode) {
            guard appliedSelectedPath != .some(selectedPath) else { return }
            guard let selectedPath else {
                appliedSelectedPath = .some(nil)
                outline.deselectAll(nil)
                return
            }
            // A file inside a collapsed folder has no row to highlight, so the
            // ancestors are opened on the way down. A folder still loading
            // returns nothing now and this runs again when its load lands.
            guard let node = revealNode(path: selectedPath, from: rootNode, in: outline) else { return }
            let row = outline.row(forItem: node)
            guard row >= 0 else { return }
            appliedSelectedPath = .some(selectedPath)
            outline.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
            outline.scrollRowToVisible(row)
        }

        /// Walks from the root toward `path`, expanding and loading each
        /// ancestor. Returns the node once every ancestor on the way is loaded,
        /// and nil while a load is still in flight.
        private func revealNode(
            path: String,
            from node: WorkspaceOutlineNode,
            in outline: NSOutlineView
        ) -> WorkspaceOutlineNode? {
            if node.url.path == path { return node }
            guard node.isDirectory, path.hasPrefix(node.url.path + "/") else { return nil }
            if case .loaded = node.state {} else {
                loadChildren(of: node)
                return nil
            }
            guard let child = node.children.first(where: {
                $0.url.path == path || path.hasPrefix($0.url.path + "/")
            }) else { return nil }
            if child.isDirectory, !outline.isItemExpanded(child) {
                outline.expandItem(child)
            }
            return revealNode(path: path, from: child, in: outline)
        }

        private func expandRecordedDescendants(of node: WorkspaceOutlineNode, in outline: NSOutlineView) {
            if node === rootNode || desiredExpandedPaths.contains(node.url.path) {
                outline.expandItem(node)
                loadChildren(of: node)
            }
            for child in node.children where child.isDirectory {
                expandRecordedDescendants(of: child, in: outline)
            }
        }

        private func persistExpansionIfNeeded() {
            guard !suppressExpansionPersistence, let outline, let rootNode else { return }
            var paths: [String] = []
            collectExpandedDirectories(from: rootNode, outline: outline, paths: &paths)
            updateExpandedPaths(paths.sorted())
        }

        private func collectExpandedDirectories(
            from node: WorkspaceOutlineNode,
            outline: NSOutlineView,
            paths: inout [String]
        ) {
            guard node.isDirectory else { return }
            if outline.isItemExpanded(node) {
                paths.append(node.url.path)
                for child in node.children {
                    collectExpandedDirectories(from: child, outline: outline, paths: &paths)
                }
            }
        }

        private static func reportFailure(path: String, message: String) {
            let payload = [
                "component": "explorer",
                "kind": "directory.read_failed",
                "path": path,
                "message": message,
            ]
            guard let data = try? JSONSerialization.data(withJSONObject: payload),
                  var line = String(data: data, encoding: .utf8)
            else { return }
            line.append("\n")
            FileHandle.standardError.write(Data(line.utf8))
        }
    }
}
