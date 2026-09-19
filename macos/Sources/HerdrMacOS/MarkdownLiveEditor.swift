import AppKit
import SwiftUI

/// The Live Markdown view: one editable text view whose storage is the source,
/// with the plan's styles applied as attributes and markup glyphs left
/// ungenerated on lines the caret is not on (D-01, D-04). Draft, autosave, Find
/// and selection therefore read the same storage the Source editor would.
struct MarkdownLiveEditor: NSViewRepresentable {
    @Binding var text: String
    let isEditable: Bool
    /// The file editor carries the scale of the pane whose chords last changed it.
    let textScale: CGFloat
    var findRequest = 0
    let openLink: (URL) -> Void
    /// Why formatting is off for the current source, or nil once a parse
    /// succeeds again (D-09).
    let reportParseFailure: (String?) -> Void
    var parse: @Sendable (String) throws -> MarkdownLivePlan = MarkdownLiveSource.plan

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text, scale: textScale, parse: parse, openLink: openLink, reportParseFailure: reportParseFailure)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let storage = NSTextStorage()
        let layoutManager = MarkdownLiveLayoutManager()
        layoutManager.delegate = context.coordinator.glyphs
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: HideTheme.Editor.documentWidth, height: .greatestFiniteMagnitude))
        container.widthTracksTextView = true
        layoutManager.addTextContainer(container)

        let textView = MarkdownLiveTextView(frame: .zero, textContainer: container)
        textView.minSize = .zero
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.delegate = context.coordinator
        // The plan's styles are per-range attributes, which is rich text to
        // AppKit: a plain-text view keeps one attribute set for the whole
        // storage and re-applies its typing attributes to all of it, which
        // turned a document into the heading the caret was in.
        textView.isRichText = true
        textView.usesFontPanel = false
        textView.usesRuler = false
        textView.importsGraphics = false
        textView.allowsImageEditing = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isEditable = isEditable
        textView.drawsBackground = true
        textView.backgroundColor = HideTheme.Native.background
        textView.insertionPointColor = HideTheme.Native.primary
        textView.linkTextAttributes = [
            .foregroundColor: HideTheme.Native.accent,
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .cursor: NSCursor.pointingHand,
        ]
        textView.openLink = { [weak coordinator = context.coordinator] url in coordinator?.openLink(url) }
        HighlightedCodeEditor.enableFindBar(on: textView)

        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = true
        scrollView.backgroundColor = HideTheme.Native.background
        scrollView.documentView = textView
        context.coordinator.textView = textView
        context.coordinator.replaceText(text)
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        guard let textView = coordinator.textView else { return }
        coordinator.openLink = openLink
        coordinator.reportParseFailure = reportParseFailure
        textView.isEditable = isEditable
        if coordinator.findRequest != findRequest {
            coordinator.findRequest = findRequest
            textView.window?.makeFirstResponder(textView)
            let item = NSMenuItem()
            item.tag = NSTextFinder.Action.showFindInterface.rawValue
            textView.performFindPanelAction(item)
        }
        if coordinator.scale != textScale {
            coordinator.scale = textScale
            coordinator.reapplyTypography()
        }
        guard textView.string != text, !coordinator.isForwardingChange else { return }
        coordinator.replaceText(text)
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate {
        @Binding private var text: String
        weak var textView: MarkdownLiveTextView?
        var scale: CGFloat
        var findRequest = 0
        var isForwardingChange = false
        var openLink: (URL) -> Void
        var reportParseFailure: (String?) -> Void
        private let parse: @Sendable (String) throws -> MarkdownLivePlan
        private var typography: MarkdownLiveTypography

        private(set) var plan = MarkdownLivePlan.empty
        /// Generates the glyphs, so it holds the markers currently drawn as
        /// nothing or as a bullet.
        let glyphs = MarkdownLiveGlyphGenerator()
        var hidden: [MarkdownLiveMarker] { glyphs.hidden }
        /// One parse runs at a time, off the main thread, on the source as it was
        /// when it started; an edit during the run marks it stale and one more
        /// parse follows, so a burst of keystrokes costs at most two parses
        /// rather than one each (D-07).
        private var version = 0
        private var parseInFlight = false
        private var parsePending = false
        private var reportedFailure: String?

        init(text: Binding<String>, scale: CGFloat, parse: @escaping @Sendable (String) throws -> MarkdownLivePlan,
             openLink: @escaping (URL) -> Void, reportParseFailure: @escaping (String?) -> Void)
        {
            _text = text
            self.scale = scale
            self.parse = parse
            self.openLink = openLink
            self.reportParseFailure = reportParseFailure
            typography = MarkdownLiveTypography(scale: scale)
        }

        // MARK: Source

        /// A snapshot replacement: the whole storage changes, so the plan starts over.
        func replaceText(_ text: String) {
            guard let textView, let storage = textView.textStorage else { return }
            let selection = textView.selectedRange()
            storage.beginEditing()
            storage.replaceCharacters(in: NSRange(location: 0, length: storage.length), with: text)
            storage.setAttributes(typography.attributes(for: .body), range: NSRange(location: 0, length: storage.length))
            storage.endEditing()
            // Replacement bypasses the typing attributes; without this an empty
            // document types in Helvetica.
            textView.typingAttributes = typography.attributes(for: .body)
            textView.setSelectedRange(NSRange(location: min(selection.location, storage.length), length: 0))
            plan = MarkdownLivePlan(length: storage.length, spans: [], markers: [])
            glyphs.hidden = []
            scheduleParse()
        }

        func textDidChange(_ notification: Notification) {
            guard let textView else { return }
            isForwardingChange = true
            text = textView.string
            isForwardingChange = false
            scheduleParse()
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            refreshHidden()
        }

        // MARK: Parsing

        private func scheduleParse() {
            version += 1
            if parseInFlight { parsePending = true } else { startParse() }
        }

        private func startParse() {
            guard let textView else { return }
            parseInFlight = true
            parsePending = false
            let version = version
            let source = textView.string
            let parse = parse
            Task { @MainActor [weak self] in
                // Foundation's parser autoreleases tens of megabytes per run and
                // a worker thread drains only when it idles; without the pool
                // sixty keystrokes on a 10,000-line document grew the process
                // by a gigabyte.
                let outcome = await Task.detached(priority: .userInitiated) {
                    autoreleasepool { Result { try parse(source) } }
                }.value
                self?.finishParse(outcome, version: version)
            }
        }

        private func finishParse(_ outcome: Result<MarkdownLivePlan, Error>, version: Int) {
            parseInFlight = false
            guard let textView else { return }
            // Re-styling under an IME composition would end it; the commit
            // fires another change, which parses again.
            if textView.hasMarkedText() { parsePending = false; return }
            if version != self.version { startParse(); return }
            let failure: String?
            switch outcome {
            case .success(let plan):
                apply(plan)
                failure = nil
            case .failure(let error):
                let length = textView.textStorage?.length ?? 0
                apply(MarkdownLivePlan(length: length,
                    spans: [MarkdownLiveSpan(range: NSRange(location: 0, length: length), style: .raw)], markers: []))
                failure = error.localizedDescription
            }
            if failure != reportedFailure {
                reportedFailure = failure
                reportParseFailure(failure)
            }
            if parsePending { startParse() }
        }

        /// Attributes change only where the plan changed: the spans before and
        /// after the edit that are the same, shifted by the edit's length, stay
        /// as they are, so a keystroke re-styles its own block rather than the
        /// document.
        private func apply(_ next: MarkdownLivePlan) {
            guard let textView, let storage = textView.textStorage, storage.length == next.length else { return }
            let previous = plan
            plan = next
            let region = next.changedRegion(from: previous)
            if region.length > 0 {
                // The pool bounds what a whole-document apply leaves behind
                // before the run loop turns: a 10,000-line open otherwise
                // peaked at two gigabytes.
                autoreleasepool {
                    storage.beginEditing()
                    storage.setAttributes(typography.attributes(for: .body), range: region)
                    for span in next.spans where NSMaxRange(span.range) > region.location && span.range.location < NSMaxRange(region) {
                        storage.setAttributes(typography.attributes(for: span.style), range: NSIntersectionRange(span.range, region))
                    }
                    storage.endEditing()
                }
            }
            textView.typingAttributes = typography.attributes(for: .body)
            refreshHidden()
        }

        func reapplyTypography() {
            typography = MarkdownLiveTypography(scale: scale)
            let current = plan
            plan = MarkdownLivePlan(length: current.length, spans: [], markers: current.markers)
            apply(current)
        }

        // MARK: Hiding

        private func refreshHidden() {
            guard let textView, let layoutManager = textView.layoutManager else { return }
            let next = plan.hiddenMarkers(for: textView.selectedRange(), in: textView.string as NSString)
            let previous = glyphs.hidden
            glyphs.hidden = next
            // Both lists are sorted subsets of the plan's markers; what is in one
            // and not the other is what changed appearance.
            var i = 0, j = 0
            var changed: [NSRange] = []
            while i < previous.count || j < next.count {
                if j == next.count || (i < previous.count && previous[i].range.location < next[j].range.location) {
                    changed.append(previous[i].range); i += 1
                } else if i == previous.count || next[j].range.location < previous[i].range.location {
                    changed.append(next[j].range); j += 1
                } else if previous[i] == next[j] {
                    i += 1; j += 1
                } else {
                    changed.append(previous[i].range); changed.append(next[j].range); i += 1; j += 1
                }
            }
            for range in changed {
                let clipped = NSIntersectionRange(range, NSRange(location: 0, length: layoutManager.textStorage?.length ?? 0))
                guard clipped.length > 0 else { continue }
                layoutManager.invalidateGlyphs(forCharacterRange: clipped, changeInLength: 0, actualCharacterRange: nil)
                layoutManager.invalidateLayout(forCharacterRange: clipped, actualCharacterRange: nil)
                layoutManager.invalidateDisplay(forCharacterRange: clipped)
            }
        }
    }
}

/// Leaves markup ungenerated: a hidden marker's characters get the null glyph
/// property, so they take no space and draw nothing while the text storage
/// still holds them; a bullet marker's `-` is drawn with the font's bullet; a
/// fence's line is given no height, since a paragraph always gets a line
/// fragment and a null newline alone would leave an empty one.
/// The layout manager calls this on the main thread from within layout.
final class MarkdownLiveGlyphGenerator: NSObject, NSLayoutManagerDelegate {
    /// Sorted by location; replaced whole by the coordinator.
    var hidden: [MarkdownLiveMarker] = []

    /// The first hidden marker ending after a character: a binary search, because
    /// glyph generation asks once per run and a document has thousands of markers.
    private func firstMarkerIndex(endingAfter characterIndex: Int) -> Int {
        var low = 0, high = hidden.count
        while low < high {
            let mid = (low + high) / 2
            if NSMaxRange(hidden[mid].range) <= characterIndex { low = mid + 1 } else { high = mid }
        }
        return low
    }

    /// The hidden marker covering a character, if any.
    func marker(at characterIndex: Int) -> MarkdownLiveMarker? {
        let index = firstMarkerIndex(endingAfter: characterIndex)
        guard index < hidden.count, NSLocationInRange(characterIndex, hidden[index].range) else { return nil }
        return hidden[index]
    }

    func layoutManager(
        _ layoutManager: NSLayoutManager, shouldGenerateGlyphs glyphs: UnsafePointer<CGGlyph>,
        properties: UnsafePointer<NSLayoutManager.GlyphProperty>, characterIndexes: UnsafePointer<Int>,
        font: NSFont, forGlyphRange glyphRange: NSRange
    ) -> Int {
        guard !hidden.isEmpty, glyphRange.length > 0 else { return 0 }
        let first = characterIndexes[0]
        let last = characterIndexes[glyphRange.length - 1]
        let firstHidden = firstMarkerIndex(endingAfter: first)
        guard firstHidden < hidden.count, hidden[firstHidden].range.location <= last else { return 0 }
        var replacementGlyphs = Array(UnsafeBufferPointer(start: glyphs, count: glyphRange.length))
        var replacementProperties = Array(UnsafeBufferPointer(start: properties, count: glyphRange.length))
        var changed = false
        for offset in 0..<glyphRange.length {
            guard let marker = marker(at: characterIndexes[offset]) else { continue }
            switch marker.kind {
            case .hidden, .fence:
                replacementProperties[offset] = .null
                changed = true
            case .bullet:
                var bullet = CGGlyph(0)
                var character: UniChar = 0x2022
                if CTFontGetGlyphsForCharacters(font, &character, &bullet, 1) {
                    replacementGlyphs[offset] = bullet
                    changed = true
                }
            }
        }
        guard changed else { return 0 }
        layoutManager.setGlyphs(replacementGlyphs, properties: replacementProperties,
            characterIndexes: characterIndexes, font: font, forGlyphRange: glyphRange)
        return glyphRange.length
    }

    func layoutManager(
        _ layoutManager: NSLayoutManager, shouldSetLineFragmentRect lineFragmentRect: UnsafeMutablePointer<NSRect>,
        lineFragmentUsedRect: UnsafeMutablePointer<NSRect>, baselineOffset: UnsafeMutablePointer<CGFloat>,
        in textContainer: NSTextContainer, forGlyphRange glyphRange: NSRange
    ) -> Bool {
        guard !hidden.isEmpty else { return false }
        // Null glyphs join a neighbouring fragment, so a hidden fence line's own
        // fragment is its newline alone; the fence is the marker ending there.
        let characters = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)
        guard characters.length == 1, characters.location > 0,
              let marker = marker(at: characters.location - 1), marker.kind == .fence,
              NSMaxRange(marker.range) == characters.location else { return false }
        lineFragmentRect.pointee.size.height = 0
        lineFragmentUsedRect.pointee.size.height = 0
        return true
    }
}

/// The text view keeps the 720pt reading measure centred in whatever width the
/// pane gives it, and opens a link on Command-click only; a plain click on a
/// link places the caret, because in an editor a click is an edit (D-08).
final class MarkdownLiveTextView: NSTextView {
    var openLink: ((URL) -> Void)?

    /// Paste and drop bring text only: the source is what gets styled, so
    /// attributes riding on a rich clipboard have nowhere to go.
    override var readablePasteboardTypes: [NSPasteboard.PasteboardType] { [.string] }

    override func setFrameSize(_ newSize: NSSize) {
        let horizontal = max(HideTheme.spacingLG, (newSize.width - HideTheme.Editor.documentWidth) / 2)
        textContainerInset = NSSize(width: horizontal, height: HideTheme.spacingLG)
        super.setFrameSize(newSize)
    }

    enum LinkAction: Equatable {
        case open
        case placeCaret
    }

    static func linkAction(for modifiers: NSEvent.ModifierFlags) -> LinkAction {
        modifiers.contains(.command) ? .open : .placeCaret
    }

    override func clicked(onLink link: Any, at charIndex: Int) {
        let event = NSApp.currentEvent
        switch Self.linkAction(for: event?.modifierFlags ?? []) {
        case .open:
            if let url = link as? URL ?? (link as? String).flatMap(URL.init(string:)) { openLink?(url) }
        case .placeCaret:
            let location = event.map { characterIndexForInsertion(at: convert($0.locationInWindow, from: nil)) } ?? charIndex
            setSelectedRange(NSRange(location: location, length: 0))
        }
    }
}

/// Draws what an attribute alone cannot: a code block's full-width fill, the
/// bar beside a quote, and the rule a hidden `---` stands for.
final class MarkdownLiveLayoutManager: NSLayoutManager {
    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let storage = textStorage, let container = textContainers.first else { return }
        let containerWidth = container.size.width
        enumerateLineFragments(forGlyphRange: glyphsToShow) { rect, _, _, glyphRange, _ in
            let characterIndex = self.characterIndexForGlyph(at: glyphRange.location)
            guard characterIndex < storage.length else { return }
            let attributes = storage.attributes(at: characterIndex, effectiveRange: nil)
            let fullWidth = NSRect(x: origin.x, y: rect.minY + origin.y, width: containerWidth, height: rect.height)
            if attributes[.markdownLiveCodeBlock] != nil {
                HideTheme.Native.panel.setFill()
                fullWidth.fill()
            }
            if let depth = attributes[.markdownLiveQuoteDepth] as? Int {
                HideTheme.Native.divider.setFill()
                for level in 0..<depth {
                    NSRect(x: origin.x + CGFloat(level) * HideTheme.spacingLG, y: fullWidth.minY,
                        width: HideTheme.Editor.quoteRuleWidth, height: fullWidth.height).fill()
                }
            }
            if attributes[.markdownLiveThematicBreak] != nil, self.ruleIsHidden(at: characterIndex, in: storage) {
                HideTheme.Native.divider.setFill()
                NSRect(x: origin.x, y: fullWidth.midY - HideTheme.Layout.hairlineWidth / 2,
                    width: containerWidth, height: HideTheme.Layout.hairlineWidth).fill()
            }
        }
    }
}

extension MarkdownLiveLayoutManager {
    /// A `---` line draws its rule only while its dashes are hidden; with them
    /// hidden the fragment holds only the newline, so the dashes are looked up
    /// from the paragraph start.
    fileprivate func ruleIsHidden(at characterIndex: Int, in storage: NSTextStorage) -> Bool {
        let paragraph = (storage.string as NSString).paragraphRange(for: NSRange(location: characterIndex, length: 0))
        guard paragraph.length > 1 else { return false }
        return propertyForGlyph(at: glyphIndexForCharacter(at: paragraph.location)) == .null
    }
}

extension NSAttributedString.Key {
    static let markdownLiveCodeBlock = NSAttributedString.Key("hide.markdownLive.codeBlock")
    static let markdownLiveQuoteDepth = NSAttributedString.Key("hide.markdownLive.quoteDepth")
    static let markdownLiveThematicBreak = NSAttributedString.Key("hide.markdownLive.thematicBreak")
}

/// The attributes each plan style resolves to at one text scale. Body and
/// heading type is Inter at the document tokens; code is the editor's
/// monospaced font, so the two editing modes agree on what code looks like.
@MainActor
final class MarkdownLiveTypography {
    let scale: CGFloat
    private let body: NSFont
    private let mono: NSFont
    private let paragraph: NSParagraphStyle
    /// One attribute set per style seen: a document has tens of thousands of
    /// spans and a handful of styles, and resolving a font by descriptor for
    /// each span cost a gigabyte of transient allocations on a large open.
    private var cache: [MarkdownLiveStyle: [NSAttributedString.Key: Any]] = [:]

    init(scale: CGFloat) {
        self.scale = scale
        body = HideTheme.nativeFont(size: HideTheme.Editor.documentFontSize * scale)
        mono = NSFont.monospacedSystemFont(ofSize: HideTheme.editorBaseFontSize * scale, weight: .regular)
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = HideTheme.Editor.documentLineSpacing * scale
        paragraph.lineBreakMode = .byWordWrapping
        self.paragraph = paragraph
    }

    func attributes(for style: MarkdownLiveStyle) -> [NSAttributedString.Key: Any] {
        if let cached = cache[style] { return cached }
        let attributes = build(style)
        cache[style] = attributes
        return attributes
    }

    private func build(_ style: MarkdownLiveStyle) -> [NSAttributedString.Key: Any] {
        var attributes: [NSAttributedString.Key: Any] = [.foregroundColor: HideTheme.Native.primary]
        let paragraph = self.paragraph.mutableCopy() as! NSMutableParagraphStyle
        var size = HideTheme.Editor.documentFontSize
        var weight: SwiftUI.Font.Weight = .regular
        var monospaced = false
        switch style.block {
        case .paragraph:
            break
        case .heading(let level):
            let sizes = HideTheme.Editor.headingFontSizes
            size = sizes[min(max(level, 1), sizes.count) - 1]
            weight = .semibold
        case .codeBlock:
            monospaced = true
            attributes[.markdownLiveCodeBlock] = true
        case .thematicBreak:
            attributes[.markdownLiveThematicBreak] = true
        case .raw:
            monospaced = true
        }
        if style.quoteDepth > 0 {
            attributes[.markdownLiveQuoteDepth] = style.quoteDepth
        }
        let indent = CGFloat(style.quoteDepth + style.listDepth) * HideTheme.spacingLG
        // A list item's continuation lines hang under its text, past the marker.
        paragraph.firstLineHeadIndent = style.listDepth > 0 ? indent - HideTheme.spacingLG : indent
        paragraph.headIndent = indent
        if style.code {
            monospaced = true
            attributes[.backgroundColor] = HideTheme.Native.panel
        }
        if style.bold { weight = .bold }
        if style.italic { attributes[.obliqueness] = HideTheme.Editor.italicSkew }
        if style.strikethrough { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        if let link = style.link { attributes[.link] = link }
        let font = monospaced ? mono : (size == HideTheme.Editor.documentFontSize && weight == .regular
            ? body : HideTheme.nativeFont(size: size * scale, weight: weight))
        attributes[.font] = font
        attributes[.paragraphStyle] = paragraph
        return attributes
    }
}
