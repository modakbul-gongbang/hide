import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Project Overview")
struct OverviewPresentationTests {
    @Test func unavailableCountsNeverRenderAsMeasuredZero() {
        #expect(OverviewPresentation.diskLabel(total: nil, confirmed: nil, failure: nil, isGit: true) == "Measuring…")
        #expect(OverviewPresentation.diskLabel(total: nil, confirmed: nil, failure: "denied", isGit: true) == "Unavailable")
        #expect(OverviewPresentation.diskLabel(total: nil, confirmed: 4096, failure: "one missing", isGit: true).hasPrefix("Partial · "))
        #expect(!OverviewPresentation.diskLabel(total: 0, confirmed: 0, failure: nil, isGit: true).contains("Unavailable"))
        #expect(OverviewPresentation.githubLabel(status: .empty, requests: nil, isGit: true) == "Not loaded")
        let ready = CoreGithubStatus(available: true, loading: false, stale: false, lastSuccessAtUnixMS: 1, unavailableReason: nil)
        #expect(OverviewPresentation.githubLabel(status: ready, requests: [], isGit: true) == "No recent pull requests")
        let failed = CoreGithubStatus(available: false, loading: false, stale: false, lastSuccessAtUnixMS: nil,
                                     unavailableReason: "Sign in", failureCategory: "not logged in")
        #expect(OverviewPresentation.githubLabel(status: failed, requests: [], isGit: true) == "Sign in required")
        let loading = CoreGithubStatus(available: false, loading: true, stale: false, lastSuccessAtUnixMS: nil, unavailableReason: nil)
        #expect(OverviewPresentation.githubLabel(status: loading, requests: [], isGit: true) == "Loading…")
    }

    @Test func mergeGraphRetainsBothParentsAndNamesOnlyRealContinuations() {
        let history = CoreGitHistory(commits: [
            CoreGitCommit(sha: "merge", parents: ["main", "feature"], subject: "Merge"),
            CoreGitCommit(sha: "feature", parents: ["base"], subject: "Feature"),
            CoreGitCommit(sha: "main", parents: ["base"], subject: "Main")
        ], truncated: true, unavailableReason: nil)
        let graph = OverviewGraph(history: history, checkouts: [], expanded: [])
        #expect(graph.rows.count == 3)
        #expect(graph.rows[0].parents == ["main", "feature"])
        #expect(graph.edges.count == 4)
        #expect(graph.edges.filter(\.continuation).count == 2)
        #expect(Set(graph.rows.map(\.lane)).count == 2)
    }

    @Test func longLinearHistoryFoldsButPreservesNamedBranchBoundariesAndExpansion() {
        let commits = (0..<80).map { i in
            CoreGitCommit(sha: "\(i)", parents: ["\(i + 1)"], subject: "Commit \(i)", decorations: i == 40 ? "refs/heads/main" : "")
        }
        let history = CoreGitHistory(commits: commits, truncated: true, unavailableReason: nil)
        let graph = OverviewGraph(history: history, checkouts: [], expanded: [])
        #expect(graph.rows.count == 3)
        #expect(graph.rows[1].id == "40")
        #expect(graph.rows[0].parents == ["40"])
        #expect(graph.edges.filter(\.continuation).count == 1)
        let expanded = OverviewGraph(history: history, checkouts: [], expanded: ["0", "41"])
        #expect(expanded.rows.count == 80)
        #expect(expanded.rows[40].id == "40")
    }

    @Test func cleanupWireKeepsFailureAndExclusionSeparateFromSize() throws {
        let json = #"{"id":1,"repository_root":"/fixture","phase":"complete","message":null,"rows":[{"path":"/fixture/linked","branch":"작업/한글","head":"abc","exclusion":null,"disk":{"path":"/fixture/linked","total_bytes":null,"largest_child_name":null,"largest_child_bytes":null,"unavailable_reason":"unreadable"},"result":"refused","message":"State changed. Review again."}]}"#
        let review = try JSONDecoder().decode(CoreCleanup.self, from: Data(json.utf8))
        #expect(review.rows[0].disk.totalBytes == nil)
        #expect(review.rows[0].disk.unavailableReason == "unreadable")
        #expect(review.rows[0].result == "refused")
        #expect(review.rows[0].message == "State changed. Review again.")
        #expect(review.rows[0].branch == "작업/한글")
    }
}
