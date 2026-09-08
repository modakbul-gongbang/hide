import Foundation

/// The four groups the core sorts every agent into, in the order the sidebar
/// reads them top to bottom.
///
/// Membership and order are decided once in the core projection. This names
/// the groups for the screen and nothing else, so the two sidebar views, the
/// pet dashboard, and the project tree cannot end up with different sets.
enum AgentGroup: String, CaseIterable {
    case needsYou = "needs_you"
    case done
    case working
    case seen

    /// The core writes each of these names from its own exhaustive enum, so a
    /// value this does not recognize can only mean a shell and a core that
    /// were not built together. Seen is where such a row lands, because Seen
    /// is already defined as everything the other three do not claim.
    init(agent: SidebarAgent) {
        self = AgentGroup(rawValue: agent.group) ?? .seen
    }

    /// The section heading above the group.
    var title: String {
        switch self {
        case .needsYou: "Needs You"
        case .done: "Done"
        case .working: "Working"
        case .seen: "Seen"
        }
    }
}

/// One group's rows, ready to draw.
struct AgentGroupSection: Identifiable, Equatable {
    var id: String { group.rawValue }
    let group: AgentGroup
    let agents: [SidebarAgent]
}

/// How the sidebar splits agents between the groups it raises to the top and
/// the space tree. Kept apart from `ShellModel` so both rules can be exercised
/// without a live core runtime.
enum SidebarGrouping {
    /// The two groups the Projects view lifts above the project tree.
    ///
    /// This is the one thing the space tree cannot answer: it says what is
    /// waiting and what finished across every space, while the tree says what
    /// is in one space. Raised agents remain in the project tree as well,
    /// so a populated Workspace always has rows to reveal.
    static let raisedGroups: [AgentGroup] = [.needsYou, .done]

    /// The rows of one group, in the order the core put them in.
    static func agents(_ agents: [SidebarAgent], in group: AgentGroup) -> [SidebarAgent] {
        agents.filter { AgentGroup(agent: $0) == group }
    }

    /// Every non-empty group in group order, for the Agents view's boundaries.
    static func sections(_ agents: [SidebarAgent]) -> [AgentGroupSection] {
        AgentGroup.allCases.compactMap { group in
            let rows = Self.agents(agents, in: group)
            return rows.isEmpty ? nil : AgentGroupSection(group: group, agents: rows)
        }
    }

    /// The rows the Projects view draws above the tree, Needs You then Done.
    static func raised(_ agents: [SidebarAgent]) -> [AgentGroupSection] {
        sections(agents).filter { raisedGroups.contains($0.group) }
    }

    /// The Scratch rows the tree below should draw.
    ///
    /// A Scratch agent that is waiting or finished is already a row at the
    /// top, so drawing it again under Scratch would show one agent twice. The
    /// project tree retains raised rows beneath its Workspace summary.
    static func scratchTabsBelowRaisedSections(
        tabs: [CoreScratchTabSnapshot],
        agents: [SidebarAgent]
    ) -> [CoreScratchTabSnapshot] {
        let raised = Set(raised(agents).flatMap(\.agents).map(\.paneID))
        return tabs.filter { tab in
            !tab.panes.contains { raised.contains($0.id) }
        }
    }

    /// The visible preorder is shared by rendering and direct-select numbering.
    /// Parentage and collapse are already decided by the core.
    static func tree(_ agents: [SidebarAgent], checkoutID: String, ownedPaneIDs: Set<String> = []) -> [SidebarAgent] {
        let byPane = Dictionary(uniqueKeysWithValues: agents.map { ($0.paneID, $0) })
        let ownedChildren = Set(agents.filter { ownedPaneIDs.contains($0.paneID) }.flatMap(\.lineageChildPaneIDs))
        let roots = agents.filter {
            ($0.lineageDepth == 0 && $0.lineageRootCheckoutID == checkoutID)
                || (ownedPaneIDs.contains($0.paneID) && $0.lineageRootCheckoutID != checkoutID
                    && !ownedChildren.contains($0.paneID))
        }
        var pending = Array(roots.map { ($0, $0.lineageDepth) }.reversed())
        var visible: [SidebarAgent] = []
        var visited = Set<String>()
        while let (source, rootDepth) = pending.popLast() {
            guard visited.insert(source.paneID).inserted else { continue }
            var row = source
            row.lineageDepth = max(0, row.lineageDepth - rootDepth)
            visible.append(row)
            if !row.lineageCollapsed {
                pending.append(contentsOf: row.lineageChildPaneIDs.reversed().compactMap { byPane[$0].map { ($0, rootDepth) } })
            }
        }
        return visible
    }

    /// The agents running in a checkout, found through the panes that checkout
    /// owns. Herdr reports the pane an agent runs in and the checkout already
    /// carries its panes, so no second grouping key is needed.
    static func agents(
        _ agents: [SidebarAgent],
        in checkout: CoreCheckoutSnapshot
    ) -> [SidebarAgent] {
        let paneIDs = Set(checkout.tabs.flatMap(\.panes).map(\.id))
        return agents.filter { paneIDs.contains($0.paneID) }
    }
}
