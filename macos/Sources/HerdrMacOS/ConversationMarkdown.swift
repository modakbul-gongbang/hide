import AppKit
import SwiftUI

/// Renders an assistant turn's Markdown for the conversation ledger. Foundation
/// owns the syntax; the rendered text never executes HTML or fetches images.
enum ConversationMarkdown {
    @MainActor static func render(
        _ source: String,
        scale: CGFloat,
        baseFontSize: CGFloat = HideTheme.Editor.documentFontSize,
        lineSpacing: CGFloat = HideTheme.Editor.documentLineSpacing,
        foregroundColor: NSColor = HideTheme.Native.primary
    ) throws -> NSAttributedString {
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
                // A textual separator stays legible at every native tab-stop
                // position; tables deliberately remain a plain reading view.
                output.append(NSAttributedString(string: cell && tableRow == previousTableRow ? "  |  " : "\n\n"))
            }
            let paragraph = NSMutableParagraphStyle()
            paragraph.lineSpacing = lineSpacing * scale
            paragraph.lineBreakMode = .byWordWrapping
            var size = baseFontSize * scale
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
                .font: font, .foregroundColor: foregroundColor, .paragraphStyle: paragraph,
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
