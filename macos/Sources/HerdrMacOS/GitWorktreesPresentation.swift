import Foundation

/// The Overview worktree row's agent line.
///
/// An empty `agents` with no reason is a worktree nobody is working in; an
/// empty one carrying a reason is a worktree Hide cannot see into. Keeping
/// those apart is what the line exists for.
struct CoreWorktreeAgentLine: Decodable, Equatable, Sendable {
    let agents: [CoreAgentChip]
    let uninstrumentedReason: String?
    let uninstrumentedLabel: String?
    let uninstrumentedCode: String?

    enum CodingKeys: String, CodingKey {
        case agents
        case uninstrumentedReason = "uninstrumented_reason"
        case uninstrumentedLabel = "uninstrumented_label"
        case uninstrumentedCode = "uninstrumented_code"
    }

    init(
        agents: [CoreAgentChip] = [],
        uninstrumentedReason: String? = nil,
        uninstrumentedLabel: String? = nil,
        uninstrumentedCode: String? = nil
    ) {
        self.agents = agents
        self.uninstrumentedReason = uninstrumentedReason
        self.uninstrumentedLabel = uninstrumentedLabel
        self.uninstrumentedCode = uninstrumentedCode
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        agents = try container.decodeIfPresent([CoreAgentChip].self, forKey: .agents) ?? []
        uninstrumentedReason = try container.decodeIfPresent(String.self, forKey: .uninstrumentedReason)
        uninstrumentedLabel = try container.decodeIfPresent(String.self, forKey: .uninstrumentedLabel)
        uninstrumentedCode = try container.decodeIfPresent(String.self, forKey: .uninstrumentedCode)
    }
}

struct CoreWorktreeDeletionGate: Decodable, Equatable, Sendable {
    let blockedReason: String?
    let warnings: [String]
    let buttonLabel: String
    let canDeleteBranch: Bool
    enum CodingKeys: String, CodingKey {
        case blockedReason = "blocked_reason", warnings
        case buttonLabel = "button_label", canDeleteBranch = "can_delete_branch"
    }
}

struct CoreWorktreeRemoval: Decodable, Sendable {
    let id: UInt64
    let repositoryRoot: String
    let checkoutPath: String
    let expectedHeadSHA: String?
    let expectedBranch: String?
    let protectedBaseBranch: String?
    let branch: String?
    let deleteBranch: Bool
    let phase: String
    let message: String?
    enum CodingKeys: String, CodingKey {
        case id, branch, phase, message
        case repositoryRoot = "repository_root", checkoutPath = "checkout_path"
        case expectedHeadSHA = "expected_head_sha", expectedBranch = "expected_branch"
        case protectedBaseBranch = "protected_base_branch", deleteBranch = "delete_branch"
    }
}

struct CoreTaskOperation: Decodable, Sendable {
    let id: UInt64
    let kind: String
    let phase: String
    let repositoryRoot: String?
    let branch: String?
    let baseBranch: String?
    let path: String?
    let paneID: String?
    let agentKind: String?
    let message: String?

    enum CodingKeys: String, CodingKey {
        case id, kind, phase, branch, path, message
        case repositoryRoot = "repository_root"
        case baseBranch = "base_branch"
        case paneID = "pane_id"
        case agentKind = "agent_kind"
    }
}

struct CoreProjectWorktrees: Decodable {
    var cleanup: CoreCleanup? = nil
    var github: CoreGithubStatus? = nil
    var pullRequests: [CorePullRequest]? = nil
    var pullRequestWindow: String? = nil
    var history: CoreGitHistory? = nil
    var sharedGitDisk: CoreDiskUsage? = nil
    var diskTotalBytes: UInt64? = nil
    var diskConfirmedBytes: UInt64? = nil
    var linkedDiskBytes: UInt64? = nil
    var diskUnavailableReason: String? = nil
    let rootPath: String
    let defaultBranch: String?
    let baseBranch: String?
    let baseBranchFallback: String?
    let baseSource: String
    let unavailableReason: String?
    let worktrees: [CoreGitWorktree]
    enum CodingKeys: String, CodingKey {
        case cleanup, github
        case pullRequests = "pull_requests", pullRequestWindow = "pull_request_window"
        case history, sharedGitDisk = "shared_git_disk", diskTotalBytes = "disk_total_bytes"
        case diskConfirmedBytes = "disk_confirmed_bytes"
        case linkedDiskBytes = "linked_disk_bytes", diskUnavailableReason = "disk_unavailable_reason"
        case rootPath = "root_path", defaultBranch = "default_branch", baseBranch = "base_branch"
        case baseBranchFallback = "base_branch_fallback", baseSource = "base_source"
        case unavailableReason = "unavailable_reason", worktrees
    }
}

struct CoreGitWorktree: Decodable, Identifiable {
    var id: String { path }
    let path: String
    let branch: String?
    let headSHA: String?
    let lastCommitSubject: String?
    let missing: Bool
    let isMain: Bool
    let dirty: Bool
    let changedFileCount: Int
    let baseBranch: String?
    let ahead: Int
    let behind: Int
    let merged: Bool?
    let upstreamState: String
    let unpushed: CoreUnpushed?
    let unavailableReason: String?
    let lastFetchAtUnixMS: Double?
    let measuredAtUnixMS: Double?
    let lastCommitUnixSeconds: Double?
    let paneCount: Int
    let runningAgentCount: Int
    /// Who is working in this worktree and on what. Empty with no mark means
    /// nobody; empty with a mark means Hide cannot see into it.
    let agentLine: CoreWorktreeAgentLine
    let disk: CoreDiskUsage
    let pullRequest: CorePullRequest?
    let github: CoreGithubStatus
    let deletionGate: CoreWorktreeDeletionGate
    let openError: String?
    enum CodingKeys: String, CodingKey {
        case path, branch, missing, dirty, ahead, behind, merged, unpushed, disk, github
        case headSHA = "head_sha", isMain = "is_main", changedFileCount = "changed_file_count", baseBranch = "base_branch"
        case lastCommitSubject = "last_commit_subject"
        case upstreamState = "upstream_state", unavailableReason = "unavailable_reason", lastFetchAtUnixMS = "last_fetch_at_unix_ms"
        case measuredAtUnixMS = "measured_at_unix_ms", lastCommitUnixSeconds = "last_commit_unix_seconds"
        case paneCount = "pane_count", runningAgentCount = "running_agent_count", pullRequest = "pull_request"
        case deletionGate = "deletion_gate", openError = "open_error"
        case agentLine = "agent_line"
    }
    var label: String { branch ?? String(headSHA?.prefix(8) ?? "unknown") }
    var pushedLabel: String {
        switch upstreamState {
        case "pushed": "pushed"
        case "unpushed": unpushed.map { "↑\($0.count)" } ?? "—"
        case "no_upstream": "no upstream"
        case "gone": "upstream gone"
        default: "—"
        }
    }
    func relativePath(root: String) -> String {
        let components = URL(fileURLWithPath: path).standardizedFileURL.pathComponents
        let rootComponents = URL(fileURLWithPath: root).standardizedFileURL.pathComponents
        let common = zip(components, rootComponents).prefix { $0 == $1 }.count
        let relative = Array(repeating: "..", count: rootComponents.count - common) + components.dropFirst(common)
        return relative.isEmpty ? "." : relative.joined(separator: "/")
    }
    var deletionConsequence: String {
        var sentences = [missing ? "Removes the missing worktree registration at \(path)." : "Deletes the folder at \(path)."]
        if !deletionGate.warnings.isEmpty { sentences.append("This worktree has \(deletionGate.warnings.joined(separator: ", ")).") }
        if let bytes = disk.totalBytes { sentences.append("This frees \(CheckoutCardPresentation.formattedBytes(bytes)).") }
        else { sentences.append(disk.unavailableReason.map { "Disk usage is unavailable: \($0)." } ?? "Disk usage has not been measured.") }
        if paneCount > 0 { sentences.append("Closes \(paneCount) panes first. If deletion then fails, the worktree remains and those panes stay closed.") }
        sentences.append(deletionGate.canDeleteBranch
            ? "The local branch stays unless you select the option below. Safe branch deletion can be refused by Git."
            : "The local branch is kept.")
        return sentences.joined(separator: " ")
    }
}

struct CoreGitHistory: Decodable, Equatable {
    var continuation: [String]? = nil
    var shallowBoundaries: [String]? = nil
    let commits: [CoreGitCommit]
    let truncated: Bool
    let unavailableReason: String?
    enum CodingKeys: String, CodingKey { case commits, truncated, continuation
        case shallowBoundaries = "shallow_boundaries", unavailableReason = "unavailable_reason" }
}
struct CoreGitCommit: Decodable, Equatable, Identifiable {
    var id: String { sha }
    let sha: String
    let parents: [String]
    let subject: String
    var decorations: String? = nil
}

struct CoreCleanup: Decodable {
    let id: UInt64
    let repositoryRoot: String
    let phase: String
    let rows: [CoreCleanupRow]
    let message: String?
    enum CodingKeys: String, CodingKey { case id, phase, rows, message; case repositoryRoot = "repository_root" }
}
struct CoreCleanupRow: Decodable, Identifiable {
    var id: String { path }
    let path: String
    let branch: String?
    let head: String?
    let exclusion: String?
    let disk: CoreDiskUsage
    let result: String?
    let message: String?
}
