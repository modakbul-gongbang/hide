import Foundation
import AppKit
import Highlightr
import SwiftUI
import Testing
@testable import HerdrMacOS

@Suite("Right panel presentation")
struct RightPanelPresentationTests {
    @Test @MainActor func lineNumberRulerDoesNotPaintOverTheDocument() throws {
        let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 300, height: 100))
        let scrollView = NSScrollView(frame: textView.frame)
        scrollView.documentView = textView
        let ruler = CodeLineNumberRulerView(textView: textView, scrollView: scrollView, fontSize: 12)
        ruler.frame = NSRect(x: 0, y: 0, width: HideTheme.Editor.lineNumberColumnWidth, height: 100)
        let bitmap = try #require(NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: 300, pixelsHigh: 100,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ))
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        NSColor.white.setFill()
        NSRect(x: 0, y: 0, width: 300, height: 100).fill()
        ruler.drawHashMarksAndLabels(in: NSRect(x: 0, y: 0, width: 300, height: 100))
        let untouched = try #require(bitmap.colorAt(x: 150, y: 50)?.usingColorSpace(.deviceRGB))
        #expect(untouched.redComponent > 0.99, "Ruler drawing must not cover the adjacent editor body")
    }

    @Test @MainActor func fileEditorDrawsMonospacedTextAfterInitialEmptyDraft() async throws {
        let editor = HighlightedCodeEditor(
            text: .constant(""),
            language: nil,
            isEditable: true,
            textScale: 1
        )
        let host = NSHostingView(rootView: editor)
        host.frame = NSRect(x: 0, y: 0, width: 800, height: 500)
        host.layoutSubtreeIfNeeded()
        host.rootView = HighlightedCodeEditor(
            text: .constant("Notice text must be visible.\nSecond line.\n"),
            language: nil,
            isEditable: true,
            textScale: 1
        )
        try await Task.sleep(for: .milliseconds(50))
        host.layoutSubtreeIfNeeded()
        func descendants(_ view: NSView) -> [NSView] {
            view.subviews.flatMap { [$0] + descendants($0) }
        }
        let textView = try #require(descendants(host).compactMap { $0 as? NSTextView }.first)
        #expect(textView.font?.isFixedPitch == true, "Plain text must retain the editor's monospaced font after loading")
        let manager = try #require(textView.layoutManager)
        let container = try #require(textView.textContainer)
        manager.ensureLayout(for: container)
        let glyphBounds = manager.boundingRect(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs), in: container)
        #expect(textView.bounds.width > 100)
        #expect(textView.bounds.height >= glyphBounds.maxY)
        #expect(textView.visibleRect.intersects(glyphBounds))
        let bitmap = try #require(textView.bitmapImageRepForCachingDisplay(in: textView.bounds))
        textView.cacheDisplay(in: textView.bounds, to: bitmap)
        var brightPixels = 0
        for y in 0..<min(bitmap.pixelsHigh, 100) {
            for x in 0..<min(bitmap.pixelsWide, 500) {
                if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
                   color.redComponent > 0.5 { brightPixels += 1 }
            }
        }
        #expect(brightPixels > 20, "The document body must draw visible glyphs, independent of its ruler")
    }

    @Test @MainActor func verificationNoRemoteArgumentDisablesRemoteTargetsOnlyForThatLaunch() {
        #expect(CoreBridge.remoteTargets(arguments: ["hide"]).contains { $0["id"] == "mini" })
        #expect(CoreBridge.remoteTargets(arguments: ["hide", "--verification-no-remote"]).isEmpty)
    }

    @Test func directoryLoaderReadsOnlyTheRequestedLevelAndKeepsRepositoryDotfiles() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-right-panel-\(UUID().uuidString)", isDirectory: true)
        let sources = root.appendingPathComponent("Sources", isDirectory: true)
        let ignoredGit = root.appendingPathComponent(".git", isDirectory: true)
        try FileManager.default.createDirectory(at: sources, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: ignoredGit, withIntermediateDirectories: true)
        #expect(FileManager.default.createFile(atPath: root.appendingPathComponent("README.md").path, contents: Data()))
        #expect(FileManager.default.createFile(atPath: root.appendingPathComponent(".gitignore").path, contents: Data()))
        #expect(FileManager.default.createFile(atPath: sources.appendingPathComponent("Nested.swift").path, contents: Data()))
        defer { try? FileManager.default.removeItem(at: root) }

        let rootEntries = try WorkspaceDirectoryLoader.loadDirectory(at: root)

        #expect(rootEntries.map(\.url.lastPathComponent) == ["Sources", ".gitignore", "README.md"])
        #expect(!rootEntries.contains { $0.url.lastPathComponent == ".git" })
        #expect(!rootEntries.contains { $0.url.lastPathComponent == "Nested.swift" })

        let sourceEntries = try WorkspaceDirectoryLoader.loadDirectory(at: sources)
        #expect(sourceEntries.map(\.url.lastPathComponent) == ["Nested.swift"])
    }

    @Test @MainActor func setiCatalogCoversRepositoryFormatsAndHasOneGenericFallback() {
        let cases: [(String, String)] = [
            ("Package.swift", "\u{E092}"),
            ("lib.rs", "\u{E082}"),
            ("theme.tsx", "\u{E07D}"),
            ("README.md", "\u{E04D}"),
            ("Cargo.lock", "\u{E05D}"),
            ("config.json", "\u{E055}"),
            ("icon.svg", "\u{E091}"),
            ("script.zsh", "\u{E089}"),
        ]

        for (fileName, expectedGlyph) in cases {
            #expect(SetiFileIconCatalog.icon(fileName: fileName).glyph == expectedGlyph)
        }
        #expect(SetiFileIconCatalog.icon(fileName: "unrecognised.hide").glyph == SetiFileIconCatalog.fallback.glyph)
        #expect(SetiFileIconCatalog.icon(fileName: "unrecognised.hide").fallbackSystemImage == "doc")
        let bundledFontBytes = SetiIconFont.resourceURL.flatMap { try? Data(contentsOf: $0).count }
        #expect(bundledFontBytes == 5_976)
        #expect(AgentMark.image(for: "codex") != nil)
        let highlighter = Highlightr()
        #expect(highlighter != nil)
        #expect(highlighter?.setTheme(to: "atom-one-dark") == true)

        let plainTextFallback = HighlightedCodeEditor.makeTextStorage(
            text: "let value = 1",
            language: "swift",
            highlightr: nil
        )
        #expect(!(plainTextFallback is CodeAttributedString))
        #expect(plainTextFallback.string == "let value = 1")
        #expect(
            plainTextFallback.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
                == HideTheme.Native.primary
        )

        let extensionlessText = HighlightedCodeEditor.makeTextStorage(
            text: "This file has no extension.",
            language: nil,
            highlightr: highlighter
        )
        #expect(
            extensionlessText.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
                == HideTheme.Native.primary
        )

        let macosRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let noticeURL = macosRoot
            .appendingPathComponent("Resources/THIRD_PARTY_NOTICES/seti-ui-MIT.txt")
        let notice = try? String(contentsOf: noticeURL, encoding: .utf8)
        #expect(notice?.contains("Copyright (c) 2014 Jesse Weed") == true)
        #expect(notice?.contains("https://github.com/jesseweed/seti-ui") == true)
    }

    @Test @MainActor func outlineReturnKeyUsesTheSameActivationBoundaryAsPointerOpen() {
        let outline = WorkspaceNSOutlineView()
        var activationCount = 0
        outline.onActivate = { activationCount += 1 }
        let returnKey = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "\r",
            charactersIgnoringModifiers: "\r",
            isARepeat: false,
            keyCode: 36
        )!

        outline.keyDown(with: returnKey)

        #expect(activationCount == 1)
    }

    /// Opening was bound to selection change, so arrow-key traversal opened a
    /// file tab for every row it passed and a directory's name did nothing.
    /// Activation now decides by what the row is, and selection decides
    /// nothing.
    @Test func activationOpensAFileTogglesADirectoryAndIgnoresAPlaceholder() {
        #expect(WorkspaceOutlineActivationPolicy.activation(
            isDirectory: false,
            isPlaceholder: false
        ) == .open)
        #expect(WorkspaceOutlineActivationPolicy.activation(
            isDirectory: true,
            isPlaceholder: false
        ) == .toggle)
        #expect(WorkspaceOutlineActivationPolicy.activation(
            isDirectory: false,
            isPlaceholder: true
        ) == .none)
        #expect(WorkspaceOutlineActivationPolicy.activation(
            isDirectory: true,
            isPlaceholder: true
        ) == .none)
    }

    @Test func unifiedEditorDeltaDecodesTabMetadataAndOnlyTheActiveDocument() throws {
        let payload = """
        {
            "tabs": [{
                "id": "file:w:c:/repo/README.md",
                "workspace_id": "w",
                "checkout_id": "c",
                "path": "/repo/README.md",
                "label": "README.md",
                "kind": "file",
                "diff_committed": null,
                "dirty": true
            }],
            "active_tab_id": "file:w:c:/repo/README.md",
            "document": {
                "path": "/repo/README.md",
                "language": "markdown",
                "contents_utf8": "draft",
                "opened_modified_at_unix_ms": 1,
                "dirty": true,
                "readonly_reason": null,
                "conflict": null
            }
        }
        """

        let decoded = try JSONDecoder().decode(CoreEditorSnapshot.self, from: Data(payload.utf8))

        #expect(decoded.tabs.map(\.label) == ["README.md"])
        #expect(decoded.activeTabID == "file:w:c:/repo/README.md")
        #expect(decoded.contentsUTF8 == "draft")
    }

    /// AC3, SC2 failure and recovery. A folder path printed minutes ago can be
    /// deleted before it is clicked. The click then leaves the screen exactly
    /// as it was - the panel does not open, nothing is expanded or selected,
    /// no editor tab appears - because no reveal is dispatched at all, and the
    /// operator is told which path could not be found. Recreating the folder
    /// makes the identical click reveal it, with nothing else done between.
    @Test @MainActor func aClickOnADeletedFolderChangesNothingOnScreenUntilTheFolderIsBack() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-reveal-recovery-\(UUID().uuidString)", isDirectory: true)
        let folder = root.appendingPathComponent("deep/nested", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-reveal-recovery-state-\(UUID().uuidString).json")
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: stateURL)
        }

        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--verification-no-remote",
            "--workspace-root", root.path,
            "--state-path", stateURL.path,
        ])
        try await Task.sleep(for: .milliseconds(100))
        guard let workspace = bridge.snapshot?.navigator.workspaces.first,
              let checkout = workspace.checkouts.first
        else {
            Issue.record("the verification fixture should project a workspace and checkout")
            return
        }
        let model = ShellModel(core: bridge)
        let paneID = "p1"

        // The folder is gone by the time the printed path is clicked.
        try FileManager.default.removeItem(at: root.appendingPathComponent("deep", isDirectory: true))
        let before = bridge.snapshot?.uiState
        model.openTerminalLink(folder.path, paneID: paneID)
        try await Task.sleep(for: .milliseconds(150))
        let after = bridge.snapshot?.uiState
        #expect(after?.rightPanelVisible == before?.rightPanelVisible, "the panel must not open for a path that is gone")
        #expect(after?.rightPanelSection == before?.rightPanelSection)
        #expect(after?.expandedPaths == before?.expandedPaths, "nothing may be expanded for a path that is gone")
        #expect(after?.selectedPath == before?.selectedPath, "nothing may be selected for a path that is gone")
        #expect(bridge.snapshot?.editor.tabs.isEmpty == true, "a folder click never opens a document")
        // The reason does not go on screen. Detection is a guess made over
        // arbitrary terminal output, so a wrong guess is ordinary and a modal
        // would make the operator dismiss a dialog for a mis-click; the reason
        // leaves through the trace instead. What has to hold here is that
        // nothing was raised, and that the reason names the clicked path.
        #expect(model.interactionNotice == nil, "a click that resolves to nothing raises no modal")
        let reason = TerminalLinkResolver.route(
            folder.path,
            paneCWD: "",
            checkoutRoot: nil,
            checkouts: [TerminalLinkCheckout(id: checkout.id, workspaceID: workspace.id, path: checkout.path)]
        )
        if case .unresolved(let message) = reason {
            #expect(message.contains(String(folder.path.prefix(40))), "the reason names the path that was clicked")
        } else {
            Issue.record("a deleted folder must resolve to nothing, got \(reason)")
        }

        // The folder comes back, and the identical click reveals it.
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        model.openTerminalLink(folder.path, paneID: paneID)
        try await Task.sleep(for: .milliseconds(150))
        let revealed = bridge.snapshot?.uiState
        #expect(revealed?.rightPanelVisible == true)
        #expect(revealed?.rightPanelSection == .explorer)
        #expect(revealed?.selectedPath == TerminalLinkResolver.canonical(folder).path)
        #expect(revealed?.expandedPaths.contains(TerminalLinkResolver.canonical(folder).path) == true)
        #expect(bridge.snapshot?.editor.tabs.isEmpty == true, "a folder is not a document")
    }

    @Test @MainActor func bridgeFileTabsDeduplicateAndCloseRestoresThePreviousFile() async throws {
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-right-panel-state-\(UUID().uuidString).json")
        let macosRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let fileURL = macosRoot.appendingPathComponent("VerificationFixtures/Sample.swift")
        let secondURL = macosRoot.appendingPathComponent("Package.swift")
        defer { try? FileManager.default.removeItem(at: stateURL) }
        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--verification-no-remote",
            "--workspace-root", macosRoot.path,
            "--state-path", stateURL.path,
        ])
        try await Task.sleep(for: .milliseconds(100))

        guard let workspace = bridge.snapshot?.navigator.workspaces.first,
              let checkout = workspace.checkouts.first
        else {
            Issue.record("verification fixture should project a workspace and checkout")
            return
        }
        bridge.openFile(fileURL, workspaceID: workspace.id, checkoutID: checkout.id)
        bridge.openFile(fileURL, workspaceID: workspace.id, checkoutID: checkout.id)
        try await Task.sleep(for: .milliseconds(50))

        #expect(bridge.snapshot?.editor.tabs.count == 1)
        #expect(bridge.snapshot?.editor.path == fileURL.path)

        bridge.openFile(secondURL, workspaceID: workspace.id, checkoutID: checkout.id)
        try await Task.sleep(for: .milliseconds(50))
        let secondTabID = try #require(bridge.snapshot?.editor.activeTabID)
        #expect(bridge.snapshot?.editor.tabs.count == 2)
        #expect(bridge.snapshot?.editor.path == secondURL.path)

        bridge.closeFileTab(secondTabID)
        try await Task.sleep(for: .milliseconds(50))

        #expect(bridge.snapshot?.editor.tabs.count == 1)
        #expect(bridge.snapshot?.editor.path == fileURL.path)
    }

    @Test func panelVisibilityDefaultsOpenAndDecodesIndependentClosedStates() throws {
        let defaults = try JSONDecoder().decode(
            CoreUIStateSnapshot.self,
            from: Data(#"{"expanded_paths":[]}"#.utf8)
        )
        let closed = try JSONDecoder().decode(
            CoreUIStateSnapshot.self,
            from: Data(#"{"expanded_paths":[],"left_sidebar_visible":false,"right_panel_visible":false}"#.utf8)
        )

        #expect(defaults.leftSidebarVisible)
        #expect(defaults.rightPanelVisible)
        #expect(!closed.leftSidebarVisible)
        #expect(!closed.rightPanelVisible)
    }

    @Test @MainActor func panelVisibilityAndUnrelatedUIStateSurviveBridgeRelaunch() async throws {
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-panel-state-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: stateURL) }
        var first: CoreBridge? = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--verification-no-remote",
            "--state-path", stateURL.path,
        ])
        try await Task.sleep(for: .milliseconds(50))
        let workspaceRegistrationIDs = first?.snapshot?.uiState.workspaceRegistrations.map(\.id)
        let deviceRegistrationIDs = first?.snapshot?.uiState.deviceRegistrations.map(\.id)
        first?.persistUIState(
            leftSidebarVisible: false,
            rightPanelVisible: false,
            expandedPaths: ["/repo/Sources"],
            selectedPath: "/repo/Sources/App.swift",
            shortcutBindings: ["split_right": "command+option+r"],
            accentHex: "#A1B2C3",
            fontSize: 15
        )
        try await Task.sleep(for: .milliseconds(50))
        first = nil

        let restored = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--verification-no-remote",
            "--state-path", stateURL.path,
        ])
        let uiState = try #require(restored.snapshot?.uiState)

        #expect(!uiState.leftSidebarVisible)
        #expect(!uiState.rightPanelVisible)
        #expect(uiState.expandedPaths == ["/repo/Sources"])
        #expect(uiState.selectedPath == "/repo/Sources/App.swift")
        #expect(uiState.shortcutBindings["split_right"] == "command+option+r")
        #expect(uiState.accentHex == "#A1B2C3")
        #expect(uiState.fontSize == 15)
        #expect(restored.snapshot?.pet.visible == true)
        #expect(uiState.workspaceRegistrations.map(\.id) == workspaceRegistrationIDs)
        #expect(uiState.deviceRegistrations.map(\.id) == deviceRegistrationIDs)
    }
}
