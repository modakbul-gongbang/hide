import Foundation

/// Preserve unavailable values at the presentation boundary; zero is a measured value.
enum OverviewPresentation {
    static func githubLabel(status: CoreGithubStatus, requests: [CorePullRequest]?, isGit: Bool) -> String {
        guard isGit else { return "Not a Git repository" }
        if status.loading { return "Loading…" }
        if status.failureCategory == "authentication" || status.failureCategory == "not logged in" { return "Sign in required" }
        if status.unavailableReason != nil { return status.stale ? "Stale · details" : "Unavailable · details" }
        guard status.available, let requests else { return "Not loaded" }
        let open = requests.filter { $0.badge != .merged && $0.badge != .closed }
        if requests.isEmpty { return "No recent pull requests" }
        return "\(open.count) active branches · \(open.filter(\.isDraft).count) draft"
    }

    static func diskLabel(total: UInt64?, confirmed: UInt64?, failure: String?, isGit: Bool) -> String {
        guard isGit else { return "Unavailable" }
        if let total { return CheckoutCardPresentation.formattedBytes(Double(total)) }
        if let confirmed { return "Partial · " + CheckoutCardPresentation.formattedBytes(Double(confirmed)) }
        return failure == nil ? "Measuring…" : "Unavailable"
    }
}

struct ProjectTaskRow: Identifiable, Equatable {
    let agent: SidebarAgent
    let checkoutID: String
    let checkoutLabel: String
    let depth: Int
    let hasChildren: Bool

    var id: String { agent.paneID }
}

/// Builds one project-wide task forest from the core's canonical agents and
/// authoritative child ids. Missing parents and cycles stay visible as roots;
/// no label or pane proximity is used to invent lineage.
enum ProjectTaskForestPresentation {
    static func rows(
        agents: [SidebarAgent],
        checkouts: [CoreCheckoutSnapshot],
        query: String = ""
    ) -> [ProjectTaskRow] {
        let checkoutByPane = Dictionary(uniqueKeysWithValues: checkouts.flatMap { checkout in
            checkout.tabs.flatMap(\.panes).map { ($0.id, checkout) }
        })
        let projectAgents = agents.filter { checkoutByPane[$0.paneID] != nil }
        let byPane = Dictionary(uniqueKeysWithValues: projectAgents.map { ($0.paneID, $0) })
        let claimedChildren = Set(projectAgents.flatMap(\.lineageChildPaneIDs))
        var roots = projectAgents.filter {
            $0.lineageDepth == 0 || !claimedChildren.contains($0.paneID)
        }
        roots.sort(by: taskOrder)

        var flattened: [ProjectTaskRow] = []
        var visited = Set<String>()
        var stack = roots.reversed().map { ($0, 0) }
        while let (agent, depth) = stack.popLast() {
            guard visited.insert(agent.paneID).inserted,
                  let checkout = checkoutByPane[agent.paneID]
            else { continue }
            let children = agent.lineageChildPaneIDs.compactMap { byPane[$0] }.sorted(by: taskOrder)
            flattened.append(ProjectTaskRow(
                agent: agent,
                checkoutID: checkout.id,
                checkoutLabel: checkout.label,
                depth: depth,
                hasChildren: !children.isEmpty
            ))
            stack.append(contentsOf: children.reversed().map { ($0, depth + 1) })
        }
        for agent in projectAgents where !visited.contains(agent.paneID) {
            guard let checkout = checkoutByPane[agent.paneID] else { continue }
            flattened.append(ProjectTaskRow(
                agent: agent,
                checkoutID: checkout.id,
                checkoutLabel: checkout.label,
                depth: 0,
                hasChildren: !agent.lineageChildPaneIDs.isEmpty
            ))
        }

        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else { return flattened }
        let matching = Set(flattened.filter {
            [$0.agent.identityLabel, $0.agent.summary, $0.agent.id, $0.checkoutLabel, $0.agent.statusLabel]
                .contains { $0.localizedCaseInsensitiveContains(normalized) }
        }.map(\.id))
        var included = matching
        for row in flattened where matching.contains(row.id) {
            var remainingDepth = row.depth
            guard remainingDepth > 0,
                  let index = flattened.firstIndex(where: { $0.id == row.id })
            else { continue }
            for ancestor in flattened[..<index].reversed() where remainingDepth > 0 {
                if ancestor.depth == remainingDepth - 1 {
                    included.insert(ancestor.id)
                    remainingDepth -= 1
                }
            }
        }
        return flattened.filter { included.contains($0.id) }
    }

    private static func taskOrder(_ left: SidebarAgent, _ right: SidebarAgent) -> Bool {
        if left.lastActivity != right.lastActivity {
            return left.lastActivity > right.lastActivity
        }
        return left.paneID < right.paneID
    }
}
