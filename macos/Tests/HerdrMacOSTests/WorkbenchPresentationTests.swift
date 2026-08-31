import Foundation
import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Workbench presentation")
struct WorkbenchPresentationTests {
    @Test func directoryLoaderReadsOnlyTheRequestedLevelAndKeepsRepositoryDotfiles() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-workbench-\(UUID().uuidString)", isDirectory: true)
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

    @Test func setiCatalogCoversRepositoryFormatsAndHasOneGenericFallback() {
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

    @Test @MainActor func viewerEscapeUsesApplicationLocalKeyRouting() {
        let escape = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "\u{1b}",
            charactersIgnoringModifiers: "\u{1b}",
            isARepeat: false,
            keyCode: 53
        )!
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

        #expect(WorkbenchViewerEscapeMonitor.handles(escape))
        #expect(!WorkbenchViewerEscapeMonitor.handles(returnKey))
    }

    @Test func olderEditorDeltaWithoutViewerVisibilityStillDecodes() throws {
        let payload = """
        {
            "path": "/repo/README.md",
            "language": "md",
            "contents_utf8": "draft",
            "opened_modified_at_unix_ms": 1,
            "dirty": true,
            "readonly_reason": null,
            "conflict": null,
            "diff": null
        }
        """

        let decoded = try JSONDecoder().decode(CoreEditorSnapshot.self, from: Data(payload.utf8))

        #expect(decoded.viewerVisible == nil)
        #expect(decoded.contentsUTF8 == "draft")
    }

    @Test @MainActor func bridgeDismissalAndSameFileReopenPreserveTheCoreDraft() async throws {
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-workbench-state-\(UUID().uuidString).json")
        let macosRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let fileURL = macosRoot.appendingPathComponent("VerificationFixtures/Sample.swift")
        defer { try? FileManager.default.removeItem(at: stateURL) }
        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--workspace-root", macosRoot.path,
            "--state-path", stateURL.path,
        ])

        bridge.openFile(fileURL)
        bridge.updateDraft("// unsaved bridge draft\n")
        bridge.setFileViewerVisible(false)
        try await Task.sleep(for: .milliseconds(50))

        #expect(bridge.snapshot?.editor.viewerVisible == false)
        #expect(bridge.snapshot?.editor.dirty == true)
        #expect(bridge.snapshot?.editor.contentsUTF8 == "// unsaved bridge draft\n")

        bridge.openFile(fileURL)
        try await Task.sleep(for: .milliseconds(50))

        #expect(bridge.snapshot?.editor.viewerVisible == true)
        #expect(bridge.snapshot?.editor.dirty == true)
        #expect(bridge.snapshot?.editor.contentsUTF8 == "// unsaved bridge draft\n")
    }

    @Test func panelVisibilityDefaultsOpenAndDecodesIndependentClosedStates() throws {
        let defaults = try JSONDecoder().decode(
            CoreUIStateSnapshot.self,
            from: Data(#"{"expanded_paths":[]}"#.utf8)
        )
        let closed = try JSONDecoder().decode(
            CoreUIStateSnapshot.self,
            from: Data(#"{"expanded_paths":[],"left_sidebar_visible":false,"right_workbench_visible":false}"#.utf8)
        )

        #expect(defaults.leftSidebarVisible)
        #expect(defaults.rightWorkbenchVisible)
        #expect(!closed.leftSidebarVisible)
        #expect(!closed.rightWorkbenchVisible)
    }

    @Test @MainActor func panelVisibilityAndUnrelatedUIStateSurviveBridgeRelaunch() async throws {
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-panel-state-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: stateURL) }
        var first: CoreBridge? = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--state-path", stateURL.path,
        ])
        try await Task.sleep(for: .milliseconds(50))
        let workspaceRegistrationIDs = first?.snapshot?.uiState.workspaceRegistrations.map(\.id)
        let deviceRegistrationIDs = first?.snapshot?.uiState.deviceRegistrations.map(\.id)
        first?.persistUIState(
            leftSidebarVisible: false,
            rightWorkbenchVisible: false,
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
            "--state-path", stateURL.path,
        ])
        let uiState = try #require(restored.snapshot?.uiState)

        #expect(!uiState.leftSidebarVisible)
        #expect(!uiState.rightWorkbenchVisible)
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
