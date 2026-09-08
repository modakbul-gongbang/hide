import Foundation
import Testing

@testable import HerdrMacOS

private func pullRequest(
    number: Int = 7,
    badge: CorePullRequestBadge,
    review: CoreReviewDecision? = nil,
    mergedAtUnixMS: Double? = nil
) -> CorePullRequest {
    CorePullRequest(
        number: number,
        headBranch: "feature",
        baseBranch: "main",
        url: "https://example.invalid/pull/\(number)",
        badge: badge,
        review: review,
        isDraft: false,
        mergedAtUnixMS: mergedAtUnixMS,
        updatedAtUnixMS: nil
    )
}

private func checkout(
    hasPanes: Bool = true,
    dirty: Bool = false,
    changedFileCount: Int = 0,
    exists: Bool = true,
    pullRequest: CorePullRequest? = nil
) -> CoreCheckoutSnapshot {
    CoreCheckoutSnapshot(
        id: "checkout-1",
        workspaceID: "workspace-1",
        label: "feature",
        path: "/tmp/hide/feature",
        branch: "feature",
        isWorktree: true,
        exists: exists,
        temporary: false,
        hasPanes: hasPanes,
        dirty: dirty,
        changedFileCount: changedFileCount,
        pullRequest: pullRequest,
        tabs: []
    )
}

@Suite("Checkout card presentation")
struct CheckoutCardPresentationTests {
    /// The three review decisions are the distinction the badge colour has to
    /// carry, so they must be three different colours and each must say which
    /// one it is in words.
    @Test func theThreeReviewDecisionsAreKeptApartByColourAndByName() {
        let approved = CheckoutCardPresentation.badgeColor(.review, review: .approved)
        let changes = CheckoutCardPresentation.badgeColor(.review, review: .changesRequested)
        let required = CheckoutCardPresentation.badgeColor(.review, review: .reviewRequired)
        #expect(approved != changes)
        #expect(changes != required)
        #expect(approved != required)
        #expect(CheckoutCardPresentation.badgeLabel(.review, review: .approved) == "approved")
        #expect(CheckoutCardPresentation.badgeLabel(.review, review: .changesRequested) == "changes")
        #expect(CheckoutCardPresentation.badgeLabel(.review, review: .reviewRequired) == "review")
    }

    @Test func everyBadgeHasItsOwnWord() {
        #expect(CheckoutCardPresentation.badgeLabel(.merged, review: nil) == "merged")
        #expect(CheckoutCardPresentation.badgeLabel(.closed, review: nil) == "closed")
        #expect(CheckoutCardPresentation.badgeLabel(.open, review: nil) == "open")
    }

    /// Merged and closed are the two states a worktree may be removed from,
    /// and nothing else is.
    @Test func onlyASettledPullRequestIsSettled() {
        #expect(CorePullRequestBadge.merged.isSettled)
        #expect(CorePullRequestBadge.closed.isSettled)
        #expect(!CorePullRequestBadge.open.isSettled)
        #expect(!CorePullRequestBadge.review.isSettled)
    }

    /// The row is almost wordless on purpose, so every badge, dot, and count
    /// has to be reachable in words.
    @Test func theRowSaysInWordsWhatItDrawsInSymbols() {
        let label = CheckoutCardPresentation.rowAccessibilityLabel(
            repoName: "hide",
            checkout: checkout(
                hasPanes: false,
                dirty: true,
                changedFileCount: 3,
                pullRequest: pullRequest(number: 12, badge: .review, review: .changesRequested)
            ),
            agentCount: 2
        )
        #expect(label.contains("hide"))
        #expect(label.contains("feature"))
        #expect(label.contains("pull request 12 changes"))
        #expect(label.contains("3 uncommitted changes"))
        #expect(label.contains("2 agents"))
        #expect(label.contains("no terminal"))
    }

    @Test func aMissingWorktreeSaysSo() {
        let label = CheckoutCardPresentation.rowAccessibilityLabel(
            repoName: "hide",
            checkout: checkout(exists: false),
            agentCount: 0
        )
        #expect(label.contains("missing"))
    }

    @Test func sizesReadAsTwoSignificantFiguresAtMost() {
        #expect(CheckoutCardPresentation.formattedBytes(512) == "512 B")
        #expect(CheckoutCardPresentation.formattedBytes(2048) == "2.0 KB")
        #expect(CheckoutCardPresentation.formattedBytes(1024 * 1024 * 340) == "340 MB")
        #expect(CheckoutCardPresentation.formattedBytes(1024 * 1024 * 1024 * 3.5) == "3.5 GB")
    }

    @Test func agesReadInTheUnitThatFits() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let ago: (Double) -> String = { seconds in
            CheckoutCardPresentation.relativeAge(
                fromUnixMS: (now.timeIntervalSince1970 - seconds) * 1000,
                now: now
            )
        }
        #expect(ago(10) == "just now")
        #expect(ago(120) == "2m ago")
        #expect(ago(7200) == "2h ago")
        #expect(ago(86_400) == "1 day ago")
        #expect(ago(86_400 * 4) == "4 days ago")
    }

    /// A failed lookup keeps the previous answer and says how old it is. A
    /// healthy one says nothing at all, because "current" is not news.
    @Test func onlyAFailedLookupCarriesAnAsOfNotice() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let lastSuccess = (now.timeIntervalSince1970 - 600) * 1000
        let stale = CoreGithubStatus(
            available: true,
            loading: false,
            stale: true,
            lastSuccessAtUnixMS: lastSuccess,
            unavailableReason: "gh pr list: network unreachable"
        )
        #expect(CheckoutCardPresentation.staleNotice(stale, now: now) == "as of 10m ago")
        #expect(CheckoutCardPresentation.githubNotice(stale) == "gh pr list: network unreachable")

        let healthy = CoreGithubStatus(
            available: true,
            loading: false,
            stale: false,
            lastSuccessAtUnixMS: lastSuccess,
            unavailableReason: nil
        )
        #expect(CheckoutCardPresentation.staleNotice(healthy, now: now) == nil)
        #expect(CheckoutCardPresentation.githubNotice(healthy) == nil)
    }

    /// gh being absent or logged out is the card's one sentence, and it must
    /// not be confused with a repository that simply has no pull requests.
    @Test func aMissingGhIsANoticeAndAnEmptyRepositoryIsNot() {
        let missing = CoreGithubStatus(
            available: false,
            loading: false,
            stale: false,
            lastSuccessAtUnixMS: nil,
            unavailableReason: "gh is not installed. Install the GitHub CLI to see pull requests."
        )
        #expect(CheckoutCardPresentation.githubNotice(missing)?.contains("gh is not installed") == true)

        let noPullRequests = CoreGithubStatus(
            available: true,
            loading: false,
            stale: false,
            lastSuccessAtUnixMS: 1000,
            unavailableReason: nil
        )
        #expect(CheckoutCardPresentation.githubNotice(noPullRequests) == nil)
    }

    /// A plain folder has no branch and no pull request to show, so the card's
    /// git rows are absent rather than empty.
    @Test func aPlainFolderShowsNoGitRows() {
        #expect(!CheckoutCardPresentation.showsGitRows(workspaceIsGit: false, checkout: checkout()))
        #expect(CheckoutCardPresentation.showsGitRows(workspaceIsGit: true, checkout: checkout()))
        #expect(!CheckoutCardPresentation.showsGitRows(workspaceIsGit: true, checkout: nil))
    }

    /// The row splits a path into the name it is identified by and the
    /// directory that is only context.
    @Test func aChangedFileRowSplitsItsNameFromItsDirectory() {
        let nested = CoreChangedFile(
            path: "/repo/src/core/lib.rs",
            relativePath: "src/core/lib.rs",
            status: .modified,
            addedLines: 12,
            removedLines: 3
        )
        #expect(nested.name == "lib.rs")
        #expect(nested.directory == "src/core")

        let root = CoreChangedFile(
            path: "/repo/README.md",
            relativePath: "README.md",
            status: .added
        )
        #expect(root.name == "README.md")
        #expect(root.directory == "")
        // A file git cannot count shows no numbers rather than zeros.
        #expect(root.addedLines == nil)
    }
}
