import Foundation
import SwiftUI

/// One dot on the constellation and everything drawn for it.
///
/// Every label, colour and size is decided here from the core's final values
/// (`group`, `symbol`, `statusLabel`, `delegated`, `stallLevel`) so the map
/// cannot say one thing about an agent while the sidebar says another. The
/// strings are formatted once per snapshot; the canvas draws them as given.
struct ProjectHomeNode: Identifiable, Equatable {
    enum Kind: Equatable {
        case project
        case checkout
        case agent
    }

    let id: String
    let kind: Kind
    /// The label under the node, cut to the token width by character count;
    /// the full text lives in `fullLabel` for the card and accessibility.
    let label: String
    let fullLabel: String
    /// A checkout's second line: its derived state as chips in the track
    /// order (changed, ahead and behind, pull request). Empty elsewhere.
    let chips: [ProjectHomeChip]
    /// The status mark drawn inside an agent node.
    let symbol: String?
    let color: Color
    let radius: CGFloat
    /// Somebody else's work: drawn smaller, with a dashed edge (PRD rule 3).
    let delegated: Bool
    /// `soft` or `hard` on a lineage root whose descendant has stalled.
    let stallLevel: String
    /// A checkout whose worktree folder is gone: a hollow node and a badge.
    let missing: Bool
    let paneID: String?
    let checkoutID: String?
    let pullRequest: CorePullRequest?
    /// Whether opening this node does anything. A missing checkout and the
    /// project itself have nowhere to go; any checkout that exists opens,
    /// primary included.
    let opensOnActivate: Bool
    let accessibilityLabel: String
    let card: ProjectHomeCard?
    let layout: ProjectHomeGraphLayout.Node

    var group: AgentGroup? { card?.group }
}

/// One chip under a checkout hub. `filled` carries a signal (dirty, a pull
/// request state); a hollow chip is a neutral known value. An unknown value
/// has no chip at all.
struct ProjectHomeChip: Equatable {
    enum Kind: Equatable {
        case changed
        case aheadBehind
        case pullRequest
    }

    let kind: Kind
    let text: String
    let color: Color
    let filled: Bool

    /// The room the chip claims in layout points, from the same glyph
    /// estimate the labels use.
    var width: CGFloat {
        CGFloat(text.count) * HideTheme.Home.chipGlyphWidth + HideTheme.Home.chipPadding * 2
    }
}

/// The hover card's lines: identity, status word, the core's second line,
/// checkout and elapsed (PRD rule 5).
struct ProjectHomeCard: Equatable {
    let title: String
    let statusLabel: String
    let statusColor: Color
    let detail: String?
    let checkoutLabel: String
    let elapsed: String
    let agentKind: String
    let group: AgentGroup
    let stallNotice: String?
}

struct ProjectHomeEdge: Identifiable, Equatable {
    enum Kind: Equatable {
        /// Checkout to project.
        case membership
        /// Agent to its checkout.
        case agent
        /// Child to the parent that spawned it: dashed.
        case delegation
    }

    let from: String
    let to: String
    let kind: Kind
    let color: Color

    var id: String { "\(from)>\(to)" }
    var dashed: Bool { kind == .delegation }
}

/// The header's glance counts in the pet badge colours; Seen has no badge
/// colour of its own (status-model.md, Badges).
struct ProjectHomeCounts: Equatable {
    let needsYou: Int
    let done: Int
    let working: Int
    let seen: Int

    static let zero = ProjectHomeCounts(needsYou: 0, done: 0, working: 0, seen: 0)

    /// Each group with the mark the agent rows draw for it, so a compact
    /// header can show the mark in place of the word.
    var entries: [(label: String, symbol: String, count: Int, color: Color)] {
        [
            ("Needs You", "?", needsYou, HideTheme.warning),
            ("Done", "✓", done, HideTheme.success),
            ("Working", "●", working, HideTheme.agentWorking),
            ("Seen", "○", seen, HideTheme.secondary),
        ]
    }
}

/// One row of the attention rail: a Needs You or Done agent, in the core's
/// order (PRD rule 6).
struct ProjectHomeRailRow: Identifiable, Equatable {
    let nodeID: String
    let paneID: String
    let title: String
    let symbol: String
    let statusLabel: String
    let statusColor: Color
    let detail: String?
    let checkoutLabel: String
    let elapsed: String
    let agentKind: String
    let group: AgentGroup

    var id: String { nodeID }
}

/// Which of the page's shapes the data can produce (PRD rule 7).
enum ProjectHomeShape: Equatable {
    /// No snapshot yet, or no focused project.
    case loading
    /// A project with no checkouts: the project node and the empty sentence.
    case noCheckouts
    /// Checkouts and nobody working in them.
    case noAgents
    case populated
}

struct ProjectHomeModel: Equatable {
    let shape: ProjectHomeShape
    let projectName: String
    let nodes: [ProjectHomeNode]
    let edges: [ProjectHomeEdge]
    let topology: ProjectHomeGraphLayout.Topology
    let counts: ProjectHomeCounts
    let rail: [ProjectHomeRailRow]
    /// The sentence under the map when the map alone would not say why it
    /// is sparse; `nil` for a populated map.
    let emptyMessage: String?
    /// Herdr stopped answering: the last projection is kept and dimmed, and
    /// the stale notice is drawn (PRD rule 7).
    let stale: Bool

    static let projectNodeID = "project"
    static func checkoutNodeID(_ checkoutID: String) -> String { "checkout:\(checkoutID)" }
    static func agentNodeID(_ paneID: String) -> String { "agent:\(paneID)" }

    static let loading = ProjectHomeModel(
        shape: .loading, projectName: "", nodes: [], edges: [],
        topology: ProjectHomeGraphLayout.Topology(nodes: [], edges: []),
        counts: .zero, rail: [], emptyMessage: nil, stale: false
    )

    func node(_ id: String) -> ProjectHomeNode? { nodes.first { $0.id == id } }
    func node(paneID: String) -> ProjectHomeNode? { nodes.first { $0.paneID == paneID && $0.kind == .agent } }
}

/// Builds the constellation from the focused project's checkouts and the
/// canonical agents.
///
/// Lineage edges come only from the core's `lineageChildPaneIDs`, the same
/// rule the Overview task forest follows: a child whose parent is not on the
/// map is drawn as a root on its own checkout, never guessed onto another.
enum ProjectHomePresentation {
    static func build(
        workspace: CoreWorkspaceSnapshot?,
        agents: [SidebarAgent],
        connected: Bool,
        now: Date = Date()
    ) -> ProjectHomeModel {
        guard let workspace else { return .loading }
        let checkouts = orderedCheckouts(workspace)
        // A pane belongs to the first checkout that lists it and an agent to
        // its first row: a checkout is keyed by path and can hold tabs from
        // several Herdr workspaces, so a repeat is a projection to draw
        // once, not a reason to trap.
        let checkoutByPane = Dictionary(
            checkouts.flatMap { checkout in checkout.tabs.flatMap(\.panes).map { ($0.id, checkout) } },
            uniquingKeysWith: { first, _ in first }
        )
        let projectAgents = agents.filter { checkoutByPane[$0.paneID] != nil }
        let agentByPane = Dictionary(projectAgents.map { ($0.paneID, $0) }, uniquingKeysWith: { first, _ in first })
        // A child hangs off its parent only when the parent is on this map.
        let parentByChild: [String: String] = Dictionary(
            projectAgents.flatMap { parent in
                parent.lineageChildPaneIDs.filter { agentByPane[$0] != nil }.map { ($0, parent.paneID) }
            },
            uniquingKeysWith: { first, _ in first }
        )

        var nodes: [ProjectHomeNode] = []
        var edges: [ProjectHomeEdge] = []
        var layoutNodes: [ProjectHomeGraphLayout.Node] = []
        var layoutEdges: [ProjectHomeGraphLayout.Edge] = []

        let projectLayout = ProjectHomeGraphLayout.Node(
            id: ProjectHomeModel.projectNodeID, kind: .project, parentID: nil,
            radius: HideTheme.Home.projectNodeRadius,
            labelWidth: labelWidth(workspace.repoName), rank: 0, siblingCount: 1
        )
        layoutNodes.append(projectLayout)
        nodes.append(ProjectHomeNode(
            id: ProjectHomeModel.projectNodeID, kind: .project,
            label: truncated(workspace.repoName), fullLabel: workspace.repoName, chips: [],
            symbol: nil, color: HideTheme.accent, radius: HideTheme.Home.projectNodeRadius,
            delegated: false, stallLevel: "", missing: false, paneID: nil, checkoutID: nil,
            pullRequest: nil, opensOnActivate: false,
            accessibilityLabel: "Project \(workspace.repoName)", card: nil, layout: projectLayout
        ))

        for (rank, checkout) in checkouts.enumerated() {
            let checkoutAgents = projectAgents
                .filter { checkoutByPane[$0.paneID]?.id == checkout.id }
                .sorted(by: agentOrder)
            let roots = checkoutAgents.filter { parentByChild[$0.paneID] == nil }
            let nodeID = ProjectHomeModel.checkoutNodeID(checkout.id)
            let checkoutRadius = min(
                HideTheme.Home.checkoutNodeRadiusMax,
                HideTheme.Home.checkoutNodeRadius + CGFloat(checkoutAgents.count) * HideTheme.Home.checkoutNodeRadiusStep
            )
            let chips = chips(for: checkout)
            let layout = ProjectHomeGraphLayout.Node(
                id: nodeID, kind: .checkout, parentID: ProjectHomeModel.projectNodeID,
                radius: checkoutRadius, labelWidth: labelWidth(checkout.label),
                chipWidths: chips.map(\.width), rank: rank, siblingCount: checkouts.count
            )
            layoutNodes.append(layout)
            layoutEdges.append(ProjectHomeGraphLayout.Edge(from: ProjectHomeModel.projectNodeID, to: nodeID))
            edges.append(ProjectHomeEdge(from: ProjectHomeModel.projectNodeID, to: nodeID, kind: .membership, color: HideTheme.divider))
            let missing = !checkout.exists
            nodes.append(ProjectHomeNode(
                id: nodeID, kind: .checkout,
                label: truncated(checkout.label), fullLabel: checkout.label,
                chips: chips, symbol: nil,
                color: missing ? HideTheme.muted : HideTheme.secondary, radius: checkoutRadius,
                delegated: false, stallLevel: "", missing: missing,
                paneID: nil, checkoutID: checkout.id, pullRequest: checkout.pullRequest,
                opensOnActivate: checkout.exists,
                accessibilityLabel: CheckoutCardPresentation.rowAccessibilityLabel(
                    repoName: workspace.repoName, checkout: checkout, agentCount: checkoutAgents.count
                ),
                card: nil, layout: layout
            ))

            // Roots orbit the checkout; each root's subtree orbits it in turn.
            var stack: [(agent: SidebarAgent, parentNodeID: String, rank: Int, siblings: Int, depth: Int)] =
                roots.enumerated().reversed().map { ($0.element, nodeID, $0.offset, roots.count, 0) }
            var visited: Set<String> = []
            while let entry = stack.popLast() {
                guard visited.insert(entry.agent.paneID).inserted else { continue }
                let agent = entry.agent
                let agentNodeID = ProjectHomeModel.agentNodeID(agent.paneID)
                let status = AgentStatusPresentation(agent: agent, connected: connected)
                let recencyRadius = radius(for: agent, now: now)
                let nodeRadius = agent.delegated ? recencyRadius * HideTheme.Home.childNodeScale : recencyRadius
                let layoutKind: ProjectHomeGraphLayout.NodeKind = entry.depth == 0 ? .agent : .child
                let layout = ProjectHomeGraphLayout.Node(
                    id: agentNodeID, kind: layoutKind, parentID: entry.parentNodeID,
                    radius: nodeRadius, labelWidth: labelWidth(agent.identityLabel),
                    rank: entry.rank, siblingCount: entry.siblings
                )
                layoutNodes.append(layout)
                layoutEdges.append(ProjectHomeGraphLayout.Edge(from: entry.parentNodeID, to: agentNodeID))
                edges.append(ProjectHomeEdge(
                    from: entry.parentNodeID, to: agentNodeID,
                    kind: entry.depth == 0 ? .agent : .delegation,
                    color: entry.depth == 0 ? HideTheme.divider : status.color
                ))
                let card = ProjectHomeCard(
                    title: agent.identityLabel, statusLabel: status.label, statusColor: status.color,
                    detail: agent.detail, checkoutLabel: checkout.label, elapsed: agent.elapsed,
                    agentKind: agent.agentKind, group: AgentGroup(agent: agent), stallNotice: agent.stallNotice
                )
                nodes.append(ProjectHomeNode(
                    id: agentNodeID, kind: .agent,
                    label: truncated(agent.identityLabel), fullLabel: agent.identityLabel,
                    chips: [], symbol: status.symbol, color: status.color, radius: nodeRadius,
                    delegated: agent.delegated, stallLevel: agent.stallLevel, missing: false,
                    paneID: agent.paneID, checkoutID: checkout.id, pullRequest: nil, opensOnActivate: true,
                    accessibilityLabel: [agent.identityLabel, agent.agentKind, status.label, agent.detail]
                        .compactMap { $0 }.joined(separator: ", "),
                    card: card, layout: layout
                ))
                let children = agent.lineageChildPaneIDs.compactMap { agentByPane[$0] }
                    .filter { parentByChild[$0.paneID] == agent.paneID }
                    .sorted(by: agentOrder)
                stack.append(contentsOf: children.enumerated().reversed().map {
                    ($0.element, agentNodeID, $0.offset, children.count, entry.depth + 1)
                })
            }
        }

        let counts = ProjectHomeCounts(
            needsYou: projectAgents.filter { AgentGroup(agent: $0) == .needsYou }.count,
            done: projectAgents.filter { AgentGroup(agent: $0) == .done }.count,
            working: projectAgents.filter { AgentGroup(agent: $0) == .working }.count,
            seen: projectAgents.filter { AgentGroup(agent: $0) == .seen }.count
        )
        let rail: [ProjectHomeRailRow] = projectAgents
            .filter { SidebarGrouping.raisedGroups.contains(AgentGroup(agent: $0)) }
            .map { agent in
                let status = AgentStatusPresentation(agent: agent, connected: connected)
                return ProjectHomeRailRow(
                    nodeID: ProjectHomeModel.agentNodeID(agent.paneID), paneID: agent.paneID,
                    title: agent.identityLabel, symbol: status.symbol, statusLabel: status.label,
                    statusColor: status.color, detail: agent.detail,
                    checkoutLabel: checkoutByPane[agent.paneID]?.label ?? agent.workspaceLabel,
                    elapsed: agent.elapsed, agentKind: agent.agentKind, group: AgentGroup(agent: agent)
                )
            }

        let shape: ProjectHomeShape
        let emptyMessage: String?
        if checkouts.isEmpty {
            shape = .noCheckouts
            emptyMessage = "No checkouts yet. Add a worktree or start a terminal here to fill the map."
        } else if projectAgents.isEmpty {
            shape = .noAgents
            emptyMessage = "Nobody is working in this project. Start a terminal to put an agent on the map."
        } else {
            shape = .populated
            emptyMessage = nil
        }
        return ProjectHomeModel(
            shape: shape, projectName: workspace.repoName, nodes: nodes, edges: edges,
            topology: ProjectHomeGraphLayout.Topology(nodes: layoutNodes, edges: layoutEdges),
            counts: counts, rail: rail, emptyMessage: emptyMessage, stale: !connected
        )
    }

    /// The default selection on entry: the focused pane's agent when it is on
    /// the map, else the focused checkout, else nothing (whole project).
    static func initialFocus(_ model: ProjectHomeModel, focusedPaneID: String?, focusedCheckoutID: String?) -> String? {
        if let focusedPaneID, let node = model.node(paneID: focusedPaneID) { return node.id }
        if let focusedCheckoutID, model.node(ProjectHomeModel.checkoutNodeID(focusedCheckoutID)) != nil {
            return ProjectHomeModel.checkoutNodeID(focusedCheckoutID)
        }
        return nil
    }

    /// The ids drawn at full strength: the hovered node's own neighbours
    /// while the pointer rests on one, else the selection's depth-2
    /// neighbourhood once the operator has asked for the local graph by
    /// clicking (PRD rules 2 and 5). `nil` means everything; the selection
    /// ring alone never dims the map.
    static func emphasized(_ model: ProjectHomeModel, focus: String?, isolated: Bool, hover: String?) -> Set<String>? {
        let edges = model.topology.edges
        if let hover, model.node(hover) != nil {
            return ProjectHomeGraphLayout.neighborhood(of: hover, depth: 1, edges: edges)
        }
        guard isolated, let focus, model.node(focus) != nil else { return nil }
        return ProjectHomeGraphLayout.neighborhood(of: focus, depth: 2, edges: edges)
    }

    /// Whether a node's label is drawn at this scale. Zoomed out past
    /// `labelThresholdScale` the map keeps only what a glance needs: the
    /// project, the hubs, and the agents that are waiting or finished; a
    /// Working or Seen agent's label returns while it is hovered or
    /// selected, and everywhere once zoomed back in.
    static func drawsLabel(_ node: ProjectHomeNode, scale: CGFloat, hovered: Bool, selected: Bool) -> Bool {
        if scale >= HideTheme.Home.labelThresholdScale || hovered || selected { return true }
        switch node.kind {
        case .project, .checkout: return true
        case .agent: return node.group == .needsYou || node.group == .done
        }
    }

    // MARK: Ordering and sizing

    /// Primary checkout first, then by branch name, then by id so two
    /// checkouts on one branch keep one order.
    static func orderedCheckouts(_ workspace: CoreWorkspaceSnapshot) -> [CoreCheckoutSnapshot] {
        workspace.checkouts.sorted { left, right in
            if left.isWorktree != right.isWorktree { return !left.isWorktree }
            let byLabel = left.label.localizedStandardCompare(right.label)
            if byLabel != .orderedSame { return byLabel == .orderedAscending }
            return left.id < right.id
        }
    }

    private static func agentOrder(_ left: SidebarAgent, _ right: SidebarAgent) -> Bool {
        let byLabel = left.identityLabel.localizedStandardCompare(right.identityLabel)
        if byLabel != .orderedSame { return byLabel == .orderedAscending }
        return left.paneID < right.paneID
    }

    /// Three sizes by how recently the agent moved: within the hour, within
    /// the day, older. An activity the core could only express as a Herdr
    /// sequence has no wall time and takes the middle size.
    static func radius(for agent: SidebarAgent, now: Date) -> CGFloat {
        let radii = HideTheme.Home.agentNodeRadii
        guard agent.lastActivity.count == 13, let millis = Double(agent.lastActivity) else { return radii[1] }
        let age = now.timeIntervalSince1970 - millis / 1000
        if age < 3600 { return radii[2] }
        if age < 86_400 { return radii[1] }
        return radii[0]
    }

    /// The checkout's derived state as chips, in the track order: changed
    /// files (warning while dirty), ahead and behind its base, the pull
    /// request in its state colour. A value the core does not know is
    /// omitted, never shown as zero: ahead and behind need a base branch, and
    /// a missing worktree has no working tree to count.
    static func chips(for checkout: CoreCheckoutSnapshot) -> [ProjectHomeChip] {
        var chips: [ProjectHomeChip] = []
        if checkout.exists {
            if checkout.changedFileCount > 0 {
                chips.append(ProjectHomeChip(kind: .changed, text: "\(checkout.changedFileCount) changed", color: HideTheme.warning, filled: checkout.dirty))
            } else if checkout.dirty {
                chips.append(ProjectHomeChip(kind: .changed, text: "dirty", color: HideTheme.warning, filled: true))
            }
        }
        if checkout.baseBranch != nil, checkout.ahead > 0 || checkout.behind > 0 {
            var parts: [String] = []
            if checkout.ahead > 0 { parts.append("↑\(checkout.ahead)") }
            if checkout.behind > 0 { parts.append("↓\(checkout.behind)") }
            chips.append(ProjectHomeChip(kind: .aheadBehind, text: parts.joined(separator: " "), color: HideTheme.secondary, filled: false))
        }
        if let pullRequest = checkout.pullRequest {
            chips.append(ProjectHomeChip(
                kind: .pullRequest, text: "PR #\(pullRequest.number)",
                color: CheckoutCardPresentation.pullRequestColor(pullRequest), filled: true
            ))
        }
        return chips
    }

    // MARK: Labels

    /// How many label characters fit the token width, counting a CJK glyph
    /// as the wide estimate. The cut is by estimate rather than measurement
    /// so it can run without a font; the canvas clamps the drawn text to the
    /// same width, and the full text stays in the card and accessibility.
    static func truncated(_ text: String) -> String {
        var width: CGFloat = 0
        var kept = ""
        for character in text {
            let glyph = isWide(character) ? HideTheme.Home.labelWideGlyphWidth : HideTheme.Home.labelGlyphWidth
            if width + glyph > HideTheme.Home.labelMaxWidth - HideTheme.Home.labelGlyphWidth * 2 {
                return kept + "…"
            }
            width += glyph
            kept.append(character)
        }
        return kept
    }

    static func labelWidth(_ text: String) -> CGFloat {
        let estimate = text.reduce(CGFloat(0)) { width, character in
            width + (isWide(character) ? HideTheme.Home.labelWideGlyphWidth : HideTheme.Home.labelGlyphWidth)
        }
        return min(HideTheme.Home.labelMaxWidth, estimate)
    }

    private static func isWide(_ character: Character) -> Bool {
        character.unicodeScalars.contains { scalar in
            // Hangul, CJK ideographs, kana, full-width forms.
            (0xAC00...0xD7AF).contains(scalar.value) || (0x4E00...0x9FFF).contains(scalar.value)
                || (0x3040...0x30FF).contains(scalar.value) || (0xFF00...0xFFEF).contains(scalar.value)
                || (0x1100...0x11FF).contains(scalar.value) || (0x3130...0x318F).contains(scalar.value)
        }
    }
}

/// Where Project Home is drawn (shared brief, entry points).
///
/// The empty checkout state hands `.idle` to Home; starting, started and
/// failed keep their own sentences because each is about the terminal that
/// is or is not coming. The overlay is the session-local toggle, and it
/// never draws over a remote context, whose empty state is untouched.
enum ProjectHomeEntryPolicy {
    static func drawsHome(startState: CheckoutStartState, hasCheckout: Bool, projectionNotice: String?) -> Bool {
        guard hasCheckout, projectionNotice == nil else { return false }
        if case .idle = startState { return true }
        return false
    }

    static func drawsOverlay(visible: Bool, remote: Bool, hasCheckout: Bool) -> Bool {
        visible && !remote && hasCheckout
    }
}
