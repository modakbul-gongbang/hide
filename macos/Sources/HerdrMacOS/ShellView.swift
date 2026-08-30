import AppKit
import SwiftUI

enum ShellMetrics {
    static let windowMinWidth: CGFloat = 960
    static let windowMinHeight: CGFloat = 620
    static let windowDefaultWidth: CGFloat = 1_440
    static let windowDefaultHeight: CGFloat = 900

    static let agentsMinWidth: CGFloat = 220
    static let agentsIdealWidth: CGFloat = 270
    static let terminalMinWidth: CGFloat = 420
    static let terminalIdealWidth: CGFloat = 760
    static let workbenchMinWidth: CGFloat = 260
    static let workbenchIdealWidth: CGFloat = 360

    static let panelPadding: CGFloat = 14
    static let compactSpacing: CGFloat = 8
    static let cardRadius: CGFloat = 10
}

private struct LegacyShellView: View {
    @EnvironmentObject private var model: ShellModel
    @FocusState private var focusedSurface: ShellSurface?

    var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                AgentsPanel()
                    .frame(
                        minWidth: ShellMetrics.agentsMinWidth,
                        idealWidth: ShellMetrics.agentsIdealWidth
                    )
                    .focusable()
                    .focused($focusedSurface, equals: .agents)
                    .onTapGesture { model.focus(.agents) }
                    .accessibilityIdentifier("agents-panel")

                TerminalPanel()
                    .frame(
                        minWidth: ShellMetrics.terminalMinWidth,
                        idealWidth: ShellMetrics.terminalIdealWidth
                    )
                    .focusable()
                    .focused($focusedSurface, equals: .terminal)
                    .onTapGesture { model.focus(.terminal) }
                    .accessibilityIdentifier("terminal-panel")

                WorkbenchPanel()
                    .frame(
                        minWidth: ShellMetrics.workbenchMinWidth,
                        idealWidth: ShellMetrics.workbenchIdealWidth
                    )
                    .focusable()
                    .focused($focusedSurface, equals: .workbench)
                    .onTapGesture { model.focus(.workbench) }
                    .accessibilityIdentifier("workbench-panel")
            }

            Divider()
            StatusBar()
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            focusedSurface = model.activeSurface
        }
        .onChange(of: model.activeSurface) { _, surface in
            focusedSurface = surface
        }
        .alert(item: $model.consequenceNotice) { notice in
            Alert(
                title: Text(notice.title),
                message: Text(notice.consequence + affectedSummary(notice.affected)),
                primaryButton: .destructive(Text("Continue"), action: model.confirmConsequencePreview),
                secondaryButton: .cancel(Text("Cancel"), action: model.cancelConsequencePreview)
            )
        }
    }

    private func affectedSummary(_ targets: [DestructiveTarget]) -> String {
        guard !targets.isEmpty else { return "" }
        return "\n\nAffected work:\n" + targets
            .map { "• \($0.label): \($0.summary)" }
            .joined(separator: "\n")
    }
}

private struct AgentsPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var agents: [SidebarAgent] { model.core.snapshot?.navigator.agents ?? [] }

    /// The empty sidebar states the herdr connection status instead of a
    /// generic prompt: a missing socket, an unreachable server, and a
    /// connected-but-empty session are different situations.
    private var emptyAgentsDescription: String {
        guard let herdr = model.core.snapshot?.status.herdr else {
            return "Connect herdr to load workspaces and agent activity."
        }
        switch herdr.state {
        case "connected":
            return "Herdr is connected but no agent panes are running."
        case "unconfigured":
            return "No herdr socket is configured for this launch."
        default:
            return herdr.message ?? "Herdr is not reachable."
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Agents",
                systemImage: "person.2.fill",
                trailing: "\(agents.count)"
            )

            Divider()

            if agents.isEmpty {
                ContentUnavailableView {
                    Label("No agents yet", systemImage: "rectangle.stack.badge.person.crop")
                } description: {
                    Text(emptyAgentsDescription)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(ShellMetrics.panelPadding)
            } else {
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(agents) { agent in
                            AgentRow(agent: agent) {
                                model.selectAgent(agent)
                            }
                        }
                    }
                    .padding(10)
                }
                .accessibilityIdentifier("agent-list")
            }

            Divider()
            RuntimeStatusCards()
            Divider()
            PetStatus()
        }
        .background(Color(nsColor: .controlBackgroundColor))
    }
}

private struct AgentRow: View {
    let agent: SidebarAgent
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 7) {
                    Text(agent.symbol)
                        .font(.system(.body, design: .rounded, weight: .bold))
                        .foregroundStyle(stateColor)
                        .frame(width: 16)
                    Text(agent.workspaceLabel)
                        .font(.callout.weight(.semibold))
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(agent.agentKind)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                HStack(spacing: 6) {
                    Text(agent.summary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(agent.elapsed)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(stateColor.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(agent.state), \(agent.workspaceLabel), \(agent.agentKind), \(agent.summary), \(agent.elapsed)")
        .accessibilityIdentifier("agent-\(agent.id)")
    }

    private var stateColor: Color {
        switch agent.state {
        case "error": .red
        case "question": .yellow
        case "approval": .orange
        case "working": .blue
        case "unseen_completion": .green
        case "idle": .secondary
        default: .gray
        }
    }
}

private struct TerminalPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var attachedPaneDescription: String {
        guard let snapshot = model.core.snapshot,
              let paneID = snapshot.paneLayout?.focusedPaneID ?? snapshot.terminal.paneID
        else {
            return "No pane selected"
        }
        let count = snapshot.paneLayout?.root.paneIDs.count ?? 1
        let zoom = snapshot.paneLayout?.zoomed == true ? " · zoomed" : ""
        let activity = PaneActivityPresentation.suffix(
            for: model.core.snapshot?.status.diagnostics ?? []
        )
        return "\(count) pane\(count == 1 ? "" : "s") · \(paneID)\(zoom)\(activity)"
    }

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Terminal",
                systemImage: "terminal",
                trailing: attachedPaneDescription
            )

            Divider()

            if !model.focusedPaneGridItems.isEmpty {
                PaneLayoutCanvas(items: model.focusedPaneGridItems) { item in
                    if let pane = model.paneMetadata(for: item.paneID) {
                        PaneTerminalCell(
                            pane: pane,
                            status: model.paneStatus(for: pane.id),
                            isFocused: item.isFocused,
                            onFocus: { model.focusPane(pane.id) }
                        ) {
                            TerminalHost(
                                bridge: model.core,
                                paneID: pane.id,
                                onOpenLink: { model.openTerminalLink($0, paneID: pane.id) }
                            )
                                .accessibilityLabel("SwiftTerm terminal for \(pane.id)")
                        }
                    }
                }
                .padding(3)
                .accessibilityIdentifier("terminal-pane-grid")
            } else {
                ContentUnavailableView {
                    Label("No terminal pane", systemImage: "terminal")
                } description: {
                    Text("Select an agent pane to attach its terminal.")
                }
            }
        }
        .background(Color(nsColor: .textBackgroundColor))
    }
}

enum PaneGridPresentation {
    static func visibleRoot(
        layout: CorePaneLayoutSnapshot
    ) -> CorePaneLayoutNode {
        layout.root
    }

    static func items(
        layout: CorePaneLayoutSnapshot,
        focusedPaneID: String? = nil
    ) -> [PaneGridItem] {
        let effectiveFocusedPaneID = focusedPaneID ?? layout.focusedPaneID
        let retained = flatten(
            node: visibleRoot(layout: layout),
            frame: .unit
        )
        return retained.map { item in
            PaneGridItem(
                paneID: item.paneID,
                retainedFrame: item.retainedFrame,
                visualFrame: layout.zoomed && item.paneID == effectiveFocusedPaneID
                    ? .unit
                    : item.retainedFrame,
                isVisible: !layout.zoomed || item.paneID == effectiveFocusedPaneID,
                isFocused: item.paneID == effectiveFocusedPaneID
            )
        }
    }

    static func items(
        remoteLayout: RemotePaneLayoutSnapshot,
        focusedPaneID: String?
    ) -> [PaneGridItem] {
        let effectiveFocusedPaneID = focusedPaneID ?? remoteLayout.focusedPaneID
        return remoteLayout.frames.compactMap { frame in
            guard frame.x.isFinite,
                  frame.y.isFinite,
                  frame.width.isFinite,
                  frame.height.isFinite,
                  frame.width > 0,
                  frame.height > 0
            else { return nil }
            let retainedFrame = PaneGridFrame(
                x: frame.x,
                y: frame.y,
                width: frame.width,
                height: frame.height
            )
            return PaneGridItem(
                paneID: frame.paneID,
                retainedFrame: retainedFrame,
                visualFrame: remoteLayout.zoomed && frame.paneID == effectiveFocusedPaneID
                    ? .unit
                    : retainedFrame,
                isVisible: !remoteLayout.zoomed || frame.paneID == effectiveFocusedPaneID,
                isFocused: frame.paneID == effectiveFocusedPaneID
            )
        }
    }

    static func uniformItems(
        paneIDs: [String],
        focusedPaneID: String?
    ) -> [PaneGridItem] {
        guard !paneIDs.isEmpty else { return [] }

        let columnCount = paneIDs.count > 1 ? 2 : 1
        let rowCount = (paneIDs.count + columnCount - 1) / columnCount

        return paneIDs.enumerated().map { index, paneID in
            let column = index % columnCount
            let row = index / columnCount
            let frame = PaneGridFrame(
                x: Double(column) / Double(columnCount),
                y: Double(row) / Double(rowCount),
                width: 1 / Double(columnCount),
                height: 1 / Double(rowCount)
            )
            return PaneGridItem(
                paneID: paneID,
                retainedFrame: frame,
                visualFrame: frame,
                isVisible: true,
                isFocused: paneID == focusedPaneID
            )
        }
    }

    private static func flatten(
        node: CorePaneLayoutNode,
        frame: PaneGridFrame
    ) -> [PaneGridItem] {
        switch node {
        case let .pane(paneID):
            return [PaneGridItem(
                paneID: paneID,
                retainedFrame: frame,
                visualFrame: frame,
                isVisible: true,
                isFocused: false
            )]
        case let .split(direction, ratio, first, second):
            let firstFrame: PaneGridFrame
            let secondFrame: PaneGridFrame
            if direction == .right {
                firstFrame = PaneGridFrame(
                    x: frame.x,
                    y: frame.y,
                    width: frame.width * ratio,
                    height: frame.height
                )
                secondFrame = PaneGridFrame(
                    x: frame.x + firstFrame.width,
                    y: frame.y,
                    width: frame.width - firstFrame.width,
                    height: frame.height
                )
            } else {
                firstFrame = PaneGridFrame(
                    x: frame.x,
                    y: frame.y,
                    width: frame.width,
                    height: frame.height * ratio
                )
                secondFrame = PaneGridFrame(
                    x: frame.x,
                    y: frame.y + firstFrame.height,
                    width: frame.width,
                    height: frame.height - firstFrame.height
                )
            }
            return flatten(node: first, frame: firstFrame)
                + flatten(node: second, frame: secondFrame)
        }
    }
}

struct PaneGridFrame: Equatable {
    static let unit = PaneGridFrame(x: 0, y: 0, width: 1, height: 1)

    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

struct PaneGridItem: Equatable {
    let paneID: String
    let retainedFrame: PaneGridFrame
    let visualFrame: PaneGridFrame
    let isVisible: Bool
    let isFocused: Bool
}

/// The single pane placement surface used by local and remote terminals.
///
/// Both local and remote panes supply authoritative Herdr split frames.
/// Spacing, clipping, focus stacking, and viewport filling stay identical;
/// only the transport that feeds each terminal differs between devices.
struct HideTerminalGrid<Content: View>: View {
    let items: [PaneGridItem]
    private let content: (PaneGridItem) -> Content

    init(
        items: [PaneGridItem],
        @ViewBuilder content: @escaping (PaneGridItem) -> Content
    ) {
        self.items = items
        self.content = content
    }

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .topLeading) {
                ForEach(items, id: \.paneID) { item in
                    let frame = item.visualFrame
                    content(item)
                        .padding(4)
                        .frame(
                            width: geometry.size.width * CGFloat(frame.width),
                            height: geometry.size.height * CGFloat(frame.height)
                        )
                        .position(
                            x: geometry.size.width * CGFloat(frame.x + frame.width / 2),
                            y: geometry.size.height * CGFloat(frame.y + frame.height / 2)
                        )
                        .opacity(item.isVisible ? 1 : 0)
                        .allowsHitTesting(item.isVisible)
                        .accessibilityHidden(!item.isVisible)
                        .zIndex(item.isFocused ? 1 : 0)
                }
            }
            .clipped()
            .transaction { transaction in
                transaction.animation = nil
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct PaneLayoutCanvas<Content: View>: View {
    let items: [PaneGridItem]
    private let content: (PaneGridItem) -> Content

    init(
        items: [PaneGridItem],
        @ViewBuilder content: @escaping (PaneGridItem) -> Content
    ) {
        self.items = items
        self.content = content
    }

    var body: some View {
        HideTerminalGrid(items: items, content: content)
    }
}

struct PaneTerminalCell<Content: View>: View {
    let pane: CorePaneSnapshot
    let status: String
    let isFocused: Bool
    let onFocus: () -> Void
    private let content: () -> Content

    init(
        pane: CorePaneSnapshot,
        status: String,
        isFocused: Bool,
        onFocus: @escaping () -> Void,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.pane = pane
        self.status = status
        self.isFocused = isFocused
        self.onFocus = onFocus
        self.content = content
    }

    var body: some View {
        HideTerminalPaneCard(
            paneID: pane.id,
            title: pane.label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? pane.id
                : pane.label,
            cwd: pane.cwd,
            status: status,
            isFocused: isFocused,
            onFocus: onFocus,
            content: content
        )
    }
}

/// Shared chrome for local and remote panes.
///
/// The terminal implementation is supplied by the caller, but the pane
/// identity, cwd, focus affordance, border, and sizing stay identical across
/// devices. This keeps a remote pane from becoming a separate visual mode.
struct HideTerminalPaneCard<Content: View>: View {
    let paneID: String
    let title: String
    let cwd: String
    let status: String
    let isFocused: Bool
    let onFocus: () -> Void
    private let content: () -> Content

    init(
        paneID: String,
        title: String,
        cwd: String,
        status: String,
        isFocused: Bool,
        onFocus: @escaping () -> Void,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.paneID = paneID
        self.title = title
        self.cwd = cwd
        self.status = status
        self.isFocused = isFocused
        self.onFocus = onFocus
        self.content = content
    }

    var body: some View {
        VStack(spacing: 0) {
            Button(action: onFocus) {
                HStack(spacing: 8) {
                    Image(systemName: isFocused ? "circle.inset.filled" : "circle")
                        .foregroundStyle(isFocused ? HideTheme.accent : HideTheme.secondary)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(title)
                            .hideFont(size: 10, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        if !cwd.isEmpty {
                            Text(cwd)
                                .hideFont(size: 9, design: .monospaced)
                                .foregroundStyle(HideTheme.muted)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                    Spacer(minLength: 4)
                    Text(status)
                        .hideFont(size: 9)
                        .foregroundStyle(status == "closed" ? HideTheme.danger : HideTheme.secondary)
                }
                .padding(.horizontal, 8)
                .frame(maxWidth: .infinity, minHeight: 38, maxHeight: 38, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Focus terminal pane \(title) (\(paneID))")

            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)

            content()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .layoutPriority(1)
                .clipped()
        }
        .frame(maxWidth: .infinity, minHeight: 120, maxHeight: .infinity)
        .background(HideTheme.panel)
        .overlay {
            RoundedRectangle(cornerRadius: 4)
                .stroke(
                    isFocused ? HideTheme.accent : HideTheme.divider,
                    lineWidth: isFocused ? 2 : 1
                )
        }
        .accessibilityIdentifier("terminal-pane-\(paneID)")
    }
}

enum PaneActivityPresentation {
    static func suffix(for diagnostics: [CoreDiagnostic]) -> String {
        guard let kind = diagnostics.last(where: { $0.kind.hasPrefix("pane.") })?.kind else {
            return ""
        }
        return switch kind {
        case "pane.split.right.requested": " · splitting right…"
        case "pane.split.down.requested": " · splitting down…"
        case "pane.zoom.requested": " · toggling zoom…"
        case "pane.attach.requested": " · attaching…"
        case "pane.attach.ready": " · attached"
        case "pane.split.right": " · split right"
        case "pane.split.down": " · split down"
        default: ""
        }
    }
}

struct PanelHeader: View {
    let title: String
    let systemImage: String
    let trailing: String

    var body: some View {
        HStack(spacing: ShellMetrics.compactSpacing) {
            Label(title, systemImage: systemImage)
                .font(.headline)
            Spacer(minLength: ShellMetrics.compactSpacing)
            Text(trailing)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(.horizontal, ShellMetrics.panelPadding)
        .frame(height: 44)
    }
}

private struct PetStatus: View {
    @EnvironmentObject private var model: ShellModel

    /// The pet's own state, said once. The pose and badge row on the pet
    /// itself are the primary encoding; this line only names what it is
    /// doing for someone reading the sidebar.
    private var petHeadline: String {
        guard let pet = model.core.pet else { return "Pet" }
        guard pet.visible else { return "Pet is hidden" }
        return switch pet.pose {
        case "disconnected": "Pet: herdr is unreachable"
        case "error": "Pet: unseen error"
        case "notification": "Pet: waiting on you"
        case "juggling", "carrying", "working": "Pet: agents working"
        case "sleeping", "yawning", "dozing", "collapsing": "Pet is asleep"
        default: "Pet is quiet"
        }
    }

    private var petDetail: String {
        guard let pet = model.core.pet else { return "" }
        guard pet.visible else {
            return "Turn it back on here, from the menu bar, or with herdr-ide://show."
        }
        if !pet.isConnected {
            return pet.connectionMessage ?? "The herdr session is not answering."
        }
        if let paneID = pet.attentionPaneIDs.first {
            return "Click the pet to jump to \(paneID)."
        }
        return "Click the pet to bring this window forward."
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: ShellMetrics.cardRadius)
                        .fill(Color.accentColor.opacity(0.12))
                    Image(systemName: "pawprint.fill")
                        .foregroundStyle(Color.accentColor)
                }
                .frame(width: 34, height: 34)

                VStack(alignment: .leading, spacing: 2) {
                    Text(petHeadline)
                        .font(.callout.weight(.medium))
                    Text(petDetail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
                Toggle("Show pet", isOn: Binding(
                    get: { model.core.pet?.visible ?? true },
                    set: { model.core.setPetVisible($0) }
                ))
                .toggleStyle(.switch)
                .labelsHidden()
                .accessibilityLabel("Show pet")
                .accessibilityIdentifier("pet-visible-toggle-sidebar")
            }

            Menu("Safety previews") {
                Button("Close working pane…") { model.previewConsequence(.pane) }
                Button("Close tab…") { model.previewConsequence(.tab) }
                Button("Close workspace…") { model.previewConsequence(.workspace) }
                Button("Remove worktree checkout…") { model.previewConsequence(.worktree) }
            }
            .menuStyle(.borderlessButton)
            .accessibilityIdentifier("safety-preview-menu")

            if let result = model.consequenceResult {
                Text(result)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(ShellMetrics.panelPadding)
        .accessibilityIdentifier("pet-status")
    }
}

private struct RuntimeStatusCards: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(spacing: 8) {
            BrowserStatusCard()
            RemoteStatusCard()
        }
        .padding(10)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

private struct BrowserStatusCard: View {
    @EnvironmentObject private var model: ShellModel
    private var receipt: BrowserRuntimeReceipt { model.browser.receipt }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Label("Chrome", systemImage: "globe")
                    .font(.caption.weight(.semibold))
                Spacer()
                Text(receipt.phase.rawValue)
                    .font(.caption2.monospaced())
                    .foregroundStyle(phaseColor(receipt.phase))
            }
            Text(receipt.currentTitle ?? receipt.message)
                .font(.caption)
                .lineLimit(2)
            if let url = receipt.currentURL {
                Text(url)
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            HStack {
                Button("Open Browser") { model.browser.openOrFocus() }
                    .disabled(receipt.phase == .loading || model.browser.profile != "default")
                    .accessibilityIdentifier("chromux-open-default")
                Button("Retry") { model.browser.refresh() }
                    .disabled(receipt.phase == .loading)
            }
            .controlSize(.small)
            if receipt.phase == .stale || receipt.phase == .failed || receipt.phase == .unavailable {
                Text(receipt.message + " Last checked: " + receipt.checkedAt)
                    .font(.caption2)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(9)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityIdentifier("chromux-status-card")
    }
}

private struct RemoteStatusCard: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Label("mini", systemImage: "externaldrive.connected.to.line.below")
                    .font(.caption.weight(.semibold))
                Spacer()
                Text(model.remote.phase.rawValue)
                    .font(.caption2.monospaced())
                    .foregroundStyle(phaseColor(model.remote.phase))
            }
            if model.remote.phase == .loading {
                ProgressView("Connecting through SSH…")
                    .controlSize(.small)
            } else {
                Text(model.remote.message)
                    .font(.caption)
                    .lineLimit(3)
            }
            if let workspace = model.remote.workspaces.first {
                Label("\(workspace.label) · \(workspace.paneCount) pane(s)", systemImage: "rectangle.3.group")
                    .font(.caption2)
                    .lineLimit(1)
                Text("Remote inline editing is disabled. Use the attached remote terminal to modify files.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Button("Refresh mini") { model.remote.refreshMini() }
                .controlSize(.small)
                .disabled(model.remote.phase == .loading)
                .accessibilityIdentifier("remote-mini-refresh")
        }
        .padding(9)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityIdentifier("remote-mini-status-card")
    }
}

private func phaseColor(_ phase: RuntimePhase) -> Color {
    switch phase {
    case .ready: .green
    case .loading: .blue
    case .stale, .unavailable: .orange
    case .failed: .red
    case .idle: .secondary
    }
}

private struct StatusBar: View {
    @EnvironmentObject private var model: ShellModel

    private var statusMessage: String {
        if let error = model.core.bridgeError { return error }
        if model.browser.receipt.phase == .failed || model.browser.receipt.phase == .stale {
            return model.browser.receipt.message
        }
        if let environment = model.core.snapshot?.status.environment.first(where: { $0.key == "SSH_AUTH_SOCK" }),
           environment.state != "available" {
            return environment.message
        }
        return "Local features ready"
    }

    var body: some View {
        HStack(spacing: 14) {
            Label(statusMessage, systemImage: "circle.fill")
                .symbolRenderingMode(.palette)
                .foregroundStyle(.orange, .orange)
            Spacer()
            Text("Chromux \(model.browser.receipt.phase.rawValue)")
            Text("mini \(model.remote.phase.rawValue)")
            Text("Core schema v1")
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .frame(height: 28)
        .background(Color(nsColor: .controlBackgroundColor))
        .accessibilityIdentifier("status-bar")
    }
}
