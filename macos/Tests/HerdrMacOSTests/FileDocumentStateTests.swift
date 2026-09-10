import Foundation
import AppKit
import SwiftUI
import Testing
@testable import HerdrMacOS

@Suite("File document state", .serialized)
@MainActor
struct FileDocumentStateTests {
    @Test func syntaxHighlightingKeepsTheRequestedFontAcrossViewUpdates() async throws {
        let source = "# Agent Notes\n\nEnglish 한글 **bold** and `code`\n"
        let host = NSHostingView(rootView: HighlightedCodeEditor(text: .constant(source), language: "markdown",
            isEditable: true, textScale: 1))
        host.frame = NSRect(x: 0, y: 0, width: 720, height: 400)
        host.layoutSubtreeIfNeeded()
        func text(in view: NSView) -> NSTextView? {
            (view as? NSTextView) ?? view.subviews.lazy.compactMap { text(in: $0) }.first
        }
        let view = try #require(text(in: host))
        for scale: CGFloat in [1, 1.5, 1] {
            for update in 0..<6 {
                host.rootView = HighlightedCodeEditor(text: .constant(source), language: "markdown",
                    isEditable: update.isMultiple(of: 2), textScale: scale)
                host.layoutSubtreeIfNeeded()
                try await Task.sleep(for: .milliseconds(60))
                let storage = try #require(view.textStorage)
                storage.enumerateAttribute(.font, in: NSRange(location: 0, length: storage.length)) { value, _, _ in
                    let font = value as? NSFont
                    #expect(font?.pointSize == HideTheme.editorBaseFontSize * scale)
                }
                // Korean may use AppKit's glyph fallback; the Latin heading
                // must retain the configured monospaced family, not Courier.
                let headingFont = storage.attribute(.font, at: 0, effectiveRange: nil) as? NSFont
                #expect(headingFont?.familyName == NSFont.monospacedSystemFont(ofSize: 12, weight: .regular).familyName)
                #expect(view.string == source)
            }
        }
    }

    @Test func typingSurvivesSnapshotEchoesAndAutosavesEveryCharacter() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("file-typing-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("typing.txt")
        try "START ".write(to: file, atomically: true, encoding: .utf8)
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually { model.focusedCheckout != nil }
        model.openFile(file)
        try await eventually { bridge.snapshot?.editor.path == file.path }
        let host = NSHostingView(rootView: EditorViewerOverlay().environmentObject(model).environmentObject(HideTooltipController()))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = host
        defer { window.contentView = nil }
        host.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(50))
        func text(in view: NSView) -> NSTextView? {
            (view as? NSTextView) ?? view.subviews.lazy.compactMap { text(in: $0) }.first
        }
        let view = try #require(text(in: host))
        window.makeFirstResponder(view)
        view.setSelectedRange(NSRange(location: view.string.utf16.count, length: 0))
        let input = "Native autosave 확인 1200 abcdefghijklmnopqrstuvwxyz"
        for character in input {
            view.insertText(String(character), replacementRange: view.selectedRange())
            try await Task.sleep(for: .milliseconds(3))
        }
        try await Task.sleep(for: .milliseconds(700))
        #expect(view.string == "START " + input)
        #expect(try String(contentsOf: file, encoding: .utf8) == "START " + input)

    }

    @Test func scheduledSaveCannotWriteIntoTheNextFileTab() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("file-save-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let first = root.appendingPathComponent("first.md")
        let second = root.appendingPathComponent("second.md")
        try "First original".write(to: first, atomically: true, encoding: .utf8)
        try "Second original".write(to: second, atomically: true, encoding: .utf8)
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        let model = ShellModel(core: bridge)
        try await eventually { model.focusedCheckout != nil }
        model.openFile(first)
        try await eventually { bridge.snapshot?.editor.path == first.path }
        let firstID = try #require(bridge.snapshot?.editor.activeTabID)
        bridge.updateDraft("First changed")
        bridge.scheduleFileSave("First changed")
        bridge.setFileView(tabID: firstID, preview: true, wrap: true)
        model.openFile(second)
        try await eventually { bridge.snapshot?.editor.path == second.path }
        // Let the first save complete after a different file is visibly active.
        try await eventually { (try? String(contentsOf: first, encoding: .utf8)) == "First changed" }
        #expect(try String(contentsOf: second, encoding: .utf8) == "Second original")
        bridge.focusFileTab(firstID)
        try await eventually { bridge.snapshot?.editor.activeTabID == firstID }
        #expect(bridge.snapshot?.editor.contentsUTF8 == "First changed")
        #expect(bridge.snapshot?.editor.tabs.first { $0.id == firstID }?.wrap == true)
    }

    private func eventually(_ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while !predicate(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        #expect(predicate())
    }
}
