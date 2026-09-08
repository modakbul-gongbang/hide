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
