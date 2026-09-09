import AppKit
import Testing
import SwiftUI
@testable import HerdrMacOS

@Suite("Markdown native document", .serialized)
@MainActor
struct MarkdownDocumentTests {
    @Test func tableColumnsRemainVisiblySeparated() throws {
        let rendered = try MarkdownDocument.render("| Column | Value |\n| --- | --- |\n| Korean | 한글 |", scale: 1)
        #expect(rendered.string.contains("Korean"))
        #expect(rendered.string.contains("한글"))
        let storage = NSTextStorage(attributedString: rendered)
        let layout = NSLayoutManager()
        let container = NSTextContainer(size: NSSize(width: 600, height: CGFloat.greatestFiniteMagnitude))
        storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        layout.ensureLayout(for: container)
        let text = rendered.string as NSString
        let leftRange = text.range(of: "Column")
        let rightRange = text.range(of: "Value")
        try #require(leftRange.location != NSNotFound && rightRange.location != NSNotFound)
        let left = layout.boundingRect(forGlyphRange: layout.glyphRange(forCharacterRange: leftRange, actualCharacterRange: nil), in: container)
        let right = layout.boundingRect(forGlyphRange: layout.glyphRange(forCharacterRange: rightRange, actualCharacterRange: nil), in: container)
        #expect(right.minX - left.maxX >= 8, "Table columns require a visible gap, not a zero-width tab")
    }

    @Test func previewPreservesBlocksAndNeverEmbedsHTMLOrFetchesImages() throws {
        let rendered = try MarkdownDocument.render("# Heading\n\n한국어 **bold** text\n\n- First\n- Second\n\n![secret](https://example.com/private.png)\n\n<script>alert(1)</script>", scale: 1)
        #expect(rendered.string.contains("Heading\n\n한국어 bold text\n\n• First\n\n• Second"))
        #expect(rendered.string.contains("[Image: secret · preview disabled]"))
        #expect(rendered.string.contains("<script>alert(1)</script>"))
        #expect(!rendered.string.contains("https://example.com/private.png"))
        rendered.enumerateAttribute(.attachment, in: NSRange(location: 0, length: rendered.length)) { value, _, _ in
            #expect(value == nil)
        }
        let body = (rendered.string as NSString).range(of: "한국어")
        let font = try #require(rendered.attribute(.font, at: body.location, effectiveRange: nil) as? NSFont)
        #expect(font.fontName.contains("Inter"))
    }

    @Test func longPreviewHasScrollableNativeDocumentGeometry() throws {
        let source = Array(repeating: "한국어와 English paragraph for the document preview.", count: 100).joined(separator: "\n\n")
        let host = NSHostingView(rootView: MarkdownPreview(text: source, textScale: 1, findRequest: 0, openLink: { _ in }))
        host.frame = NSRect(x: 0, y: 0, width: 720, height: 240)
        host.layoutSubtreeIfNeeded()
        func scroll(in view: NSView) -> NSScrollView? {
            (view as? NSScrollView) ?? view.subviews.lazy.compactMap { scroll(in: $0) }.first
        }
        let scrollView = try #require(scroll(in: host))
        let document = try #require(scrollView.documentView as? NSTextView)
        #expect(document.string.contains("한국어와 English"))
        #expect(document.frame.height > scrollView.contentSize.height)
        #expect(document.frame.width <= scrollView.contentSize.width)
    }

    @Test func koreanAndEnglishWrapAtReadableWidthUsingActualFont() throws {
        let source = Array(repeating: "한국어 문서에서 줄바꿈을 확인합니다. English words wrap inside the document.", count: 30).joined(separator: " ")
        let storage = NSTextStorage(attributedString: try MarkdownDocument.render(source, scale: 1))
        let layout = NSLayoutManager()
        let container = NSTextContainer(size: NSSize(width: HideTheme.Editor.documentWidth, height: .greatestFiniteMagnitude))
        storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        layout.ensureLayout(for: container)
        let wideHeight = layout.usedRect(for: container).height
        container.containerSize.width = HideTheme.Editor.documentWidth / 2
        layout.ensureLayout(for: container)
        #expect(layout.usedRect(for: container).height > wideHeight)
        #expect(layout.usedRect(for: container).width <= container.containerSize.width)
    }
}
