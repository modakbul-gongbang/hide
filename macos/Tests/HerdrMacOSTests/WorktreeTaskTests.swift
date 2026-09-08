import Testing

@testable import HerdrMacOS

@Suite("Worktree menus")
struct WorktreeMenuTests {
    @Test func projectMenuReplacesNewChatWithNewWorktree() {
        #expect(!WorktreeMenuPolicy.projectItems.contains("New chat here"))
        #expect(WorktreeMenuPolicy.projectItems.contains("New worktree…"))
    }

    @Test func onlyLinkedWorktreesCanStartAgentsHere() {
        #expect(!WorktreeMenuPolicy.checkoutItems(isMain: true).contains("Start agent here"))
        #expect(WorktreeMenuPolicy.checkoutItems(isMain: false).contains("Start agent here"))
    }

    @Test func everyCheckoutCanSetItsBranchAsBase() {
        #expect(WorktreeMenuPolicy.checkoutItems(isMain: true).contains("Set as base branch"))
        #expect(WorktreeMenuPolicy.checkoutItems(isMain: false).contains("Set as base branch"))
    }
}

@Suite("New worktree sheet")
struct WorktreeSheetTests {
    @Test func branchIsRequired() {
        var draft = WorktreeSheetDraft()
        #expect(!draft.canSubmit)
        draft.branch = "   "
        #expect(!draft.canSubmit)
        draft.branch = "feature/topic"
        #expect(draft.canSubmit)
    }

    @Test func successClosesAndFocusesTheCreatedPane() {
        let completion = WorktreeSubmissionPresentation.completion(paneID: "w2:p1")
        #expect(completion.closeSheet)
        #expect(completion.focusPaneID == "w2:p1")
    }
}

@Suite("Project base branch")
struct ProjectBaseBranchPolicyTests {
    @Test func persistedSelectionOverridesTheRepositoryDefaultEverywhere() {
        let selected = ProjectBaseBranchPolicy.selected(
            projectPath: "/repo",
            defaultBranch: "main",
            overrides: ["/repo": "release"]
        )

        #expect(selected == "release")
        #expect(
            ProjectBaseBranchPolicy.orderedBranches(
                ["topic", "main", "release"],
                selectedBase: selected
            ) == ["release", "main", "topic"]
        )
    }

    @Test func repositoryDefaultIsUsedUntilTheUserSelectsABase() {
        #expect(
            ProjectBaseBranchPolicy.selected(
                projectPath: "/repo",
                defaultBranch: "main",
                overrides: [:]
            ) == "main"
        )
    }
}

@Suite("New worktree submission")
struct WorktreeSheetSubmissionTests {
    @Test func workingStateLocksInputsShowsProgressAndRemovesCancel() {
        #expect(WorktreeSubmissionPresentation.isLocked(phase: "working"))
        #expect(!WorktreeSubmissionPresentation.showsCancel(phase: "working"))
        #expect(WorktreeSubmissionPresentation.primaryLabel(phase: "working") == "Creating…")
    }

    @Test func escapeDiscardsEveryDraftValue() {
        var draft = WorktreeSheetDraft(branch: "topic", baseBranch: "main", agent: .codex)
        draft.reset(branches: ["main"], preferredBase: "main")
        #expect(draft == WorktreeSheetDraft(branch: "", baseBranch: "main", agent: nil))
    }

    @Test func anExternalFailureIsRenderedAsExactlyOneLine() {
        #expect(WorktreeSubmissionPresentation.oneLine("git refused\nextra envelope") == "git refused extra envelope")
    }
}
