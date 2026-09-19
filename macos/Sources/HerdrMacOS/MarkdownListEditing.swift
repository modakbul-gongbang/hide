import AppKit

/// The list editing rules a Markdown document follows on Enter, Tab,
/// Shift-Tab and Backspace (D-14). They are orca's Tiptap rules carried over
/// to source text: every function reads the caret's line, and answers with the
/// one edit the key means, or nil when the key keeps its ordinary meaning.
/// Both the Live and the Source view apply the same answers.
enum MarkdownListEditing {
    /// One list level is two spaces.
    static let indentUnit = 2

    enum Marker: Equatable {
        case bullet(Character)
        case ordered(number: Int, delimiter: Character)

        var text: String {
            switch self {
            case .bullet(let character): String(character)
            case .ordered(let number, let delimiter): "\(number)\(delimiter)"
            }
        }
    }

    /// A line that is a list item: its indent, its marker, and where the
    /// item's own text starts.
    struct Item: Equatable {
        var indent: Int
        var marker: Marker
        /// Offset of the first character after the marker and its space.
        var contentStart: Int
        var content: String

        var isEmpty: Bool { content.allSatisfy(\.isWhitespace) }
    }

    /// One edit: the range to replace, its replacement, and where the caret
    /// lands afterwards, all in UTF-16 offsets of the whole text.
    struct Edit: Equatable {
        var range: NSRange
        var replacement: String
        var caret: Int
    }

    /// The list item a line is, or nil. A task box (`- [ ]`, `- [x]`) is not
    /// one: Live does not draw it and Enter does not continue it (D-03).
    static func item(in line: String) -> Item? {
        let scalars = Array(line.utf16)
        var index = 0
        while index < scalars.count, scalars[index] == 0x20 { index += 1 }
        let indent = index
        guard index < scalars.count else { return nil }
        let marker: Marker
        if let bullet = "-*+".utf16.first(where: { $0 == scalars[index] }) {
            marker = .bullet(Character(UnicodeScalar(bullet)!))
            index += 1
        } else {
            var digits = 0
            var number = 0
            while index < scalars.count, (0x30...0x39).contains(scalars[index]), digits < 9 {
                number = number * 10 + Int(scalars[index] - 0x30)
                index += 1
                digits += 1
            }
            guard digits > 0, index < scalars.count, scalars[index] == 0x2E || scalars[index] == 0x29 else { return nil }
            marker = .ordered(number: number, delimiter: Character(UnicodeScalar(scalars[index])!))
            index += 1
        }
        // The marker needs a space after it, or to end the line (the item the
        // user is still typing).
        guard index == scalars.count || scalars[index] == 0x20 else { return nil }
        if index < scalars.count { index += 1 }
        let content = String(utf16CodeUnits: Array(scalars[index...]), count: scalars.count - index)
        if content.hasPrefix("[ ] ") || content.hasPrefix("[x] ") || content.hasPrefix("[X] ")
            || content == "[ ]" || content == "[x]" || content == "[X]" {
            return nil
        }
        return Item(indent: indent, marker: marker, contentStart: index, content: content)
    }

    // MARK: Keys

    /// Enter: a new item of the same kind after a non-empty item (2); the marker
    /// removed after an empty one (3); a lone `1. ` kept as typed (6).
    static func newline(in text: String, caret: Int) -> Edit? {
        guard let context = Context(text: text, caret: caret), caret >= context.line.location + context.item.contentStart else { return nil }
        var lines = context.lines
        let index = context.lineIndex
        let item = context.item
        if item.isEmpty {
            if case .ordered = item.marker, item.indent == 0, context.isSoleItem {
                return nil
            }
            lines[index] = ""
            let changed = renumber(&lines, column: item.indent, around: index + 1)
            return context.edit(lines: lines, touching: index...index, changed, caret: context.line.location)
        }
        let before = String(context.lineText.utf16.prefix(caret - context.line.location))!
        let after = String(context.lineText.utf16.dropFirst(caret - context.line.location))!
        let nextMarker: Marker = switch item.marker {
        case .bullet: item.marker
        case .ordered(let number, let delimiter): .ordered(number: number + 1, delimiter: delimiter)
        }
        let prefix = String(repeating: " ", count: item.indent) + nextMarker.text + " "
        lines[index] = before
        lines.insert(prefix + after, at: index + 1)
        let changed = renumber(&lines, column: item.indent, around: index + 1)
        let caret = context.line.location + before.utf16.count + 1 + prefix.utf16.count
        return context.edit(lines: lines, touching: index...(index + 1), changed, caret: caret)
    }

    /// Tab: the item moves one level in and, when numbered, counts in its new
    /// column (4); the column it left renumbers (7).
    static func indent(in text: String, caret: Int) -> Edit? {
        guard let context = Context(text: text, caret: caret) else { return nil }
        var lines = context.lines
        let index = context.lineIndex
        let item = context.item
        lines[index] = String(repeating: " ", count: indentUnit) + context.lineText
        let inner = renumber(&lines, column: item.indent + indentUnit, around: index)
        let outer = renumber(&lines, column: item.indent, around: index + 1)
        let caret = context.caretAfterPrefixChange(lines[index])
        return context.edit(lines: lines, touching: index...index, inner, outer, caret: caret)
    }

    /// Shift-Tab: one level out, back to the outer column's numbering (4, 7).
    static func outdent(in text: String, caret: Int) -> Edit? {
        guard let context = Context(text: text, caret: caret) else { return nil }
        let item = context.item
        guard item.indent > 0 else {
            return Edit(range: NSRange(location: caret, length: 0), replacement: "", caret: caret)
        }
        var lines = context.lines
        let index = context.lineIndex
        let removed = min(indentUnit, item.indent)
        lines[index] = String(context.lineText.utf16.dropFirst(removed))!
        let outer = renumber(&lines, column: item.indent - removed, around: index)
        let inner = renumber(&lines, column: item.indent, around: index + 1)
        let caret = context.caretAfterPrefixChange(lines[index])
        return context.edit(lines: lines, touching: index...index, inner, outer, caret: caret)
    }

    /// Backspace on an empty item removes the marker and keeps the line (5).
    static func deleteBackward(in text: String, caret: Int) -> Edit? {
        guard let context = Context(text: text, caret: caret), context.item.isEmpty,
              caret == NSMaxRange(context.line) else { return nil }
        var lines = context.lines
        let index = context.lineIndex
        lines[index] = String(repeating: " ", count: context.item.indent)
        let changed = renumber(&lines, column: context.item.indent, around: index + 1)
        return context.edit(lines: lines, touching: index...index, changed, caret: context.line.location + context.item.indent)
    }

    // MARK: Columns

    /// Renumbers the ordered items of one column around a line: the run of
    /// items at that indent, not interrupted by a shallower item or a line
    /// outside the list, counted from 1 in order. Deeper items are skipped.
    /// Returns the lines it changed.
    static func renumber(_ lines: inout [String], column indent: Int, around index: Int) -> ClosedRange<Int>? {
        guard index < lines.count else { return nil }
        var start = index
        while start > 0, belongs(lines[start - 1], toColumn: indent) { start -= 1 }
        var end = index
        while end + 1 < lines.count, belongs(lines[end + 1], toColumn: indent) { end += 1 }
        var number = 0
        var changed: ClosedRange<Int>?
        for line in start...end {
            guard let item = item(in: lines[line]), item.indent == indent else { continue }
            guard case .ordered(let current, let delimiter) = item.marker else { continue }
            number += 1
            if current != number {
                let space = item.contentStart > indent + item.marker.text.utf16.count ? " " : ""
                lines[line] = String(repeating: " ", count: indent) + "\(number)\(delimiter)" + space + item.content
                changed = (changed.map { min($0.lowerBound, line) } ?? line)...line
            }
        }
        return changed
    }

    /// A line stays in a column's run when it is an item at that indent or
    /// deeper, or a continuation line indented past the column.
    private static func belongs(_ line: String, toColumn indent: Int) -> Bool {
        if let item = item(in: line) { return item.indent >= indent }
        let leading = line.utf16.prefix { $0 == 0x20 }.count
        return leading > indent && leading < line.utf16.count
    }

    // MARK: Context

    /// The caret's line, parsed, with the whole text split into lines so an
    /// edit can rewrite the lines it touches and nothing else.
    private struct Context {
        let text: NSString
        let lines: [String]
        let lineIndex: Int
        /// The caret's line without its newline.
        let line: NSRange
        let lineText: String
        let item: Item
        let caret: Int

        init?(text: String, caret: Int) {
            let ns = text as NSString
            guard caret >= 0, caret <= ns.length else { return nil }
            let full = ns.lineRange(for: NSRange(location: caret, length: 0))
            let content = ns.substring(with: full)
            let trimmed = content.hasSuffix("\r\n") ? String(content.dropLast(2)) : (content.hasSuffix("\n") ? String(content.dropLast()) : content)
            guard let item = MarkdownListEditing.item(in: trimmed) else { return nil }
            self.text = ns
            self.lines = text.components(separatedBy: "\n")
            self.lineIndex = ns.substring(to: full.location).components(separatedBy: "\n").count - 1
            self.line = NSRange(location: full.location, length: trimmed.utf16.count)
            self.lineText = trimmed
            self.item = item
            self.caret = caret
        }

        /// No item of the same column directly above or below: the list has
        /// this one item.
        var isSoleItem: Bool {
            for neighbour in [lineIndex - 1, lineIndex + 1] where lines.indices.contains(neighbour) {
                if let other = MarkdownListEditing.item(in: lines[neighbour]), other.indent == item.indent { return false }
            }
            return true
        }

        /// The edit that replaces the touched lines of the original text with
        /// the rewritten lines at the same indices; `lines` may hold one line
        /// more than the original (a split), never fewer.
        func edit(lines: [String], touching: ClosedRange<Int>, _ changed: ClosedRange<Int>?..., caret: Int) -> Edit {
            let originalLines = self.lines
            let delta = lines.count - originalLines.count
            var from = touching.lowerBound
            var through = touching.upperBound
            for range in changed.compactMap({ $0 }) {
                from = min(from, range.lowerBound)
                through = max(through, range.upperBound)
            }
            let originalThrough = min(through - delta, originalLines.count - 1)
            let start = originalLines[..<from].reduce(0) { $0 + $1.utf16.count + 1 }
            let end = originalLines[...originalThrough].reduce(0) { $0 + $1.utf16.count + 1 } - 1
            let replacement = lines[from...through].joined(separator: "\n")
            return Edit(range: NSRange(location: start, length: end - start), replacement: replacement, caret: caret)
        }

        /// The caret keeps its place in the item's text when the prefix
        /// before the text changes length; inside the prefix it moves to the
        /// text start.
        func caretAfterPrefixChange(_ newLine: String) -> Int {
            let offset = caret - line.location
            let newStart = MarkdownListEditing.item(in: newLine)?.contentStart ?? 0
            if offset < item.contentStart { return line.location + newStart }
            return line.location + newStart + (offset - item.contentStart)
        }
    }
}

/// The key commands a Markdown text view routes through the rules above.
enum MarkdownListKeys {
    enum Command { case newline, tab, backtab, deleteBackward }

    /// Applies the rule for a command to the text view, or returns false so
    /// the view's own command runs. A composition in progress (marked text)
    /// always falls through (8).
    @MainActor
    static func handle(_ command: Command, in textView: NSTextView) -> Bool {
        guard textView.isEditable, !textView.hasMarkedText() else { return false }
        let selection = textView.selectedRange()
        guard selection.length == 0 else { return false }
        let text = textView.string
        let edit: MarkdownListEditing.Edit? = switch command {
        case .newline: MarkdownListEditing.newline(in: text, caret: selection.location)
        case .tab: MarkdownListEditing.indent(in: text, caret: selection.location)
        case .backtab: MarkdownListEditing.outdent(in: text, caret: selection.location)
        case .deleteBackward: MarkdownListEditing.deleteBackward(in: text, caret: selection.location)
        }
        guard let edit else { return false }
        if edit.range.length > 0 || !edit.replacement.isEmpty {
            textView.insertText(edit.replacement, replacementRange: edit.range)
        }
        textView.setSelectedRange(NSRange(location: edit.caret, length: 0))
        return true
    }
}
