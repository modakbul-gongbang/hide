import AppKit
import Foundation

struct CoreIssueReference: Decodable, Equatable, Hashable {
    let repository: String
    let number: Int
    var token: String { "\(repository)#\(number)" }
}

struct CoreIssue: Decodable, Equatable, Identifiable {
    let reference: CoreIssueReference
    let title: String
    let url: String
    let state: String
    let projectStatus: String?
    let updatedAtUnixMS: Double?
    var id: String { reference.token }
    enum CodingKeys: String, CodingKey {
        case reference, title, url, state
        case projectStatus = "project_status"
        case updatedAtUnixMS = "updated_at_unix_ms"
    }
}

struct CoreIssueLink: Decodable, Equatable {
    let issue: CoreIssue
    let source: String
}

struct CoreProjectIssues: Decodable {
    var repository: String? = nil
    var issues: [CoreIssue] = []
    var overflow = false
}

enum ProjectHomeViewKind: String, CaseIterable { case agents = "Agents", tasks = "Tasks" }
enum ProjectHomeStage: String, CaseIterable {
    case ready = "준비", working = "작업 중", review = "리뷰", merged = "머지됨"
    static func stage(_ checkout: CoreCheckoutSnapshot) -> Self {
        if checkout.worktree?.merged == true || checkout.pullRequest?.badge == .merged { return .merged }
        if let pr = checkout.pullRequest, pr.badge != .closed, pr.badge != .merged { return .review }
        if checkout.changedFileCount > 0 || checkout.ahead > 0 { return .working }
        return .ready
    }
    func matches(projectStatus: String) -> Bool {
        let value = projectStatus.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        switch self {
        case .ready: return ["준비", "todo", "to do", "backlog", "ready"].contains(value)
        case .working: return ["작업 중", "working", "in progress"].contains(value)
        case .review: return ["리뷰", "review", "in review"].contains(value)
        case .merged: return ["머지됨", "done", "merged", "complete", "completed"].contains(value)
        }
    }
}

struct ProjectHomeRow: Identifiable {
    static func showsDetail(_ agent: SidebarAgent) -> Bool {
        agent.group != "working" && (agent.demand == "question" || agent.demand == "error" || agent.group == "done")
    }

    let agent: SidebarAgent
    let depth: Int
    let foreignBranch: String?
    var id: String { agent.paneID }
}

struct ProjectHomeCard: Identifiable {
    let id: String
    let checkout: CoreCheckoutSnapshot?
    let rows: [ProjectHomeRow]
    let issue: CoreIssueLink?
    let stage: ProjectHomeStage?
    let agentsView: Bool
    let backlog: Bool
    var githubStatus: CoreGithubStatus = .empty
    var needsYou: Bool { rows.contains { $0.agent.group == "needs_you" } }
    var error: Bool { rows.contains { $0.agent.demand == "error" } }
    var root: SidebarAgent? { rows.first?.agent }
    var title: String? { issue?.issue.title ?? checkout?.purpose?.text }
    var requestCount: Int { rows.filter { !$0.agent.delegated }.count }
    var stageLabel: String? {
        guard let stage, let checkout else { return nil }
        switch stage {
        case .ready: return nil
        case .working: return checkout.changedFileCount > 0 ? "\(checkout.changedFileCount) changed" : "↑\(checkout.ahead)"
        case .review: return checkout.pullRequest.map { "PR #\($0.number)" }
        case .merged: return "merged"
        }
    }
    var mismatch: String? {
        guard let status = issue?.issue.projectStatus, let stage, !stage.matches(projectStatus: status) else { return nil }
        return status
    }
    var mismatchHelp: String {
        "Project: \(mismatch ?? "") · git: \(stageLabel ?? stage?.rawValue ?? "")\(stage == .review ? " 열림" : "")"
    }
    var issueHelp: String {
        guard let link = issue else { return "" }
        var lines = ["\(link.issue.reference.token) · \(link.issue.state == "CLOSED" ? "닫힘" : "열림")", link.issue.title]
        if let status = link.issue.projectStatus { lines.append("Project: \(status)") }
        lines.append(link.source)
        let status = checkout?.github ?? githubStatus
        if status.loading || status.stale,
           let success = status.lastSuccessAtUnixMS {
            lines.append("GitHub: \(max(0, Int((Date().timeIntervalSince1970 * 1000 - success) / 60000)))분 전")
        }
        return lines.joined(separator: "\n")
    }
}

struct ProjectHomeBoard {
    let tasks: [ProjectHomeCard]
    let adHoc: [ProjectHomeCard]
    let agents: [ProjectHomeCard]
    let overflow: Bool
    let isGit: Bool

    static func build(workspace: CoreWorkspaceSnapshot, agents: [SidebarAgent]) -> Self {
        let owners = Dictionary(workspace.checkouts.flatMap { checkout in
            checkout.tabs.flatMap { $0.panes.map { ($0.id, checkout) } }
        }, uniquingKeysWith: { first, _ in first })
        let byID = Dictionary(agents.map { ($0.paneID, $0) }, uniquingKeysWith: { first, _ in first })
        func rows(_ roots: [SidebarAgent], checkout: CoreCheckoutSnapshot?) -> [ProjectHomeRow] {
            var result: [ProjectHomeRow] = []
            var seen = Set<String>()
            func append(_ agent: SidebarAgent, depth: Int) {
                guard seen.insert(agent.paneID).inserted else { return }
                let owner = owners[agent.paneID]
                result.append(ProjectHomeRow(agent: agent, depth: depth,
                    foreignBranch: depth > 0 && owner?.id != checkout?.id ? (owner?.branch ?? agent.checkoutLabel) : nil))
                for child in agent.lineageChildPaneIDs.compactMap({ byID[$0] }) { append(child, depth: depth + 1) }
            }
            for root in roots { append(root, depth: 0) }
            return result
        }
        func prioritized(_ cards: [ProjectHomeCard]) -> [ProjectHomeCard] {
            cards.enumerated().sorted {
                if $0.element.needsYou != $1.element.needsYou { return $0.element.needsYou }
                return $0.offset < $1.offset
            }.map(\.element)
        }
        var tasks: [ProjectHomeCard] = []
        var adHoc: [ProjectHomeCard] = []
        for checkout in workspace.checkouts {
            let local = agents.filter { owners[$0.paneID]?.id == checkout.id }
            let localIDs = Set(local.map(\.paneID))
            let roots = local.filter { $0.lineageParentPaneID.map { !localIDs.contains($0) } ?? true }
            let card = ProjectHomeCard(id: "task:\(checkout.id)", checkout: checkout, rows: rows(roots, checkout: checkout),
                issue: checkout.issue, stage: checkout.isWorktree && workspace.isGit ? .stage(checkout) : nil,
                agentsView: false, backlog: false)
            if checkout.isWorktree && workspace.isGit { tasks.append(card) }
            else if !local.isEmpty { adHoc.append(card) }
        }
        let linked = Set(workspace.checkouts.compactMap { $0.issue?.issue.id })
        tasks += workspace.homeIssues.issues.filter { $0.state == "OPEN" && !linked.contains($0.id) }.map {
            ProjectHomeCard(id: "issue:\($0.id)", checkout: nil, rows: [],
                issue: CoreIssueLink(issue: $0, source: "GitHub"), stage: .ready, agentsView: false, backlog: true,
                githubStatus: workspace.checkouts.first?.github ?? .empty)
        }
        let roots = agents.filter { agent in
            (agent.lineageParentPaneID.map { byID[$0] == nil } ?? true)
                && rows([agent], checkout: owners[agent.paneID]).contains { owners[$0.agent.paneID] != nil }
        }
        let agentCards = roots.map { root in
            let checkout = owners[root.paneID]
            return ProjectHomeCard(id: "agent:\(root.paneID)", checkout: checkout, rows: rows([root], checkout: checkout),
                issue: checkout?.issue, stage: checkout.flatMap { $0.isWorktree ? .stage($0) : nil }, agentsView: true, backlog: false)
        }
        return Self(tasks: prioritized(tasks), adHoc: prioritized(adHoc), agents: prioritized(agentCards),
                    overflow: workspace.homeIssues.overflow, isGit: workspace.isGit)
    }
}

final class ProjectHomeMemo {
    struct Key: Equatable { let revision: UInt64; let workspaceID: String; let connected: Bool }
    private var key: Key?
    private var value: ProjectHomeBoard?
    func board(for key: Key, build: () -> ProjectHomeBoard) -> ProjectHomeBoard {
        if self.key == key, let value { return value }
        let value = build()
        self.key = key
        self.value = value
        return value
    }
}

enum ProjectHomeShortcutPolicy {
    static func shouldClose(_ event: NSEvent, visible: Bool, sheetPresented: Bool) -> Bool {
        visible && !sheetPresented && event.type == .keyDown && event.keyCode == 53
            && event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty
    }
}
