import Foundation

enum WorktreeMenuPolicy {
    static let pinProject = "Pin"
    static let unpinProject = "Unpin"
    static let newWorktree = "New worktree…"
    static let removeProject = "Remove project…"
    static let setBaseBranch = "Set as base branch"
    static let setPurpose = "Set purpose…"
    static let copyPath = "Copy Path"
    static let openIn = "Open in"

    /// The registered project row's menu, the same in `⋯` and on right-click
    /// (D-05). An unregistered folder row has no pin and no removal (D-06).
    static let projectItems = [pinProject, newWorktree, removeProject]
    static let checkoutItems = [newWorktree, setBaseBranch, copyPath, openIn]
}

struct WorktreeSheetDraft: Equatable {
    var branch = ""
    var baseBranch: String?
    var agent: AgentProvider?
    var purpose = ""

    var canSubmit: Bool {
        !branch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    mutating func reset(branches: [String], preferredBase: String?) {
        branch = ""
        baseBranch = preferredBase.flatMap { branches.contains($0) ? $0 : nil }
        agent = nil
        purpose = ""
    }
}

enum PurposeInputPresentation {
    static let recommendedLimit = 40
    static let hardLimit = 80

    static func normalized(_ text: String) -> String {
        let oneLine = text.replacingOccurrences(of: "\n", with: " ")
            .replacingOccurrences(of: "\r", with: " ")
        var limited = String.UnicodeScalarView()
        limited.append(contentsOf: oneLine.unicodeScalars.prefix(hardLimit))
        return String(limited)
    }

    static func countLabel(_ text: String) -> String {
        "\(text.unicodeScalars.count) / \(recommendedLimit)"
    }

    static func isWarning(_ text: String) -> Bool {
        text.unicodeScalars.count > recommendedLimit
    }
}

struct CheckoutPurposeRequest: Identifiable {
    let workspace: CoreWorkspaceSnapshot
    let checkout: CoreCheckoutSnapshot
    var id: String { checkout.id }
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
        phase == "working" ? "Creating…" : "Create worktree"
    }

    static func completion(paneID: String?) -> (closeSheet: Bool, focusPaneID: String?) {
        (paneID != nil, paneID)
    }
}
