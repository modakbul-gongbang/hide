import Foundation

enum WorktreeMenuPolicy {
    static let newWorktree = "New worktree…"
    static let removeRegistration = "Remove registration"
    static let startAgentHere = "Start agent here"
    static let setBaseBranch = "Set as base branch"
    static let copyPath = "Copy Path"
    static let openIn = "Open in"

    static let projectItems = [newWorktree, removeRegistration]
    static func checkoutItems(isMain: Bool) -> [String] {
        var items = isMain ? [] : [startAgentHere]
        items += [setBaseBranch, copyPath, openIn]
        return items
    }
}

struct WorktreeSheetDraft: Equatable {
    var branch = ""
    var baseBranch: String?
    var agent: AgentProvider?

    var canSubmit: Bool {
        !branch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    mutating func reset(branches: [String], preferredBase: String?) {
        branch = ""
        baseBranch = preferredBase.flatMap { branches.contains($0) ? $0 : nil }
        agent = nil
    }
}

enum MainWorktreeBranchState: Equatable {
    case neutral(String)
    case warning(branch: String, base: String)
    case unknown(String?)
}

enum MainWorktreePresentation {
    static func state(branch: String?, base: String?) -> MainWorktreeBranchState {
        guard let branch else { return .unknown(nil) }
        guard let base else { return .unknown(branch) }
        return branch == base ? .neutral(branch) : .warning(branch: branch, base: base)
    }
}

enum ProjectBaseBranchPolicy {
    static func selected(
        projectPath: String,
        defaultBranch: String?,
        overrides: [String: String]
    ) -> String? {
        overrides[projectPath] ?? defaultBranch
    }

    static func orderedBranches(_ branches: [String], selectedBase: String?) -> [String] {
        branches.sorted { left, right in
            if left == selectedBase { return true }
            if right == selectedBase { return false }
            return left < right
        }
    }
}

struct BranchMigrationRequest: Identifiable, Equatable {
    let repositoryRoot: String
    let branch: String
    let baseBranch: String
    var id: String { repositoryRoot }

    var consequence: String {
        "Hide will check out \(baseBranch) in the main worktree, then create a new worktree for \(branch). Open panes and keyboard focus will not move. Any agent running there will see the files in the main worktree change."
    }
}

enum WorktreeSubmissionPresentation {
    static func oneLine(_ message: String) -> String {
        let parts = message.split(whereSeparator: \.isNewline).map(String.init)
        return parts.isEmpty ? message : parts.joined(separator: " ")
    }

    static func isLocked(phase: String?) -> Bool { phase == "working" }
    static func showsCancel(phase: String?) -> Bool { phase != "working" }
    static func primaryLabel(phase: String?) -> String {
        phase == "working" ? "Creating…" : "Create"
    }

    static func completion(paneID: String?) -> (closeSheet: Bool, focusPaneID: String?) {
        (paneID != nil, paneID)
    }
}
