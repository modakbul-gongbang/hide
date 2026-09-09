import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Markdown native document", .serialized)
@MainActor
struct MarkdownDocumentTests {
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
