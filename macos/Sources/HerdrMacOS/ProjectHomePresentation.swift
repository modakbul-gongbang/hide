import AppKit
import Foundation
import SwiftUI

/// Everything the mission board reads, gathered once from the snapshot.
///
/// The view assembles it from `ShellModel`; a test builds it directly with
/// the memberwise inits the snapshot types already carry. Nothing in it is a
/// decision: every label, colour, order and state the board draws is made in
/// `ProjectHomeBoard`, so the same fixture always yields the same board.
struct ProjectHomeInput {
    let projectLabel: String
    let projectPath: String
    let isGit: Bool
    let checkouts: [CoreCheckoutSnapshot]
    /// The core's canonical agents in the core's order: group first, then
    /// recency. The board never reorders them; it only places them.
    let agents: [SidebarAgent]
    let connected: Bool
    /// The Overview worktree facts the checkout snapshot does not carry,
    /// keyed by checkout path. A path with no entry has no Git worktree
    /// context yet, which the track says rather than fills.
    let worktreeFacts: [String: ProjectHomeWorktreeFact]

    init(
        projectLabel: String,
        projectPath: String,
        isGit: Bool = true,
        checkouts: [CoreCheckoutSnapshot],
        agents: [SidebarAgent],
        connected: Bool = true,
        worktreeFacts: [String: ProjectHomeWorktreeFact] = [:]
    ) {
        self.projectLabel = projectLabel
        self.projectPath = projectPath
        self.isGit = isGit
        self.checkouts = checkouts
        self.agents = agents
        self.connected = connected
        self.worktreeFacts = worktreeFacts
    }
}

/// The two facts the board takes from `CoreGitWorktree`, lifted out so a test
/// can state them without decoding the whole worktree record.
struct ProjectHomeWorktreeFact: Equatable {
    /// Whether the branch is merged into the project's base by ancestry;
    /// `nil` when the reader could not say.
    let merged: Bool?
    let agentLine: CoreWorktreeAgentLine

    init(merged: Bool? = nil, agentLine: CoreWorktreeAgentLine = CoreWorktreeAgentLine()) {
        self.merged = merged
        self.agentLine = agentLine
    }
}

/// The fixed stages of a lane's progress track, in the order they are drawn.
enum ProjectHomeStageKind: String, CaseIterable, Identifiable {
    case changes
    case commits
    case pullRequest
    case ci
    case merged

    var id: String { rawValue }

    var title: String {
        switch self {
        case .changes: "Changes"
        case .commits: "Commits"
        case .pullRequest: "PR"
        case .ci: "CI"
        case .merged: "Merged"
        }
    }
}

/// One segment of the track. A stage the data cannot fill is hollow and says
/// why in its tooltip; it is never drawn as if it had a value (PRD rule 2).
struct ProjectHomeTrackStage: Equatable, Identifiable {
    enum State: Equatable {
        case filled
        case hollow(reason: String)
    }

    let kind: ProjectHomeStageKind
    /// The short text drawn inside the segment.
    let label: String
    let state: State
    let color: Color
    let tooltip: String

    var id: ProjectHomeStageKind { kind }
    var isHollow: Bool {
        if case .hollow = state { return true }
        return false
    }
}

/// One agent on the board. The card carries the core's own identity, second
/// line and status; the board decides only where it sits and what lineage it
/// is drawn with.
struct ProjectHomeCard: Equatable, Identifiable {
    let agent: SidebarAgent
    let status: AgentStatusPresentation
    let laneID: String
    /// Nesting inside the lane: a child drawn under its parent is one deeper.
    let depth: Int
    /// The parent card in the same lane, when the child is nested under it.
    let parentPaneID: String?
    /// The children nested under this card, in the core's order.
    let childPaneIDs: [String]
    /// The parent when it lives in another lane: the child was moved to its
    /// own checkout and is a root here, captioned with whose work it is.
    let foreignParentPaneID: String?
    let foreignParentLabel: String?
    /// Children that live in other lanes, in the core's order, with the
    /// label of the lane each one sits in; the fan-out a lane cannot nest.
    let foreignChildPaneIDs: [String]
    let foreignChildLaneLabels: [String]

    /// How many lanes the `↳ to` caption names before it counts the rest.
    static let namedForeignLanes = 2

    var id: String { agent.paneID }
    var stallNotice: String? { agent.stallNotice }
    /// `↳ from <parent>`, drawn under a root whose parent is elsewhere.
    var fromParentCaption: String? { foreignParentLabel.map { "↳ from \($0)" } }
    /// `↳ to <branch>, <branch> +N`, drawn under a parent whose children
    /// were moved to other lanes, so the fan-out is readable without hover.
    var toLanesCaption: String? {
        var lanes: [String] = []
        for label in foreignChildLaneLabels where !lanes.contains(label) { lanes.append(label) }
        guard !lanes.isEmpty else { return nil }
        let named = lanes.prefix(Self.namedForeignLanes).joined(separator: ", ")
        let rest = lanes.count - Self.namedForeignLanes
        return rest > 0 ? "↳ to \(named) +\(rest)" : "↳ to \(named)"
    }
}

/// One checkout, drawn as a full-width band.
struct ProjectHomeLane: Equatable, Identifiable {
    let id: String
    let label: String
    let path: String
    let kindIcon: String
    let isPrimary: Bool
    let isMissing: Bool
    let isDetached: Bool
    /// Whether clicking the lane can open the checkout. A missing worktree
    /// has nowhere to open.
    let canOpen: Bool
    let track: [ProjectHomeTrackStage]
    let cards: [ProjectHomeCard]
    /// Hide cannot see into this checkout's sessions; the mark's sentence
    /// and accessible name (docs/status-model.md, uninstrumented).
    let uninstrumentedReason: String?
    let uninstrumentedLabel: String?
    let pullRequest: CorePullRequest?
    /// The Overview's attention rank, kept so a test can read why the lane
    /// sits where it does.
    let rank: Int
    /// The sidebar Workspace row's agent summary, the same value the row
    /// draws its chip and tooltip from, so the header says what is inside
    /// the lane even when its cards are scrolled away.
    let summary: SidebarCheckoutPresentation

    private var byID: [String: ProjectHomeCard] {
        Dictionary(uniqueKeysWithValues: cards.map { ($0.id, $0) })
    }

    func ancestors(of card: ProjectHomeCard) -> [String] {
        var result: [String] = []
        var cursor = card.parentPaneID
        let cards = byID
        while let paneID = cursor, let parent = cards[paneID] {
            result.append(paneID)
            cursor = parent.parentPaneID
        }
        return result
    }

    func descendants(of card: ProjectHomeCard) -> [String] {
        var result: [String] = []
        var stack = card.childPaneIDs
        let cards = byID
        while let paneID = stack.popLast() {
            guard let child = cards[paneID] else { continue }
            result.append(paneID)
            stack.append(contentsOf: child.childPaneIDs)
        }
        return result
    }

    /// The lineage a hover raises: the card, its ancestors and its
    /// descendants in this lane. Everything else in the lane is dimmed.
    func lineage(of paneID: String) -> Set<String> {
        guard let card = byID[paneID] else { return [] }
        return Set([paneID] + ancestors(of: card) + descendants(of: card))
    }
}

/// The header's four counts, in the pet badge order and colours.
struct ProjectHomeCounts: Equatable {
    let needsYou: Int
    let done: Int
    let working: Int
    let seen: Int

    var total: Int { needsYou + done + working + seen }
}

/// The lineage chain the inspector prints, root first.
struct ProjectHomeLineageStep: Equatable, Identifiable {
    let paneID: String
    let label: String
    let laneLabel: String

    var id: String { paneID }
}

/// The whole page as a value: lanes in attention order, the attention strip,
/// the counts, and every state the page can be in. A pure function of its
/// input (PRD rule 8), so the view memoizes it by the snapshot revision.
struct ProjectHomeBoard: Equatable {
    let projectLabel: String
    let counts: ProjectHomeCounts
    let lanes: [ProjectHomeLane]
    /// The Needs You and Done cards across every lane, in the core's order.
    let attention: [ProjectHomeCard]
    let connected: Bool

    /// The sentence the one empty lane carries when the project has no
    /// checkout at all.
    static let noCheckoutsSentence = "No checkouts in this project yet."
    /// The strip's one muted line when nothing is waiting on the operator.
    static let nothingNeedsYouSentence = "Nothing needs you right now."
    /// What a lane says beside its track when nobody is working in it.
    static let noAgentsSentence = "No agents"
    /// The notice over a dimmed board while the server is away. The counts
    /// and cards keep their last reading; the mark says it is not current.
    static let disconnectedNotice = "Live agent status unavailable; showing the last known board"

    var isEmpty: Bool { lanes.isEmpty }

    static func build(_ input: ProjectHomeInput) -> ProjectHomeBoard {
        var checkoutByPane: [String: String] = [:]
        for checkout in input.checkouts {
            for tab in checkout.tabs {
                for pane in tab.panes where checkoutByPane[pane.id] == nil {
                    checkoutByPane[pane.id] = checkout.id
                }
            }
        }
        let projectAgents = input.agents.filter { checkoutByPane[$0.paneID] != nil }
        var order: [String: Int] = [:]
        var byPane: [String: SidebarAgent] = [:]
        for (index, agent) in projectAgents.enumerated() where byPane[agent.paneID] == nil {
            order[agent.paneID] = index
            byPane[agent.paneID] = agent
        }
        // Lineage is the parent's claim, never the child's depth: only an
        // authoritative child id makes an edge (docs/status-model.md).
        var parentOf: [String: String] = [:]
        for agent in projectAgents {
            for child in agent.lineageChildPaneIDs where byPane[child] != nil && parentOf[child] == nil {
                parentOf[child] = agent.paneID
            }
        }

        var laneLabels: [String: String] = [:]
        for checkout in input.checkouts { laneLabels[checkout.id] = checkout.label }
        var lanes: [ProjectHomeLane] = []
        for checkout in input.checkouts {
            let laneAgents: [SidebarAgent] = projectAgents.filter { checkoutByPane[$0.paneID] == checkout.id }
            lanes.append(lane(
                for: checkout,
                input: input,
                agents: laneAgents,
                laneOf: checkoutByPane,
                laneLabels: laneLabels,
                parentOf: parentOf,
                byPane: byPane,
                order: order
            ))
        }
        lanes.sort { left, right in
            left.rank == right.rank ? left.path < right.path : left.rank < right.rank
        }

        var cardsByPane: [String: ProjectHomeCard] = [:]
        for lane in lanes {
            for card in lane.cards { cardsByPane[card.id] = card }
        }
        let attention = projectAgents
            .filter { $0.group == "needs_you" || $0.group == "done" }
            .compactMap { cardsByPane[$0.paneID] }

        return ProjectHomeBoard(
            projectLabel: input.projectLabel,
            counts: ProjectHomeCounts(
                needsYou: projectAgents.filter { $0.group == "needs_you" }.count,
                done: projectAgents.filter { $0.group == "done" }.count,
                working: projectAgents.filter { $0.group == "working" }.count,
                seen: projectAgents.filter { $0.group == "seen" }.count
            ),
            lanes: lanes,
            attention: attention,
            connected: input.connected
        )
    }

    private static func lane(
        for checkout: CoreCheckoutSnapshot,
        input: ProjectHomeInput,
        agents: [SidebarAgent],
        laneOf: [String: String],
        laneLabels: [String: String],
        parentOf: [String: String],
        byPane: [String: SidebarAgent],
        order: [String: Int]
    ) -> ProjectHomeLane {
        let fact = input.worktreeFacts[checkout.path]
        let isPrimary = !checkout.isWorktree && checkout.path == input.projectPath
        let isDetached = checkout.worktree.map { $0.branch == nil } ?? false
        let line = fact?.agentLine
        return ProjectHomeLane(
            id: checkout.id,
            label: checkout.label,
            path: checkout.path,
            kindIcon: input.isGit ? "arrow.triangle.branch" : "folder",
            isPrimary: isPrimary,
            isMissing: !checkout.exists,
            isDetached: isDetached,
            canOpen: checkout.exists,
            track: track(for: checkout, input: input, fact: fact),
            cards: cards(
                in: checkout, agents: agents, laneOf: laneOf, laneLabels: laneLabels, parentOf: parentOf,
                byPane: byPane, order: order, connected: input.connected
            ),
            uninstrumentedReason: line?.uninstrumentedReason,
            uninstrumentedLabel: line?.uninstrumentedLabel,
            pullRequest: checkout.pullRequest,
            rank: rank(checkout),
            summary: SidebarCheckoutPresentation(
                projectPath: input.projectPath, checkout: checkout, agents: input.agents, connected: input.connected
            )
        )
    }

    /// The Overview's checkout order: Needs You first, then Done, Working,
    /// has agents, none. The primary checkout is not pinned (PRD rule 1).
    private static func rank(_ checkout: CoreCheckoutSnapshot) -> Int {
        let summary = checkout.agentSummary
        if summary.needsYou > 0 { return 0 }
        if summary.done > 0 { return 1 }
        if summary.working > 0 { return 2 }
        return summary.total > 0 ? 3 : 4
    }

    /// Cards in the core's order, a child nested under its parent when both
    /// live in this lane. A child whose parent is elsewhere is a root here
    /// with a caption naming the parent; a cycle or a claim the lane cannot
    /// resolve leaves the card a root rather than dropping it.
    private static func cards(
        in checkout: CoreCheckoutSnapshot,
        agents: [SidebarAgent],
        laneOf: [String: String],
        laneLabels: [String: String],
        parentOf: [String: String],
        byPane: [String: SidebarAgent],
        order: [String: Int],
        connected: Bool
    ) -> [ProjectHomeCard] {
        // A child is foreign when the lane it sits in is not this one and the
        // claim is the one the forest honours (the first parent to claim it).
        func foreignChildren(of agent: SidebarAgent) -> [SidebarAgent] {
            agent.lineageChildPaneIDs
                .compactMap { byPane[$0] }
                .filter { parentOf[$0.paneID] == agent.paneID && laneOf[$0.paneID] != checkout.id }
                .sorted { (order[$0.paneID] ?? .max) < (order[$1.paneID] ?? .max) }
        }
        func localParent(of agent: SidebarAgent) -> String? {
            guard let parent = parentOf[agent.paneID], laneOf[parent] == checkout.id else { return nil }
            return parent
        }
        // The forest's rule: the core's depth-zero rows are roots even when
        // a claim points at them, which is what keeps a cycle drawn.
        let roots = agents.filter { $0.lineageDepth == 0 || localParent(of: $0) == nil }
        var result: [ProjectHomeCard] = []
        var visited = Set<String>()
        var stack: [(SidebarAgent, Int, String?)] = roots.reversed().map { ($0, 0, nil) }
        while let (agent, depth, parent) = stack.popLast() {
            guard visited.insert(agent.paneID).inserted else { continue }
            let children = agent.lineageChildPaneIDs
                .compactMap { byPane[$0] }
                .filter { laneOf[$0.paneID] == checkout.id && !visited.contains($0.paneID) }
                .sorted { (order[$0.paneID] ?? .max) < (order[$1.paneID] ?? .max) }
            let foreignParent = parent == nil
                ? parentOf[agent.paneID].flatMap { laneOf[$0] == checkout.id ? nil : byPane[$0] }
                : nil
            let foreign = foreignChildren(of: agent)
            result.append(ProjectHomeCard(
                agent: agent,
                status: AgentStatusPresentation(agent: agent, connected: connected),
                laneID: checkout.id,
                depth: depth,
                parentPaneID: parent,
                childPaneIDs: children.map(\.paneID),
                foreignParentPaneID: foreignParent?.paneID,
                foreignParentLabel: foreignParent?.identityLabel,
                foreignChildPaneIDs: foreign.map(\.paneID),
                foreignChildLaneLabels: foreign.compactMap { laneOf[$0.paneID].flatMap { laneLabels[$0] } }
            ))
            stack.append(contentsOf: children.reversed().map { ($0, depth + 1, agent.paneID) })
        }
        for agent in agents where !visited.contains(agent.paneID) {
            result.append(ProjectHomeCard(
                agent: agent,
                status: AgentStatusPresentation(agent: agent, connected: connected),
                laneID: checkout.id,
                depth: 0,
                parentPaneID: nil,
                childPaneIDs: [],
                foreignParentPaneID: nil,
                foreignParentLabel: nil,
                foreignChildPaneIDs: [],
                foreignChildLaneLabels: []
            ))
        }
        return result
    }

    /// The stages, each filled from the checkout's own facts or hollow with
    /// the reason it could not be. CI and Merged derive from the pull request,
    /// so without one the track stops at a single hollow `PR` chip that says
    /// why; twelve lanes of `PR · CI · Merged` outlines said nothing. The one
    /// exception is a branch the base already contains: that Merged is read
    /// from ancestry, not from GitHub, so it is still drawn filled.
    private static func track(
        for checkout: CoreCheckoutSnapshot,
        input: ProjectHomeInput,
        fact: ProjectHomeWorktreeFact?
    ) -> [ProjectHomeTrackStage] {
        func hollow(_ kind: ProjectHomeStageKind, _ reason: String) -> ProjectHomeTrackStage {
            ProjectHomeTrackStage(kind: kind, label: kind.title, state: .hollow(reason: reason),
                                  color: HideTheme.muted, tooltip: "\(kind.title): \(reason)")
        }
        let withoutPullRequest: [ProjectHomeStageKind] = [.changes, .commits, .pullRequest]
        guard input.isGit else {
            return withoutPullRequest.map { hollow($0, "Not a Git repository") }
        }
        guard checkout.exists else {
            return withoutPullRequest.map { hollow($0, "Worktree folder is missing") }
        }

        let changes: ProjectHomeTrackStage
        if checkout.dirty {
            let count = checkout.changedFileCount
            changes = ProjectHomeTrackStage(
                kind: .changes, label: "\(count) changed", state: .filled, color: HideTheme.warning,
                tooltip: count == 1 ? "1 uncommitted change" : "\(count) uncommitted changes"
            )
        } else {
            changes = ProjectHomeTrackStage(
                kind: .changes, label: "clean", state: .filled, color: HideTheme.secondary,
                tooltip: "No uncommitted changes"
            )
        }

        let commits: ProjectHomeTrackStage
        if let base = checkout.baseBranch {
            commits = ProjectHomeTrackStage(
                kind: .commits, label: "↑\(checkout.ahead) ↓\(checkout.behind)", state: .filled,
                color: checkout.ahead > 0 ? HideTheme.primary : HideTheme.secondary,
                tooltip: "\(checkout.ahead) ahead of \(base), \(checkout.behind) behind"
            )
        } else {
            commits = hollow(.commits, "No base branch to compare against")
        }

        let pullRequest: ProjectHomeTrackStage
        var ci: ProjectHomeTrackStage?
        if let request = checkout.pullRequest {
            let state = CheckoutCardPresentation.pullRequestState(request)
            let stale = CheckoutCardPresentation.staleNotice(checkout.github).map { " · last known \($0)" } ?? ""
            let title = request.title.flatMap { $0.isEmpty ? nil : $0 }.map { " · \($0)" } ?? ""
            pullRequest = ProjectHomeTrackStage(
                kind: .pullRequest, label: "#\(request.number) \(state)", state: .filled,
                color: CheckoutCardPresentation.pullRequestColor(request),
                tooltip: "PR #\(request.number)\(title) · \(state)\(stale)"
            )
            let checks = CheckoutCardPresentation.checksLabel(request.checks)
            switch request.checks {
            case .passing, .failed, .pending, .none?:
                ci = ProjectHomeTrackStage(
                    kind: .ci, label: checks, state: .filled,
                    color: CheckoutCardPresentation.checksColor(request.checks),
                    tooltip: "CI: \(checks)\(stale)"
                )
            case .unknown, nil:
                ci = hollow(.ci, "GitHub reported no check result for PR #\(request.number)")
            }
        } else if checkout.github.loading {
            pullRequest = hollow(.pullRequest, "Looking up the pull request…")
        } else if let reason = checkout.github.unavailableReason {
            pullRequest = hollow(.pullRequest, reason)
        } else if !checkout.github.available {
            pullRequest = hollow(.pullRequest, "GitHub has not answered yet")
        } else {
            pullRequest = hollow(.pullRequest, "No pull request for this branch")
        }

        var merged: ProjectHomeTrackStage?
        if checkout.pullRequest?.badge == .merged {
            merged = ProjectHomeTrackStage(
                kind: .merged, label: "merged", state: .filled, color: HideTheme.PullRequest.merged,
                tooltip: "PR #\(checkout.pullRequest!.number) is merged"
            )
        } else if fact?.merged == true {
            merged = ProjectHomeTrackStage(
                kind: .merged, label: "merged", state: .filled, color: HideTheme.PullRequest.merged,
                tooltip: "Merged into \(checkout.baseBranch ?? "the base branch") by ancestry"
            )
        } else if checkout.pullRequest != nil, fact?.merged == false {
            merged = hollow(.merged, "Not merged into \(checkout.baseBranch ?? "the base branch")")
        } else if checkout.pullRequest != nil {
            merged = hollow(.merged, "Merge state not read yet")
        }

        return [changes, commits, pullRequest] + [ci, merged].compactMap { $0 }
    }

    /// The board narrowed to a query: a lane whose own name matches keeps
    /// every card; otherwise it keeps the cards that match and the ancestors
    /// they hang from, and a lane left with nothing is dropped. An empty
    /// query is the whole board.
    func filtered(query: String) -> ProjectHomeBoard {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else { return self }
        func matches(_ card: ProjectHomeCard) -> Bool {
            [card.agent.identityLabel, card.agent.detail, card.agent.task, card.agent.statusLabel, card.agent.paneID]
                .compactMap { $0 }
                .contains { $0.localizedCaseInsensitiveContains(needle) }
        }
        let lanes = lanes.compactMap { lane -> ProjectHomeLane? in
            if lane.label.localizedCaseInsensitiveContains(needle) || lane.path.localizedCaseInsensitiveContains(needle) {
                return lane
            }
            var kept = Set(lane.cards.filter(matches).map(\.id))
            for card in lane.cards where kept.contains(card.id) {
                kept.formUnion(lane.ancestors(of: card))
            }
            guard !kept.isEmpty else { return nil }
            return ProjectHomeLane(
                id: lane.id, label: lane.label, path: lane.path, kindIcon: lane.kindIcon,
                isPrimary: lane.isPrimary, isMissing: lane.isMissing, isDetached: lane.isDetached,
                canOpen: lane.canOpen, track: lane.track,
                cards: lane.cards.filter { kept.contains($0.id) },
                uninstrumentedReason: lane.uninstrumentedReason,
                uninstrumentedLabel: lane.uninstrumentedLabel,
                pullRequest: lane.pullRequest, rank: lane.rank, summary: lane.summary
            )
        }
        let visible = Set(lanes.flatMap(\.cards).map(\.id))
        return ProjectHomeBoard(
            projectLabel: projectLabel,
            counts: counts,
            lanes: lanes,
            attention: attention.filter { visible.contains($0.id) },
            connected: connected
        )
    }

    /// The cards a hover or selection raises: the card, its ancestors and its
    /// descendants across every lane, so a parent's fan-out into other
    /// checkouts lights up with it (PRD rule 5, round 2).
    func family(of paneID: String) -> Set<String> {
        guard let start = card(paneID) else { return [] }
        var result: Set<String> = [paneID]
        var cursor = start.parentPaneID ?? start.foreignParentPaneID
        while let parent = cursor, result.insert(parent).inserted, let card = card(parent) {
            cursor = card.parentPaneID ?? card.foreignParentPaneID
        }
        var stack = start.childPaneIDs + start.foreignChildPaneIDs
        while let child = stack.popLast() {
            guard result.insert(child).inserted, let card = card(child) else { continue }
            stack.append(contentsOf: card.childPaneIDs + card.foreignChildPaneIDs)
        }
        return result
    }

    func lane(containing paneID: String) -> ProjectHomeLane? {
        lanes.first { lane in lane.cards.contains { $0.id == paneID } }
    }

    func card(_ paneID: String) -> ProjectHomeCard? {
        lanes.lazy.flatMap(\.cards).first { $0.id == paneID }
    }

    /// The chain the inspector prints for a card, root first and ending at
    /// the card itself, crossing lanes where a child was moved.
    func lineage(of paneID: String) -> [ProjectHomeLineageStep] {
        var chain: [ProjectHomeLineageStep] = []
        var cursor: String? = paneID
        var seen = Set<String>()
        var laneLabels: [String: String] = [:]
        for lane in lanes { laneLabels[lane.id] = lane.label }
        while let current = cursor, seen.insert(current).inserted, let card = card(current) {
            chain.append(ProjectHomeLineageStep(
                paneID: current, label: card.agent.identityLabel, laneLabel: laneLabels[card.laneID] ?? ""
            ))
            cursor = card.parentPaneID ?? card.foreignParentPaneID
        }
        return chain.reversed()
    }
}

/// The slots lanes keep while the page stays open. Attention rank decides
/// the order when Home opens and when the operator asks for a sort; between
/// those moments a lane keeps its place, a lane that entered Needs You gets
/// its mark and the strip surfaces it, and a new checkout appends at the
/// bottom, so the lane being read never moves under the pointer.
struct ProjectHomeLaneOrder: Equatable {
    /// The rank a lane has while an agent in it needs the operator.
    static let needsYouRank = 0

    let projectPath: String
    let laneIDs: [String]
    /// The lanes that were in Needs You when the order was last ranked. A
    /// lane in Needs You now and not here entered it while its slot was
    /// frozen, and its header says so until the next rank.
    let needsYouAtRank: Set<String>

    /// The order to draw `rankedLanes` in: rank order when there is no order
    /// yet or the project changed, otherwise the kept order with gone lanes
    /// dropped and new lanes appended in rank order.
    static func settle(
        _ previous: ProjectHomeLaneOrder?,
        projectPath: String,
        rankedLanes: [ProjectHomeLane]
    ) -> ProjectHomeLaneOrder {
        let ranked = rankedLanes.map(\.id)
        guard let previous, previous.projectPath == projectPath else {
            return ProjectHomeLaneOrder(
                projectPath: projectPath, laneIDs: ranked,
                needsYouAtRank: Set(rankedLanes.filter { $0.rank == needsYouRank }.map(\.id))
            )
        }
        let present = Set(ranked)
        var kept = previous.laneIDs.filter { present.contains($0) }
        let known = Set(kept)
        kept.append(contentsOf: ranked.filter { !known.contains($0) })
        return ProjectHomeLaneOrder(projectPath: projectPath, laneIDs: kept, needsYouAtRank: previous.needsYouAtRank)
    }

    /// The lanes that entered Needs You since the last rank and are still
    /// in it; a lane that left it again drops out on its own.
    func enteredNeedsYou(in lanes: [ProjectHomeLane]) -> Set<String> {
        Set(lanes.filter { $0.rank == Self.needsYouRank && !needsYouAtRank.contains($0.id) }.map(\.id))
    }

    /// `lanes` in this order; a lane the order does not know (a filter
    /// cannot add one, but a stale order could miss one) keeps rank order at
    /// the end.
    func apply(to lanes: [ProjectHomeLane]) -> [ProjectHomeLane] {
        var slot: [String: Int] = [:]
        for (index, id) in laneIDs.enumerated() { slot[id] = index }
        return lanes.enumerated().sorted { left, right in
            let l = slot[left.element.id] ?? laneIDs.count + left.offset
            let r = slot[right.element.id] ?? laneIDs.count + right.offset
            return l < r
        }.map(\.element)
    }

    /// Whether a sort would move anything: false when the kept order is
    /// already the rank order, so the control can say so.
    func isRanked(against rankedLanes: [ProjectHomeLane]) -> Bool {
        apply(to: rankedLanes).map(\.id) == rankedLanes.map(\.id)
    }
}

/// How the Needs You strip folds: two rows of compact cards, with the last
/// slot given to a `+N more` chip when there is more, and everything on
/// demand behind it (PRD rule 4, round 2).
enum ProjectHomeAttentionFold: Equatable {
    static let rows = 2
    static let fewerLabel = "Show fewer"

    static func moreLabel(hidden: Int) -> String { "+\(hidden) more" }

    /// Cards per row at `width`; never fewer than one.
    static func perRow(width: CGFloat, cardWidth: CGFloat, spacing: CGFloat) -> Int {
        max(1, Int(((width + spacing) / (cardWidth + spacing)).rounded(.down)))
    }

    /// How many cards are drawn. Everything when it fits in the rows or the
    /// operator expanded; otherwise the rows minus the chip's slot.
    static func visibleCount(total: Int, perRow: Int, expanded: Bool) -> Int {
        let capacity = rows * perRow
        guard total > capacity, !expanded else { return total }
        return max(0, capacity - 1)
    }
}

/// Which surface draws the board: the checkout's empty state, or the overlay
/// the operator raised over a checkout that has tabs.
enum ProjectHomeMode: Equatable {
    case emptyState
    case overlay
}

/// The overlay's own keyboard contract: Escape closes it, and only when no
/// sheet is above it to take the key first.
enum ProjectHomeShortcutPolicy {
    static let closeKeyCode: UInt16 = 53

    static func shouldClose(_ event: NSEvent, visible: Bool, sheetPresented: Bool) -> Bool {
        visible && !sheetPresented && event.type == .keyDown && event.keyCode == closeKeyCode
            && event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty
    }

    static func isToggle(_ event: NSEvent) -> Bool {
        event.type == .keyDown && ShellMenuCommand.projectHome.shortcut.matches(event)
    }
}
