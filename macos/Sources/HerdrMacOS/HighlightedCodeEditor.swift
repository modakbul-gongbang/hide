import AppKit
import Highlightr
import SwiftUI

struct HighlightedCodeEditor: NSViewRepresentable {
    @Binding var text: String
    let language: String?
    let isEditable: Bool
    /// The file editor is one surface rather than a pane, so it carries the
    /// scale of the pane whose chords last changed it.
    let textScale: CGFloat

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let storage = Self.makeTextStorage(text: text, language: language)

        let layoutManager = NSLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(
            size: NSSize(
                width: CGFloat.greatestFiniteMagnitude,
                height: CGFloat.greatestFiniteMagnitude
            )
        )
        container.widthTracksTextView = false
        layoutManager.addTextContainer(container)

        let textView = NSTextView(frame: .zero, textContainer: container)
        // A text view inside a scroll view has to be told it may grow, and how
        // far. Without this AppKit keeps it at its frame height, the scroll
        // view sizes its document to that, and a long file stops scrolling
        // partway down with the rest of the text laid out below the clip.
        textView.minSize = NSSize(width: 0, height: 0)
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = true
        textView.autoresizingMask = [NSView.AutoresizingMask.width]
        textView.delegate = context.coordinator
        textView.font = NSFont.monospacedSystemFont(
            ofSize: HideTheme.editorBaseFontSize * textScale,
            weight: NSFont.Weight.regular
        )
        textView.isRichText = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isEditable = isEditable
        textView.textContainerInset = NSSize(
            width: HideTheme.Editor.contentInset,
            height: HideTheme.Editor.contentInset
        )
        textView.drawsBackground = true
        textView.backgroundColor = HideTheme.Native.background
        textView.textColor = HideTheme.Native.primary
        textView.insertionPointColor = HideTheme.Native.primary
        Self.enableFindBar(on: textView)
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = true
        scrollView.backgroundColor = HideTheme.Native.background
        scrollView.documentView = textView
        let ruler = CodeLineNumberRulerView(
            textView: textView,
            scrollView: scrollView,
            fontSize: HideTheme.editorBaseFontSize * textScale
        )
        scrollView.hasVerticalRuler = true
        scrollView.rulersVisible = true
        scrollView.verticalRulerView = ruler
        context.coordinator.textView = textView
        context.coordinator.lineNumberRuler = ruler
        return scrollView
    }

    /// AppKit's own find bar, which the editor never enabled, so `⌘F` had
    /// nothing to reveal over a file tab. Incremental searching is what
    /// highlights every match rather than only the one the caret is on, and
    /// what reports a term that matches nothing.
    static func enableFindBar(on textView: NSTextView) {
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
    }

    static func makeTextStorage(
        text: String = "",
        language: String?,
        highlightr: Highlightr? = Highlightr()
    ) -> NSTextStorage {
        guard let highlightr else {
            let payload = [
                "component": "code_editor",
                "fallback": "plain_text",
                "kind": "syntax_highlighter.unavailable",
                "message": "Highlightr resources are unavailable; syntax highlighting was disabled",
            ]
            if let data = try? JSONSerialization.data(withJSONObject: payload),
               var line = String(data: data, encoding: .utf8)
            {
                line.append("\n")
                FileHandle.standardError.write(Data(line.utf8))
            }
            return plainTextStorage(text)
        }

        let storage = CodeAttributedString(highlightr: highlightr)
        storage.language = language
        _ = highlightr.setTheme(to: "atom-one-dark")
        storage.replaceCharacters(in: NSRange(location: 0, length: 0), with: text)
        if language == nil {
            applyPlainTextColor(to: storage)
        }
        return storage
    }

    private static func plainTextStorage(_ text: String) -> NSTextStorage {
        NSTextStorage(
            string: text,
            attributes: [.foregroundColor: HideTheme.Native.primary]
        )
    }

    private static func applyPlainTextColor(to storage: NSTextStorage) {
        guard storage.length > 0 else { return }
        storage.addAttribute(
            .foregroundColor,
            value: HideTheme.Native.primary,
            range: NSRange(location: 0, length: storage.length)
        )
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = context.coordinator.textView else { return }
        textView.isEditable = isEditable
        let size = HideTheme.editorBaseFontSize * textScale
        let font = NSFont.monospacedSystemFont(ofSize: size, weight: NSFont.Weight.regular)
        if textView.font?.pointSize != size {
            textView.font = font
            context.coordinator.lineNumberRuler?.fontSize = size
            context.coordinator.lineNumberRuler?.needsDisplay = true
        }
        if let storage = textView.textStorage as? CodeAttributedString {
            storage.language = language
            if language == nil {
                Self.applyPlainTextColor(to: storage)
            }
        }
        guard textView.string != text, !context.coordinator.isForwardingChange else { return }
        let selection = textView.selectedRange()
        context.coordinator.isApplyingSnapshot = true
        textView.textStorage?.replaceCharacters(
            in: NSRange(location: 0, length: textView.string.utf16.count),
            with: text
        )
        // Snapshot replacement bypasses NSTextView's typing attributes. In
        // an initially empty editor AppKit otherwise inserts Helvetica even
        // though the text view was configured with a monospaced font.
        textView.font = font
        if language == nil, let storage = textView.textStorage {
            Self.applyPlainTextColor(to: storage)
        }
        textView.setSelectedRange(NSRange(
            location: min(selection.location, text.utf16.count),
            length: 0
        ))
        context.coordinator.isApplyingSnapshot = false
        context.coordinator.lineNumberRuler?.needsDisplay = true
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        @Binding private var text: String
        weak var textView: NSTextView?
        weak var lineNumberRuler: CodeLineNumberRulerView?
        var isApplyingSnapshot = false
        var isForwardingChange = false

        init(text: Binding<String>) {
            _text = text
        }

        func textDidChange(_ notification: Notification) {
            guard !isApplyingSnapshot, let textView else { return }
            isForwardingChange = true
            text = textView.string
            isForwardingChange = false
            lineNumberRuler?.needsDisplay = true
        }
    }
}

final class CodeLineNumberRulerView: NSRulerView {
    private weak var textView: NSTextView?
    var fontSize: CGFloat

    init(textView: NSTextView, scrollView: NSScrollView, fontSize: CGFloat) {
        self.textView = textView
        self.fontSize = fontSize
        super.init(scrollView: scrollView, orientation: .verticalRuler)
        ruleThickness = HideTheme.Editor.lineNumberColumnWidth
        scrollView.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(viewportChanged),
            name: NSView.boundsDidChangeNotification,
            object: scrollView.contentView
        )
    }

    @available(*, unavailable)
    required init(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    @objc private func viewportChanged() {
        needsDisplay = true
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        // Ruler callbacks can carry a dirty rectangle larger than the
        // ruler. Constrain this shared drawing context before painting the
        // background, or it can cover the adjacent document and tab strip.
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        bounds.clip()
        HideTheme.Native.panel.setFill()
        bounds.intersection(rect).fill()
        guard let textView,
              let layoutManager = textView.layoutManager,
              let textContainer = textView.textContainer,
              let scrollView
        else { return }

        HideTheme.Native.divider.setFill()
        NSRect(
            x: bounds.maxX - HideTheme.Layout.hairlineWidth,
            y: bounds.minY,
            width: HideTheme.Layout.hairlineWidth,
            height: bounds.height
        ).fill()

        let visibleRect = scrollView.contentView.bounds
        let glyphRange = layoutManager.glyphRange(forBoundingRect: visibleRect, in: textContainer)
        let characterRange = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)
        let content = textView.string as NSString
        var lineRange = content.lineRange(for: NSRange(location: characterRange.location, length: 0))
        var lineNumber = CodeLineNumbers.number(at: lineRange.location, in: content)
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: fontSize, weight: .regular),
            .foregroundColor: HideTheme.Native.muted,
        ]

        while lineRange.location < NSMaxRange(characterRange), lineRange.location < content.length {
            let glyphIndex = layoutManager.glyphIndexForCharacter(at: lineRange.location)
            let fragment = layoutManager.lineFragmentRect(forGlyphAt: glyphIndex, effectiveRange: nil)
            let label = "\(lineNumber)" as NSString
            let size = label.size(withAttributes: attributes)
            let y = fragment.minY + textView.textContainerOrigin.y - visibleRect.minY
                + (fragment.height - size.height) / 2
            label.draw(
                at: NSPoint(
                    x: ruleThickness - HideTheme.spacingSM - size.width,
                    y: y
                ),
                withAttributes: attributes
            )
            let next = NSMaxRange(lineRange)
            if next <= lineRange.location { break }
            lineRange = content.lineRange(for: NSRange(location: next, length: 0))
            lineNumber += 1
        }
    }
}

enum CodeLineNumbers {
    static func number(at utf16Location: Int, in text: NSString) -> Int {
        guard utf16Location > 0 else { return 1 }
        return text
            .substring(to: min(utf16Location, text.length))
            .reduce(1) { count, character in character == "\n" ? count + 1 : count }
    }
}
