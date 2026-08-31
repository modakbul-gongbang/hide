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
                checkoutPath: "/tmp/project",
                idempotencyKey: "launch-1",
                bypassWarnings: firstBypass
            ) == baseArguments(agent: "claude", idempotencyKey: "launch-1") + [
                "--", "--dangerously-skip-permissions",
            ]
        )
        let secondBypass = draft.consumeBypassWarnings()
        #expect(
            AgentLaunchArguments.build(
                provider: .claude,
                paneID: "w1:p1",
                checkoutPath: "/tmp/project",
                idempotencyKey: "launch-2",
                bypassWarnings: secondBypass
            ) == baseArguments(agent: "claude", idempotencyKey: "launch-2")
        )
    }

    @Test func codexArgumentsUseItsExactProviderFlag() {
        #expect(
            AgentLaunchArguments.build(
                provider: .codex,
                paneID: "w1:p1",
                checkoutPath: "/tmp/project",
                idempotencyKey: "launch-3",
                bypassWarnings: true
            ) == baseArguments(agent: "codex", idempotencyKey: "launch-3") + [
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

    private func baseArguments(agent: String, idempotencyKey: String) -> [String] {
        [
            "agent", "new", "hide-\(agent)",
            "--kind", agent,
            "--pane", "w1:p1",
            "--idempotency-key", idempotencyKey,
            "--cwd", "/tmp/project",
            "--no-focus",
        ]
    }
}
