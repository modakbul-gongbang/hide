import AppKit
import SwiftUI

/// Project Home: the focused project as a constellation.
///
/// Drawn in the empty checkout state and, on request, over a checkout that
/// has tabs. The presentation decides every node, edge, label and colour;
/// this view fits the map to its canvas, draws it in one `Canvas` pass and
/// dispatches the intent a click stands for. Selection and hover are local
/// to the view and publish nothing (DESIGN.md, Project Home).
struct ProjectHome: View {
    @EnvironmentObject private var model: ShellModel
    /// The node whose neighbourhood is drawn at full strength; `nil` is the
    /// whole project (PRD rule 2).
    @State private var focus: String?
    @State private var focusSeeded = false
    @State private var hover: String?
    @State private var hoverPoint: CGPoint?

    private var home: ProjectHomeModel {
        ProjectHomePresentation.build(
            workspace: model.focusedWorkspace,
            agents: model.agents,
            connected: model.agentsConnected
        )
    }

    var body: some View {
        let home = home
        GeometryReader { proxy in
            // Below the collapse width the rail folds into the canvas and
            // the header keeps only what fits on one line.
            let compact = proxy.size.width < HideTheme.Home.railCollapseWidth
            VStack(spacing: HideTheme.spacingNone) {
                ProjectHomeHeader(home: home, compact: compact, focus: $focus)
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
                let showsRail = !compact && !home.rail.isEmpty
                HStack(spacing: HideTheme.spacingNone) {
                    ProjectHomeCanvas(
                        home: home,
                        positions: model.projectHomeLayout.positions(for: home.topology),
                        focus: $focus,
                        hover: $hover,
                        hoverPoint: $hoverPoint,
                        onActivate: activate
                    )
                    if showsRail {
                        Rectangle()
                            .fill(HideTheme.divider)
                            .frame(width: HideTheme.Layout.hairlineWidth)
                        ProjectHomeRail(rows: home.rail, focus: $focus, onActivate: activate)
                            .frame(width: HideTheme.Home.railWidth)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .onAppear { seedFocus(home) }
        .onChange(of: home.shape) { _, _ in seedFocus(home) }
        .accessibilityIdentifier("project-home")
    }

    /// The focused pane's agent, or the focused checkout, on entry only. A
    /// later focus change while the page is open is the operator's own
    /// selection to keep.
    private func seedFocus(_ home: ProjectHomeModel) {
        guard !focusSeeded, home.shape != .loading else { return }
        focusSeeded = true
        focus = ProjectHomePresentation.initialFocus(
            home, focusedPaneID: model.focusedPaneID, focusedCheckoutID: model.focusedCheckout?.id
        )
    }

    /// A node is opened by activating it while it is already selected; the
    /// first activation selects it and shows its neighbourhood.
    private func activate(_ node: ProjectHomeNode) {
        guard focus == node.id || !node.opensOnActivate else {
            focus = node.id
            return
        }
        switch node.kind {
        case .agent:
            if let paneID = node.paneID { model.selectAgent(paneID: paneID) }
        case .checkout:
            if let checkout = model.focusedWorkspace?.checkouts.first(where: { $0.id == node.checkoutID }) {
                model.selectCheckout(checkout)
            }
        case .pullRequest:
            if let pullRequest = node.pullRequest { model.openPullRequest(pullRequest) }
        case .project:
            focus = nil
        }
    }
}

// MARK: - Header

/// The glance row outside the canvas: project, counts, Start new terminal,
/// and the way back to the whole project (PRD rule 6).
private struct ProjectHomeHeader: View {
    @EnvironmentObject private var model: ShellModel
    let home: ProjectHomeModel
    /// The page is narrower than the rail collapse width.
    let compact: Bool
    @Binding var focus: String?

    /// Counts show their mark instead of their word where the row is short
    /// of room: a narrow page, or the stale notice taking the words' place.
    private var compactCounts: Bool { compact || home.stale }

    var body: some View {
        let compact = compactCounts
        HStack(spacing: HideTheme.spacingMD) {
            Text(home.projectName)
                .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                .foregroundStyle(HideTheme.primary)
                .lineLimit(1)
                .layoutPriority(1)
            HStack(spacing: compact ? HideTheme.spacingSM : HideTheme.Home.countGap) {
                ForEach(home.counts.entries, id: \.label) { entry in
                    HStack(spacing: HideTheme.spacingXS) {
                        if compact {
                            AgentStatusMark(symbol: entry.symbol, color: entry.count > 0 ? entry.color : HideTheme.muted)
                        }
                        Text("\(entry.count)")
                            .hideFont(size: HideTheme.Typography.title, weight: .semibold, design: .monospaced)
                            .foregroundStyle(entry.count > 0 ? entry.color : HideTheme.muted)
                        if !compact {
                            Text(entry.label)
                                .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                                .foregroundStyle(entry.count > 0 ? HideTheme.secondary : HideTheme.muted)
                        }
                    }
                    .fixedSize()
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel("\(entry.count) \(entry.label)")
                    .hideTooltip(entry.label)
                }
            }
            if home.stale {
                Label("Herdr reconnecting", systemImage: "bolt.slash")
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .foregroundStyle(HideTheme.warning)
                    .lineLimit(1)
                    .hideTooltip("Live status unavailable while Herdr reconnects")
                    .accessibilityLabel("Live status unavailable while Herdr reconnects")
                    .accessibilityIdentifier("project-home-stale")
            }
            Spacer(minLength: HideTheme.spacingSM)
            if focus != nil {
                Button("Whole project") { focus = nil }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    .fixedSize()
                    .hideTooltip("Show every checkout and agent")
                    .accessibilityIdentifier("project-home-whole-project")
            }
            if model.projectHomeVisible {
                HideIconButton(
                    systemImage: "xmark",
                    help: "Close Project Home",
                    variant: .toolbar,
                    command: .menu(.projectHome),
                    action: model.toggleProjectHome
                )
                .accessibilityIdentifier("project-home-close")
            }
            Button("Start new terminal") { model.addTab() }
                .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                .fixedSize()
                .hideTooltip("Start new terminal", command: .menu(.newTab))
                .accessibilityIdentifier("project-home-start-terminal")
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .frame(height: HideTheme.Home.headerHeight)
        .background(HideTheme.panel)
    }
}

// MARK: - Canvas

/// The map. One drawing pass over the presentation's nodes and edges, hit
/// testing as a pure function of the positions, and a card beside the
/// hovered node. Internal so a test can host it with a hover set, since a
/// background window never receives the pointer.
struct ProjectHomeCanvas: View {
    let home: ProjectHomeModel
    let positions: [String: CGPoint]
    @Binding var focus: String?
    @Binding var hover: String?
    @Binding var hoverPoint: CGPoint?
    let onActivate: (ProjectHomeNode) -> Void
    @Environment(\.hideFontScale) private var fontScale

    var body: some View {
        GeometryReader { proxy in
            let fit = ProjectHomeFit(
                bounds: ProjectHomeGraphLayout.bounds(positions, nodes: home.topology.nodes),
                canvas: proxy.size
            )
            let emphasized = ProjectHomePresentation.emphasized(home, focus: focus, hover: hover)
            ScrollView([.horizontal, .vertical], showsIndicators: fit.scrolls) {
                ZStack(alignment: .topLeading) {
                    Canvas(rendersAsynchronously: false) { context, _ in
                        draw(in: &context, fit: fit, emphasized: emphasized)
                    }
                    .frame(width: fit.content.width, height: fit.content.height)
                    .opacity(home.stale ? HideTheme.Opacity.dimmed : 1)
                    .accessibilityChildren {
                        ForEach(home.nodes) { node in
                            Text(node.accessibilityLabel)
                        }
                    }
                    .accessibilityLabel("Project map")
                    .onContinuousHover(coordinateSpace: .local) { phase in
                        switch phase {
                        case .active(let point):
                            let hit = ProjectHomeGraphLayout.hit(
                                fit.layoutPoint(point), positions: positions,
                                nodes: home.topology.nodes, margin: HideTheme.Home.hitMargin
                            )
                            if hit != hover { hover = hit }
                            hoverPoint = hit == nil ? nil : point
                        case .ended:
                            hover = nil
                            hoverPoint = nil
                        }
                    }
                    .onTapGesture(coordinateSpace: .local) { point in
                        guard let hit = ProjectHomeGraphLayout.hit(
                            fit.layoutPoint(point), positions: positions,
                            nodes: home.topology.nodes, margin: HideTheme.Home.hitMargin
                        ), let node = home.node(hit) else {
                            focus = nil
                            return
                        }
                        onActivate(node)
                    }
                    if let message = home.emptyMessage {
                        Text(message)
                            .hideFont(size: HideTheme.Typography.subhead)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: HideTheme.Home.cardWidth + HideTheme.Home.cardWidth / 2)
                            .position(x: fit.content.width / 2, y: fit.content.height - HideTheme.Home.canvasInset)
                            .accessibilityIdentifier("project-home-empty")
                    }
                    if let hover, let hoverPoint, let node = home.node(hover), let card = node.card {
                        ProjectHomeHoverCard(card: card)
                            .frame(width: HideTheme.Home.cardWidth)
                            .fixedSize()
                            .position(fit.cardCentre(near: hoverPoint, radius: node.radius * fit.scale))
                            .allowsHitTesting(false)
                            .transition(.opacity)
                    }
                }
            }
            .scrollDisabled(!fit.scrolls)
            .defaultScrollAnchor(.center)
        }
        .clipped()
        .accessibilityIdentifier("project-home-canvas")
    }

    private func draw(in context: inout GraphicsContext, fit: ProjectHomeFit, emphasized: Set<String>?) {
        func strength(_ ids: String...) -> Double {
            guard let emphasized else { return 1 }
            return ids.allSatisfy { emphasized.contains($0) } ? 1 : HideTheme.Opacity.dimmed
        }
        for edge in home.edges {
            guard let a = positions[edge.from], let b = positions[edge.to] else { continue }
            var path = Path()
            path.move(to: fit.canvasPoint(a))
            path.addLine(to: fit.canvasPoint(b))
            context.stroke(
                path,
                with: .color(edge.color.opacity(strength(edge.from, edge.to))),
                style: StrokeStyle(lineWidth: edge.width, dash: edge.dashed ? [HideTheme.spacingXS, HideTheme.spacingXS] : [])
            )
        }
        for node in home.nodes {
            guard let layoutPoint = positions[node.id] else { continue }
            let centre = fit.canvasPoint(layoutPoint)
            let radius = node.radius * fit.scale
            let opacity = strength(node.id)
            let disc = Path(ellipseIn: CGRect(x: centre.x - radius, y: centre.y - radius, width: radius * 2, height: radius * 2))
            switch node.kind {
            case .project:
                context.fill(disc, with: .color(HideTheme.elevated.opacity(opacity)))
                context.stroke(disc, with: .color(node.color.opacity(opacity)), lineWidth: HideTheme.Home.edgeWidth)
            case .checkout:
                if node.missing {
                    context.stroke(disc, with: .color(node.color.opacity(opacity)),
                                   style: StrokeStyle(lineWidth: HideTheme.Layout.hairlineWidth, dash: [HideTheme.spacingXXS, HideTheme.spacingXXS]))
                } else {
                    context.fill(disc, with: .color(HideTheme.elevated.opacity(opacity)))
                    context.stroke(disc, with: .color(node.color.opacity(opacity)), lineWidth: HideTheme.Layout.hairlineWidth)
                }
                var icon = context.resolve(Image(systemName: "arrow.triangle.branch"))
                icon.shading = .color(node.color.opacity(opacity))
                let side = radius
                context.draw(icon, in: CGRect(x: centre.x - side / 2, y: centre.y - side / 2, width: side, height: side))
            case .agent:
                if node.stallLevel == "soft" || node.stallLevel == "hard" {
                    let halo = radius + HideTheme.Home.haloWidth
                    context.stroke(
                        Path(ellipseIn: CGRect(x: centre.x - halo, y: centre.y - halo, width: halo * 2, height: halo * 2)),
                        with: .color(HideTheme.warning.opacity(opacity * (node.stallLevel == "hard" ? 1 : HideTheme.Opacity.secondary))),
                        lineWidth: HideTheme.Home.haloWidth / 2
                    )
                }
                context.fill(disc, with: .color(node.color.opacity(HideTheme.Opacity.emphasisFill * opacity)))
                context.stroke(disc, with: .color(node.color.opacity(opacity)), lineWidth: node.delegated ? HideTheme.Layout.hairlineWidth : HideTheme.Home.edgeWidth)
                if let symbol = node.symbol {
                    let mark = Text(symbol)
                        .font(HideTheme.font(size: HideTheme.Typography.micro * fontScale * fit.labelFontScale, weight: .bold, design: .monospaced))
                        .foregroundColor(node.color.opacity(opacity))
                    context.draw(mark, at: centre, anchor: .center)
                }
            case .pullRequest:
                context.fill(disc, with: .color(node.color.opacity(opacity)))
            }
            if node.id == focus {
                let ring = radius + HideTheme.Home.selectionRingInset
                context.stroke(
                    Path(ellipseIn: CGRect(x: centre.x - ring, y: centre.y - ring, width: ring * 2, height: ring * 2)),
                    with: .color(HideTheme.primary.opacity(opacity)),
                    lineWidth: HideTheme.Layout.hairlineWidth
                )
            }
            // Labels are always drawn (PRD rule 4); the frame is the one the
            // layout separated, so what it cleared on paper is clear here.
            let frame = fit.canvasRect(ProjectHomeGraphLayout.labelFrame(for: node.layout, at: layoutPoint))
            let labelColor: Color = switch node.kind {
            case .project: HideTheme.primary
            case .checkout: node.missing ? HideTheme.muted : HideTheme.primary
            case .agent: node.delegated ? HideTheme.muted : HideTheme.secondary
            case .pullRequest: node.color
            }
            let weight: Font.Weight = node.kind == .project || node.kind == .checkout ? .semibold : .regular
            let label = context.resolve(
                Text(node.label)
                    .font(HideTheme.font(size: HideTheme.Typography.caption * fontScale * fit.labelFontScale, weight: weight, design: .default))
                    .foregroundColor(labelColor.opacity(opacity))
            )
            let size = label.measure(in: CGSize(width: HideTheme.Home.labelMaxWidth * fit.labelFontScale, height: HideTheme.Home.labelHeight * fit.labelFontScale))
            context.draw(label, in: CGRect(x: frame.midX - size.width / 2, y: frame.minY, width: size.width, height: size.height))
            if let detail = node.detail {
                let line = context.resolve(
                    Text(detail)
                        .font(HideTheme.font(size: HideTheme.Typography.micro * fontScale * fit.labelFontScale, weight: .regular, design: .monospaced))
                        .foregroundColor(HideTheme.muted.opacity(opacity))
                )
                let detailSize = line.measure(in: CGSize(width: HideTheme.Home.labelMaxWidth * fit.labelFontScale, height: HideTheme.Home.labelHeight * fit.labelFontScale))
                context.draw(line, in: CGRect(x: frame.midX - detailSize.width / 2, y: frame.maxY, width: detailSize.width, height: detailSize.height))
            }
            if node.missing {
                let badge = context.resolve(
                    Text("missing")
                        .font(HideTheme.font(size: HideTheme.Typography.micro * fontScale * fit.labelFontScale, weight: .medium, design: .default))
                        .foregroundColor(HideTheme.danger.opacity(opacity))
                )
                let badgeSize = badge.measure(in: CGSize(width: HideTheme.Home.labelMaxWidth * fit.labelFontScale, height: HideTheme.Home.labelHeight * fit.labelFontScale))
                context.draw(badge, in: CGRect(x: frame.midX - badgeSize.width / 2, y: frame.maxY, width: badgeSize.width, height: badgeSize.height))
            }
        }
    }
}

/// How the layout's points land on the canvas: grown to fill it up to the
/// scale cap, centred when smaller, scrolled at scale 1 when larger.
struct ProjectHomeFit: Equatable {
    let bounds: CGRect
    let canvas: CGSize
    let scale: CGFloat
    let content: CGSize
    let origin: CGPoint

    init(bounds: CGRect, canvas: CGSize) {
        self.bounds = bounds
        self.canvas = canvas
        let inset = HideTheme.Home.canvasInset
        let available = CGSize(width: max(1, canvas.width - inset * 2), height: max(1, canvas.height - inset * 2))
        let fit = bounds.isEmpty ? 1 : min(available.width / bounds.width, available.height / bounds.height)
        scale = min(HideTheme.Home.maxScale, max(HideTheme.Home.minScale, fit))
        let mapSize = CGSize(width: bounds.width * scale + inset * 2, height: bounds.height * scale + inset * 2)
        content = CGSize(width: max(canvas.width, mapSize.width), height: max(canvas.height, mapSize.height))
        origin = CGPoint(
            x: (content.width - bounds.width * scale) / 2 - bounds.minX * scale,
            y: (content.height - bounds.height * scale) / 2 - bounds.minY * scale
        )
    }

    var scrolls: Bool { content.width > canvas.width || content.height > canvas.height }

    /// The type size the labels are drawn at: the caption scaled with the
    /// map, never below the floor.
    var labelFontScale: CGFloat { min(1, max(HideTheme.Home.labelMinFontScale, scale)) }

    func canvasPoint(_ point: CGPoint) -> CGPoint {
        CGPoint(x: origin.x + point.x * scale, y: origin.y + point.y * scale)
    }

    func canvasRect(_ rect: CGRect) -> CGRect {
        let topLeft = canvasPoint(rect.origin)
        return CGRect(x: topLeft.x, y: topLeft.y, width: rect.width * scale, height: rect.height * scale)
    }

    func layoutPoint(_ point: CGPoint) -> CGPoint {
        CGPoint(x: (point.x - origin.x) / scale, y: (point.y - origin.y) / scale)
    }

    /// Where the hover card sits: to the right of the node, flipped left
    /// when that would leave the content, and kept inside vertically.
    func cardCentre(near point: CGPoint, radius: CGFloat) -> CGPoint {
        let width = HideTheme.Home.cardWidth
        let gap = radius + HideTheme.Home.cardOffset
        var x = point.x + gap + width / 2
        if x + width / 2 > content.width { x = point.x - gap - width / 2 }
        let y = min(max(point.y, HideTheme.Home.cardWidth / 4), content.height - HideTheme.Home.cardWidth / 4)
        return CGPoint(x: x, y: y)
    }
}

// MARK: - Hover card

private struct ProjectHomeHoverCard: View {
    let card: ProjectHomeCard

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
            HStack(spacing: HideTheme.spacingXS) {
                AgentBadge(agentKind: card.agentKind, stateColor: card.statusColor, size: HideTheme.compactAgentBadgeSize)
                Text(card.title)
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(2)
            }
            HStack(spacing: HideTheme.spacingXS) {
                Text(card.statusLabel)
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .foregroundStyle(card.statusColor)
                Text("·")
                    .foregroundStyle(HideTheme.muted)
                Text(card.elapsed)
                    .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            if let detail = card.detail {
                Text(detail)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.secondary)
                    .lineLimit(3)
            }
            if let notice = card.stallNotice {
                Text(notice)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.warning)
                    .lineLimit(2)
            }
            Label(card.checkoutLabel, systemImage: "arrow.triangle.branch")
                .hideFont(size: HideTheme.Typography.caption)
                .foregroundStyle(HideTheme.muted)
                .lineLimit(1)
        }
        .padding(HideTheme.spacingSM)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        .overlay(
            RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        )
        .accessibilityHidden(true)
    }
}

// MARK: - Attention rail

/// Needs You and Done, as rows, so "act now" is found without scanning the
/// map. One click selects the node; a second opens the pane (PRD rule 6).
private struct ProjectHomeRail: View {
    @EnvironmentObject private var model: ShellModel
    let rows: [ProjectHomeRailRow]
    @Binding var focus: String?
    let onActivate: (ProjectHomeNode) -> Void

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: HideTheme.spacingNone, pinnedViews: []) {
                ForEach(SidebarGrouping.raisedGroups, id: \.rawValue) { group in
                    let groupRows = rows.filter { $0.group == group }
                    if !groupRows.isEmpty {
                        HideSectionLabel(title: group.title, count: groupRows.count)
                        ForEach(groupRows) { row in
                            railRow(row)
                        }
                    }
                }
            }
            .padding(.vertical, HideTheme.spacingSM)
        }
        .background(HideTheme.sidebar)
        .accessibilityIdentifier("project-home-rail")
    }

    private func railRow(_ row: ProjectHomeRailRow) -> some View {
        Button {
            if focus == row.nodeID {
                model.selectAgent(paneID: row.paneID)
            } else {
                focus = row.nodeID
            }
        } label: {
            HStack(alignment: .top, spacing: HideTheme.spacingXS) {
                AgentStatusMark(symbol: row.symbol, color: row.statusColor)
                AgentBadge(agentKind: row.agentKind, stateColor: row.statusColor, size: HideTheme.compactAgentBadgeSize)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    HStack(spacing: HideTheme.spacingXS) {
                        Text(row.title)
                            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(row.elapsed)
                            .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    Text(row.detail ?? row.statusLabel)
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(row.detail == nil ? row.statusColor : HideTheme.secondary)
                        .lineLimit(1)
                    Text(row.checkoutLabel)
                        .hideFont(size: HideTheme.Typography.micro)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.compactAgentRowVerticalPadding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(focus == row.nodeID ? HideTheme.elevated : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .hideTooltip(focus == row.nodeID ? "Open \(row.title)" : "Show \(row.title) on the map")
        .accessibilityLabel([row.title, row.agentKind, row.statusLabel, row.detail].compactMap { $0 }.joined(separator: ", "))
        .accessibilityIdentifier("project-home-rail-\(row.paneID)")
    }
}
