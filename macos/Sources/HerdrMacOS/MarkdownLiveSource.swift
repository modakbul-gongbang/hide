import Foundation

/// The Live view's reading of a Markdown source: where each style applies and
/// which characters are markup the layout manager hides while no caret sits on
/// their line. Every value is a UTF-16 range into the source string, which is
/// the text storage itself; nothing here is rendered text, so the draft,
/// autosave, Find and selection all keep reading the source.
struct MarkdownLivePlan: Equatable, Sendable {
    /// UTF-16 length of the source the plan was computed for.
    var length: Int
    /// Sorted and non-overlapping; a character in no span carries the body style.
    var spans: [MarkdownLiveSpan]
    /// Sorted by location.
    var markers: [MarkdownLiveMarker]

    static let empty = MarkdownLivePlan(length: 0, spans: [], markers: [])
}

struct MarkdownLiveStyle: Hashable, Sendable {
    enum Block: Hashable, Sendable {
        case paragraph
        case heading(Int)
        case codeBlock
        case thematicBreak
        /// Syntax the Live view does not draw and leaves as monospaced source:
        /// tables, images, HTML, footnotes and task lists.
        case raw
    }

    var block: Block = .paragraph
    var quoteDepth = 0
    var listDepth = 0
    var bold = false
    var italic = false
    var strikethrough = false
    var code = false
    var link: URL?

    static let body = MarkdownLiveStyle()
    static let raw = MarkdownLiveStyle(block: .raw)
}

struct MarkdownLiveSpan: Hashable, Sendable {
    var range: NSRange
    var style: MarkdownLiveStyle
}

struct MarkdownLiveMarker: Hashable, Sendable {
    enum Kind: Hashable, Sendable {
        /// Drawn as nothing: heading hashes, emphasis delimiters, backticks,
        /// link brackets and URLs, quote prefixes, list indentation, fences.
        case hidden
        /// Drawn as a bullet in place of the `-`, `*` or `+` list marker.
        case bullet
        /// A code fence: drawn as nothing, and its line collapses to no height,
        /// so the block reads as its body alone.
        case fence
    }

    var range: NSRange
    var kind: Kind
    /// The characters whose lines reveal this marker when the selection touches
    /// them: the marker's own line, or the whole block for a fenced code block.
    var unit: NSRange
}

/// Parses a source with Foundation's Markdown parser and turns its runs, which
/// carry source positions, back into ranges over the source.
///
/// A run's source position covers the run's text and nothing else, so within a
/// block the characters no run covers are its markup. That reading is what
/// finds emphasis delimiters, backticks, link syntax, heading hashes, quote
/// prefixes and list indentation without a grammar of its own. The one block
/// Foundation gives no position for is the thematic break; its line is located
/// by syntax between the neighbouring blocks (`thematicBreakLine`).
enum MarkdownLiveSource {
    struct Failure: LocalizedError, Equatable {
        let reason: String
        var errorDescription: String? { reason }
    }

    /// Live is off above this size; the parse is whole-document per edit, and the
    /// bound is what keeps that cost predictable (D-07).
    static let byteLimit = 256 * 1024

    static func plan(for source: String) throws -> MarkdownLivePlan {
        // Foundation refuses an empty document rather than parsing nothing.
        guard !source.isEmpty else { return .empty }
        var options = AttributedString.MarkdownParsingOptions()
        options.interpretedSyntax = .full
        options.appliesSourcePositionAttributes = true
        let parsed = try NSAttributedString(markdown: source, options: options)
        let text = source as NSString
        let map = try SourceMap(source)
        let runs = try collectRuns(parsed, map: map, text: text)
        var builder = PlanBuilder(text: text)
        try builder.build(runs)
        return MarkdownLivePlan(length: text.length, spans: builder.spans, markers: builder.markers)
    }

    // MARK: - Runs

    struct Run {
        var range: NSRange
        var components: [PresentationIntent.IntentType]
        var inline: InlinePresentationIntent
        var link: URL?
        var image: Bool
        /// The innermost block this run belongs to, or nil for block HTML.
        var block: PresentationIntent.IntentType? { components.first }
    }

    private static func collectRuns(_ parsed: NSAttributedString, map: SourceMap, text: NSString) throws -> [Run] {
        var runs: [Run] = []
        var failure: Failure?
        var cursor = 0
        parsed.enumerateAttributes(in: NSRange(location: 0, length: parsed.length)) { attributes, _, stop in
            let intent = attributes[.presentationIntentAttributeName] as? PresentationIntent
            let inlineRaw = (attributes[.inlinePresentationIntent] as? NSNumber)?.uintValue ?? 0
            let inline = InlinePresentationIntent(rawValue: UInt(inlineRaw))
            let components = intent?.components ?? []
            // A thematic break has no source text; a soft or hard line break is
            // the newline the neighbouring runs already bound.
            if components.first?.kind == .thematicBreak {
                runs.append(Run(range: NSRange(location: NSNotFound, length: 0), components: components, inline: [], link: nil, image: false))
                return
            }
            guard let position = attributes[.markdownSourcePosition] as? AttributedString.MarkdownSourcePosition else {
                if inline.isDisjoint(with: [.softBreak, .lineBreak]) {
                    failure = Failure(reason: "A run has no source position")
                    stop.pointee = true
                }
                return
            }
            guard let range = map.range(of: position), range.location >= cursor, NSMaxRange(range) <= text.length else {
                failure = Failure(reason: "Source position \(position.startLine):\(position.startColumn) is outside the document")
                stop.pointee = true
                return
            }
            cursor = NSMaxRange(range)
            runs.append(Run(range: range, components: components, inline: inline,
                link: attributes[.link] as? URL, image: attributes[.imageURL] != nil))
        }
        if let failure { throw failure }
        return runs
    }

    // MARK: - Source positions

    /// Foundation reports a run as 1-based line numbers and 1-based UTF-8 byte
    /// columns with an inclusive end; the text storage addresses UTF-16 units.
    struct SourceMap {
        private let lineStartBytes: [Int]
        /// The UTF-16 offset at each UTF-8 byte offset, with one entry past the end.
        private let utf16ByByte: [Int32]

        init(_ source: String) throws {
            let units = Array(source.utf16)
            guard units.count < Int(Int32.max) else { throw Failure(reason: "The document is too long to map") }
            var lineStarts = [0]
            var utf16ByByte: [Int32] = []
            utf16ByByte.reserveCapacity(source.utf8.count + 1)
            for (index, unit) in units.enumerated() {
                let bytes: Int
                switch unit {
                case 0..<0x80: bytes = 1
                case 0x80..<0x800: bytes = 2
                case 0xD800..<0xDC00: bytes = 4
                case 0xDC00..<0xE000: bytes = 0
                default: bytes = 3
                }
                for _ in 0..<bytes { utf16ByByte.append(Int32(index)) }
                // The parser ends a line at `\n`, `\r\n` or a lone `\r`.
                if unit == 0x0A || (unit == 0x0D && (index + 1 >= units.count || units[index + 1] != 0x0A)) {
                    lineStarts.append(utf16ByByte.count)
                }
            }
            utf16ByByte.append(Int32(units.count))
            self.utf16ByByte = utf16ByByte
            self.lineStartBytes = lineStarts
        }

        func range(of position: AttributedString.MarkdownSourcePosition) -> NSRange? {
            guard let start = byteOffset(line: position.startLine, column: position.startColumn),
                  let end = byteOffset(line: position.endLine, column: position.endColumn + 1),
                  start <= end, end < utf16ByByte.count
            else { return nil }
            let location = Int(utf16ByByte[start])
            return NSRange(location: location, length: Int(utf16ByByte[end]) - location)
        }

        private func byteOffset(line: Int, column: Int) -> Int? {
            guard line >= 1, line <= lineStartBytes.count, column >= 1 else { return nil }
            return lineStartBytes[line - 1] + column - 1
        }
    }

    // MARK: - Plan

    struct PlanBuilder {
        let text: NSString
        var spans: [MarkdownLiveSpan] = []
        var markers: [MarkdownLiveMarker] = []
        /// List items that already placed their marker: only an item's first
        /// block carries `- ` or `1. `.
        private var markedListItems: Set<Int> = []

        init(text: NSString) { self.text = text }

        mutating func build(_ runs: [Run]) throws {
            var index = 0
            var previousBlockEnd = 0
            while index < runs.count {
                let run = runs[index]
                guard let block = run.block else {
                    // Block HTML: the run is the whole block and stays source.
                    spans.append(MarkdownLiveSpan(range: run.range, style: .raw))
                    previousBlockEnd = NSMaxRange(run.range)
                    index += 1
                    continue
                }
                if block.kind == .thematicBreak {
                    let nextStart = runs[(index + 1)...].first { $0.range.location != NSNotFound }?.range.location ?? text.length
                    if let line = thematicBreakLine(after: previousBlockEnd, before: nextStart) {
                        spans.append(MarkdownLiveSpan(range: line, style: MarkdownLiveStyle(block: .thematicBreak)))
                        markers.append(MarkdownLiveMarker(range: line, kind: .hidden, unit: line))
                        previousBlockEnd = NSMaxRange(line)
                    }
                    index += 1
                    continue
                }
                // Every run of one block, in order; a table is one block across its cells.
                let groupIdentity = tableIdentity(run.components) ?? block.identity
                var end = index
                while end + 1 < runs.count, runs[end + 1].range.location != NSNotFound,
                      (tableIdentity(runs[end + 1].components) ?? runs[end + 1].block?.identity) == groupIdentity
                {
                    end += 1
                }
                let group = Array(runs[index...end])
                let extent = blockExtent(group)
                previousBlockEnd = NSMaxRange(extent)
                if isRaw(group) {
                    spans.append(MarkdownLiveSpan(range: extent, style: .raw))
                } else {
                    layOut(group, extent: extent)
                }
                index = end + 1
            }
        }

        private func tableIdentity(_ components: [PresentationIntent.IntentType]) -> Int? {
            components.first { if case .table = $0.kind { return true }; return false }?.identity
        }

        /// The lines a block occupies: from the start of its first run's line to
        /// the end of its last run's line, newline excluded. A fenced code block's
        /// run already spans both fences.
        private func blockExtent(_ group: [Run]) -> NSRange {
            let first = group.first!.range
            let last = group.last!.range
            var start = 0
            text.getLineStart(&start, end: nil, contentsEnd: nil, for: NSRange(location: first.location, length: 0))
            var contentsEnd = 0
            let lastLocation = max(last.location, NSMaxRange(last) - 1)
            text.getLineStart(nil, end: nil, contentsEnd: &contentsEnd, for: NSRange(location: lastLocation, length: 0))
            return NSRange(location: start, length: max(contentsEnd, NSMaxRange(last)) - start)
        }

        /// Syntax D-03 leaves as monospaced source.
        private func isRaw(_ group: [Run]) -> Bool {
            if group.contains(where: { $0.image || $0.inline.contains(.inlineHTML) }) { return true }
            if tableIdentity(group[0].components) != nil { return true }
            let firstText = text.substring(with: group[0].range)
            if group[0].components.contains(where: { if case .listItem = $0.kind { return true }; return false }),
               firstText.hasPrefix("[ ] ") || firstText.hasPrefix("[x] ") || firstText.hasPrefix("[X] ")
            {
                return true
            }
            return group.contains { text.substring(with: $0.range).contains("[^") }
        }

        private mutating func layOut(_ group: [Run], extent: NSRange) {
            var base = MarkdownLiveStyle()
            var listItem: (identity: Int, ordered: Bool)?
            for (offset, component) in group[0].components.enumerated() {
                switch component.kind {
                case .header(let level): base.block = .heading(level)
                case .codeBlock: base.block = .codeBlock
                case .blockQuote: base.quoteDepth += 1
                case .listItem:
                    base.listDepth += 1
                    if listItem == nil {
                        let ordered = offset + 1 < group[0].components.count
                            && group[0].components[offset + 1].kind == .orderedList
                        listItem = (component.identity, ordered)
                    }
                default: break
                }
            }
            if base.block == .codeBlock {
                spans.append(MarkdownLiveSpan(range: extent, style: base))
                markFences(extent)
                return
            }
            var cursor = extent.location
            for run in group {
                if run.range.location > cursor {
                    spans.append(MarkdownLiveSpan(range: NSRange(location: cursor, length: run.range.location - cursor), style: base))
                    markGap(NSRange(location: cursor, length: run.range.location - cursor))
                }
                var style = base
                style.bold = run.inline.contains(.stronglyEmphasized)
                style.italic = run.inline.contains(.emphasized)
                style.strikethrough = run.inline.contains(.strikethrough)
                style.code = run.inline.contains(.code)
                style.link = run.link
                spans.append(MarkdownLiveSpan(range: run.range, style: style))
                cursor = NSMaxRange(run.range)
            }
            if NSMaxRange(extent) > cursor {
                spans.append(MarkdownLiveSpan(range: NSRange(location: cursor, length: NSMaxRange(extent) - cursor), style: base))
                markGap(NSRange(location: cursor, length: NSMaxRange(extent) - cursor))
            }
            if let listItem, !markedListItems.contains(listItem.identity) {
                markedListItems.insert(listItem.identity)
                placeListMarker(in: extent, ordered: listItem.ordered)
            }
        }

        /// Characters of a block no run covers are markup, one marker per line so
        /// the newline itself stays a line break.
        private mutating func markGap(_ gap: NSRange) {
            var location = gap.location
            let end = NSMaxRange(gap)
            while location < end {
                let line = lineRange(containing: location)
                var contentsEnd = 0
                text.getLineStart(nil, end: nil, contentsEnd: &contentsEnd, for: NSRange(location: location, length: 0))
                let stop = min(contentsEnd, end)
                if stop > location {
                    markers.append(MarkdownLiveMarker(range: NSRange(location: location, length: stop - location), kind: .hidden, unit: line))
                }
                guard NSMaxRange(line) > location else { break }
                location = NSMaxRange(line)
            }
        }

        /// The gap before a list item's first run was hidden whole; carve the
        /// marker back out so `-` draws as a bullet and `1.` keeps its digits.
        private mutating func placeListMarker(in extent: NSRange, ordered: Bool) {
            let line = lineRange(containing: extent.location)
            let content = text.substring(with: line)
            var indent = 0
            for character in content.utf16 { if character == 0x20 || character == 0x09 { indent += 1 } else { break } }
            let rest = content.utf16.dropFirst(indent)
            let markerLength: Int
            if ordered {
                var digits = 0
                for character in rest { if (0x30...0x39).contains(character) { digits += 1 } else { break } }
                guard digits > 0, let closer = rest.dropFirst(digits).first, closer == 0x2E || closer == 0x29 else { return }
                markerLength = digits + 1
            } else {
                guard let marker = rest.first, marker == 0x2D || marker == 0x2A || marker == 0x2B else { return }
                markerLength = 1
            }
            // Keep the marker and the space after it visible; the gap after them
            // and the indentation before them stay hidden.
            let markerRange = NSRange(location: line.location + indent, length: markerLength)
            let keep = NSRange(location: markerRange.location, length: markerLength + 1)
            guard let gapIndex = markers.lastIndex(where: { NSIntersectionRange($0.range, keep).length > 0 }) else { return }
            let gap = markers[gapIndex].range
            markers.remove(at: gapIndex)
            var replacement: [MarkdownLiveMarker] = []
            if keep.location > gap.location {
                replacement.append(MarkdownLiveMarker(range: NSRange(location: gap.location, length: keep.location - gap.location), kind: .hidden, unit: line))
            }
            if !ordered {
                replacement.append(MarkdownLiveMarker(range: markerRange, kind: .bullet, unit: line))
            }
            if NSMaxRange(gap) > NSMaxRange(keep) {
                replacement.append(MarkdownLiveMarker(range: NSRange(location: NSMaxRange(keep), length: NSMaxRange(gap) - NSMaxRange(keep)), kind: .hidden, unit: line))
            }
            markers.insert(contentsOf: replacement, at: gapIndex)
        }

        /// Both fence lines of a code block hide, and the whole block is the
        /// unit that reveals them (D-04).
        private mutating func markFences(_ extent: NSRange) {
            let firstLine = lineRange(containing: extent.location)
            guard isFence(text.substring(with: firstLine)) else { return }
            var firstEnd = 0
            text.getLineStart(nil, end: nil, contentsEnd: &firstEnd, for: NSRange(location: extent.location, length: 0))
            markers.append(MarkdownLiveMarker(range: NSRange(location: extent.location, length: min(firstEnd, NSMaxRange(extent)) - extent.location), kind: .fence, unit: extent))
            let lastLine = lineRange(containing: max(extent.location, NSMaxRange(extent) - 1))
            guard lastLine.location > firstLine.location, isFence(text.substring(with: lastLine)) else { return }
            markers.append(MarkdownLiveMarker(range: NSRange(location: lastLine.location, length: NSMaxRange(extent) - lastLine.location), kind: .fence, unit: extent))
        }

        private func isFence(_ line: String) -> Bool {
            let trimmed = line.drop { $0 == " " }
            return trimmed.hasPrefix("```") || trimmed.hasPrefix("~~~")
        }

        /// A thematic break is the first line of `---`, `***` or `___` syntax
        /// between the previous block's end and the next block's start.
        private func thematicBreakLine(after start: Int, before end: Int) -> NSRange? {
            var location = start
            while location < end, location < text.length {
                var lineEnd = 0
                var contentsEnd = 0
                text.getLineStart(nil, end: &lineEnd, contentsEnd: &contentsEnd, for: NSRange(location: location, length: 0))
                let line = text.substring(with: NSRange(location: location, length: contentsEnd - location))
                if Self.isThematicBreak(line) {
                    return NSRange(location: location, length: contentsEnd - location)
                }
                if lineEnd <= location { break }
                location = lineEnd
            }
            return nil
        }

        static func isThematicBreak(_ line: String) -> Bool {
            let content = line.filter { $0 != " " && $0 != "\t" }
            guard content.count >= 3, let marker = content.first, "-*_".contains(marker) else { return false }
            return content.allSatisfy { $0 == marker } && line.prefix { $0 == " " }.count <= 3
        }

        /// The whole line holding `location`, newline included, so a marker's unit
        /// meets a selection expanded to line boundaries.
        private func lineRange(containing location: Int) -> NSRange {
            text.lineRange(for: NSRange(location: min(location, text.length), length: 0))
        }
    }
}

extension MarkdownLivePlan {
    /// The range whose attributes differ from `previous`, in this plan's
    /// coordinates: everything between the spans that stayed the same before
    /// the edit and the spans that stayed the same after it, shifted by the
    /// edit's length. A keystroke restyles its own block, not the document.
    func changedRegion(from previous: MarkdownLivePlan) -> NSRange {
        let delta = length - previous.length
        var prefix = 0
        while prefix < previous.spans.count, prefix < spans.count, previous.spans[prefix] == spans[prefix] {
            prefix += 1
        }
        if prefix == spans.count, prefix == previous.spans.count, delta == 0 {
            return NSRange(location: length, length: 0)
        }
        var suffix = 0
        while suffix < previous.spans.count - prefix, suffix < spans.count - prefix {
            let old = previous.spans[previous.spans.count - 1 - suffix]
            let new = spans[spans.count - 1 - suffix]
            guard old.style == new.style, old.range.length == new.range.length,
                  old.range.location + delta == new.range.location else { break }
            suffix += 1
        }
        let start = prefix > 0 ? NSMaxRange(spans[prefix - 1].range) : 0
        let end = suffix > 0 ? spans[spans.count - suffix].range.location : length
        return NSRange(location: start, length: max(0, end - start))
    }

    /// Which markers stay hidden for a selection: those whose unit shares no
    /// line with it. The caret's line, and every line a selection crosses,
    /// shows its source (D-04).
    func hiddenMarkers(for selection: NSRange, in text: NSString) -> [MarkdownLiveMarker] {
        let active = text.lineRange(for: NSRange(location: min(selection.location, text.length),
            length: min(selection.length, max(0, text.length - selection.location))))
        return markers.filter { NSIntersectionRange($0.unit, active).length == 0 }
    }
}
