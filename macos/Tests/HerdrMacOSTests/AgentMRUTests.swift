import Testing

@testable import HerdrMacOS

@Suite("Project MRU and switcher")
struct ProjectMRUTests {
    @Test func focusObservationIsDeterministicAndRemovesUnavailableProjects() {
        var mru = ProjectMRU()

        mru.observe(focusedProjectID: nil, availableProjectIDs: ["p1", "p2"])
        #expect(mru.projectIDs == ["p1", "p2"])

        mru.observe(focusedProjectID: "p1", availableProjectIDs: ["p1", "p2"])
        mru.observe(focusedProjectID: "p1", availableProjectIDs: ["p1", "p2"])
        #expect(mru.projectIDs == ["p1", "p2"])

        mru.observe(focusedProjectID: "p2", availableProjectIDs: ["p1", "p2"])
        #expect(mru.projectIDs == ["p2", "p1"])

        mru.observe(focusedProjectID: nil, availableProjectIDs: ["p1"])
        #expect(mru.projectIDs == ["p1"])

        mru.observe(focusedProjectID: "removed", availableProjectIDs: ["p1"])
        #expect(mru.projectIDs == ["p1"])
    }

    @Test func emptyAndSingleProjectNeverOpenACycle() {
        #expect(ProjectSwitcherCycle(originalProjectID: nil, projectIDs: []) == nil)
        #expect(ProjectSwitcherCycle(originalProjectID: "p1", projectIDs: ["p1"]) == nil)
        #expect(
            ProjectSwitcherCycle(originalProjectID: "p1", projectIDs: ["p1"], direction: .backward) == nil
        )
    }

    @Test func openingBackwardStartsAtTheLeastRecentProject() {
        // Index 0 is the project already focused. Forward skips to the previous
        // one; backward has to land on the far end, not back on the current.
        var forward = ProjectSwitcherCycle(originalProjectID: "p1", projectIDs: ["p1", "p2", "p3", "p4"])
        var backward = ProjectSwitcherCycle(
            originalProjectID: "p1",
            projectIDs: ["p1", "p2", "p3", "p4"],
            direction: .backward
        )

        #expect(forward?.selectedProjectID == "p2")
        #expect(backward?.selectedProjectID == "p4")

        forward?.advance()
        backward?.retreat()
        #expect(forward?.selectedProjectID == "p3")
        #expect(backward?.selectedProjectID == "p3")
    }

    @Test func retreatWrapsPastTheFirstEntryOntoTheLast() {
        var cycle = ProjectSwitcherCycle(originalProjectID: "p1", projectIDs: ["p1", "p2", "p3"])

        cycle?.retreat()
        #expect(cycle?.selectedProjectID == "p1")
        cycle?.retreat()
        #expect(cycle?.selectedProjectID == "p3")
    }

    @Test func advanceAndRetreatAreInverses() {
        var cycle = ProjectSwitcherCycle(originalProjectID: "p1", projectIDs: ["p1", "p2", "p3", "p4"])
        let start = cycle?.selectedProjectID

        cycle?.advance()
        cycle?.advance()
        cycle?.retreat()
        cycle?.retreat()

        #expect(cycle?.selectedProjectID == start)
    }

    @Test func twoProjectsOpenOnTheSameEntryInBothDirections() {
        // With one other project there is nowhere else to go, so the reverse
        // chord must not select the already-focused project.
        let forward = ProjectSwitcherCycle(originalProjectID: "p1", projectIDs: ["p1", "p2"])
        let backward = ProjectSwitcherCycle(
            originalProjectID: "p1",
            projectIDs: ["p1", "p2"],
            direction: .backward
        )

        #expect(forward?.selectedProjectID == "p2")
        #expect(backward?.selectedProjectID == "p2")
    }

    @Test func manyProjectsCycleFromThePreviousProjectAndWrapOncePerAdvance() throws {
        var cycle = try #require(ProjectSwitcherCycle(
            originalProjectID: "p1",
            projectIDs: ["p1", "p2", "p2", "p3"]
        ))

        #expect(cycle.projectIDs == ["p1", "p2", "p3"])
        #expect(cycle.selectedProjectID == "p2")
        cycle.advance()
        #expect(cycle.selectedProjectID == "p3")
        cycle.advance()
        #expect(cycle.selectedProjectID == "p1")
        #expect(cycle.committedProjectID(availableProjectIDs: ["p1", "p2", "p3"]) == "p1")
    }

    @Test func removedSelectionCannotCommitAndCancelKeepsTheOriginalFocus() throws {
        let focusedProjectID = "p1"
        var cycle: ProjectSwitcherCycle? = try #require(ProjectSwitcherCycle(
            originalProjectID: focusedProjectID,
            projectIDs: ["p1", "p2", "p3"]
        ))

        cycle?.advance()
        #expect(cycle?.selectedProjectID == "p3")
        #expect(cycle?.committedProjectID(availableProjectIDs: ["p1", "p2"]) == nil)
        #expect(cycle?.originalProjectID == focusedProjectID)

        cycle = nil
        #expect(cycle == nil)
        #expect(focusedProjectID == "p1")
    }
}

@Suite("Tab MRU and switcher")
struct TabMRUTests {
    @Test func recencyIsScopedToTheProject() {
        var mru = TabMRU()

        mru.observe(
            contextID: "local:project-a",
            focusedTabID: "tab-a2",
            availableTabIDs: ["tab-a1", "tab-a2"]
        )
        #expect(mru.tabIDs(in: "local:project-a") == ["tab-a2", "tab-a1"])

        mru.observe(
            contextID: "local:project-b",
            focusedTabID: "tab-b1",
            availableTabIDs: ["tab-b1", "tab-b2"]
        )
        #expect(mru.tabIDs(in: "local:project-b") == ["tab-b1", "tab-b2"])
        #expect(!mru.tabIDs(in: "local:project-b").contains("tab-a2"))
    }

    @Test func removedTabsCannotRemainInTheCycleOrCommit() throws {
        var mru = TabMRU()
        mru.observe(
            contextID: "local:project-a",
            focusedTabID: "tab-a1",
            availableTabIDs: ["tab-a1", "tab-a2", "tab-a3"]
        )
        mru.observe(
            contextID: "local:project-a",
            focusedTabID: "tab-a3",
            availableTabIDs: ["tab-a1", "tab-a3"]
        )
        #expect(mru.tabIDs(in: "local:project-a") == ["tab-a3", "tab-a1"])

        var cycle = try #require(TabSwitcherCycle(
            originalTabID: "tab-a3",
            tabIDs: mru.tabIDs(in: "local:project-a")
        ))
        #expect(cycle.selectedTabID == "tab-a1")
        cycle.advance()
        #expect(cycle.selectedTabID == "tab-a3")
        #expect(cycle.committedTabID(availableTabIDs: ["tab-a1"]) == nil)
    }

    @Test func reverseTabSwitchingStartsAtTheLeastRecentTab() throws {
        let cycle = try #require(TabSwitcherCycle(
            originalTabID: "tab-1",
            tabIDs: ["tab-1", "tab-2", "tab-3"],
            direction: .backward
        ))

        #expect(cycle.selectedTabID == "tab-3")
    }
}

@Suite("Two-level recent navigation")
struct RecentNavigationTests {
    @Test func visitingAnotherProjectPreservesEverySurfaceAndCheckoutRecency() throws {
        var tabs = TabMRU()
        let alpha = ["terminal-main", "file-main", "diff-worktree", "browser-worktree"]
        tabs.observe(contextID: "alpha", focusedTabID: "file-main", availableTabIDs: alpha)
        tabs.observe(contextID: "alpha", focusedTabID: "browser-worktree", availableTabIDs: alpha)
        tabs.observe(contextID: "beta", focusedTabID: "beta-file", availableTabIDs: ["beta-file"])
        // Re-entering alpha restores the last surface; background snapshots
        // must not reset that history to strip order or checkout order.
        tabs.observe(contextID: "alpha", focusedTabID: nil, availableTabIDs: alpha)
        #expect(tabs.tabIDs(in: "alpha") == ["browser-worktree", "file-main", "terminal-main", "diff-worktree"])
        var cycle = try #require(TabSwitcherCycle(originalTabID: "browser-worktree", tabIDs: tabs.tabIDs(in: "alpha")))
        #expect(cycle.selectedTabID == "file-main")
        cycle.advance()
        #expect(cycle.selectedTabID == "terminal-main")
        cycle.advance()
        #expect(cycle.selectedTabID == "diff-worktree")
        tabs.retainContexts(["beta"])
        #expect(tabs.tabIDs(in: "alpha").isEmpty)
    }

    @Test func removalDuringAHoldConvergesAndNeverChangesTheOriginalSelection() throws {
        var cycle = try #require(TabSwitcherCycle(originalTabID: "file", tabIDs: ["file", "diff", "browser", "terminal"]))
        #expect(cycle.selectedTabID == "diff")
        let reconciled1 = cycle.reconcile(available: ["file", "browser", "terminal"])
        #expect(reconciled1)
        #expect(cycle.tabIDs == ["file", "browser", "terminal"])
        #expect(cycle.selectedTabID == "browser")
        #expect(cycle.originalTabID == "file")
        let reconciled2 = cycle.reconcile(available: ["terminal"])
        #expect(reconciled2)
        #expect(cycle.selectedTabID == "terminal")
        let reconciled3 = !cycle.reconcile(available: [])
        #expect(reconciled3)
    }

    @Test func disappearingProjectsArePrunedWithoutReorderingAHeldCycle() throws {
        var cycle = try #require(ProjectSwitcherCycle(originalProjectID: "alpha", projectIDs: ["alpha", "beta", "gamma"]))
        let reconciled4 = cycle.reconcile(available: ["alpha", "gamma"])
        #expect(reconciled4)
        #expect(cycle.projectIDs == ["alpha", "gamma"])
        #expect(cycle.selectedProjectID == "gamma")
        #expect(cycle.originalProjectID == "alpha")
    }

    @Test func repeatedInputHasBoundedVisibleRowsAndPreservesEveryStep() throws {
        for count in [2, 9, 10000] {
            let ids = (0..<count).map { "surface-\($0)" }
            var cycle = try #require(TabSwitcherCycle(originalTabID: ids[0], tabIDs: ids))
            for step in 1...20000 {
                #expect(cycle.selectedTabID == ids[step % count])
                #expect(cycle.visibleIDs.count <= 9)
                #expect(cycle.visibleIDs.contains(cycle.selectedTabID))
                cycle.advance()
            }
            #expect(cycle.tabIDs.count == count)
        }
    }

    @Test func unselectedContextDoesNotSkipTheMostRecentCandidate() throws {
        let cycle = try #require(TabSwitcherCycle(originalTabID: nil, tabIDs: ["file", "terminal"]))
        #expect(cycle.selectedTabID == "file")
    }
}
