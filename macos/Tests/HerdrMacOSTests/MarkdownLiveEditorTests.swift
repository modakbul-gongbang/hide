import AppKit
import SwiftUI
import Testing
@testable import HerdrMacOS

@Suite("Markdown Live editor", .serialized)
@MainActor
struct MarkdownLiveEditorTests {
    /// A hosted Live editor over `source`, parsed and laid out.
    private func hosted(
        _ source: String, width: CGFloat = 720,
        parse: @escaping @Sendable (String) throws -> MarkdownLivePlan = MarkdownLiveSource.plan,
        failure: @escaping (String?) -> Void = { _ in }
    ) async throws -> (host: NSHostingView<MarkdownLiveEditor>, view: MarkdownLiveTextView, coordinator: MarkdownLiveEditor.Coordinator) {
        let editor = MarkdownLiveEditor(text: .constant(source), isEditable: true, textScale: 1,
            openLink: { _ in }, reportParseFailure: failure, parse: parse)
        let host = NSHostingView(rootView: editor)
        host.frame = NSRect(x: 0, y: 0, width: width, height: 400)
        host.layoutSubtreeIfNeeded()
        let view = try #require(find(MarkdownLiveTextView.self, in: host))
        let coordinator = try #require(view.delegate as? MarkdownLiveEditor.Coordinator)
        // Every source here has at least one block, so a landed parse has a span.
        try await eventually { coordinator.plan.length == source.utf16.count && !coordinator.plan.spans.isEmpty }
        return (host, view, coordinator)
    }

    private func find<T: NSView>(_ type: T.Type, in view: NSView) -> T? {
        (view as? T) ?? view.subviews.lazy.compactMap { find(type, in: $0) }.first
    }

    private func isDrawn(_ text: String, in view: NSTextView, backwards: Bool = false) -> Bool {
        let range = (view.string as NSString).range(of: text, options: backwards ? .backwards : [])
        let layout = view.layoutManager!
        layout.ensureLayout(for: view.textContainer!)
        let glyphs = layout.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
        return (glyphs.location..<NSMaxRange(glyphs)).contains { layout.propertyForGlyph(at: $0) != .null }
    }

    @Test func markupIsHiddenOffTheCaretLineAndRevealedOnIt() async throws {
        let (host, view, _) = try await hosted("# Title\n**bold** text\n")
        defer { withExtendedLifetime(host) {} }
        view.setSelectedRange(NSRange(location: 0, length: 0))
        #expect(isDrawn("# ", in: view))
        #expect(!isDrawn("**", in: view))
        #expect(isDrawn("bold", in: view))
        view.setSelectedRange(NSRange(location: 10, length: 0))
        #expect(!isDrawn("# ", in: view))
        #expect(isDrawn("**", in: view))
        view.setSelectedRange(NSRange(location: 2, length: 8))
        #expect(isDrawn("# ", in: view))
        #expect(isDrawn("**", in: view))
        let heading = try #require(view.textStorage?.attribute(.font, at: 2, effectiveRange: nil) as? NSFont)
        #expect(heading.pointSize == HideTheme.Editor.headingFontSizes[0])
        let bold = try #require(view.textStorage?.attribute(.font, at: 10, effectiveRange: nil) as? NSFont)
        #expect(bold.pointSize == HideTheme.Editor.documentFontSize)
        #expect(bold.fontName.contains("Inter"))
    }

    @Test func fencedBlockCollapsesItsFencesUntilTheCaretEnters() async throws {
        let source = "before\n\n```swift\nlet x = 1\n```\n\nafter\n"
        let (host, view, _) = try await hosted(source)
        defer { withExtendedLifetime(host) {} }
        let layout = view.layoutManager!
        func lineY(_ text: String) -> CGFloat {
            layout.ensureLayout(for: view.textContainer!)
            let location = (source as NSString).range(of: text).location
            return layout.lineFragmentRect(forGlyphAt: layout.glyphIndexForCharacter(at: location), effectiveRange: nil).minY
        }
        view.setSelectedRange(NSRange(location: 0, length: 0))
        #expect(!isDrawn("```swift", in: view))
        #expect(!isDrawn("```", in: view, backwards: true))
        let collapsedGap = lineY("after") - lineY("let x")
        view.setSelectedRange(NSRange(location: 20, length: 0))
        #expect(isDrawn("```swift", in: view))
        #expect(isDrawn("```", in: view, backwards: true))
        let revealedGap = lineY("after") - lineY("let x")
        #expect(revealedGap > collapsedGap, "a hidden fence line takes no height")
    }

    @Test func typingIntoAFormattedLineLandsAtTheCaret() async throws {
        let (host, view, coordinator) = try await hosted("# Title\nbody **b**\n")
        defer { withExtendedLifetime(host) {} }
        view.setSelectedRange(NSRange(location: 7, length: 0))
        view.insertText("X", replacementRange: view.selectedRange())
        #expect(view.string == "# TitleX\nbody **b**\n")
        view.setSelectedRange(NSRange(location: 16, length: 0))
        view.insertText("한", replacementRange: view.selectedRange())
        #expect(view.string == "# TitleX\nbody **한b**\n")
        try await eventually { coordinator.plan.length == view.string.utf16.count && !coordinator.plan.spans.isEmpty }
        // The Latin neighbour keeps Inter at bold; Korean takes AppKit's fallback face.
        let font = try #require(view.textStorage?.attribute(.font, at: 17, effectiveRange: nil) as? NSFont)
        #expect(font.fontName.contains("Inter") && font.pointSize == HideTheme.Editor.documentFontSize)
    }

    @Test func koreanAndEnglishWrapInsideTheCentredMeasure() async throws {
        let source = Array(repeating: "한국어 문서에서 줄바꿈을 확인합니다. English words wrap inside the document.", count: 12).joined(separator: " ") + "\n"
        let (host, view, _) = try await hosted(source, width: 1000)
        defer { withExtendedLifetime(host) {} }
        let layout = view.layoutManager!
        layout.ensureLayout(for: view.textContainer!)
        #expect(view.textContainer!.size.width == HideTheme.Editor.documentWidth)
        #expect(view.textContainerInset.width == (1000 - HideTheme.Editor.documentWidth) / 2)
        let used = layout.usedRect(for: view.textContainer!)
        #expect(used.width <= HideTheme.Editor.documentWidth)
        #expect(used.height > HideTheme.Editor.documentFontSize * 3)
        let english = (source as NSString).range(of: "English").location
        let body = try #require(view.textStorage?.attribute(.font, at: english, effectiveRange: nil) as? NSFont)
        #expect(body.fontName.contains("Inter") && body.pointSize == HideTheme.Editor.documentFontSize)
    }

    @Test func plainClickOnALinkPlacesTheCaretAndCommandOpensIt() async throws {
        var opened: URL?
        let editor = MarkdownLiveEditor(text: .constant("[text](https://example.com/a)\n"), isEditable: true, textScale: 1,
            openLink: { opened = $0 }, reportParseFailure: { _ in })
        let host = NSHostingView(rootView: editor)
        host.frame = NSRect(x: 0, y: 0, width: 720, height: 200)
        host.layoutSubtreeIfNeeded()
        let view = try #require(find(MarkdownLiveTextView.self, in: host))
        #expect(MarkdownLiveTextView.linkAction(for: []) == .placeCaret)
        #expect(MarkdownLiveTextView.linkAction(for: [.command]) == .open)
        #expect(MarkdownLiveTextView.linkAction(for: [.command, .shift]) == .open)
        view.clicked(onLink: URL(string: "https://example.com/a")!, at: 3)
        #expect(opened == nil)
        #expect(view.selectedRange() == NSRange(location: 3, length: 0))
    }

    @Test func aFailedParseLeavesTheSourceMonospacedWithTheReason() async throws {
        var reported: [String?] = []
        let (host, view, _) = try await hosted("# Title\n**bold**\n",
            parse: { _ in throw MarkdownLiveSource.Failure(reason: "no plan") }, failure: { reported.append($0) })
        defer { withExtendedLifetime(host) {} }
        try await eventually { reported.last == "no plan" }
        #expect(isDrawn("# ", in: view) && isDrawn("**", in: view))
        let mono = NSFont.monospacedSystemFont(ofSize: HideTheme.editorBaseFontSize, weight: .regular)
        view.textStorage?.enumerateAttribute(.font, in: NSRange(location: 0, length: view.string.utf16.count)) { value, _, _ in
            #expect((value as? NSFont)?.familyName == mono.familyName)
        }
        view.insertText("more", replacementRange: NSRange(location: 0, length: 0))
        #expect(view.string.hasPrefix("more# Title"))
    }

    @Test func textScaleChordsResizeLiveType() async throws {
        let source = "# Title\nbody\n"
        let editor = MarkdownLiveEditor(text: .constant(source), isEditable: true, textScale: 1, openLink: { _ in }, reportParseFailure: { _ in })
        let host = NSHostingView(rootView: editor)
        host.frame = NSRect(x: 0, y: 0, width: 720, height: 200)
        host.layoutSubtreeIfNeeded()
        let view = try #require(find(MarkdownLiveTextView.self, in: host))
        let coordinator = try #require(view.delegate as? MarkdownLiveEditor.Coordinator)
        try await eventually { !coordinator.plan.spans.isEmpty }
        host.rootView = MarkdownLiveEditor(text: .constant(source), isEditable: true, textScale: 1.5, openLink: { _ in }, reportParseFailure: { _ in })
        host.layoutSubtreeIfNeeded()
        let heading = try #require(view.textStorage?.attribute(.font, at: 2, effectiveRange: nil) as? NSFont)
        #expect(heading.pointSize == HideTheme.Editor.headingFontSizes[0] * 1.5)
        let body = try #require(view.textStorage?.attribute(.font, at: 9, effectiveRange: nil) as? NSFont)
        #expect(body.pointSize == HideTheme.Editor.documentFontSize * 1.5)
    }

    // MARK: Through the document overlay

    private func fixture(_ name: String, files: [(String, String)]) throws -> (root: URL, model: ShellModel, bridge: CoreBridge) {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("\(name)-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        for (file, contents) in files {
            try contents.write(to: root.appendingPathComponent(file), atomically: true, encoding: .utf8)
        }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        return (root, ShellModel(core: bridge), bridge)
    }

    private func hostOverlay(_ model: ShellModel) -> (NSWindow, NSHostingView<some View>) {
        let host = NSHostingView(rootView: EditorViewerOverlay().environmentObject(model).environmentObject(HideTooltipController()))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        return (window, host)
    }

    @Test func markdownOpensLiveAndAutosavesTypedTextThroughTheSameDraftPath() async throws {
        let (root, model, bridge) = try fixture("live-typing", files: [("notes.md", "# Title\n\n")])
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("notes.md")
        try await eventually { model.focusedCheckout != nil }
        model.openFile(file)
        try await eventually { bridge.snapshot?.editor.path == file.path }
        #expect(bridge.snapshot?.editor.tabs.first?.markdownLive == true)
        let (window, host) = hostOverlay(model)
        defer { window.contentView = nil }
        try await Task.sleep(for: .milliseconds(50))
        let view = try #require(find(MarkdownLiveTextView.self, in: host))
        window.makeFirstResponder(view)
        view.setSelectedRange(NSRange(location: view.string.utf16.count, length: 0))
        let input = "Live autosave 확인 **굵게** abc"
        for character in input {
            view.insertText(String(character), replacementRange: view.selectedRange())
            try await Task.sleep(for: .milliseconds(3))
        }
        try await Task.sleep(for: .milliseconds(700))
        #expect(view.string == "# Title\n\n" + input)
        #expect(try String(contentsOf: file, encoding: .utf8) == "# Title\n\n" + input)
        // Switching to Source shows the same draft in the source editor.
        let tabID = try #require(bridge.snapshot?.editor.activeTabID)
        bridge.setFileView(tabID: tabID, live: false, wrap: false)
        try await eventually { bridge.snapshot?.editor.tabs.first?.markdownLive == false }
        host.layoutSubtreeIfNeeded()
        try await eventually { self.find(MarkdownLiveTextView.self, in: host) == nil && self.find(CodeLineNumberRulerView.self, in: host) != nil }
        let source = try #require(find(NSTextView.self, in: host))
        #expect(source.string == "# Title\n\n" + input)
    }

    @Test func emptyMarkdownOpensAsAnEmptyLiveEditor() async throws {
        let (root, model, bridge) = try fixture("live-empty", files: [("empty.md", "")])
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("empty.md")
        try await eventually { model.focusedCheckout != nil }
        model.openFile(file)
        try await eventually { bridge.snapshot?.editor.path == file.path }
        let (window, host) = hostOverlay(model)
        defer { window.contentView = nil }
        try await Task.sleep(for: .milliseconds(50))
        let view = try #require(find(MarkdownLiveTextView.self, in: host))
        #expect(view.string.isEmpty && view.isEditable)
        window.makeFirstResponder(view)
        view.insertText("# 새 문서", replacementRange: NSRange(location: 0, length: 0))
        #expect(view.string == "# 새 문서")
        let font = try #require(view.textStorage?.attribute(.font, at: 0, effectiveRange: nil) as? NSFont)
        #expect(font.fontName.contains("Inter"))
    }

    @Test func aDocumentOverTheByteLimitOpensInSourceWithLiveDisabled() async throws {
        let big = String(repeating: "# Heading\n\nparagraph with **bold** text\n\n", count: 8000)
        #expect(big.utf8.count > MarkdownLiveSource.byteLimit)
        let (root, model, bridge) = try fixture("live-oversized", files: [("big.md", big)])
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("big.md")
        try await eventually { model.focusedCheckout != nil }
        model.openFile(file)
        try await eventually { bridge.snapshot?.editor.path == file.path }
        let (window, host) = hostOverlay(model)
        defer { window.contentView = nil }
        try await Task.sleep(for: .milliseconds(100))
        #expect(find(MarkdownLiveTextView.self, in: host) == nil)
        #expect(find(CodeLineNumberRulerView.self, in: host) != nil)
        #expect(EditorViewerOverlay.liveUnavailableNotice == "Live preview is off for files over 256 KB")
    }

    private func eventually(_ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while !predicate(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        #expect(predicate())
    }
}
