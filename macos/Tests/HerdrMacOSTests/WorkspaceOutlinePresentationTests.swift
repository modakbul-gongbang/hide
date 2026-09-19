import Foundation
import Testing
@testable import HerdrMacOS

/// The explorer's file-management decisions, taken away from the view so
/// each can be asked directly: which menu a click gets, whether a typed name
/// may be sent, what a path is relative to the root, and where a drop lands.
@Suite struct WorkspaceOutlinePresentationTests {
    @Test func gitDecorationsUseTheFullChangedSetAndKeepTheHighestRiskState() {
        let decorations = WorkspaceGitDecorations(
            rootPath: "/repo",
            entries: [
                CoreChangedFile(
                    path: "/repo/src/old.swift",
                    relativePath: "src/old.swift",
                    status: .deleted
                ),
                CoreChangedFile(
                    path: "/repo/src/new name.swift",
                    relativePath: "src/new name.swift",
                    previousRelativePath: "src/old name.swift",
                    status: .renamed
                ),
                CoreChangedFile(
                    path: "/repo/src/conflicted.swift",
                    relativePath: "src/conflicted.swift",
                    status: .conflict
                ),
            ]
        )
        #expect(decorations.decoration(for: "/repo/src", isDirectory: true)?.status == .conflict)
        #expect(decorations.decoration(for: "/repo/src/new name.swift", isDirectory: false)?.badge == "R")
        #expect(decorations.decoration(for: "/repo/README.md", isDirectory: false) == nil)
    }

    @Test func deletedDescendantsMarkAnExistingFolderWithoutInventingADeletedRow() {
        let decorations = WorkspaceGitDecorations(
            rootPath: "/repo",
            entries: [
                CoreChangedFile(
                    path: "/repo/removed/file.swift",
                    relativePath: "removed/file.swift",
                    status: .deleted
                ),
            ]
        )
        let folder = decorations.decoration(for: "/repo/removed", isDirectory: true)
        #expect(folder?.badge == "●")
        #expect(folder?.status == .deleted)
        #expect(decorations.decoration(for: "/repo/removed/file.swift", isDirectory: false)?.badge == "D")
    }

    /// B5, D-04: a file row carries the two open items between creation and
    /// reveal; a folder row, the empty area and a remote tree do not.
    @Test func localRowsGetTheFullMenuInVSCodeOrderAndTheEmptyAreaOnlyCreates() {
        let folder: [WorkspaceOutlineMenuItem] = [
            .newFile, .newFolder,
            .separator,
            .revealInFinder, .copyPath, .copyRelativePath,
            .separator,
            .rename,
            .separator,
            .delete,
        ]
        let file: [WorkspaceOutlineMenuItem] = [
            .newFile, .newFolder,
            .separator,
            .openWithDefaultApp, .openInBrowserPane,
            .separator,
            .revealInFinder, .copyPath, .copyRelativePath,
            .separator,
            .rename,
            .separator,
            .delete,
        ]
        #expect(WorkspaceOutlineMenuPresentation.items(for: .item(isDirectory: true), isRemote: false) == folder)
        #expect(WorkspaceOutlineMenuPresentation.items(for: .item(isDirectory: false), isRemote: false) == file)
        #expect(WorkspaceOutlineMenuPresentation.items(for: .emptyArea, isRemote: false) == [.newFile, .newFolder])
        #expect(WorkspaceOutlineMenuItem.openWithDefaultApp.title == "Open with Default App")
        #expect(WorkspaceOutlineMenuItem.openInBrowserPane.title == "Open in Browser Pane")
        #expect(!WorkspaceOutlineMenuPresentation.items(for: .item(isDirectory: false), isRemote: true).contains(.openInBrowserPane))
    }

    /// B12, B14, D-08: one reason at a time, in the order the operator can
    /// act on it, and none once every condition holds.
    @Test func browserPaneItemGivesOneReasonAtATime() {
        typealias Conditions = WorkspaceOutlineMenuPresentation.BrowserPaneConditions
        let ready = Conditions(isRemote: false, nodeOnPath: true, herdrConnected: true, hasFocusedPane: true, opening: false)
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(ready) == .available)
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(ready).reason == nil)

        var conditions = ready
        conditions.nodeOnPath = false
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(conditions) == .unavailable("Node.js is not on PATH"))
        conditions = ready
        conditions.herdrConnected = false
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(conditions) == .unavailable("Not connected to Herdr"))
        conditions = ready
        conditions.hasFocusedPane = false
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(conditions) == .unavailable("No focused pane to open beside"))
        conditions = ready
        conditions.opening = true
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(conditions) == .unavailable("Opening…"))
        conditions = ready
        conditions.isRemote = true
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(conditions) == .unavailable("Remote files open on their device"))

        // Several reasons at once still read as one: the in-flight request
        // outranks a missing tool, because it resolves on its own.
        conditions = Conditions(isRemote: false, nodeOnPath: false, herdrConnected: false, hasFocusedPane: false, opening: true)
        #expect(WorkspaceOutlineMenuPresentation.browserPaneAvailability(conditions) == .unavailable("Opening…"))
    }

    @Test func remoteRowsOfferOnlyTheTwoCopies() {
        #expect(
            WorkspaceOutlineMenuPresentation.items(for: .item(isDirectory: false), isRemote: true)
                == [.copyPath, .copyRelativePath]
        )
        #expect(
            WorkspaceOutlineMenuPresentation.items(for: .emptyArea, isRemote: true)
                == [.copyPath, .copyRelativePath]
        )
        #expect(WorkspaceOutlineMenuItem.copyRelativePath.title == "Copy Relative Path")
    }

    /// B1, D-04: Delete is the last item, alone behind its separator, and it
    /// is the one item that carries the catalog chord; a remote tree has no
    /// Delete at all.
    @Test func deleteIsLastAloneBehindItsSeparatorAndCarriesTheTrashChord() {
        let local = WorkspaceOutlineMenuPresentation.items(for: .item(isDirectory: false), isRemote: false)
        #expect(local.suffix(2) == [.separator, .delete])
        #expect(WorkspaceOutlineMenuItem.delete.title == "Delete")
        #expect(WorkspaceOutlineMenuItem.delete.command == .moveToTrash)
        #expect(local.filter { $0.command != nil } == [.delete])
        #expect(!WorkspaceOutlineMenuPresentation.items(for: .item(isDirectory: true), isRemote: true).contains(.delete))
        #expect(!WorkspaceOutlineMenuPresentation.items(for: .emptyArea, isRemote: false).contains(.delete))
    }

    /// B3, D-03: the title names the item, a folder is told its contents go
    /// too, and both say where the item can be restored from.
    @Test func trashPromptNamesTheItemAndWarnsAboutAFoldersContents() {
        let root = URL(fileURLWithPath: "/repo", isDirectory: true)
        let file = WorkspaceOutlineTrashPrompt(
            root: root, path: root.appendingPathComponent("src/lib.rs"), isDirectory: false,
            selectAfter: root.appendingPathComponent("src/main.rs"), inode: 42
        )
        #expect(file.title == "Move 'lib.rs' to Trash?")
        #expect(file.message == "You can restore it from Finder.")
        let folder = WorkspaceOutlineTrashPrompt(
            root: root, path: root.appendingPathComponent("src"), isDirectory: true,
            selectAfter: root, inode: nil
        )
        #expect(folder.title == "Move 'src' to Trash?")
        #expect(folder.message == "This folder and everything in it will move to the Trash. You can restore it from Finder.")
        #expect(WorkspaceOutlineTrashPrompt.confirmTitle == "Move to Trash")
        #expect(file.id == "/repo/src/lib.rs")
    }

    /// B4, D-05: the next sibling, else the previous, else the parent.
    @Test func selectionAfterRemovalPrefersTheNextSiblingThenThePreviousThenTheParent() {
        let siblings = ["/repo/src/a.rs", "/repo/src/b.rs", "/repo/src/c.rs"]
        #expect(
            WorkspaceOutlineSelectionPolicy.selectionAfterRemoving("/repo/src/a.rs", from: siblings, parent: "/repo/src")
                == "/repo/src/b.rs"
        )
        #expect(
            WorkspaceOutlineSelectionPolicy.selectionAfterRemoving("/repo/src/b.rs", from: siblings, parent: "/repo/src")
                == "/repo/src/c.rs"
        )
        #expect(
            WorkspaceOutlineSelectionPolicy.selectionAfterRemoving("/repo/src/c.rs", from: siblings, parent: "/repo/src")
                == "/repo/src/b.rs"
        )
        #expect(
            WorkspaceOutlineSelectionPolicy.selectionAfterRemoving("/repo/src/only.rs", from: ["/repo/src/only.rs"], parent: "/repo/src")
                == "/repo/src"
        )
        #expect(
            WorkspaceOutlineSelectionPolicy.selectionAfterRemoving("/repo/src/gone.rs", from: siblings, parent: "/repo/src")
                == "/repo/src"
        )
    }

    @Test func nameVerdictRefusesEmptySlashDotsAndSiblingsAndReportsAnUnchangedRename() {
        let siblings: Set<String> = ["README.md", "src"]
        #expect(WorkspaceOutlineNamePolicy.verdict(name: "notes.md", siblings: siblings) == .accepted)
        #expect(WorkspaceOutlineNamePolicy.verdict(name: "", siblings: siblings) == .rejected("A name is required"))
        #expect(WorkspaceOutlineNamePolicy.verdict(name: "a/b", siblings: siblings) == .rejected("A name cannot contain /"))
        #expect(WorkspaceOutlineNamePolicy.verdict(name: "..", siblings: siblings) == .rejected(".. is not a valid name"))
        #expect(
            WorkspaceOutlineNamePolicy.verdict(name: "README.md", siblings: siblings)
                == .rejected("README.md already exists here")
        )
        #expect(
            WorkspaceOutlineNamePolicy.verdict(name: "README.md", siblings: siblings, current: "README.md")
                == .unchanged
        )
        #expect(
            WorkspaceOutlineNamePolicy.verdict(name: "src", siblings: siblings, current: "README.md")
                == .rejected("src already exists here")
        )
    }

    @Test func relativePathIsTakenFromTheRootAndNeverGuessedOutsideIt() {
        #expect(WorkspaceOutlinePathPresentation.relativePath("/repo/src/lib.rs", root: "/repo") == "src/lib.rs")
        #expect(WorkspaceOutlinePathPresentation.relativePath("/repo/src/lib.rs", root: "/repo/") == "src/lib.rs")
        #expect(WorkspaceOutlinePathPresentation.relativePath("/repo", root: "/repo") == ".")
        #expect(WorkspaceOutlinePathPresentation.relativePath("/repo-other/a", root: "/repo") == "/repo-other/a")
    }

    @Test func dropLandsInAFolderTheParentOfAFileOrTheRootAndRefusesNoOps() {
        typealias Target = WorkspaceOutlineDropPolicy.Target
        let root = "/repo"
        let source = "/repo/src/lib.rs"
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: source, over: Target(path: "/repo/docs", isDirectory: true), root: root
            ) == "/repo/docs"
        )
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: source, over: Target(path: "/repo/docs/guide.md", isDirectory: false), root: root
            ) == "/repo/docs"
        )
        #expect(WorkspaceOutlineDropPolicy.destinationDirectory(source: source, over: nil, root: root) == "/repo")

        // The same parent, the item itself, and the item's own subtree.
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: source, over: Target(path: "/repo/src", isDirectory: true), root: root
            ) == nil
        )
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: source, over: Target(path: "/repo/src/main.rs", isDirectory: false), root: root
            ) == nil
        )
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: "/repo/src", over: Target(path: "/repo/src", isDirectory: true), root: root
            ) == nil
        )
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: "/repo/src", over: Target(path: "/repo/src/nested", isDirectory: true), root: root
            ) == nil
        )
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: "/repo/src", over: Target(path: "/repo/src/nested/deep.rs", isDirectory: false), root: root
            ) == nil
        )
        // A sibling folder whose name merely starts the same is not the subtree.
        #expect(
            WorkspaceOutlineDropPolicy.destinationDirectory(
                source: "/repo/src", over: Target(path: "/repo/src2", isDirectory: true), root: root
            ) == "/repo/src2"
        )
    }

    @Test func explorerOperationDecodesTheCoreSlot() throws {
        let payload = """
        {"id": 3, "kind": "path_move", "phase": "failed", "path": "/repo/a", "destination": "/repo/b/a", "message": "a already exists in b"}
        """
        let decoded = try JSONDecoder().decode(CoreExplorerOperation.self, from: Data(payload.utf8))
        #expect(decoded.id == 3)
        #expect(decoded.isSettled)
        #expect(decoded.message == "a already exists in b")
        let working = try JSONDecoder().decode(
            CoreExplorerOperation.self,
            from: Data(#"{"id": 4, "kind": "file_create", "phase": "working", "path": "/r/x", "destination": "/r/x", "message": null}"#.utf8)
        )
        #expect(!working.isSettled)
    }
}
