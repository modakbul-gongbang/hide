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
    /// The selected node, drawn with a ring. On entry it is the focused
    /// pane's agent and the map stays whole; only a click asks for the
    /// local graph (PRD rule 2).
    @State private var focus: String?
    /// The operator clicked a node or a rail row: the selection's
    /// neighbourhood is drawn at full strength and the rest dimmed, until
    /// `Whole project`.
    @State private var isolated = false
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
                ProjectHomeHeader(home: home, compact: compact, cache: model.projectHomeLayout, isolated: $isolated)
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
                let showsRail = !compact && !home.rail.isEmpty
                HStack(spacing: HideTheme.spacingNone) {
                    ProjectHomeCanvas(
                        home: home,
                        positions: model.projectHomeLayout.positions(for: home.topology),
                        cache: model.projectHomeLayout,
                        focus: $focus,
                        isolated: $isolated,
                        hover: $hover,
                        hoverPoint: $hoverPoint,
                        onActivate: activate,
                        onOpenPullRequest: { model.openPullRequest($0) }
                    )
                    if showsRail {
                        Rectangle()
                            .fill(HideTheme.divider)
                            .frame(width: HideTheme.Layout.hairlineWidth)
                        ProjectHomeRail(rows: home.rail, focus: $focus, isolated: $isolated)
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

    /// The focused pane's agent, or the focused checkout, ringed on entry
    /// with the map whole. A later focus change while the page is open is
    /// the operator's own selection to keep.
    private func seedFocus(_ home: ProjectHomeModel) {
        guard !focusSeeded, home.shape != .loading else { return }
        focusSeeded = true
        focus = ProjectHomePresentation.initialFocus(
            home, focusedPaneID: model.focusedPaneID, focusedCheckoutID: model.focusedCheckout?.id
        )
        isolated = false
    }

    /// A click selects the node and shows its neighbourhood; a click on the
    /// node already shown that way opens it. The project node is the way
    /// back to the whole map.
    private func activate(_ node: ProjectHomeNode) {
        if node.kind == .project {
            focus = nil
            isolated = false
            return
        }
        guard focus == node.id, isolated, node.opensOnActivate else {
            focus = node.id
            isolated = true
            return
        }
        switch node.kind {
        case .agent:
            if let paneID = node.paneID { model.selectAgent(paneID: paneID) }
        case .checkout:
            if let checkout = model.focusedWorkspace?.checkouts.first(where: { $0.id == node.checkoutID }) {
                model.selectCheckout(checkout)
            }
        case .project:
            break
        }
    }
}

// MARK: - Header

/// The glance row outside the canvas: project, counts, Start new terminal,
/// the way back to the whole project (PRD rule 6) and to the fitted map.
private struct ProjectHomeHeader: View {
    @EnvironmentObject private var model: ShellModel
    let home: ProjectHomeModel
    /// The page is narrower than the rail collapse width.
    let compact: Bool
    @ObservedObject var cache: ProjectHomeLayoutCache
    @Binding var isolated: Bool

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
            if cache.view != .identity {
                HideIconButton(
                    systemImage: "arrow.up.left.and.arrow.down.right",
                    help: "Fit the map to the window",
                    variant: .toolbar,
                    action: { cache.view = .identity }
                )
                .accessibilityIdentifier("project-home-fit")
            }
            if isolated {
                Button("Whole project") { isolated = false }
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
/// testing as a pure function of the positions, a card beside the hovered
/// node, and the pointer answered by `ProjectHomeInput` above it. Internal
/// so a test can host it with a hover and a view set.
struct ProjectHomeCanvas: View {
    let home: ProjectHomeModel
    let positions: [String: CGPoint]
    @ObservedObject var cache: ProjectHomeLayoutCache
    @Binding var focus: String?
    @Binding var isolated: Bool
    @Binding var hover: String?
    @Binding var hoverPoint: CGPoint?
    let onActivate: (ProjectHomeNode) -> Void
    let onOpenPullRequest: (CorePullRequest) -> Void
    @Environment(\.hideFontScale) private var fontScale

    var body: some View {
        GeometryReader { proxy in
            let fit = ProjectHomeFit(
                bounds: ProjectHomeGraphLayout.bounds(positions, nodes: home.topology.nodes),
                canvas: proxy.size,
                view: cache.view
            )
            let emphasized = ProjectHomePresentation.emphasized(home, focus: focus, isolated: isolated, hover: hover)
            ZStack(alignment: .topLeading) {
                Canvas(rendersAsynchronously: false) { context, _ in
                    draw(in: &context, fit: fit, emphasized: emphasized)
                }
                .opacity(home.stale ? HideTheme.Opacity.dimmed : 1)
                .accessibilityChildren {
                    ForEach(home.nodes) { node in
                        Text(node.accessibilityLabel)
                    }
                }
                .accessibilityLabel("Project map")
                if let message = home.emptyMessage {
                    Text(message)
                        .hideFont(size: HideTheme.Typography.subhead)
                        .foregroundStyle(HideTheme.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: HideTheme.Home.cardWidth + HideTheme.Home.cardWidth / 2)
                        .position(x: proxy.size.width / 2, y: proxy.size.height - HideTheme.Home.canvasInset)
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
                ProjectHomeInput(
                    onHover: { point in
                        let hit = point.flatMap { hitNode(at: $0, fit: fit) }
                        if hit != hover { hover = hit }
                        hoverPoint = hit == nil ? nil : point
                    },
                    onClick: { point, clicks in
                        if clicks == 2 {
                            cache.view = .identity
                            return
                        }
                        if let id = hitNode(at: point, fit: fit), let node = home.node(id) {
                            onActivate(node)
                        } else if let pullRequest = hitPullRequest(at: point, fit: fit) {
                            onOpenPullRequest(pullRequest)
                        } else {
                            isolated = false
                        }
                    },
                    onPan: { delta in
                        cache.view = cache.view.panned(by: delta, scale: fit.scale)
                    },
                    onZoom: { factor, point in
                        let offset = CGPoint(x: point.x - proxy.size.width / 2, y: point.y - proxy.size.height / 2)
                        cache.view = cache.view.zoomed(by: factor, fitScale: fit.fitScale, keeping: offset)
                    }
                )
            }
        }
        .clipped()
        .accessibilityIdentifier("project-home-canvas")
    }

    private func hitNode(at point: CGPoint, fit: ProjectHomeFit) -> String? {
        ProjectHomeGraphLayout.hit(
            fit.layoutPoint(point), positions: positions,
            nodes: home.topology.nodes, margin: HideTheme.Home.hitMargin / fit.scale
        )
    }

    /// The pull request whose chip is under the point, if any.
    private func hitPullRequest(at point: CGPoint, fit: ProjectHomeFit) -> CorePullRequest? {
        let layoutPoint = fit.layoutPoint(point)
        for node in home.nodes where node.pullRequest != nil {
            guard let centre = positions[node.id],
                  let index = node.chips.firstIndex(where: { $0.kind == .pullRequest }) else { continue }
            let frames = ProjectHomeGraphLayout.chipFrames(for: node.layout, at: centre)
            if index < frames.count, frames[index].contains(layoutPoint) { return node.pullRequest }
        }
        return nil
    }

    private func draw(in context: inout GraphicsContext, fit: ProjectHomeFit, emphasized: Set<String>?) {
        func strength(_ ids: String...) -> Double {
            guard let emphasized else { return 1 }
            return ids.allSatisfy { emphasized.contains($0) } ? 1 : HideTheme.Opacity.dimmed
        }
        let lineWidth = max(HideTheme.Layout.hairlineWidth, HideTheme.Home.edgeWidth * fit.scale)
        for edge in home.edges {
            guard let a = positions[edge.from], let b = positions[edge.to] else { continue }
            var path = Path()
            path.move(to: fit.canvasPoint(a))
            path.addLine(to: fit.canvasPoint(b))
            context.stroke(
                path,
                with: .color(edge.color.opacity(strength(edge.from, edge.to))),
                style: StrokeStyle(lineWidth: lineWidth, dash: edge.dashed ? [HideTheme.spacingXS * fit.scale, HideTheme.spacingXS * fit.scale] : [])
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
                context.stroke(disc, with: .color(node.color.opacity(opacity)), lineWidth: lineWidth)
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
                if node.group == .needsYou {
                    // A soft static halo: found before its label is read.
                    let halo = radius * HideTheme.Home.attentionHaloScale
                    context.fill(
                        Path(ellipseIn: CGRect(x: centre.x - halo, y: centre.y - halo, width: halo * 2, height: halo * 2)),
                        with: .color(HideTheme.warning.opacity(HideTheme.Opacity.subtleFill * opacity))
                    )
                }
                if node.stallLevel == "soft" || node.stallLevel == "hard" {
                    let halo = radius + HideTheme.Home.haloWidth * fit.scale
                    context.stroke(
                        Path(ellipseIn: CGRect(x: centre.x - halo, y: centre.y - halo, width: halo * 2, height: halo * 2)),
                        with: .color(HideTheme.warning.opacity(opacity * (node.stallLevel == "hard" ? 1 : HideTheme.Opacity.secondary))),
                        lineWidth: HideTheme.Home.haloWidth * fit.scale / 2
                    )
                }
                context.fill(disc, with: .color(node.color.opacity(HideTheme.Opacity.emphasisFill * opacity)))
                context.stroke(disc, with: .color(node.color.opacity(opacity)), lineWidth: node.delegated ? HideTheme.Layout.hairlineWidth : lineWidth)
                if let symbol = node.symbol {
                    let mark = Text(symbol)
                        .font(HideTheme.font(size: HideTheme.Typography.micro * fontScale * fit.labelFontScale, weight: .bold, design: .monospaced))
                        .foregroundColor(node.color.opacity(opacity))
                    context.draw(mark, at: centre, anchor: .center)
                }
            }
            if node.id == focus {
                let ring = radius + HideTheme.Home.selectionRingInset * fit.scale
                context.stroke(
                    Path(ellipseIn: CGRect(x: centre.x - ring, y: centre.y - ring, width: ring * 2, height: ring * 2)),
                    with: .color(HideTheme.primary.opacity(opacity)),
                    lineWidth: HideTheme.Layout.hairlineWidth
                )
            }
            guard ProjectHomePresentation.drawsLabel(node, scale: fit.scale, hovered: node.id == hover, selected: node.id == focus) else { continue }
            // The frame is the one the layout separated, so what it cleared
            // on paper is clear here (PRD rule 4).
            let frame = fit.canvasRect(ProjectHomeGraphLayout.labelFrame(for: node.layout, at: layoutPoint))
            let labelColor: Color = switch node.kind {
            case .project: HideTheme.primary
            case .checkout: node.missing ? HideTheme.muted : HideTheme.primary
            case .agent: node.delegated ? HideTheme.muted : HideTheme.secondary
            }
            let weight: Font.Weight = node.kind == .project || node.kind == .checkout ? .semibold : .regular
            let label = context.resolve(
                Text(node.label)
                    .font(HideTheme.font(size: HideTheme.Typography.caption * fontScale * fit.labelFontScale, weight: weight, design: .default))
                    .foregroundColor(labelColor.opacity(opacity))
            )
            let size = label.measure(in: CGSize(width: HideTheme.Home.labelMaxWidth * fit.labelFontScale, height: HideTheme.Home.labelHeight * fit.labelFontScale))
            let labelRect = CGRect(x: frame.midX - size.width / 2, y: frame.minY, width: size.width, height: size.height)
            if fit.scale < HideTheme.Home.labelClearScale {
                // Zoomed out, a label is wider than the room the layout
                // cleared for it, so it sits on a plate to stay legible over
                // whatever it now crosses.
                context.fill(
                    Path(roundedRect: labelRect.insetBy(dx: -HideTheme.spacingXXS, dy: 0), cornerRadius: HideTheme.radiusExtraSmall),
                    with: .color(HideTheme.background.opacity(HideTheme.Opacity.secondary * opacity))
                )
            }
            context.draw(label, in: labelRect)
            // Chips are detail: they go with the thinned labels.
            let chipFrames = fit.scale < HideTheme.Home.labelThresholdScale ? [] : ProjectHomeGraphLayout.chipFrames(for: node.layout, at: layoutPoint)
            for (chip, layoutFrame) in zip(node.chips, chipFrames) {
                let chipFrame = fit.canvasRect(layoutFrame)
                let shape = Path(roundedRect: chipFrame, cornerRadius: HideTheme.radiusExtraSmall * fit.scale)
                if chip.filled {
                    context.fill(shape, with: .color(chip.color.opacity(HideTheme.Opacity.emphasisFill * opacity)))
                }
                context.stroke(shape, with: .color(chip.color.opacity((chip.filled ? HideTheme.Opacity.secondary : HideTheme.Opacity.disabled) * opacity)), lineWidth: HideTheme.Layout.hairlineWidth)
                let text = context.resolve(
                    Text(chip.text)
                        .font(HideTheme.font(size: HideTheme.Typography.micro * fontScale * fit.labelFontScale, weight: .medium, design: .monospaced))
                        .foregroundColor((chip.filled ? chip.color : HideTheme.secondary).opacity(opacity))
                )
                context.draw(text, at: CGPoint(x: chipFrame.midX, y: chipFrame.midY), anchor: .center)
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

/// How the layout's points land on the canvas: the whole map fitted to the
/// canvas up to the scale cap, then the operator's zoom and pan over that.
struct ProjectHomeFit: Equatable {
    let bounds: CGRect
    let canvas: CGSize
    /// The scale that fits the whole map, before any zoom.
    let fitScale: CGFloat
    let scale: CGFloat
    let view: ProjectHomeViewState

    init(bounds: CGRect, canvas: CGSize, view: ProjectHomeViewState = .identity) {
        self.bounds = bounds
        self.canvas = canvas
        self.view = view
        let inset = HideTheme.Home.canvasInset
        let available = CGSize(width: max(1, canvas.width - inset * 2), height: max(1, canvas.height - inset * 2))
        let fit = bounds.isEmpty ? 1 : min(available.width / bounds.width, available.height / bounds.height)
        fitScale = min(HideTheme.Home.maxScale, fit)
        scale = fitScale * view.zoom
    }

    /// The type size the labels are drawn at: the caption scaled with the
    /// map, never below the floor.
    var labelFontScale: CGFloat { max(HideTheme.Home.labelMinFontScale, scale) }

    /// The map is wider or taller than the canvas at this scale.
    var overflows: Bool { bounds.width * scale > canvas.width || bounds.height * scale > canvas.height }

    private var centre: CGPoint { CGPoint(x: bounds.midX, y: bounds.midY) }

    func canvasPoint(_ point: CGPoint) -> CGPoint {
        CGPoint(
            x: canvas.width / 2 + (point.x - centre.x + view.pan.x) * scale,
            y: canvas.height / 2 + (point.y - centre.y + view.pan.y) * scale
        )
    }

    func canvasRect(_ rect: CGRect) -> CGRect {
        let topLeft = canvasPoint(rect.origin)
        return CGRect(x: topLeft.x, y: topLeft.y, width: rect.width * scale, height: rect.height * scale)
    }

    func layoutPoint(_ point: CGPoint) -> CGPoint {
        CGPoint(
            x: (point.x - canvas.width / 2) / scale + centre.x - view.pan.x,
            y: (point.y - canvas.height / 2) / scale + centre.y - view.pan.y
        )
    }

    /// Where the hover card sits: to the right of the node, flipped left
    /// when that would leave the canvas, and kept inside vertically.
    func cardCentre(near point: CGPoint, radius: CGFloat) -> CGPoint {
        let width = HideTheme.Home.cardWidth
        let gap = radius + HideTheme.Home.cardOffset
        var x = point.x + gap + width / 2
        if x + width / 2 > canvas.width { x = point.x - gap - width / 2 }
        let y = min(max(point.y, HideTheme.Home.cardWidth / 4), canvas.height - HideTheme.Home.cardWidth / 4)
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
    @Binding var isolated: Bool

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
            if focus == row.nodeID, isolated {
                model.selectAgent(paneID: row.paneID)
            } else {
                focus = row.nodeID
                isolated = true
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
            .background(focus == row.nodeID && isolated ? HideTheme.elevated : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .hideTooltip(focus == row.nodeID && isolated ? "Open \(row.title)" : "Show \(row.title) on the map")
        .accessibilityLabel([row.title, row.agentKind, row.statusLabel, row.detail].compactMap { $0 }.joined(separator: ", "))
        .accessibilityIdentifier("project-home-rail-\(row.paneID)")
    }
}
