import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Conversation Markdown", .serialized)
@MainActor
struct ConversationMarkdownTests {
    @Test func tableColumnsRemainVisiblySeparated() throws {
        let rendered = try ConversationMarkdown.render("| Column | Value |\n| --- | --- |\n| Korean | 한글 |", scale: 1)
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

    @Test func turnPreservesBlocksAndNeverEmbedsHTMLOrFetchesImages() throws {
        let rendered = try ConversationMarkdown.render("# Heading\n\n한국어 **bold** text\n\n- First\n- Second\n\n![secret](https://example.com/private.png)\n\n<script>alert(1)</script>", scale: 1)
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
}
