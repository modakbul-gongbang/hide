import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Markdown list editing rules")
struct MarkdownListEditingTests {
    /// Applies an edit and returns the text with `|` where the caret lands.
    private func applied(_ edit: MarkdownListEditing.Edit?, to text: String) -> String? {
        guard let edit else { return nil }
        let result = (text as NSString).replacingCharacters(in: edit.range, with: edit.replacement) as NSString
        return result.substring(to: edit.caret) + "|" + result.substring(from: edit.caret)
    }

    private func caret(_ marked: String) -> (String, Int) {
        let ns = marked as NSString
        let at = ns.range(of: "|").location
        return (ns.replacingCharacters(in: NSRange(location: at, length: 1), with: ""), at)
    }

    private func newline(_ marked: String) -> String? { let (t, c) = caret(marked); return applied(MarkdownListEditing.newline(in: t, caret: c), to: t) }
    private func tab(_ marked: String) -> String? { let (t, c) = caret(marked); return applied(MarkdownListEditing.indent(in: t, caret: c), to: t) }
    private func backtab(_ marked: String) -> String? { let (t, c) = caret(marked); return applied(MarkdownListEditing.outdent(in: t, caret: c), to: t) }
    private func backspace(_ marked: String) -> String? { let (t, c) = caret(marked); return applied(MarkdownListEditing.deleteBackward(in: t, caret: c), to: t) }

    @Test func aLineIsAnItemByItsMarkerAndNotByATaskBox() {
        #expect(MarkdownListEditing.item(in: "- one") == .init(indent: 0, marker: .bullet("-"), contentStart: 2, content: "one"))
        #expect(MarkdownListEditing.item(in: "  12. two") == .init(indent: 2, marker: .ordered(number: 12, delimiter: "."), contentStart: 6, content: "two"))
        #expect(MarkdownListEditing.item(in: "* ") == .init(indent: 0, marker: .bullet("*"), contentStart: 2, content: ""))
        #expect(MarkdownListEditing.item(in: "1.") == .init(indent: 0, marker: .ordered(number: 1, delimiter: "."), contentStart: 2, content: ""))
        #expect(MarkdownListEditing.item(in: "-one") == nil)
        #expect(MarkdownListEditing.item(in: "1 one") == nil)
        #expect(MarkdownListEditing.item(in: "plain") == nil)
        #expect(MarkdownListEditing.item(in: "- [ ] task") == nil)
        #expect(MarkdownListEditing.item(in: "- [x] done") == nil)
    }

    @Test func enterContinuesAnItemWithTheSameMarkerOrTheNextNumber() {
        #expect(newline("- one|") == "- one\n- |")
        #expect(newline("  * one|") == "  * one\n  * |")
        #expect(newline("1. one|") == "1. one\n2. |")
        #expect(newline("- ab|cd") == "- ab\n- |cd")
        #expect(newline("1. one|\n2. two") == "1. one\n2. |\n3. two")
        #expect(newline("1. one\n  1. a|\n  2. b\n2. two") == "1. one\n  1. a\n  2. |\n  3. b\n2. two")
        #expect(newline("한국어 문장|") == nil)
        #expect(newline("-| one") == nil, "inside the marker the key keeps its meaning")
    }

    @Test func enterOnAnEmptyItemLeavesTheListAndRenumbersBelow() {
        #expect(newline("- one\n- |") == "- one\n|")
        #expect(newline("1. one\n2. |\n3. three") == "1. one\n|\n1. three")
        #expect(newline("1. one\n  1. a\n  2. |\n2. two") == "1. one\n  1. a\n|\n2. two")
        #expect(newline("- one\n- |\nafter") == "- one\n|\nafter")
    }

    @Test func enterOnALoneNumberedMarkerKeepsItAsText() {
        #expect(newline("1. |") == nil)
        #expect(newline("text\n\n1. |\n\nmore") == nil)
        #expect(newline("1. one\n2. |") == "1. one\n|", "a second item exits as usual")
        #expect(newline("  1. |") == "  |".replacingOccurrences(of: "  |", with: "|"), "a nested lone item exits")
    }

    @Test func tabIndentsAndCountsInTheNewColumn() {
        #expect(tab("- one\n- tw|o") == "- one\n  - tw|o")
        #expect(tab("1. one\n2. two|\n3. three") == "1. one\n  1. two|\n2. three")
        #expect(tab("1. one\n  1. a\n  2. b\n2. c|\n3. d") == "1. one\n  1. a\n  2. b\n  3. c|\n2. d")
        #expect(tab("- |one") == "  - |one")
        #expect(tab("|- one") == "  - |one", "inside the prefix the caret moves to the text")
        #expect(tab("plain|") == nil, "outside a list Tab is a tab character")
    }

    @Test func shiftTabOutdentsBackToTheOuterNumbering() {
        #expect(backtab("- one\n  - tw|o") == "- one\n- tw|o")
        #expect(backtab("1. one\n  1. a\n  2. b|\n  3. c\n2. two") == "1. one\n  1. a\n2. b|\n  1. c\n3. two")
        let (text, at) = caret("- on|e")
        #expect(MarkdownListEditing.outdent(in: text, caret: at) == .init(range: NSRange(location: at, length: 0), replacement: "", caret: at), "at the top level the key is consumed and nothing moves")
        #expect(backtab("plain|") == nil)
    }

    @Test func backspaceOnAnEmptyItemRemovesOnlyTheMarker() {
        #expect(backspace("- one\n- |") == "- one\n|")
        #expect(backspace("1. one\n  1. |\n2. two") == "1. one\n  |\n2. two")
        #expect(backspace("1. one\n2. |\n3. three") == "1. one\n|\n1. three")
        #expect(backspace("- on|e") == nil, "with text the key deletes as usual")
        #expect(backspace("- |") == "|")
    }

    @Test func renumberingStaysInsideOneColumnAndStopsAtABlankLine() {
        var lines = ["1. a", "3. b", "  5. x", "  9. y", "7. c", "", "4. other"]
        let changed = MarkdownListEditing.renumber(&lines, column: 0, around: 1)
        #expect(lines == ["1. a", "2. b", "  5. x", "  9. y", "3. c", "", "4. other"])
        #expect(changed == 1...4)
        var nested = ["1. a", "  5. x", "  9. y", "2. b", "  4. z"]
        _ = MarkdownListEditing.renumber(&nested, column: 2, around: 1)
        #expect(nested == ["1. a", "  1. x", "  2. y", "2. b", "  4. z"], "the column under the next parent is another run")
    }
}
