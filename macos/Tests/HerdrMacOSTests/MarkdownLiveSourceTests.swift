import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Markdown Live source plan")
struct MarkdownLiveSourceTests {
    private func plan(_ source: String) throws -> MarkdownLivePlan {
        try MarkdownLiveSource.plan(for: source)
    }

    /// The markup each marker covers, in source order, with bullets and fences tagged.
    private func markup(_ plan: MarkdownLivePlan, in source: String) -> [String] {
        plan.markers.map { marker in
            let text = (source as NSString).substring(with: marker.range)
            switch marker.kind {
            case .hidden: return text
            case .bullet: return "•\(text)"
            case .fence: return "≡\(text)"
            }
        }
    }

    private func style(_ plan: MarkdownLivePlan, of text: String, in source: String) -> MarkdownLiveStyle? {
        let range = (source as NSString).range(of: text)
        return plan.spans.first { NSLocationInRange(range.location, $0.range) }?.style
    }

    @Test func headingHidesItsHashesAndSizesTheText() throws {
        let source = "## Section title\n\nBody line\n"
        let plan = try plan(source)
        #expect(markup(plan, in: source) == ["## "])
        #expect(style(plan, of: "Section title", in: source)?.block == .heading(2))
        #expect(style(plan, of: "Body line", in: source) == .body)
        #expect(plan.markers[0].unit == NSRange(location: 0, length: 17))
    }

    @Test func inlineDelimitersAndLinkSyntaxAreMarkup() throws {
        let source = "Plain **bold** and *it* and ~~gone~~ and `code` and [text](https://example.com/a) end\n"
        let plan = try plan(source)
        #expect(markup(plan, in: source) == ["**", "**", "*", "*", "~~", "~~", "`", "`", "[", "](https://example.com/a)"])
        #expect(style(plan, of: "bold", in: source)?.bold == true)
        #expect(style(plan, of: "it", in: source)?.italic == true)
        #expect(style(plan, of: "gone", in: source)?.strikethrough == true)
        #expect(style(plan, of: "code", in: source)?.code == true)
        #expect(style(plan, of: "text", in: source)?.link == URL(string: "https://example.com/a"))
        #expect(style(plan, of: " end", in: source) == .body)
        // Every character of the paragraph belongs to a span, so an edit anywhere restyles cleanly.
        let covered = plan.spans.reduce(0) { $0 + $1.range.length }
        #expect(covered == source.utf16.count - 1)
    }

    @Test func listMarkersDrawAsBulletsOrKeepTheirDigits() throws {
        let source = "- one\n1. two\n   - nested **b**\n"
        let plan = try plan(source)
        #expect(markup(plan, in: source) == ["•-", "   ", "•-", "**", "**"])
        #expect(style(plan, of: "one", in: source)?.listDepth == 1)
        #expect(style(plan, of: "two", in: source)?.listDepth == 1)
        #expect(style(plan, of: "nested", in: source)?.listDepth == 2)
    }

    @Test func quotePrefixesHideAndRulesHideTheirWholeLine() throws {
        let source = "> quoted\n> again\n\n---\n\nafter\n"
        let plan = try plan(source)
        #expect(markup(plan, in: source) == ["> ", "> ", "---"])
        #expect(style(plan, of: "quoted", in: source)?.quoteDepth == 1)
        #expect(style(plan, of: "---", in: source)?.block == .thematicBreak)
        #expect(style(plan, of: "after", in: source) == .body)
        let rule = plan.markers[2]
        #expect(rule.unit == rule.range)
    }

    @Test func fencedCodeBlockFencesHideTogetherWithTheirNewlines() throws {
        let source = "before\n\n```swift\nlet x = 1\nlet y = 2\n```\n\nafter\n"
        let plan = try plan(source)
        #expect(markup(plan, in: source) == ["≡```swift", "≡```"])
        let block = (source as NSString).range(of: "```swift\nlet x = 1\nlet y = 2\n```")
        #expect(plan.markers.allSatisfy { $0.unit == block })
        #expect(style(plan, of: "let x", in: source)?.block == .codeBlock)
        #expect(style(plan, of: "```swift", in: source)?.block == .codeBlock)
    }

    @Test func syntaxOutsideTheLiveSetStaysMonospacedSource() throws {
        let source = """
        | a | b |
        | - | - |
        | 1 | 2 |

        ![alt](x.png) after

        <div>html</div>

        - [ ] task
        - [x] done

        foot[^1] note

        [^1]: the note

        """
        let plan = try plan(source)
        #expect(plan.markers.isEmpty)
        for text in ["| a | b |", "![alt]", "<div>", "- [ ] task", "- [x] done", "foot[^1]", "[^1]: the note"] {
            #expect(style(plan, of: text, in: source) == .raw, "\(text) stays source")
        }
    }

    @Test func koreanAndEmojiColumnsMapToTextStorageOffsets() throws {
        let source = "## 한글 제목 **굵게** 😀 *기울임*\r\n> 인용 `코드`\r\n"
        let plan = try plan(source)
        #expect(markup(plan, in: source) == ["## ", "**", "**", "*", "*", "> ", "`", "`"])
        #expect(style(plan, of: "굵게", in: source)?.bold == true)
        #expect(style(plan, of: "기울임", in: source)?.italic == true)
        #expect(style(plan, of: "코드", in: source)?.code == true)
        #expect(style(plan, of: "인용", in: source)?.quoteDepth == 1)
    }

    @Test func caretLineAndSelectedLinesRevealTheirMarkup() throws {
        let source = "# One\n**two**\n> three\n"
        let plan = try plan(source)
        let text = source as NSString
        func hidden(_ selection: NSRange) -> [String] {
            plan.hiddenMarkers(for: selection, in: text).map { text.substring(with: $0.range) }
        }
        #expect(hidden(NSRange(location: 0, length: 0)) == ["**", "**", "> "])
        #expect(hidden(NSRange(location: 8, length: 0)) == ["# ", "> "])
        #expect(hidden(NSRange(location: text.length, length: 0)) == ["# ", "**", "**", "> "])
        #expect(hidden(NSRange(location: 3, length: 6)) == ["> "])
        #expect(hidden(NSRange(location: 0, length: text.length)) == [])
    }

    @Test func fencedBlockRevealsAsOneUnit() throws {
        let source = "```\nbody\n```\n# H\n"
        let plan = try plan(source)
        let text = source as NSString
        let inside = plan.hiddenMarkers(for: NSRange(location: 5, length: 0), in: text)
        #expect(inside.map { text.substring(with: $0.range) } == ["# "])
        let outside = plan.hiddenMarkers(for: NSRange(location: 14, length: 0), in: text)
        #expect(outside.map { text.substring(with: $0.range) } == ["```", "```"])
    }

    @Test func changedRegionCoversOnlyTheEditedBlocks() throws {
        let before = try plan("# A\n\nfirst para\n\nlast **b**\n")
        let after = try plan("# A\n\nfirst paragraph\n\nlast **b**\n")
        let region = after.changedRegion(from: before)
        #expect(region == NSRange(location: 3, length: 19))
        let same = after.changedRegion(from: after)
        #expect(same.length == 0)
    }

    @Test func emptyAndWhitespaceOnlySourcesPlanNothing() throws {
        #expect(try plan("") == MarkdownLivePlan(length: 0, spans: [], markers: []))
        let blank = try plan("\n\n  \n")
        #expect(blank.spans.isEmpty && blank.markers.isEmpty && blank.length == 5)
    }
}
