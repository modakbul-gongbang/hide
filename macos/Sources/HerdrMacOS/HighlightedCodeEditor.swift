import AppKit
import Highlightr
import SwiftUI

struct HighlightedCodeEditor: NSViewRepresentable {
    @Binding var text: String
    let language: String?
    let isEditable: Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let storage = Self.makeTextStorage(language: language)

        let layoutManager = NSLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = true
        layoutManager.addTextContainer(container)

        let textView = NSTextView(frame: .zero, textContainer: container)
        textView.autoresizingMask = [NSView.AutoresizingMask.width]
        textView.delegate = context.coordinator
        textView.font = NSFont.monospacedSystemFont(ofSize: 12, weight: NSFont.Weight.regular)
        textView.isRichText = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isEditable = isEditable
        textView.textContainerInset = NSSize(width: 10, height: 10)
        storage.replaceCharacters(in: NSRange(location: 0, length: 0), with: text)

        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.documentView = textView
        context.coordinator.textView = textView
        return scrollView
    }

    static func makeTextStorage(
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
            return NSTextStorage()
        }

        let storage = CodeAttributedString(highlightr: highlightr)
        storage.language = language
        _ = highlightr.setTheme(to: "atom-one-dark")
        return storage
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = context.coordinator.textView else { return }
        textView.isEditable = isEditable
        if let storage = textView.textStorage as? CodeAttributedString {
            storage.language = language
        }
        guard textView.string != text, !context.coordinator.isForwardingChange else { return }
        let selection = textView.selectedRange()
        context.coordinator.isApplyingSnapshot = true
        textView.textStorage?.replaceCharacters(
            in: NSRange(location: 0, length: textView.string.utf16.count),
            with: text
        )
        textView.setSelectedRange(NSRange(
            location: min(selection.location, text.utf16.count),
            length: 0
        ))
        context.coordinator.isApplyingSnapshot = false
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        @Binding private var text: String
        weak var textView: NSTextView?
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
        }
    }
}
