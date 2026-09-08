import SwiftUI

/// Geometry is a pure view of ordered parent IDs. Activity never changes lanes.
struct OverviewGraph {
    struct Row: Identifiable {
        let id: String
        let subject: String
        var decorations: String = ""
        let parents: [String]
        let folded: [CoreGitCommit]
        let checkouts: [CoreCheckoutSnapshot]
        var lane: Int = 0
        var y: CGFloat = 0
        var height: CGFloat {
            checkouts.isEmpty ? HideTheme.Overview.commitHeight
                : CGFloat(checkouts.count) * HideTheme.Overview.workspaceHeight
        }
    }
    struct Edge { let from: CGPoint; let to: CGPoint; let continuation: Bool; let lane: Int }
    var rows: [Row] = []
    var edges: [Edge] = []
    var width: CGFloat = HideTheme.Overview.railWidth
    var height: CGFloat = 0

    init(history: CoreGitHistory?, checkouts: [CoreCheckoutSnapshot], expanded: Set<String>) {
        guard let history else { return }
        let attached = Dictionary(grouping: checkouts.filter { $0.worktree?.headSHA != nil }, by: { $0.worktree!.headSHA! })
        let children = Dictionary(grouping: history.commits.flatMap(\.parents), by: { $0 }).mapValues(\.count)
        var index = 0
        while index < history.commits.count {
            let commit = history.commits[index]
            var chain = [commit]
            if attached[commit.sha] == nil && commit.parents.count == 1 && (commit.decorations ?? "").isEmpty {
                while index + chain.count < history.commits.count {
                    let next = history.commits[index + chain.count]
                    guard chain.last?.parents == [next.sha], children[next.sha] == 1,
                          attached[next.sha] == nil, next.parents.count == 1, (next.decorations ?? "").isEmpty else { break }
                    chain.append(next)
                }
            }
            if chain.count > 2 && !expanded.contains(commit.sha) {
                rows.append(Row(id: commit.sha, subject: "\(chain.count) commits", parents: chain.last!.parents,
                                folded: chain, checkouts: []))
            } else {
                rows += chain.map { Row(id: $0.sha, subject: $0.subject, decorations: $0.decorations ?? "", parents: $0.parents,
                                        folded: [], checkouts: attached[$0.sha] ?? []) }
            }
            index += chain.count
        }
        var active: [String?] = []
        var positions: [String: CGPoint] = [:]
        for i in rows.indices {
            let lane: Int
            if let found = active.firstIndex(where: { $0 == rows[i].id }) { lane = found }
            else if let free = active.firstIndex(where: { $0 == nil }) { lane = free }
            else { lane = active.count; active.append(nil) }
            rows[i].lane = lane; rows[i].y = height
            positions[rows[i].id] = CGPoint(x: HideTheme.Overview.laneInset + CGFloat(lane) * HideTheme.Overview.laneSpacing,
                                          y: height + HideTheme.Overview.nodeOffset)
            height += rows[i].height
            active[lane] = nil
            for parent in rows[i].parents where !active.contains(where: { $0 == parent }) {
                if let free = active.firstIndex(where: { $0 == nil }) { active[free] = parent }
                else { active.append(parent) }
            }
        }
        width = max(width, HideTheme.Overview.laneInset * 2 + CGFloat(active.count) * HideTheme.Overview.laneSpacing)
        for row in rows {
            guard let from = positions[row.id] else { continue }
            for parent in row.parents {
                let target = positions[parent]
                let lane = active.firstIndex(where: { $0 == parent }) ?? row.lane
                let to = target ?? CGPoint(x: HideTheme.Overview.laneInset + CGFloat(lane) * HideTheme.Overview.laneSpacing,
                                           y: height + HideTheme.Overview.commitHeight)
                edges.append(Edge(from: from, to: to, continuation: target == nil, lane: row.lane))
            }
        }
        height += HideTheme.Overview.commitHeight
    }
}

struct OverviewGitTree<RowContent: View>: View {
    let history: CoreGitHistory?
    let checkouts: [CoreCheckoutSnapshot]
    let selectedID: String?
    @ViewBuilder let rowContent: (CoreCheckoutSnapshot) -> RowContent
    @State private var expanded = Set<String>()
    @State private var graph = OverviewGraph(history: nil, checkouts: [], expanded: [])
    private var heads: [String] { checkouts.map { "\($0.id):\($0.worktree?.headSHA ?? "")" } }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            ScrollView([.vertical, .horizontal]) {
                if let reason = history?.unavailableReason {
                    Text(reason).foregroundStyle(HideTheme.warning).padding(HideTheme.spacingMD)
                } else if history == nil {
                    Text("Reading Git history…").foregroundStyle(HideTheme.secondary).padding(HideTheme.spacingMD)
                } else {
                    VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                        if !expanded.isEmpty {
                            Button("Fold history") { expanded.removeAll(); rebuild() }.buttonStyle(.plain)
                        }
                        if history?.shallowBoundaries?.isEmpty == false {
                            Text("Shallow history · earlier ancestry is unavailable").foregroundStyle(HideTheme.secondary)
                        }
                        ZStack(alignment: .topLeading) {
                            Canvas { context, _ in
                                for edge in graph.edges {
                                    var path = Path()
                                    path.move(to: edge.from)
                                    if edge.from.x == edge.to.x { path.addLine(to: edge.to) }
                                    else {
                                        let corner = CGPoint(x: edge.from.x, y: edge.to.y - HideTheme.spacingMD)
                                        path.addLine(to: corner)
                                        path.addQuadCurve(to: edge.to, control: CGPoint(x: edge.from.x, y: edge.to.y))
                                    }
                                    context.stroke(path, with: .color(HideTheme.Overview.laneColor(edge.lane)),
                                                   style: StrokeStyle(lineWidth: HideTheme.Overview.graphLineWidth,
                                                                      dash: edge.continuation ? [HideTheme.spacingXS] : []))
                                }
                                for row in graph.rows {
                                    let point = CGPoint(x: HideTheme.Overview.laneInset + CGFloat(row.lane) * HideTheme.Overview.laneSpacing,
                                                        y: row.y + HideTheme.Overview.nodeOffset)
                                    if !row.checkouts.isEmpty {
                                        let junctionX = graph.width - HideTheme.Overview.laneInset
                                        for offset in row.checkouts.indices {
                                            var attachment = Path()
                                            attachment.move(to: point)
                                            attachment.addLine(to: CGPoint(x: junctionX, y: point.y))
                                            let targetY = row.y + HideTheme.Overview.nodeOffset
                                                + CGFloat(offset) * HideTheme.Overview.workspaceHeight
                                            attachment.addLine(to: CGPoint(x: junctionX, y: targetY))
                                            attachment.addLine(to: CGPoint(x: graph.width, y: targetY))
                                            context.stroke(attachment, with: .color(HideTheme.muted),
                                                style: StrokeStyle(lineWidth: HideTheme.Layout.hairlineWidth, dash: [HideTheme.spacingXS]))
                                        }
                                    }
                                    let size = HideTheme.Overview.nodeSize
                                    context.fill(Path(ellipseIn: CGRect(x: point.x - size / 2, y: point.y - size / 2, width: size, height: size)),
                                                 with: .color(row.checkouts.contains { $0.worktree?.isMain == true } ? HideTheme.secondary : HideTheme.Overview.laneColor(row.lane)))
                                    if row.checkouts.contains(where: { $0.id == selectedID }) {
                                        let ring = HideTheme.Overview.selectedNodeSize
                                        context.stroke(Path(ellipseIn: CGRect(x: point.x - ring / 2, y: point.y - ring / 2, width: ring, height: ring)),
                                                       with: .color(HideTheme.primary), lineWidth: HideTheme.Layout.hairlineWidth)
                                    }
                                }
                            }.frame(width: graph.width, height: graph.height).accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                                ForEach(graph.rows) { row in
                                    if !row.checkouts.isEmpty {
                                        ForEach(row.checkouts) { cached in
                                            if let current = checkouts.first(where: { $0.id == cached.id }) {
                                                rowContent(current).frame(height: HideTheme.Overview.workspaceHeight)
                                            }
                                        }
                                    } else if !row.folded.isEmpty {
                                        Button { expanded.insert(row.id); rebuild() } label: {
                                            Label(row.subject, systemImage: "ellipsis")
                                        }.buttonStyle(.plain).foregroundStyle(HideTheme.secondary)
                                            .frame(height: row.height).hideTooltip("Expand \(row.folded.count) commits")
                                    } else {
                                        Text(row.decorations.isEmpty ? row.subject : "\(row.decorations.replacingOccurrences(of: "refs/heads/", with: "").replacingOccurrences(of: "refs/remotes/", with: "")) · \(row.subject)").lineLimit(1).foregroundStyle(HideTheme.muted)
                                            .frame(height: row.height).hideTooltip("\(row.id)\n\(row.subject)")
                                    }
                                }
                            }.padding(.leading, graph.width).frame(width: graph.width + HideTheme.Overview.rowWidth)
                        }.frame(height: graph.height)
                        if history?.truncated == true || graph.edges.contains(where: \.continuation) {
                            Text("Earlier history outside this window").foregroundStyle(HideTheme.secondary)
                        }
                        ForEach(checkouts.filter { row in
                            !graph.rows.contains { $0.checkouts.contains { $0.id == row.id } }
                        }) { row in
                            rowContent(row)
                            Text(row.worktree?.headSHA == nil ? "No commit available" : "HEAD outside loaded history")
                                .foregroundStyle(HideTheme.muted)
                        }
                        if history?.commits.isEmpty == true && checkouts.isEmpty {
                            Text("No commits yet").foregroundStyle(HideTheme.secondary)
                        }
                    }.hideFont(size: HideTheme.Typography.subhead).padding(.horizontal, HideTheme.spacingSM)
                }
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: HideTheme.spacingSM) { legend }
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) { legend }
            }.hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.secondary)
                .frame(maxWidth: .infinity, alignment: .leading).padding(HideTheme.spacingSM)
        }.onAppear(perform: rebuild)
            .onChange(of: history) { _, _ in rebuild() }
            .onChange(of: heads) { _, _ in rebuild() }
            .accessibilityIdentifier("overview-git-tree")
    }
    @ViewBuilder private var legend: some View {
        Text("━ Git history")
        Text("┄ Worktree HEAD")
        Text("··· Folded commits")
    }
    private func rebuild() { graph = OverviewGraph(history: history, checkouts: checkouts, expanded: expanded) }
}
