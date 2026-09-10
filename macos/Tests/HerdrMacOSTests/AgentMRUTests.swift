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

@Suite("Surface MRU and switcher")
struct SurfaceMRUTests {
    @Test func recencySpansEveryProject() {
        var mru = SurfaceMRU()

        mru.observe(
            focusedSurfaceID: "tab-a2",
            availableSurfaceIDs: ["tab-a1", "tab-a2", "tab-b1", "tab-b2"]
        )
        #expect(mru.surfaceIDs == ["tab-a2", "tab-a1", "tab-b1", "tab-b2"])

        mru.observe(
            focusedSurfaceID: "tab-b1",
            availableSurfaceIDs: ["tab-a1", "tab-a2", "tab-b1", "tab-b2"]
        )
        #expect(mru.surfaceIDs == ["tab-b1", "tab-a2", "tab-a1", "tab-b2"])
    }

    @Test func removedTabsCannotRemainInTheCycleOrCommit() throws {
        var mru = SurfaceMRU()
        mru.observe(
            focusedSurfaceID: "tab-a1",
            availableSurfaceIDs: ["tab-a1", "tab-a2", "tab-a3"]
        )
        mru.observe(
            focusedSurfaceID: "tab-a3",
            availableSurfaceIDs: ["tab-a1", "tab-a3"]
        )
        #expect(mru.surfaceIDs == ["tab-a3", "tab-a1"])

        var cycle = try #require(TabSwitcherCycle(
            originalTabID: "tab-a3",
            tabIDs: mru.surfaceIDs
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
    @Test func visitingAnotherProjectKeepsOneOrderOverEverySurface() throws {
        var tabs = SurfaceMRU()
        let all = ["terminal-main", "file-main", "diff-worktree", "browser-worktree", "beta-file"]
        tabs.observe(focusedSurfaceID: "file-main", availableSurfaceIDs: all)
        tabs.observe(focusedSurfaceID: "browser-worktree", availableSurfaceIDs: all)
        // The other project's visit takes the front of the same order rather
        // than starting a history of its own.
        tabs.observe(focusedSurfaceID: "beta-file", availableSurfaceIDs: all)
        #expect(tabs.surfaceIDs == ["beta-file", "browser-worktree", "file-main", "terminal-main", "diff-worktree"])
        // A background snapshot with no focus must not reset that history to
        // strip order or checkout order.
        tabs.observe(focusedSurfaceID: nil, availableSurfaceIDs: all)
        #expect(tabs.surfaceIDs == ["beta-file", "browser-worktree", "file-main", "terminal-main", "diff-worktree"])
        var cycle = try #require(TabSwitcherCycle(originalTabID: "beta-file", tabIDs: tabs.surfaceIDs))
        #expect(cycle.selectedTabID == "browser-worktree")
        cycle.advance()
        #expect(cycle.selectedTabID == "file-main")
        cycle.advance()
        #expect(cycle.selectedTabID == "terminal-main")
        tabs.observe(focusedSurfaceID: nil, availableSurfaceIDs: ["beta-file"])
        #expect(tabs.surfaceIDs == ["beta-file"])
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
