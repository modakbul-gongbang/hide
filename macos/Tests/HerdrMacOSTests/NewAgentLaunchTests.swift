import Testing

@testable import HerdrMacOS

@Suite("New Agent launch")
struct NewAgentLaunchTests {
    @Test func supportedProvidersAreExactlyClaudeAndCodex() {
        #expect(NewAgentProvider.allCases.map(\.rawValue) == ["claude", "codex"])
    }

    @Test func claudeArgumentsApplyBypassOnlyToRequestedLaunch() {
        var draft = NewAgentDraft.fresh(
            selectedKind: "claude",
            selectedCheckoutID: "checkout-1",
            focusedCheckoutID: nil,
            selectedDeviceID: "local"
        )
        draft.bypassWarnings = true
        let firstBypass = draft.consumeBypassWarnings()

        #expect(
            AgentLaunchArguments.build(
                provider: .claude,
                paneID: "w1:p1",
                bypassWarnings: firstBypass
            ) == baseArguments(agent: "claude") + [
                "--", "--dangerously-skip-permissions",
            ]
        )
        let secondBypass = draft.consumeBypassWarnings()
        #expect(
            AgentLaunchArguments.build(
                provider: .claude,
                paneID: "w1:p1",
                bypassWarnings: secondBypass
            ) == baseArguments(agent: "claude")
        )
    }

    @Test func codexArgumentsUseItsExactProviderFlag() {
        #expect(
            AgentLaunchArguments.build(
                provider: .codex,
                paneID: "w1:p1",
                bypassWarnings: true
            ) == baseArguments(agent: "codex") + [
                "--", "--dangerously-bypass-approvals-and-sandbox",
            ]
        )
    }

    @Test func everyFreshModalDraftStartsWithBypassOff() {
        var first = NewAgentDraft.fresh(
            selectedKind: "codex",
            selectedCheckoutID: nil,
            focusedCheckoutID: "focused-checkout",
            selectedDeviceID: "local"
        )
        #expect(!first.bypassWarnings)
        #expect(first.selectedCheckoutID == "focused-checkout")

        first.bypassWarnings = true
        let consumedBypass = first.consumeBypassWarnings()
        #expect(consumedBypass)
        #expect(!first.bypassWarnings)

        let reopened = NewAgentDraft.fresh(
            selectedKind: first.selectedKind,
            selectedCheckoutID: first.selectedCheckoutID,
            focusedCheckoutID: nil,
            selectedDeviceID: first.selectedDeviceID
        )
        #expect(!reopened.bypassWarnings)
    }

    @Test func anExistingProjectAlwaysGetsAFocusedNewTab() {
        #expect(
            AgentRootPaneArguments.build(
                workspaceID: "w1",
                checkoutPath: "/tmp/project",
                agent: "codex"
            ) == [
                "tab", "create",
                "--workspace", "w1",
                "--cwd", "/tmp/project",
                "--label", "hide codex",
                "--focus",
            ]
        )
    }

    @Test func aProjectWithoutAHerdrWorkspaceGetsAFocusedRootPane() {
        #expect(
            AgentRootPaneArguments.build(
                workspaceID: nil,
                checkoutPath: "/tmp/project",
                agent: "claude"
            ) == [
                "workspace", "create",
                "--cwd", "/tmp/project",
                "--label", "hide claude",
                "--focus",
            ]
        )
    }

    @Test func liveWorkspaceIdentityComesFromTheSelectedCheckoutsHerdrTab() {
        let tab = CoreTabSnapshot(
            id: "w3:t1",
            workspaceID: "workspace:catalog-id",
            checkoutID: "checkout-1",
            label: "Tab 1",
            empty: false,
            panes: []
        )

        #expect(HerdrLiveWorkspaceIdentity.workspaceID(for: [tab]) == "w3")
    }

    @Test func checkoutWithoutALiveHerdrTabDoesNotInventAWorkspaceIdentity() {
        let fileOnlyPlaceholder = CoreTabSnapshot(
            id: nil,
            workspaceID: "workspace:catalog-id",
            checkoutID: "checkout-1",
            label: nil,
            empty: true,
            panes: []
        )

        #expect(HerdrLiveWorkspaceIdentity.workspaceID(for: [fileOnlyPlaceholder]) == nil)
        #expect(HerdrLiveWorkspaceIdentity.workspaceID(fromTabID: "w3:terminal") == nil)
    }

    private func baseArguments(agent: String) -> [String] {
        [
            "agent", "start", "hide-\(agent)",
            "--kind", agent,
            "--pane", "w1:p1",
        ]
    }
}
