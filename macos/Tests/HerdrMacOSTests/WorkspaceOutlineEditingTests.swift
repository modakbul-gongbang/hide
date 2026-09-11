import AppKit
import Foundation
import Testing
@testable import HerdrMacOS

/// Drives the hosted outline through the flows the operator performs: the
/// menu on a row, the draft row a creation opens, Enter and Esc in the
/// field, a refused name, and the core's settled slot arriving afterwards.
/// These run against real AppKit row insertion and first-responder changes,
/// because a wrong item count or a field editor ending early is what an
/// `NSOutlineView` fails on, not what a pure value test can see.
@Suite(.serialized) @MainActor struct WorkspaceOutlineEditingTests {
    final class Calls {
        var created: [(URL, String)] = []
        var createdDirectories: [(URL, String)] = []
        var renamed: [(URL, String)] = []
        var moved: [(URL, URL)] = []
    }

    @MainActor struct Host {
        let root: URL
        let window: NSWindow
        let coordinator: WorkspaceOutlineView.Coordinator
        let calls: Calls

        var outline: WorkspaceNSOutlineView {
            (window.contentView as! NSScrollView).documentView as! WorkspaceNSOutlineView
        }

        func apply(operation: CoreExplorerOperation? = nil, expanded: Set<String> = [], selected: String? = nil) {
            coordinator.apply(
                rootURL: root,
                expandedPaths: expanded,
                selectedPath: selected,
                fontScale: 1,
                operation: operation
            )
        }

        /// Loads land on a later main-actor turn; wait for the rows to say so.
        func settle(until condition: () -> Bool) async throws {
            for _ in 0..<200 where !condition() {
                try await Task.sleep(for: .milliseconds(10))
            }
            #expect(condition(), "rows: \(coordinator.visibleRowNames)")
        }

        func menu(forRowNamed name: String) -> NSMenu? {
            guard let row = coordinator.visibleRowNames.firstIndex(of: name) else { return nil }
            return coordinator.contextMenu(forRow: row)
        }

        func choose(_ title: String, in menu: NSMenu) {
            let index = menu.items.firstIndex { $0.title == title }!
            menu.performActionForItem(at: index)
        }

        func type(_ text: String) -> NSTextView {
            let field = coordinator.inlineEditor!
            #expect(window.firstResponder === field.currentEditor(), "the field editor holds the keyboard")
            let editor = field.currentEditor() as! NSTextView
            editor.string = text
            return editor
        }

        func press(_ command: Selector) {
            let field = coordinator.inlineEditor!
            let editor = field.currentEditor() as! NSTextView
            _ = coordinator.control(field, textView: editor, doCommandBy: command)
        }

        /// The outline holds its delegate and data source unretained, so
        /// they are detached before the coordinator can go away with the
        /// host; a deferred AppKit callback into a freed coordinator is a
        /// crash the next test would take the blame for.
        func tearDown() {
            window.endEditing(for: nil)
            if let scroll = window.contentView as? NSScrollView,
               let outline = scroll.documentView as? NSOutlineView {
                outline.delegate = nil
                outline.dataSource = nil
            }
            window.contentView = nil
            window.close()
            try? FileManager.default.removeItem(at: root)
        }
    }

    @MainActor static func makeHost() async throws -> Host {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-outline-editing-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("src"), withIntermediateDirectories: true)
        #expect(FileManager.default.createFile(atPath: root.appendingPathComponent("README.md").path, contents: Data()))
        #expect(FileManager.default.createFile(atPath: root.appendingPathComponent("src/lib.rs").path, contents: Data()))
        let calls = Calls()
        let coordinator = WorkspaceOutlineView.Coordinator(
            openFile: { _ in },
            updateExpandedPaths: { _ in },
            fileOperations: WorkspaceFileOperations(
                createFile: { calls.created.append(($0, $1)) },
                createDirectory: { calls.createdDirectories.append(($0, $1)) },
                rename: { calls.renamed.append(($0, $1)) },
                move: { calls.moved.append(($0, $1)) }
            )
        )
        let scroll = WorkspaceOutlineView.makeScrollView(coordinator: coordinator)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 400),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        // A programmatic window releases itself on close; the Swift
        // reference would then release it a second time.
        window.isReleasedWhenClosed = false
        window.contentView = scroll
        let host = Host(root: root, window: window, coordinator: coordinator, calls: calls)
        host.apply()
        try await host.settle { coordinator.visibleRowNames == [root.lastPathComponent, "src", "README.md"] }
        return host
    }

    @Test func rowMenuFollowsTheTargetAndTheEmptyAreaOffersOnlyCreation() async throws {
        let host = try await Self.makeHost()
        defer { host.tearDown() }

        let rowMenu = try #require(host.menu(forRowNamed: "README.md"))
        #expect(
            rowMenu.items.map { $0.isSeparatorItem ? "-" : $0.title }
                == ["New File", "New Folder", "-", "Reveal in Finder", "Copy Path", "Copy Relative Path", "-", "Rename"]
        )
        let emptyArea = try #require(host.coordinator.contextMenu(forRow: -1))
        #expect(emptyArea.items.map(\.title) == ["New File", "New Folder"])
        let rootRow = try #require(host.coordinator.contextMenu(forRow: 0))
        #expect(rootRow.items.map(\.title) == ["New File", "New Folder"])

        host.choose("Copy Relative Path", in: try #require(host.menu(forRowNamed: "src")))
        #expect(NSPasteboard.general.string(forType: .string) == "src")
        host.choose("Copy Path", in: try #require(host.menu(forRowNamed: "README.md")))
        #expect(NSPasteboard.general.string(forType: .string) == host.root.appendingPathComponent("README.md").path)
    }

    @Test func newFileOpensADraftRowAndEnterSendsTheNameToTheCore() async throws {
        let host = try await Self.makeHost()
        defer { host.tearDown() }

        // New File on a file targets its parent, here the root.
        host.choose("New File", in: try #require(host.menu(forRowNamed: "README.md")))
        #expect(host.coordinator.visibleRowNames == [host.root.lastPathComponent, "draft", "src", "README.md"])
        #expect(host.coordinator.isEditingInline)

        let editor = host.type("notes.md")
        // Return goes through the field editor, as a key press does, so the
        // delegate wiring itself is what is exercised here.
        editor.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        #expect(host.calls.created.map(\.1) == ["notes.md"])
        #expect(host.calls.created.first?.0 == host.root)
        // The row waits for the core; a second Enter sends nothing more.
        host.press(#selector(NSResponder.insertNewline(_:)))
        #expect(host.calls.created.count == 1)

        // The core answers: the file is on disk and the slot is finished.
        let created = host.root.appendingPathComponent("notes.md")
        #expect(FileManager.default.createFile(atPath: created.path, contents: Data()))
        host.apply(
            operation: CoreExplorerOperation(
                id: 1, kind: "file_create", phase: "finished", path: created.path, destination: created.path, message: nil
            ),
            selected: created.path
        )
        try await host.settle { host.coordinator.visibleRowNames == [host.root.lastPathComponent, "src", "notes.md", "README.md"] }
        #expect(!host.coordinator.isEditingInline)
        #expect(host.outline.selectedRow == 2)
        #expect(host.window.firstResponder === host.outline)
    }

    /// D-12, B10: the core created the file but could not open its editor tab.
    /// The finished slot carries the reason, so the file stays in the tree and
    /// the one-line reason shows under its row.
    @Test func aCreatedFileThatCannotOpenKeepsTheFileAndShowsTheReasonUnderIt() async throws {
        let host = try await Self.makeHost()
        defer { host.tearDown() }

        let created = host.root.appendingPathComponent("notes.md")
        #expect(FileManager.default.createFile(atPath: created.path, contents: Data()))
        host.apply(
            operation: CoreExplorerOperation(
                id: 1, kind: "file_create", phase: "finished",
                path: created.path, destination: created.path,
                message: "notes.md could not be opened"
            ),
            selected: created.path
        )
        try await host.settle {
            host.coordinator.visibleRowNames
                == [
                    host.root.lastPathComponent, "src", "notes.md",
                    "failure:notes.md could not be opened", "README.md",
                ]
        }
    }

    @Test func refusedNamesKeepTheFieldWithTheReasonUnderItAndEscapeDropsTheDraft() async throws {
        let host = try await Self.makeHost()
        defer { host.tearDown() }
        host.choose("New Folder", in: try #require(host.menu(forRowNamed: "src")))
        try await host.settle { host.coordinator.visibleRowNames.contains("draft") }
        #expect(host.coordinator.visibleRowNames == [host.root.lastPathComponent, "src", "draft", "lib.rs", "README.md"])

        _ = host.type("lib.rs")
        host.press(#selector(NSResponder.insertNewline(_:)))
        #expect(host.calls.createdDirectories.isEmpty)
        #expect(
            host.coordinator.visibleRowNames
                == [host.root.lastPathComponent, "src", "draft", "failure:lib.rs already exists here", "lib.rs", "README.md"]
        )
        #expect(host.coordinator.isEditingInline)
        #expect(host.window.firstResponder === host.coordinator.inlineEditor?.currentEditor())

        _ = host.type("a/b")
        host.press(#selector(NSResponder.insertNewline(_:)))
        #expect(host.coordinator.visibleRowNames.contains("failure:A name cannot contain /"))
        #expect(!host.coordinator.visibleRowNames.contains("failure:lib.rs already exists here"))

        host.press(#selector(NSResponder.cancelOperation(_:)))
        #expect(host.coordinator.visibleRowNames == [host.root.lastPathComponent, "src", "lib.rs", "README.md"])
        #expect(!host.coordinator.isEditingInline)
        #expect(host.calls.createdDirectories.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: host.root.appendingPathComponent("src/lib.rs/anything").path))
    }

    @Test func renameEditsTheRowInPlaceAndACoreRefusalStaysUnderIt() async throws {
        let host = try await Self.makeHost()
        defer { host.tearDown() }
        host.choose("Rename", in: try #require(host.menu(forRowNamed: "README.md")))
        #expect(host.coordinator.isEditingInline)
        let editor = host.type("README.md")
        // An unchanged name closes the field and asks nothing.
        editor.string = "README.md"
        host.press(#selector(NSResponder.insertNewline(_:)))
        #expect(host.calls.renamed.isEmpty)
        #expect(!host.coordinator.isEditingInline)

        host.choose("Rename", in: try #require(host.menu(forRowNamed: "README.md")))
        _ = host.type("GUIDE.md")
        host.press(#selector(NSResponder.insertNewline(_:)))
        #expect(host.calls.renamed.map(\.1) == ["GUIDE.md"])
        #expect(host.calls.renamed.first?.0 == host.root.appendingPathComponent("README.md"))

        host.apply(operation: CoreExplorerOperation(
            id: 2, kind: "path_rename", phase: "failed",
            path: host.root.appendingPathComponent("README.md").path,
            destination: host.root.appendingPathComponent("GUIDE.md").path,
            message: "README.md is not writable"
        ))
        #expect(
            host.coordinator.visibleRowNames
                == [host.root.lastPathComponent, "src", "README.md", "failure:README.md is not writable"]
        )
        #expect(host.coordinator.isEditingInline)

        host.press(#selector(NSResponder.cancelOperation(_:)))
        #expect(host.coordinator.visibleRowNames == [host.root.lastPathComponent, "src", "README.md"])
        #expect(!host.coordinator.isEditingInline)
    }

    @Test func aSlotSettledBeforeTheViewExistedIsNotReplayed() async throws {
        let host = try await Self.makeHost()
        defer { host.tearDown() }
        let coordinator = WorkspaceOutlineView.Coordinator(
            openFile: { _ in }, updateExpandedPaths: { _ in },
            fileOperations: WorkspaceFileOperations(
                createFile: { _, _ in }, createDirectory: { _, _ in }, rename: { _, _ in }, move: { _, _ in }
            )
        )
        host.outline.delegate = nil
        host.outline.dataSource = nil
        let scroll = WorkspaceOutlineView.makeScrollView(coordinator: coordinator)
        host.window.contentView = scroll
        let stale = CoreExplorerOperation(
            id: 9, kind: "path_move", phase: "failed",
            path: host.root.appendingPathComponent("README.md").path,
            destination: host.root.appendingPathComponent("src/README.md").path,
            message: "old news"
        )
        coordinator.apply(rootURL: host.root, expandedPaths: [], selectedPath: nil, fontScale: 1, operation: stale)
        for _ in 0..<200 where coordinator.visibleRowNames.count < 3 {
            try await Task.sleep(for: .milliseconds(10))
        }
        coordinator.apply(rootURL: host.root, expandedPaths: [], selectedPath: nil, fontScale: 1, operation: stale)
        #expect(coordinator.visibleRowNames == [host.root.lastPathComponent, "src", "README.md"])

        let fresh = CoreExplorerOperation(
            id: 10, kind: "path_move", phase: "failed", path: stale.path, destination: stale.destination, message: "new failure"
        )
        coordinator.apply(rootURL: host.root, expandedPaths: [], selectedPath: nil, fontScale: 1, operation: fresh)
        #expect(coordinator.visibleRowNames == [host.root.lastPathComponent, "src", "README.md", "failure:new failure"])
    }
}
