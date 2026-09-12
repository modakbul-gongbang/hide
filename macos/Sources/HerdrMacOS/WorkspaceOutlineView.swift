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
            let isDirectory = values.isDirectory == true
            // The child keeps the parent's spelling of the path. The listing
            // can come back through a resolved symlink (`/private/var` for
            // `/var`), and a node whose path does not start with its
            // parent's cannot be found again by path, relative to the root,
            // or judged inside the root by the core.
            return WorkspaceDirectoryEntry(
                url: url.appendingPathComponent(child.lastPathComponent, isDirectory: isDirectory),
                isDirectory: isDirectory
            )
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

    /// What a row stands for. Only an entry is on disk; the others are rows
    /// the tree puts among a folder's children to say something in place:
    /// a load in progress or failed, the name field for an item that does
    /// not exist yet, and the one-line reason a change was refused, drawn
    /// under the row it was asked of.
    enum Role: Equatable {
        case entry
        case placeholder(String)
        case draft(WorkspaceOutlineDraftKind)
        case failure(String)
    }

    let url: URL
    let isDirectory: Bool
    let role: Role
    var children: [WorkspaceOutlineNode] = []
    var state: State

    init(entry: WorkspaceDirectoryEntry) {
        url = entry.url
        isDirectory = entry.isDirectory
        role = .entry
        state = entry.isDirectory ? .unloaded : .loaded
    }

    init(placeholderMessage: String, parentURL: URL) {
        url = parentURL.appendingPathComponent(".hide-placeholder")
        isDirectory = false
        role = .placeholder(placeholderMessage)
        state = .loaded
    }

    init(draft kind: WorkspaceOutlineDraftKind, parentURL: URL) {
        url = parentURL.appendingPathComponent(".hide-draft")
        isDirectory = kind == .folder
        role = .draft(kind)
        state = .loaded
    }

    init(failure message: String, under target: URL) {
        url = target.appendingPathComponent(".hide-failure")
        isDirectory = false
        role = .failure(message)
        state = .loaded
    }

    var isEntry: Bool { role == .entry }
    var isPlaceholder: Bool { !isEntry }
    var name: String {
        switch role {
        case .entry: url.lastPathComponent
        case .placeholder(let message), .failure(let message): message
        case .draft: ""
        }
    }
}

enum WorkspaceOutlineDraftKind: Equatable {
    case file
    case folder
}

/// The changes the tree can ask for. They are closures rather than a
/// `ShellModel` reference so the outline stays testable without one and so
/// the view cannot reach any other event. Four go to the core directly; a
/// trash goes to the shell as a prompt, and only the modal's confirmation
/// turns it into the core event (D-03).
struct WorkspaceFileOperations {
    var createFile: (_ parent: URL, _ name: String) -> Void
    var createDirectory: (_ parent: URL, _ name: String) -> Void
    var rename: (_ path: URL, _ name: String) -> Void
    var move: (_ path: URL, _ destination: URL) -> Void
    var requestTrash: (WorkspaceOutlineTrashPrompt) -> Void
}

/// A row draws hover, but it does not decide it.
///
/// The row used to own a tracking area and set `isHovered` from its own
/// enter/exit events. Two things broke that. `NSOutlineView` recycles row
/// views, so a row that scrolled away handed its `true` to whatever item took
/// its place; and a wheel scroll moves content under a stationary pointer,
/// which AppKit never reports as an exit. The result was a band of rows all
/// drawing hover at once, which reads as a multiple selection because the
/// selected and hovered fills differ only in alpha. The outline view now owns
/// the hovered row: a crossing is reported to it rather than acted on, and it
/// re-reads the pointer whenever rows move under it, so no fill outlives the
/// item it described.
private final class WorkspaceOutlineRowView: NSTableRowView {
    var level = 0 { didSet { needsDisplay = true } }
    var isHovered = false { didSet { needsDisplay = true } }
    private var trackingAreaReference: NSTrackingArea?

    /// A row still carries the tracking area, because enter and exit are the
    /// only pointer events available here: `mouseMoved` needs the window to
    /// accept moved events, which no owner of this window turns on. What
    /// changed is that the row reports the crossing rather than acting on it.
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let trackingAreaReference { removeTrackingArea(trackingAreaReference) }
        let tracking = NSTrackingArea(
            rect: bounds,
            options: [.activeInKeyWindow, .mouseEnteredAndExited, .inVisibleRect],
            owner: self
        )
        addTrackingArea(tracking)
        trackingAreaReference = tracking
    }

    override func mouseEntered(with event: NSEvent) {
        (superview as? WorkspaceNSOutlineView)?.hoverEntered(self)
    }

    override func mouseExited(with event: NSEvent) {
        (superview as? WorkspaceNSOutlineView)?.hoverExited(self)
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        isHovered = false
    }

    override func drawBackground(in dirtyRect: NSRect) {
        if isSelected {
            drawRowBackground(opacity: HideTheme.Opacity.selectedFill)
        } else if isHovered {
            drawRowBackground(opacity: HideTheme.Opacity.subtleFill)
        }

        guard level > 0 else { return }
        HideTheme.Native.primary.withAlphaComponent(HideTheme.Opacity.subtleFill).setStroke()
        for depth in 0..<level {
            let x = HideTheme.checkoutIconWidth
                + CGFloat(depth) * HideTheme.spacingSM
                + HideTheme.Layout.hairlineWidth / 2
            let guide = NSBezierPath()
            guide.move(to: NSPoint(x: x, y: bounds.minY))
            guide.line(to: NSPoint(x: x, y: bounds.maxY))
            guide.lineWidth = HideTheme.Layout.hairlineWidth
            guide.stroke()
        }
    }

    private func drawRowBackground(opacity: Double) {
        HideTheme.Native.primary.withAlphaComponent(opacity).setFill()
        NSBezierPath(
            roundedRect: bounds.insetBy(dx: HideTheme.spacingXS, dy: HideTheme.Layout.hairlineWidth),
            xRadius: HideTheme.radiusExtraSmall,
            yRadius: HideTheme.radiusExtraSmall
        ).fill()
    }
}

final class WorkspaceNSOutlineView: NSOutlineView {
    var onActivate: (() -> Void)?
    /// ⌘⌫ on the selected row. Answered here and nowhere else, so the chord
    /// reaches a file only while the tree holds the keyboard (D-04).
    var onTrash: (() -> Void)?
    /// Asked with the row under the pointer, or -1 for the empty area.
    var contextMenu: ((Int) -> NSMenu?)?

    private var hoveredRow = -1

    /// `super` records `clickedRow` and draws the row's contextual ring; the
    /// menu itself is the coordinator's, because it depends on the item.
    override func menu(for event: NSEvent) -> NSMenu? {
        _ = super.menu(for: event)
        let point = convert(event.locationInWindow, from: nil)
        return contextMenu?(row(at: point))
    }

    /// `.inVisibleRect` has AppKit rebuild tracking areas as the view scrolls,
    /// and that rebuild is the one signal a wheel scroll reliably produces, so
    /// the hovered row is recomputed from the pointer here.
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        syncHoveredRow()
    }

    func hoverEntered(_ rowView: NSTableRowView) {
        applyHoveredRow(rowIndex(of: rowView))
    }

    func hoverExited(_ rowView: NSTableRowView) {
        guard rowIndex(of: rowView) == hoveredRow else { return }
        applyHoveredRow(-1)
    }

    /// Reads the row under the pointer now, rather than trusting a value an
    /// earlier crossing left behind. This covers the two ways the row under
    /// the pointer changes without a crossing: a scroll, and a row view
    /// arriving on screen.
    func syncHoveredRow() {
        guard let window, window.isKeyWindow else {
            applyHoveredRow(-1)
            return
        }
        let point = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        applyHoveredRow(visibleRect.contains(point) ? row(at: point) : -1)
    }

    private func rowIndex(of target: NSTableRowView) -> Int {
        let visible = rows(in: visibleRect)
        guard visible.length > 0 else { return -1 }
        for candidate in visible.location..<(visible.location + visible.length)
        where rowView(atRow: candidate, makeIfNecessary: false) === target {
            return candidate
        }
        return -1
    }

    /// Sweeps every visible row rather than only the pair that changed, so a
    /// recycled or newly attached row view cannot keep a fill that belonged to
    /// the item it replaced.
    private func applyHoveredRow(_ newRow: Int) {
        hoveredRow = newRow
        let visible = rows(in: visibleRect)
        guard visible.length > 0 else { return }
        for candidate in visible.location..<(visible.location + visible.length) {
            guard let rowView = rowView(atRow: candidate, makeIfNecessary: false)
                as? WorkspaceOutlineRowView
            else { continue }
            let hovered = candidate == newRow
            if rowView.isHovered != hovered {
                rowView.isHovered = hovered
            }
        }
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 76 {
            onActivate?()
            return
        }
        if ShellMenuCommand.moveToTrash.shortcut.matches(event) {
            onTrash?()
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
    static let dragType = NSPasteboard.PasteboardType("dev.hide.explorer-path")

    let rootURL: URL
    let expandedPaths: Set<String>
    let selectedPath: String?
    let fontScale: CGFloat
    let operation: CoreExplorerOperation?
    let openFile: (URL) -> Void
    let updateExpandedPaths: ([String]) -> Void
    let fileOperations: WorkspaceFileOperations

    func makeCoordinator() -> Coordinator {
        Coordinator(
            openFile: openFile,
            updateExpandedPaths: updateExpandedPaths,
            fileOperations: fileOperations
        )
    }

    func makeNSView(context: Context) -> NSScrollView {
        Self.makeScrollView(coordinator: context.coordinator)
    }

    /// The outline and its scroll view, built the one way the app builds
    /// them, so a test can host the same view and drive it.
    static func makeScrollView(coordinator: Coordinator) -> NSScrollView {
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
        outline.delegate = coordinator
        outline.dataSource = coordinator
        outline.target = coordinator
        outline.action = #selector(Coordinator.activateClickedRow)
        outline.onActivate = { [weak coordinator] in
            coordinator?.activateSelection()
        }
        outline.onTrash = { [weak coordinator] in
            coordinator?.requestTrashOfSelection()
        }
        outline.contextMenu = { [weak coordinator] row in
            coordinator?.contextMenu(forRow: row)
        }
        // A drag moves within this tree only. The private type keeps Finder
        // and other apps from reading it as a file drop, and the empty mask
        // for non-local targets keeps the item from being copied out.
        outline.registerForDraggedTypes([Self.dragType])
        outline.setDraggingSourceOperationMask(.move, forLocal: true)
        outline.setDraggingSourceOperationMask([], forLocal: false)
        outline.setAccessibilityIdentifier("explorer-file-tree")

        let scroll = WorkspaceOutlineScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = NSColor(HideTheme.panel)
        coordinator.attach(outline: outline)
        return scroll
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        context.coordinator.openFile = openFile
        context.coordinator.updateExpandedPaths = updateExpandedPaths
        context.coordinator.fileOperations = fileOperations
        context.coordinator.apply(
            rootURL: rootURL,
            expandedPaths: expandedPaths,
            selectedPath: selectedPath,
            fontScale: fontScale,
            operation: operation
        )
    }

    /// The name field open on one row, and what closing it does.
    private struct InlineEdit {
        enum Mode {
            case create(kind: WorkspaceOutlineDraftKind, parent: WorkspaceOutlineNode)
            case rename
        }

        let mode: Mode
        /// The row carrying the field: the draft row for a creation, the
        /// item's own row for a rename.
        let node: WorkspaceOutlineNode
        /// What the field shows when its row is (re)configured: the current
        /// name for a rename, nothing for a draft.
        let text: String
        /// True from Enter until the core answers, so a second Enter and a
        /// focus change during the round trip change nothing.
        var pending = false
    }

    @MainActor
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate, NSTextFieldDelegate {
        var openFile: (URL) -> Void
        var updateExpandedPaths: ([String]) -> Void
        var fileOperations: WorkspaceFileOperations
        private weak var outline: WorkspaceNSOutlineView?
        private var inlineEdit: InlineEdit?
        /// A creation asked of a folder whose children are still loading;
        /// the draft row is inserted when the load lands.
        private var pendingDraft: (kind: WorkspaceOutlineDraftKind, parent: WorkspaceOutlineNode)?
        /// The failure row on screen, so exactly one can be up at a time.
        private var failureNode: WorkspaceOutlineNode?
        /// The last core operation this view acted on. Seeded from the first
        /// snapshot so a slot settled before the view existed is not replayed
        /// as a failure under a row.
        private var handledOperationID: UInt64??
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

        init(
            openFile: @escaping (URL) -> Void,
            updateExpandedPaths: @escaping ([String]) -> Void,
            fileOperations: WorkspaceFileOperations
        ) {
            self.openFile = openFile
            self.updateExpandedPaths = updateExpandedPaths
            self.fileOperations = fileOperations
        }

        fileprivate func attach(outline: WorkspaceNSOutlineView) {
            self.outline = outline
        }

        /// What each visible row shows, top to bottom, for tests that drive
        /// the hosted view: an entry's name, or the text a special row draws.
        var visibleRowNames: [String] {
            guard let outline else { return [] }
            return (0..<outline.numberOfRows).compactMap { row in
                (outline.item(atRow: row) as? WorkspaceOutlineNode).map { node in
                    switch node.role {
                    case .entry: node.name
                    case .placeholder(let message): "placeholder:\(message)"
                    case .draft: "draft"
                    case .failure(let message): "failure:\(message)"
                    }
                }
            }
        }

        var isEditingInline: Bool { inlineEdit != nil }

        /// The name field currently open, if any.
        var inlineEditor: NSTextField? { editorField() }

        func apply(
            rootURL: URL,
            expandedPaths: Set<String>,
            selectedPath: String?,
            fontScale: CGFloat,
            operation: CoreExplorerOperation?
        ) {
            desiredExpandedPaths = expandedPaths
            self.selectedPath = selectedPath
            self.fontScale = fontScale
            if handledOperationID == nil {
                handledOperationID = .some(operation?.id)
            }
            if rootPath != rootURL.path {
                rootGeneration &+= 1
                rootPath = rootURL.path
                rootNode = WorkspaceOutlineNode(entry: .init(url: rootURL, isDirectory: true))
                appliedSelectedPath = nil
                inlineEdit = nil
                pendingDraft = nil
                failureNode = nil
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
            observe(operation)
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
            guard let node = item as? WorkspaceOutlineNode else { return false }
            return node.isDirectory && node.isEntry
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

        /// A scroll brings rows under a pointer that never moved, so the row
        /// that just attached has to be asked whether it is the hovered one.
        func outlineView(
            _ outlineView: NSOutlineView,
            didAdd rowView: NSTableRowView,
            forRow row: Int
        ) {
            (outlineView as? WorkspaceNSOutlineView)?.syncHoveredRow()
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
            let editing = inlineEdit?.node === node
            label?.stringValue = editing ? (inlineEdit?.text ?? "") : node.name
            label?.font = HideTheme.nativeFont(size: HideTheme.Typography.body * fontScale)
            switch node.role {
            case .entry: label?.textColor = HideTheme.Native.primary
            case .draft: label?.textColor = HideTheme.Native.primary
            case .placeholder: label?.textColor = HideTheme.Native.muted
            case .failure: label?.textColor = HideTheme.Native.danger
            }
            // The cell is recycled, so the field is put back to a label on
            // every row that is not the one being edited.
            label?.isEditable = editing
            label?.isSelectable = editing
            label?.delegate = editing ? self : nil
            label?.drawsBackground = editing
            label?.backgroundColor = editing ? HideTheme.Native.elevated : .clear
            label?.placeholderString = editing ? "Name" : nil
            cell.setAccessibilityIdentifier(
                editing ? "workspace-item-editor" : "workspace-item-\(node.url.path)"
            )
            cell.setAccessibilityLabel(node.name)

            guard let iconView = cell.subviews.first(where: { $0.identifier?.rawValue == "icon" }) as? NSTextField else {
                return
            }
            if case .failure = node.role {
                iconView.stringValue = ""
            } else if case .placeholder = node.role {
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
                if let pendingDraft, pendingDraft.parent === node {
                    self.pendingDraft = nil
                    if case .loaded = node.state {
                        insertDraft(kind: pendingDraft.kind, in: node)
                    }
                }
            }
        }

        /// Re-reads a loaded folder and keeps every child that is still
        /// there, so the folders inside it stay expanded and loaded rather
        /// than being rebuilt from the persisted set one level at a time.
        private func refreshChildren(
            of node: WorkspaceOutlineNode,
            then completion: (@MainActor () -> Void)? = nil
        ) {
            guard node.isDirectory, let loadRoot = rootNode else { return }
            guard case .loaded = node.state else {
                if case .unloaded = node.state { loadChildren(of: node) }
                return
            }
            let loadGeneration = rootGeneration
            let url = node.url
            Task {
                let result = await Task.detached(priority: .userInitiated) {
                    Result { try WorkspaceDirectoryLoader.loadDirectory(at: url) }
                }.value
                guard loadGeneration == rootGeneration, loadRoot === rootNode else { return }
                switch result {
                case .success(let entries):
                    var existing: [String: WorkspaceOutlineNode] = [:]
                    for child in node.children where child.isEntry {
                        existing[child.url.path] = child
                    }
                    node.children = entries.map { entry in
                        if let kept = existing[entry.url.path], kept.isDirectory == entry.isDirectory {
                            return kept
                        }
                        return WorkspaceOutlineNode(entry: entry)
                    }
                case .failure(let error):
                    node.children = []
                    node.state = .failed("Could not read \(url.lastPathComponent): \(error.localizedDescription)")
                    Self.reportFailure(path: url.path, message: error.localizedDescription)
                }
                // The reload drops the selection of any row under the
                // folder; a kept node is the same object, so the row the
                // operator had is found again and stays selected.
                let selected = outline.flatMap { outline in
                    outline.selectedRow >= 0 ? outline.item(atRow: outline.selectedRow) as? WorkspaceOutlineNode : nil
                }
                outline?.reloadItem(node, reloadChildren: true)
                if let outline, let selected {
                    let row = outline.row(forItem: selected)
                    if row >= 0 { outline.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false) }
                }
                restoreVisibleState()
                completion?()
            }
        }

        /// The loaded node at `path`, or nil when any ancestor is not loaded.
        /// Nothing is loaded on the way: a folder the operator has not opened
        /// has no rows to refresh.
        private func loadedNode(at path: String) -> WorkspaceOutlineNode? {
            guard let rootNode else { return nil }
            var node = rootNode
            while node.url.path != path {
                guard node.isDirectory, case .loaded = node.state,
                      let child = node.children.first(where: {
                          $0.isEntry && (path == $0.url.path || path.hasPrefix($0.url.path + "/"))
                      })
                else { return nil }
                node = child
            }
            return node
        }

        // MARK: Context menu

        func contextMenu(forRow row: Int) -> NSMenu? {
            guard let outline, let rootNode else { return nil }
            let node = row >= 0 ? outline.item(atRow: row) as? WorkspaceOutlineNode : nil
            // The root row stands for the tree as the empty area does: it can
            // take a new item but is not itself renamed or moved.
            let target: WorkspaceOutlineMenuTarget
            switch node?.role {
            case .none:
                target = .emptyArea
            case .entry where node === rootNode:
                target = .emptyArea
            case .entry:
                target = .item(isDirectory: node?.isDirectory == true)
            case .placeholder, .draft, .failure:
                return nil
            }
            let subject = node ?? rootNode
            let menu = NSMenu()
            for item in WorkspaceOutlineMenuPresentation.items(for: target, isRemote: false) {
                switch item {
                case .separator:
                    menu.addItem(.separator())
                default:
                    let entry = NSMenuItem(title: item.title, action: selector(for: item), keyEquivalent: "")
                    entry.target = self
                    entry.representedObject = subject
                    if let command = item.command {
                        entry.keyEquivalent = command.shortcut.menuKeyEquivalent
                        entry.keyEquivalentModifierMask = command.shortcut.modifierFlags
                    }
                    menu.addItem(entry)
                }
            }
            return menu
        }

        private func selector(for item: WorkspaceOutlineMenuItem) -> Selector? {
            switch item {
            case .newFile: #selector(menuNewFile(_:))
            case .newFolder: #selector(menuNewFolder(_:))
            case .revealInFinder: #selector(menuReveal(_:))
            case .copyPath: #selector(menuCopyPath(_:))
            case .copyRelativePath: #selector(menuCopyRelativePath(_:))
            case .rename: #selector(menuRename(_:))
            case .delete: #selector(menuDelete(_:))
            case .separator: nil
            }
        }

        private func subject(of sender: Any?) -> WorkspaceOutlineNode? {
            (sender as? NSMenuItem)?.representedObject as? WorkspaceOutlineNode
        }

        /// The folder a creation goes into: the clicked folder, or the parent
        /// of the clicked file.
        private func creationParent(for node: WorkspaceOutlineNode) -> WorkspaceOutlineNode? {
            if node.isDirectory { return node }
            return loadedNode(at: WorkspaceOutlinePathPresentation.parentPath(node.url.path))
        }

        @objc private func menuNewFile(_ sender: Any?) {
            guard let node = subject(of: sender), let parent = creationParent(for: node) else { return }
            beginCreate(kind: .file, in: parent)
        }

        @objc private func menuNewFolder(_ sender: Any?) {
            guard let node = subject(of: sender), let parent = creationParent(for: node) else { return }
            beginCreate(kind: .folder, in: parent)
        }

        @objc private func menuReveal(_ sender: Any?) {
            guard let node = subject(of: sender) else { return }
            ExternalFileOpener.reveal(node.url)
        }

        @objc private func menuCopyPath(_ sender: Any?) {
            guard let node = subject(of: sender) else { return }
            copyToPasteboard(node.url.path)
        }

        @objc private func menuCopyRelativePath(_ sender: Any?) {
            guard let node = subject(of: sender), let rootPath else { return }
            copyToPasteboard(WorkspaceOutlinePathPresentation.relativePath(node.url.path, root: rootPath))
        }

        @objc private func menuRename(_ sender: Any?) {
            guard let node = subject(of: sender), node !== rootNode else { return }
            beginRename(node)
        }

        @objc private func menuDelete(_ sender: Any?) {
            guard let node = subject(of: sender) else { return }
            requestTrash(node)
        }

        /// ⌘⌫ with the tree focused: the selected row, if it is an item.
        /// The root row, a placeholder, a draft and the failure line are not
        /// items, and no selection is nothing to ask about (D-01).
        func requestTrashOfSelection() {
            guard let outline, outline.selectedRow >= 0,
                  let node = outline.item(atRow: outline.selectedRow) as? WorkspaceOutlineNode
            else { return }
            requestTrash(node)
        }

        // MARK: Trash

        /// Hands the shell the prompt for the modal. Nothing is sent to the
        /// core from here: the shell dispatches only when the modal's
        /// destructive button is pressed (D-03). The successor row is
        /// decided now, from the parent's rows as they stand, so the
        /// selection has somewhere to go the moment the item is gone (D-05).
        private func requestTrash(_ node: WorkspaceOutlineNode) {
            guard node.isEntry, node !== rootNode, let rootPath,
                  let parent = loadedNode(at: WorkspaceOutlinePathPresentation.parentPath(node.url.path))
            else { return }
            cancelInlineEdit()
            clearFailure()
            let siblings = parent.children.filter(\.isEntry).map(\.url.path)
            let selectAfter = WorkspaceOutlineSelectionPolicy.selectionAfterRemoving(
                node.url.path, from: siblings, parent: parent.url.path
            )
            fileOperations.requestTrash(WorkspaceOutlineTrashPrompt(
                root: URL(fileURLWithPath: rootPath, isDirectory: true),
                path: node.url,
                isDirectory: node.isDirectory,
                selectAfter: URL(fileURLWithPath: selectAfter)
            ))
        }

        private func copyToPasteboard(_ string: String) {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(string, forType: .string)
        }

        // MARK: Inline editing

        private func beginCreate(kind: WorkspaceOutlineDraftKind, in parent: WorkspaceOutlineNode) {
            guard let outline else { return }
            cancelInlineEdit()
            clearFailure()
            if !outline.isItemExpanded(parent) {
                outline.expandItem(parent)
            }
            switch parent.state {
            case .loaded:
                insertDraft(kind: kind, in: parent)
            case .unloaded, .loading:
                pendingDraft = (kind, parent)
                loadChildren(of: parent)
            case .failed:
                return
            }
        }

        /// Rows the tree adds among a folder's children - the draft and the
        /// failure line - go in and out with `insertItems`/`removeItems`
        /// rather than a reload of the folder, because a reload reconfigures
        /// every sibling row and ends the field editor on the row being
        /// typed in.
        private func insert(_ node: WorkspaceOutlineNode, at index: Int, in parent: WorkspaceOutlineNode) {
            parent.children.insert(node, at: index)
            outline?.insertItems(at: IndexSet(integer: index), inParent: parent, withAnimation: [])
        }

        private func remove(_ node: WorkspaceOutlineNode, from parent: WorkspaceOutlineNode) {
            guard let index = parent.children.firstIndex(where: { $0 === node }) else { return }
            parent.children.remove(at: index)
            outline?.removeItems(at: IndexSet(integer: index), inParent: parent, withAnimation: [])
        }

        private func insertDraft(kind: WorkspaceOutlineDraftKind, in parent: WorkspaceOutlineNode) {
            let draft = WorkspaceOutlineNode(draft: kind, parentURL: parent.url)
            inlineEdit = InlineEdit(mode: .create(kind: kind, parent: parent), node: draft, text: "")
            insert(draft, at: 0, in: parent)
            focusEditor(on: draft, selectStemOnly: false)
        }

        private func beginRename(_ node: WorkspaceOutlineNode) {
            cancelInlineEdit()
            clearFailure()
            inlineEdit = InlineEdit(mode: .rename, node: node, text: node.name)
            outline?.reloadItem(node, reloadChildren: false)
            focusEditor(on: node, selectStemOnly: !node.isDirectory)
        }

        private func focusEditor(on node: WorkspaceOutlineNode, selectStemOnly: Bool) {
            guard let outline else { return }
            let row = outline.row(forItem: node)
            guard row >= 0 else { return }
            outline.scrollRowToVisible(row)
            guard let cell = outline.view(atColumn: 0, row: row, makeIfNecessary: true) as? NSTableCellView,
                  let field = cell.textField, let window = outline.window
            else { return }
            window.makeFirstResponder(field)
            if selectStemOnly, let editor = field.currentEditor() {
                let stem = (node.name as NSString).deletingPathExtension
                editor.selectedRange = NSRange(location: 0, length: (stem as NSString).length)
            }
        }

        /// Ends the field without changing anything: the draft row leaves,
        /// a renamed row shows its name again.
        func cancelInlineEdit() {
            guard let edit = inlineEdit else { return }
            inlineEdit = nil
            pendingDraft = nil
            clearFailure()
            switch edit.mode {
            case .create(_, let parent):
                remove(edit.node, from: parent)
            case .rename:
                outline?.reloadItem(edit.node, reloadChildren: false)
            }
            if let outline, outline.window?.firstResponder !== outline {
                outline.window?.makeFirstResponder(outline)
            }
        }

        private func commitInlineEdit(name: String) {
            guard var edit = inlineEdit, !edit.pending else { return }
            let parent: WorkspaceOutlineNode?
            let current: String?
            switch edit.mode {
            case .create(_, let creationParent):
                parent = creationParent
                current = nil
            case .rename:
                parent = loadedNode(at: WorkspaceOutlinePathPresentation.parentPath(edit.node.url.path))
                current = edit.node.name
            }
            let siblings = Set((parent?.children ?? []).filter(\.isEntry).map(\.name))
            switch WorkspaceOutlineNamePolicy.verdict(name: name, siblings: siblings, current: current) {
            case .unchanged:
                cancelInlineEdit()
                return
            case .rejected(let reason):
                showFailure(reason, under: edit.node)
                return
            case .accepted:
                break
            }
            clearFailure()
            edit.pending = true
            inlineEdit = edit
            switch edit.mode {
            case .create(let kind, let creationParent):
                switch kind {
                case .file: fileOperations.createFile(creationParent.url, name)
                case .folder: fileOperations.createDirectory(creationParent.url, name)
                }
            case .rename:
                fileOperations.rename(edit.node.url, name)
            }
        }

        private func editorField() -> NSTextField? {
            guard let outline, let edit = inlineEdit else { return nil }
            let row = outline.row(forItem: edit.node)
            guard row >= 0 else { return nil }
            return (outline.view(atColumn: 0, row: row, makeIfNecessary: false) as? NSTableCellView)?.textField
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            switch commandSelector {
            case #selector(NSResponder.insertNewline(_:)):
                commitInlineEdit(name: textView.string)
                return true
            case #selector(NSResponder.cancelOperation(_:)):
                cancelInlineEdit()
                return true
            default:
                return false
            }
        }

        /// Leaving the field any other way - a click elsewhere, a tab switch -
        /// is a cancel. A commit already in flight keeps its row until the
        /// core answers.
        func controlTextDidEndEditing(_ notification: Notification) {
            guard let edit = inlineEdit, !edit.pending else { return }
            cancelInlineEdit()
        }

        // MARK: Failure line

        private func showFailure(_ message: String, under node: WorkspaceOutlineNode) {
            clearFailure()
            let parentPath = WorkspaceOutlinePathPresentation.parentPath(node.url.path)
            guard let parent = loadedNode(at: parentPath) ?? (node === inlineEdit?.node ? draftParent() : nil),
                  let index = parent.children.firstIndex(where: { $0 === node })
            else { return }
            let failure = WorkspaceOutlineNode(failure: message, under: node.url)
            failureNode = failure
            insert(failure, at: index + 1, in: parent)
            if node === inlineEdit?.node, let field = editorField(), let window = outline?.window,
               window.firstResponder !== field.currentEditor() {
                window.makeFirstResponder(field)
            }
        }

        private func draftParent() -> WorkspaceOutlineNode? {
            guard let edit = inlineEdit, case .create(_, let parent) = edit.mode else { return nil }
            return parent
        }

        private func clearFailure() {
            guard let failureNode else { return }
            self.failureNode = nil
            let parentPath = WorkspaceOutlinePathPresentation.parentPath(
                WorkspaceOutlinePathPresentation.parentPath(failureNode.url.path)
            )
            guard let parent = loadedNode(at: parentPath) ?? draftParent() else { return }
            remove(failureNode, from: parent)
        }

        // MARK: Core results

        /// Acts once on each settled core operation: a finished change
        /// reloads the folders it touched and closes the field; a failed one
        /// keeps the field and says why under it, or under the moved row.
        private func observe(_ operation: CoreExplorerOperation?) {
            guard let operation, operation.isSettled,
                  handledOperationID != .some(operation.id)
            else { return }
            handledOperationID = .some(operation.id)
            switch operation.phase {
            case "finished":
                if let edit = inlineEdit, edit.pending {
                    inlineEdit = nil
                    clearFailure()
                    if case .create(_, let parent) = edit.mode {
                        remove(edit.node, from: parent)
                    }
                    if let outline, outline.window?.firstResponder !== outline {
                        outline.window?.makeFirstResponder(outline)
                    }
                }
                var parents = [WorkspaceOutlinePathPresentation.parentPath(operation.path)]
                let destinationParent = WorkspaceOutlinePathPresentation.parentPath(operation.destination)
                if destinationParent != parents[0] { parents.append(destinationParent) }
                // D-12: a created file that could not open an editor tab still
                // exists, so the reason rides the finished slot and shows as
                // the same one-line failure under the created row (B10). The
                // row appears only after the destination folder reloads, so
                // the reason waits for that reload to land.
                let openFailure = operation.message
                let destination = operation.destination
                for path in parents {
                    guard let node = loadedNode(at: path) else { continue }
                    if path == destinationParent, let message = openFailure {
                        refreshChildren(of: node) { [weak self] in
                            guard let self, let created = self.loadedNode(at: destination) else { return }
                            self.showFailure(message, under: created)
                        }
                    } else {
                        refreshChildren(of: node)
                    }
                }
            case "failed":
                let message = operation.message ?? "The change was not applied"
                if var edit = inlineEdit, edit.pending {
                    edit.pending = false
                    inlineEdit = edit
                    showFailure(message, under: edit.node)
                } else if let node = loadedNode(at: operation.path) {
                    showFailure(message, under: node)
                }
            default:
                break
            }
        }

        // MARK: Drag and drop

        func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> NSPasteboardWriting? {
            guard let node = item as? WorkspaceOutlineNode, node.isEntry, node !== rootNode,
                  inlineEdit == nil
            else { return nil }
            let writer = NSPasteboardItem()
            writer.setString(node.url.path, forType: WorkspaceOutlineView.dragType)
            return writer
        }

        private func draggedPath(_ info: NSDraggingInfo) -> String? {
            guard info.draggingSource as? NSOutlineView === outline else { return nil }
            return info.draggingPasteboard.string(forType: WorkspaceOutlineView.dragType)
        }

        /// Where the drop would deliver the item, retargeted onto the folder
        /// row that receives it so the highlight says what will happen.
        private func dropDestination(
            _ info: NSDraggingInfo,
            proposedItem item: Any?
        ) -> (path: String, node: WorkspaceOutlineNode?)? {
            guard let source = draggedPath(info), let rootPath else { return nil }
            let target: WorkspaceOutlineDropPolicy.Target?
            if let node = item as? WorkspaceOutlineNode {
                guard node.isEntry else { return nil }
                target = .init(path: node.url.path, isDirectory: node.isDirectory)
            } else {
                target = nil
            }
            guard let destination = WorkspaceOutlineDropPolicy.destinationDirectory(
                source: source, over: target, root: rootPath
            ) else { return nil }
            return (destination, loadedNode(at: destination))
        }

        func outlineView(
            _ outlineView: NSOutlineView,
            validateDrop info: NSDraggingInfo,
            proposedItem item: Any?,
            proposedChildIndex index: Int
        ) -> NSDragOperation {
            guard let destination = dropDestination(info, proposedItem: item) else { return [] }
            outlineView.setDropItem(destination.node, dropChildIndex: NSOutlineViewDropOnItemIndex)
            return .move
        }

        func outlineView(
            _ outlineView: NSOutlineView,
            acceptDrop info: NSDraggingInfo,
            item: Any?,
            childIndex index: Int
        ) -> Bool {
            guard let source = draggedPath(info),
                  let destination = dropDestination(info, proposedItem: item)
            else { return false }
            clearFailure()
            fileOperations.move(
                URL(fileURLWithPath: source),
                URL(fileURLWithPath: destination.path, isDirectory: true)
            )
            return true
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
        /// and nil while a load is still in flight. The target itself is not
        /// expanded: a folder selected as the cursor's landing place after a
        /// removal, or as a new item, is highlighted, not opened.
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
                $0.isEntry && ($0.url.path == path || path.hasPrefix($0.url.path + "/"))
            }) else { return nil }
            if child.isDirectory, child.url.path != path, !outline.isItemExpanded(child) {
                outline.expandItem(child)
            }
            return revealNode(path: path, from: child, in: outline)
        }

        private func expandRecordedDescendants(of node: WorkspaceOutlineNode, in outline: NSOutlineView) {
            if node === rootNode || desiredExpandedPaths.contains(node.url.path) {
                outline.expandItem(node)
                loadChildren(of: node)
            }
            for child in node.children where child.isDirectory && child.isEntry {
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
            guard node.isDirectory, node.isEntry else { return }
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
