import Foundation
import AppKit
import SwiftUI

/// Herdr owns the leaf's identity and geometry; this describes what Hide draws
/// inside it. Browser identities are independent of transient CDP endpoints.
enum CorePaneContent: Decodable, Equatable, Sendable {
    case terminal
    case browser(BrowserPaneBinding)
    case unavailable(String)

    var closeConsequence: String? {
        switch self {
        case .terminal: nil
        case .browser(let binding): binding.closeConsequence
        case .unavailable:
            "Closing this pane stops its content host. Browser tabs created by that host will also close; attached existing tabs stay open."
        }
    }

    private enum CodingKeys: String, CodingKey {
        case kind, reason
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(String.self, forKey: .kind) {
        case "terminal": self = .terminal
        case "browser": self = .browser(try BrowserPaneBinding(from: decoder))
        case "unavailable": self = .unavailable(try container.decode(String.self, forKey: .reason))
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .kind, in: container, debugDescription: "Unsupported pane content kind"
            )
        }
    }
}

struct BrowserPaneBinding: Decodable, Equatable, Hashable, Sendable {
    let bindingID: String
    let profile: String
    let targetID: String
    let session: String
    let cdpPort: UInt16
    let ownsTarget: Bool

    var closeConsequence: String {
        ownsTarget
            ? "Closing this pane also closes its Chromium tab. Unsaved page input may be lost."
            : "Closing this pane detaches its viewer. The existing browser tab and session stay open."
    }

    enum CodingKeys: String, CodingKey {
        case bindingID = "binding_id"
        case profile
        case targetID = "target_id"
        case session
        case cdpPort = "cdp_port"
        case ownsTarget = "owns_target"
    }
}

private extension NSAttributedString.Key {
    static let conversationRole = NSAttributedString.Key("HerdrConversationRole")
}

struct ConversationLedgerMetadata: Equatable {
    let timestamp: Date?
    let elapsed: String?
    let isWorking: Bool
}

struct ConversationLedgerDocument {
    let attributedString: NSAttributedString
    let metadata: [Int: ConversationLedgerMetadata]
}

enum ConversationLedgerFormatting {
    enum Role: Equatable {
        case human
        case assistant
    }

    struct Turn: Equatable {
        let role: Role
        let text: String
        let timestamp: Date?
        let elapsed: String?
        let promptGlyph: String?
        var isWorking: Bool
    }

    static func turns(
        messages: [ConversationMessage],
        provider: ConversationProvider,
        activity: String,
        now: Date
    ) -> [Turn] {
        var lastUserDate: Date?
        var result: [Turn] = []
        for message in messages {
            switch message.role {
            case .user:
                lastUserDate = message.timestamp
                result.append(Turn(
                    role: .human,
                    text: message.text,
                    timestamp: message.timestamp,
                    elapsed: nil,
                    promptGlyph: promptGlyph(for: provider),
                    isWorking: false
                ))
            case .assistant:
                if let last = result.last, last.role == .assistant {
                    let text: String
                    if last.text.isEmpty {
                        text = message.text
                    } else if message.text.isEmpty {
                        text = last.text
                    } else {
                        text = "\(last.text)\n\n\(message.text)"
                    }
                    let timestamp = message.timestamp ?? last.timestamp
                    result[result.count - 1] = Turn(
                        role: .assistant,
                        text: text,
                        timestamp: timestamp,
                        elapsed: elapsedText(from: lastUserDate, to: timestamp),
                        promptGlyph: nil,
                        isWorking: false
                    )
                } else {
                    result.append(Turn(
                        role: .assistant,
                        text: message.text,
                        timestamp: message.timestamp,
                        elapsed: elapsedText(from: lastUserDate, to: message.timestamp),
                        promptGlyph: nil,
                        isWorking: false
                    ))
                }
            }
        }

        guard activity == "working", let lastUserDate else { return result }
        if let index = result.indices.last, result[index].role == .assistant {
            result[index] = Turn(
                role: result[index].role,
                text: result[index].text,
                timestamp: result[index].timestamp,
                elapsed: elapsedText(from: lastUserDate, to: now),
                promptGlyph: result[index].promptGlyph,
                isWorking: true
            )
        } else {
            result.append(Turn(
                role: .assistant,
                text: "",
                timestamp: nil,
                elapsed: elapsedText(from: lastUserDate, to: now),
                promptGlyph: nil,
                isWorking: true
            ))
        }
        return result
    }

    static func markdown(
        messages: [ConversationMessage],
        provider: ConversationProvider,
        activity: String,
        now: Date
    ) -> String {
        turns(messages: messages, provider: provider, activity: activity, now: now).map { turn in
            switch turn.role {
            case .human:
                let time = turn.timestamp.map { "`\(timeText($0))`  " } ?? ""
                return "\(time)**\(turn.promptGlyph ?? "")** \(turn.text)"
            case .assistant:
                let time = turn.timestamp.map { "`\(timeText($0))`  " } ?? ""
                let marker = turn.elapsed.map { "  _\($0)_" } ?? ""
                return "\(time)\(turn.text)\(marker)"
            }
        }.joined(separator: "\n\n")
    }

    @MainActor
    static func document(
        messages: [ConversationMessage],
        provider: ConversationProvider,
        activity: String,
        now: Date,
        textScale: CGFloat
    ) -> ConversationLedgerDocument {
        let turns = turns(messages: messages, provider: provider, activity: activity, now: now)
        let output = NSMutableAttributedString()
        var metadata: [Int: ConversationLedgerMetadata] = [:]

        for (index, turn) in turns.enumerated() {
            if index > 0 {
                output.append(NSAttributedString(string: "\n"))
            }
            let start = output.length
            switch turn.role {
            case .human:
                let paragraph = promptParagraph()
                let text = "\(turn.promptGlyph ?? "") \(turn.text)"
                let attributes: [NSAttributedString.Key: Any] = [
                    .font: NSFont.monospacedSystemFont(
                        ofSize: HideTheme.Conversation.promptFontSize * textScale,
                        weight: .regular
                    ),
                    .foregroundColor: HideTheme.Native.primary,
                    .paragraphStyle: paragraph,
                    .conversationRole: "human",
                ]
                output.append(NSAttributedString(string: text, attributes: attributes))
            case .assistant:
                let rendered: NSMutableAttributedString
                do {
                    rendered = NSMutableAttributedString(attributedString: try MarkdownDocument.render(
                        turn.text.isEmpty ? "\u{200B}" : turn.text,
                        scale: textScale,
                        baseFontSize: HideTheme.Conversation.bodyFontSize,
                        lineSpacing: HideTheme.Conversation.bodyLineSpacing,
                        foregroundColor: HideTheme.Native.primary
                    ))
                } catch {
                    rendered = NSMutableAttributedString(string: "Markdown preview failed: \(error.localizedDescription)\n\n\(turn.text)")
                    rendered.addAttributes([
                        .font: HideTheme.nativeFont(size: HideTheme.Conversation.bodyFontSize * textScale),
                        .foregroundColor: HideTheme.Native.primary,
                    ], range: NSRange(location: 0, length: rendered.length))
                }
                addConversationParagraphStyle(to: rendered, textScale: textScale)
                rendered.addAttribute(
                    .conversationRole,
                    value: "assistant",
                    range: NSRange(location: 0, length: rendered.length)
                )
                output.append(rendered)
            }
            metadata[start] = ConversationLedgerMetadata(
                timestamp: turn.timestamp,
                elapsed: turn.elapsed,
                isWorking: turn.isWorking
            )
        }

        return ConversationLedgerDocument(attributedString: output, metadata: metadata)
    }

    static func promptGlyph(for provider: ConversationProvider) -> String {
        provider == .claude ? ">" : "›"
    }

    static func elapsedText(from start: Date?, to end: Date?) -> String? {
        guard let start, let end else { return nil }
        let seconds = max(0, Int(end.timeIntervalSince(start)))
        if seconds < 60 { return "\(seconds)s" }
        return "\(seconds / 60)m \(seconds % 60)s"
    }

    private static func promptParagraph() -> NSMutableParagraphStyle {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = HideTheme.spacingSM
        paragraph.lineBreakMode = .byWordWrapping
        let rail = HideTheme.Conversation.timeRailWidth
        paragraph.firstLineHeadIndent = rail
        paragraph.headIndent = rail + HideTheme.Conversation.promptGlyphSize
        paragraph.paragraphSpacingBefore = HideTheme.Conversation.promptBandVerticalInset
        paragraph.paragraphSpacing = HideTheme.Conversation.promptBandVerticalInset
        return paragraph
    }

    private static func addConversationParagraphStyle(
        to rendered: NSMutableAttributedString,
        textScale: CGFloat
    ) {
        let rail = HideTheme.Conversation.timeRailWidth
        let indent = rail + HideTheme.Conversation.turnInset
        let lastParagraph = rendered.length > 0
            ? (rendered.string as NSString).paragraphRange(
                for: NSRange(location: rendered.length - 1, length: 0)
            )
            : nil
        rendered.enumerateAttribute(.paragraphStyle, in: NSRange(location: 0, length: rendered.length)) { value, range, _ in
            let paragraph = (value as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle ?? NSMutableParagraphStyle()
            paragraph.firstLineHeadIndent += indent
            paragraph.headIndent += indent
            if let lastParagraph, NSIntersectionRange(lastParagraph, range).length > 0 {
                // The elapsed label lives below the turn's first line in the
                // time rail. Reserve that line so the next prompt band never
                // paints over it.
                paragraph.paragraphSpacing += HideTheme.Typography.caption * textScale + HideTheme.spacingXS
            }
            rendered.addAttribute(.paragraphStyle, value: paragraph, range: range)
        }
    }

    static func timeText(_ date: Date) -> String {
        let components = Calendar.current.dateComponents([.hour, .minute, .second], from: date)
        return String(
            format: "%02d:%02d:%02d",
            components.hour ?? 0,
            components.minute ?? 0,
            components.second ?? 0
        )
    }
}

private final class ConversationLedgerTextView: NSTextView {
    var metadata: [Int: ConversationLedgerMetadata] = [:] {
        didSet { needsDisplay = true }
    }
    var followsBottom = true
    var textScale: CGFloat = 1

    func updateTextContainerWidth() {
        guard let textContainer else { return }
        let availableWidth = max(1, bounds.width)
        let width = min(HideTheme.Conversation.measureWidth, availableWidth)
        guard abs(textContainer.containerSize.width - width) > 0.5 else { return }
        textContainer.widthTracksTextView = false
        textContainer.containerSize = NSSize(width: width, height: .greatestFiniteMagnitude)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateTextContainerWidth()
    }

    override func scrollWheel(with event: NSEvent) {
        super.scrollWheel(with: event)
        DispatchQueue.main.async { [weak self] in self?.updateFollowState() }
    }

    func updateFollowState() {
        guard let scroll = enclosingScrollView else { return }
        let clip = scroll.contentView
        let maxY = max(0, bounds.height - clip.bounds.height)
        followsBottom = maxY - clip.bounds.origin.y <= 2
    }

    override func drawBackground(in rect: NSRect) {
        super.drawBackground(in: rect)
        guard let layoutManager, let textContainer else { return }
        let glyphRange = layoutManager.glyphRange(forBoundingRect: rect, in: textContainer)
        layoutManager.enumerateLineFragments(forGlyphRange: glyphRange) { _, usedRect, _, lineGlyphRange, _ in
            let characters = layoutManager.characterRange(forGlyphRange: lineGlyphRange, actualGlyphRange: nil)
            guard characters.length > 0,
                  let role = self.textStorage?.attribute(.conversationRole, at: characters.location, effectiveRange: nil) as? String,
                  role == "human" else { return }
            var rowRect = usedRect.offsetBy(dx: self.textContainerOrigin.x, dy: self.textContainerOrigin.y)
            rowRect.origin.y -= HideTheme.Conversation.promptBandVerticalInset
            rowRect.size.height += HideTheme.Conversation.promptBandVerticalInset * 2
            rowRect.origin.x = 0
            rowRect.size.width = self.bounds.width
            HideTheme.Native.panel.setFill()
            rowRect.fill()
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard !metadata.isEmpty, let layoutManager else { return }
        for (location, item) in metadata {
            let glyphRange = layoutManager.glyphRange(
                forCharacterRange: NSRange(location: location, length: min(1, max(0, (textStorage?.length ?? 0) - location))),
                actualCharacterRange: nil
            )
            guard glyphRange.length > 0 else { continue }
            var lineRect: NSRect?
            layoutManager.enumerateLineFragments(forGlyphRange: glyphRange) { _, usedRect, _, _, _ in
                lineRect = usedRect.offsetBy(dx: self.textContainerOrigin.x, dy: self.textContainerOrigin.y)
            }
            guard let lineRect else { continue }
            drawMetadata(item, in: lineRect)
        }
    }

    private func drawMetadata(_ item: ConversationLedgerMetadata, in lineRect: NSRect) {
        let scale = max(0.1, textScale)
        if let timestamp = item.timestamp {
            let timeAttributes: [NSAttributedString.Key: Any] = [
                .font: NSFont.monospacedSystemFont(ofSize: HideTheme.Typography.micro * scale, weight: .regular),
                .foregroundColor: HideTheme.Native.muted,
                .paragraphStyle: rightAlignedParagraph,
            ]
            let time = ConversationLedgerFormatting.timeText(timestamp)
            NSString(string: time).draw(
                in: NSRect(x: 0, y: lineRect.minY, width: HideTheme.Conversation.timeRailWidth - HideTheme.spacingXS, height: lineRect.height),
                withAttributes: timeAttributes
            )
        }
        guard let elapsed = item.elapsed else { return }
        let elapsedAttributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: HideTheme.Typography.caption * scale, weight: .regular),
            .foregroundColor: item.isWorking ? HideTheme.Native.agentWorking : HideTheme.Native.secondary,
            .paragraphStyle: rightAlignedParagraph,
        ]
        let label = item.isWorking ? "● \(elapsed)" : elapsed
        NSString(string: label).draw(
            in: NSRect(x: 0, y: lineRect.minY + lineRect.height, width: HideTheme.Conversation.timeRailWidth - HideTheme.spacingXS, height: lineRect.height),
            withAttributes: elapsedAttributes
        )
    }

    private var rightAlignedParagraph: NSParagraphStyle {
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .right
        return paragraph
    }
}

struct ConversationLedgerView: NSViewRepresentable {
    let messages: [ConversationMessage]
    let provider: ConversationProvider
    let activity: String
    let now: Date
    let textScale: CGFloat
    let openLink: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(openLink: openLink) }

    func makeNSView(context: Context) -> NSScrollView {
        let view = ConversationLedgerTextView()
        view.isEditable = false
        view.isSelectable = true
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.autoresizingMask = [.width]
        view.textContainer?.widthTracksTextView = false
        view.textContainer?.containerSize = NSSize(
            width: HideTheme.Conversation.measureWidth,
            height: .greatestFiniteMagnitude
        )
        view.textContainer?.lineFragmentPadding = 0
        view.textContainerInset = NSSize(width: 0, height: HideTheme.spacingSM)
        view.drawsBackground = false
        view.delegate = context.coordinator
        HighlightedCodeEditor.enableFindBar(on: view)

        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.documentView = view
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? ConversationLedgerTextView else { return }
        context.coordinator.openLink = openLink
        view.textScale = textScale
        view.updateTextContainerWidth()
        view.updateFollowState()
        let turns = ConversationLedgerFormatting.turns(
            messages: messages,
            provider: provider,
            activity: activity,
            now: now
        )
        let signature = turns.map {
            "\($0.role)|\($0.text)|\($0.timestamp?.timeIntervalSince1970 ?? -1)|\($0.elapsed ?? "")|\($0.isWorking)"
        }.joined(separator: "\n")
            + "|\(provider)|\(textScale)"
        if context.coordinator.source != signature {
            let document = ConversationLedgerFormatting.document(
                messages: messages,
                provider: provider,
                activity: activity,
                now: now,
                textScale: textScale
            )
            context.coordinator.source = signature
            view.metadata = document.metadata
            view.textStorage?.setAttributedString(document.attributedString)
            view.needsDisplay = true
            if view.followsBottom {
                DispatchQueue.main.async { scrollToBottom(view) }
            }
        }
    }

    private func scrollToBottom(_ view: ConversationLedgerTextView) {
        view.layoutSubtreeIfNeeded()
        guard let scroll = view.enclosingScrollView else { return }
        let maxY = max(0, view.bounds.height - scroll.contentView.bounds.height)
        scroll.contentView.scroll(to: NSPoint(x: 0, y: maxY))
        scroll.reflectScrolledClipView(scroll.contentView)
        view.followsBottom = true
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var source: String?
        var openLink: (String) -> Void

        init(openLink: @escaping (String) -> Void) { self.openLink = openLink }

        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            if let url = link as? URL { openLink(url.isFileURL ? url.path : url.absoluteString) }
            return true
        }
    }
}

struct ConversationPaneView<TerminalFallback: View>: View {
    let provider: ConversationProvider
    let sessionID: String?
    let cwd: String
    let agent: SidebarAgent
    let textScale: CGFloat
    let onShowTerminal: () -> Void
    let openLink: (String) -> Void
    private let terminalFallback: () -> TerminalFallback
    private let reader: ConversationReader

    @State private var result: ConversationReadResult?
    @State private var refreshInFlight = false
    @State private var refreshPending = false
    @State private var refreshSequence = 0
    @State private var now = Date()

    init(
        provider: ConversationProvider,
        sessionID: String?,
        cwd: String,
        agent: SidebarAgent,
        textScale: CGFloat,
        onShowTerminal: @escaping () -> Void,
        openLink: @escaping (String) -> Void,
        reader: ConversationReader = ConversationReader(),
        @ViewBuilder terminalFallback: @escaping () -> TerminalFallback
    ) {
        self.provider = provider
        self.sessionID = sessionID
        self.cwd = cwd
        self.agent = agent
        self.textScale = textScale
        self.onShowTerminal = onShowTerminal
        self.openLink = openLink
        self.reader = reader
        self.terminalFallback = terminalFallback
    }

    var body: some View {
        if needsYou {
            terminalFallback()
        } else {
            ZStack {
                // Keep the SwiftTerm host mounted while the ledger is shown.
                // The PTY must keep its registration and viewport even though
                // its pixels are hidden from the operator.
                terminalFallback()
                    .opacity(0)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                content
            }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(HideTheme.background)
            .task(id: sourceKey) { refresh() }
            .onReceive(Timer.publish(every: 1, on: .main, in: .common).autoconnect()) { date in
                if agent.activity == "working" { now = date }
                refresh()
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch result {
        case nil:
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        case let .empty(path):
            VStack(spacing: HideTheme.spacingSM) {
                Text("No messages yet")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Text(path)
                    .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Button("Show terminal", action: onShowTerminal)
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        case let .failed(path, line, reason):
            VStack(spacing: HideTheme.spacingSM) {
                Label("Conversation failed", systemImage: "exclamationmark.triangle")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                    .foregroundStyle(HideTheme.warning)
                Text(line > 0 ? "\(path):\(line)" : path)
                    .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
                    .textSelection(.enabled)
                Text(reason)
                    .hideFont(size: HideTheme.Typography.subhead)
                    .foregroundStyle(HideTheme.secondary)
                Button("Retry", action: refresh)
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        case let .loaded(_, messages):
            ConversationLedgerView(
                messages: messages,
                provider: provider,
                activity: agent.activity,
                now: now,
                textScale: textScale,
                openLink: openLink
            )
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var needsYou: Bool {
        agent.blocked || ["question", "approval", "error"].contains(agent.demand)
    }

    private var sourceKey: String {
        [provider.rawValue, sessionID ?? "", cwd].joined(separator: "\u{0}")
    }

    private func refresh() {
        guard !refreshInFlight else {
            refreshPending = true
            return
        }
        refreshInFlight = true
        refreshSequence += 1
        let reader = self.reader
        let provider = self.provider
        let sessionID = self.sessionID
        let cwd = self.cwd
        let requestKey = sourceKey
        let requestID = refreshSequence
        DispatchQueue.global(qos: .utility).async {
            let value = reader.read(provider: provider, sessionID: sessionID, cwd: cwd)
            DispatchQueue.main.async {
                guard requestID == refreshSequence else { return }
                guard requestKey == sourceKey else {
                    refreshInFlight = false
                    refreshPending = false
                    refresh()
                    return
                }
                result = value
                refreshInFlight = false
                if refreshPending {
                    refreshPending = false
                    refresh()
                }
            }
        }
    }

}
