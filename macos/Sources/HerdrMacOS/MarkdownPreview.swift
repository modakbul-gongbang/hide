import AppKit
import SwiftUI

/// Foundation owns Markdown syntax. Native text rendering never executes HTML or fetches images.
enum MarkdownDocument {
    @MainActor static func render(_ source: String, scale: CGFloat) throws -> NSAttributedString {
        let parsed = try AttributedString(markdown: source)
        let output = NSMutableAttributedString()
        var previousBlock: Int?
        var previousTableRow: Int?
        for run in parsed.runs {
            let intent = run.presentationIntent
            let components = intent?.components ?? []
            let block = components.first?.identity
            let cell = components.contains { if case .tableCell = $0.kind { return true }; return false }
            let tableRow = components.first { component in
                switch component.kind { case .tableRow, .tableHeaderRow: true; default: false }
            }?.identity
            if output.length > 0, block != previousBlock {
                output.append(NSAttributedString(string: cell && tableRow == previousTableRow ? "\t" : "\n\n"))
            }
            let paragraph = NSMutableParagraphStyle()
            paragraph.lineSpacing = HideTheme.Editor.documentLineSpacing * scale
            paragraph.lineBreakMode = .byWordWrapping
            var size = HideTheme.Editor.documentFontSize * scale
            var weight: SwiftUI.Font.Weight = .regular
            var code = run.inlinePresentationIntent?.contains(.code) == true
            var prefix = ""
            for component in components {
                switch component.kind {
                case .header(let level):
                    size = (level == 1 ? HideTheme.Typography.display : HideTheme.Typography.headline) * scale
                    weight = .semibold
                case .codeBlock: code = true
                case .listItem(let ordinal):
                    if block != previousBlock {
                        let ordered = components.contains { if case .orderedList = $0.kind { return true }; return false }
                        prefix = ordered ? "\(ordinal). " : "• "
                    }
                    paragraph.headIndent = HideTheme.spacingLG
                case .blockQuote:
                    paragraph.headIndent = HideTheme.spacingLG
                    paragraph.firstLineHeadIndent = HideTheme.spacingLG
                default: break
                }
            }
            if run.inlinePresentationIntent?.contains(.stronglyEmphasized) == true { weight = .bold }
            var font = code ? NSFont.monospacedSystemFont(ofSize: HideTheme.editorBaseFontSize * scale, weight: .regular)
                : HideTheme.nativeFont(size: size, weight: weight)
            if run.inlinePresentationIntent?.contains(.emphasized) == true {
                font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
            }
            var attributes: [NSAttributedString.Key: Any] = [
                .font: font, .foregroundColor: HideTheme.Native.primary, .paragraphStyle: paragraph,
            ]
            if let link = run.link { attributes[.link] = link }
            if run.inlinePresentationIntent?.contains(.strikethrough) == true { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            if code { attributes[.backgroundColor] = HideTheme.Native.panel }
            var text = prefix + String(parsed[run.range].characters)
            if run.imageURL != nil { text = "[Image: \(text.isEmpty ? "no description" : text) · preview disabled]" }
            output.append(NSAttributedString(string: text, attributes: attributes))
            previousBlock = block
            previousTableRow = tableRow
        }
        return output
    }
}

struct MarkdownPreview: NSViewRepresentable {
    let text: String
    let textScale: CGFloat
    let findRequest: Int
    let openLink: (URL) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(openLink: openLink) }

    func makeNSView(context: Context) -> NSScrollView {
        let view = NSTextView()
        view.isEditable = false
        view.isSelectable = true
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.autoresizingMask = [.width]
        view.textContainer?.widthTracksTextView = true
        view.textContainerInset = NSSize(width: HideTheme.spacingLG, height: HideTheme.spacingLG)
        view.backgroundColor = HideTheme.Native.background
        view.delegate = context.coordinator
        HighlightedCodeEditor.enableFindBar(on: view)
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.backgroundColor = HideTheme.Native.background
        scroll.documentView = view
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? NSTextView else { return }
        context.coordinator.openLink = openLink
        if context.coordinator.source != text || context.coordinator.scale != textScale {
            context.coordinator.source = text
            context.coordinator.scale = textScale
            do {
                view.textStorage?.setAttributedString(try MarkdownDocument.render(text, scale: textScale))
            } catch {
                view.string = "Markdown preview failed: \(error.localizedDescription)\n\n\(text)"
                view.font = HideTheme.nativeFont(size: HideTheme.Editor.documentFontSize * textScale)
                view.textColor = HideTheme.Native.primary
            }
        }
        if context.coordinator.findRequest != findRequest {
            context.coordinator.findRequest = findRequest
            view.window?.makeFirstResponder(view)
            let item = NSMenuItem()
            item.tag = NSTextFinder.Action.showFindInterface.rawValue
            view.performFindPanelAction(item)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var source: String?
        var scale: CGFloat?
        var findRequest = 0
        var openLink: (URL) -> Void
        init(openLink: @escaping (URL) -> Void) { self.openLink = openLink }
        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            if let url = link as? URL { openLink(url) }
            return true
        }
    }
}
