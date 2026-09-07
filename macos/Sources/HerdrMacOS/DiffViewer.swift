import SwiftUI

/// A unified diff rendered as central, read-only editor content.
struct DiffText: View {
    let diff: CoreChangedFileDiff

    var body: some View {
        GeometryReader { viewport in
            ScrollView([.vertical, .horizontal]) {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                    if let reason = diff.notice {
                        Text(reason)
                            .hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.warning)
                            .padding(.horizontal, HideTheme.spacingMD)
                            .padding(.vertical, HideTheme.spacingXS)
                    }
                    ForEach(DiffLinePresentation.rows(in: diff.text)) { line in
                        DiffLine(line: line)
                    }
                }
                .padding(.vertical, HideTheme.spacingXS)
                // AppKit centers an undersized two-axis scroll document.
                // Give it the viewport as a minimum canvas and anchor the
                // rows at its top; long diffs still grow and scroll normally.
                .frame(
                    minWidth: max(HideTheme.Editor.minimumContentWidth, viewport.size.width),
                    minHeight: viewport.size.height,
                    alignment: .topLeading
                )
            }
        }
        .accessibilityIdentifier("changes-diff-text")
    }
}

private struct DiffLine: View {
    let line: DiffLinePresentation

    var body: some View {
        HStack(spacing: HideTheme.spacingNone) {
            lineNumber(line.oldNumber)
            lineNumber(line.newNumber)
            Rectangle()
                .fill(HideTheme.divider)
                .frame(width: HideTheme.Layout.hairlineWidth)
            Text(line.text.isEmpty ? " " : line.text)
                .font(.system(size: HideTheme.editorBaseFontSize, design: .monospaced))
                .foregroundStyle(foreground)
                .lineLimit(1)
                .fixedSize(horizontal: true, vertical: false)
                .padding(.leading, HideTheme.spacingMD)
                .padding(.trailing, HideTheme.spacingLG)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(background)
    }

    private func lineNumber(_ number: Int?) -> some View {
        Text(number.map(String.init) ?? "")
            .font(.system(size: HideTheme.Typography.caption, design: .monospaced))
            .foregroundStyle(HideTheme.muted)
            .frame(width: HideTheme.Editor.diffLineNumberColumnWidth, alignment: .trailing)
            .padding(.trailing, HideTheme.spacingXS)
    }

    private var foreground: Color {
        switch line.kind {
        case .added: HideTheme.diffAdded
        case .removed: HideTheme.diffRemoved
        case .hunk: HideTheme.secondary
        case .context: HideTheme.primary
        }
    }

    private var background: Color {
        switch line.kind {
        case .added: HideTheme.diffAddedBackground
        case .removed: HideTheme.diffRemovedBackground
        case .hunk, .context: Color.clear
        }
    }
}

struct DiffLinePresentation: Identifiable, Equatable {
    let id: Int
    let text: String
    let kind: DiffLineKind
    let oldNumber: Int?
    let newNumber: Int?

    static func rows(in text: String) -> [DiffLinePresentation] {
        var oldLine: Int?
        var newLine: Int?
        return text
            .split(separator: "\n", omittingEmptySubsequences: false)
            .enumerated()
            .map { index, slice in
                let text = String(slice)
                let kind = DiffLineKind.of(text)
                if kind == .hunk, let starts = hunkStarts(text) {
                    oldLine = starts.old
                    newLine = starts.new
                    return DiffLinePresentation(
                        id: index,
                        text: text,
                        kind: kind,
                        oldNumber: nil,
                        newNumber: nil
                    )
                }
                let presented: DiffLinePresentation
                switch kind {
                case .added:
                    presented = DiffLinePresentation(
                        id: index, text: text, kind: kind, oldNumber: nil, newNumber: newLine
                    )
                    newLine = newLine.map { $0 + 1 }
                case .removed:
                    presented = DiffLinePresentation(
                        id: index, text: text, kind: kind, oldNumber: oldLine, newNumber: nil
                    )
                    oldLine = oldLine.map { $0 + 1 }
                case .context where text.hasPrefix(" "):
                    presented = DiffLinePresentation(
                        id: index, text: text, kind: kind, oldNumber: oldLine, newNumber: newLine
                    )
                    oldLine = oldLine.map { $0 + 1 }
                    newLine = newLine.map { $0 + 1 }
                case .hunk, .context:
                    presented = DiffLinePresentation(
                        id: index, text: text, kind: kind, oldNumber: nil, newNumber: nil
                    )
                }
                return presented
            }
    }

    private static func hunkStarts(_ line: String) -> (old: Int, new: Int)? {
        let fields = line.split(separator: " ")
        guard fields.count >= 3,
              let old = lineStart(fields[1], prefix: "-"),
              let new = lineStart(fields[2], prefix: "+")
        else { return nil }
        return (old, new)
    }

    private static func lineStart(_ field: Substring, prefix: Character) -> Int? {
        guard field.first == prefix else { return nil }
        return Int(field.dropFirst().split(separator: ",", maxSplits: 1)[0])
    }
}

enum DiffLineKind {
    case added
    case removed
    case hunk
    case context

    static func of(_ line: String) -> DiffLineKind {
        if line.hasPrefix("+++") || line.hasPrefix("---") { return .hunk }
        if line.hasPrefix("@@") || line.hasPrefix("diff ") || line.hasPrefix("index ") {
            return .hunk
        }
        if line.hasPrefix("+") { return .added }
        if line.hasPrefix("-") { return .removed }
        return .context
    }
}
