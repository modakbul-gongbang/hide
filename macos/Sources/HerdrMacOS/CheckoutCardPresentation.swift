import Foundation
import SwiftUI

/// What the summary card and the sidebar row show, decided here rather than
/// inside a view body.
///
/// The rules this file holds are the ones the PRD writes down - which badge a
/// pull request gets and what counts as stale -
/// so they can be checked against fixed input instead of against a screenshot.
enum CheckoutCardPresentation {
    /// The badge's colour. Merged and closed share one because both mean the
    /// work is over and the whole row is dimmed anyway; the three review
    /// decisions are the distinction the colour actually has to carry.
    ///
    /// No saturated accent is used: DESIGN.md reserves those for category
    /// illustration, and this is chrome.
    static func badgeColor(
        _ badge: CorePullRequestBadge,
        review: CoreReviewDecision?
    ) -> Color {
        switch badge {
        case .merged, .closed: HideTheme.muted
        case .open: HideTheme.secondary
        case .review:
            switch review {
            case .approved: HideTheme.success
            case .changesRequested: HideTheme.danger
            case .reviewRequired, .none: HideTheme.warning
            }
        }
    }

    /// The badge's text. A review badge says which decision it is, because
    /// three shades of one word is not a distinction anyone can name.
    static func badgeLabel(
        _ badge: CorePullRequestBadge,
        review: CoreReviewDecision?
    ) -> String {
        switch badge {
        case .merged: "merged"
        case .closed: "closed"
        case .open: "open"
        case .review:
            switch review {
            case .approved: "approved"
            case .changesRequested: "changes"
            case .reviewRequired, .none: "review"
            }
        }
    }

    /// Whether the row is drawn dimmed. A worktree with no terminal is not
    /// where work is happening, and a merged or closed one is where it has
    /// stopped; both are things to look past rather than at.
    static func isDimmed(_ checkout: CoreCheckoutSnapshot) -> Bool {
        if let badge = checkout.pullRequest?.badge, badge.isSettled {
            return true
        }
        return !checkout.hasPanes
    }

    /// The row's accessibility label: every badge, dot, and count said in
    /// words, because the row itself is deliberately almost wordless.
    static func rowAccessibilityLabel(
        repoName: String,
        checkout: CoreCheckoutSnapshot,
        agentCount: Int
    ) -> String {
        var parts = [repoName, checkout.label]
        if !checkout.exists {
            parts.append("missing")
        }
        if let pullRequest = checkout.pullRequest {
            parts.append(
                "pull request \(pullRequest.number) \(badgeLabel(pullRequest.badge, review: pullRequest.review))"
            )
        }
        if checkout.dirty {
            parts.append(
                checkout.changedFileCount == 1
                    ? "1 uncommitted change"
                    : "\(checkout.changedFileCount) uncommitted changes"
            )
        }
        if agentCount > 0 {
            parts.append(agentCount == 1 ? "1 agent" : "\(agentCount) agents")
        }
        if !checkout.hasPanes {
            parts.append("no terminal")
        }
        return parts.joined(separator: ", ")
    }

    /// A byte count as the card shows it: two significant figures at most, so
    /// the column stays the same width whatever the number is.
    static func formattedBytes(_ bytes: Double) -> String {
        let units = ["B", "KB", "MB", "GB", "TB"]
        var value = bytes
        var unit = 0
        while value >= 1024, unit < units.count - 1 {
            value /= 1024
            unit += 1
        }
        if unit == 0 {
            return "\(Int(value)) \(units[unit])"
        }
        return value < 10
            ? String(format: "%.1f %@", value, units[unit])
            : String(format: "%.0f %@", value, units[unit])
    }

    /// How long ago an instant was, in the card's shorthand. Used for the
    /// pull-request lookup's `as of` and for a merge's age.
    static func relativeAge(fromUnixMS: Double, now: Date = Date()) -> String {
        let seconds = max(0, now.timeIntervalSince1970 - fromUnixMS / 1000)
        let minutes = Int(seconds / 60)
        if minutes < 1 { return "just now" }
        if minutes < 60 { return "\(minutes)m ago" }
        let hours = minutes / 60
        if hours < 24 { return "\(hours)h ago" }
        let days = hours / 24
        return days == 1 ? "1 day ago" : "\(days) days ago"
    }

    /// The card's one allowed sentence about `gh`, or `nil` when there is
    /// nothing to say.
    ///
    /// "No pull request on this branch" is not a notice: it is the normal
    /// answer, and the card shows it by having no pull-request row.
    static func githubNotice(_ status: CoreGithubStatus) -> String? {
        status.unavailableReason
    }

    /// What the pull-request row's `as of` says, present only when the last
    /// lookup failed and the values shown are the previous ones.
    static func staleNotice(_ status: CoreGithubStatus, now: Date = Date()) -> String? {
        guard status.stale, let last = status.lastSuccessAtUnixMS else { return nil }
        return "as of \(relativeAge(fromUnixMS: last, now: now))"
    }

    /// Whether the card shows the git rows at all. A plain folder has no
    /// branch, no pull request, and no changes to compare - only a name, a
    /// size, and whatever is running in it.
    static func showsGitRows(workspaceIsGit: Bool, checkout: CoreCheckoutSnapshot?) -> Bool {
        workspaceIsGit && checkout != nil
    }
}
