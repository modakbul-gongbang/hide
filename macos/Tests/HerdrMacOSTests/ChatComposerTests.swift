import Testing

@testable import HerdrMacOS

/// The composer's submission pipeline and its Send rule.
///
/// Every case here is a regression the feature can silently lose: a message
/// sent before the agent is ready, a step order that quietly reverses, a
/// bypass flag that goes missing or appears uninvited, a Send button that
/// lights up on an empty box or a second time during a launch.
@Suite("Chat composer")
struct ChatComposerTests {
    @Test func supportedProvidersAreExactlyClaudeAndCodex() {
        #expect(AgentProvider.allCases.map(\.rawValue) == ["claude", "codex"])
    }

    // MARK: Send rule

    @Test func sendIsOffForAnEmptyOrBlankMessage() {
        #expect(
            !ChatComposerPolicy.canSend(
                message: "",
                agentIsInstalled: true,
                isSubmitting: false
            )
        )
        #expect(
            !ChatComposerPolicy.canSend(
                message: "   \n\t ",
                agentIsInstalled: true,
                isSubmitting: false
            )
        )
    }

    @Test func oneCharacterIsEnoughToSend() {
        #expect(
            ChatComposerPolicy.canSend(
                message: "?",
                agentIsInstalled: true,
                isSubmitting: false
            )
        )
    }

    @Test func sendIsOffWhenTheChosenAgentIsNotInstalled() {
        #expect(
            !ChatComposerPolicy.canSend(
                message: "build me a parser",
                agentIsInstalled: false,
                isSubmitting: false
            )
        )
    }

    /// The defect this blocks: a second `⌘↩` while the first launch is still
    /// running, which would open two tabs for one question.
    @Test func sendIsOffWhileASubmissionIsInFlight() {
        #expect(
            !ChatComposerPolicy.canSend(
                message: "build me a parser",
                agentIsInstalled: true,
                isSubmitting: true
            )
        )
    }

    // MARK: Title

    @Test func theTitleIsTheFirstLineCutToFortyCharacters() {
        #expect(ChatTitle.fromMessage("  hello there  \nsecond") == "hello there")
        #expect(ChatTitle.fromMessage(String(repeating: "a", count: 60))
            == String(repeating: "a", count: 40))
    }

    /// Forty characters, not forty bytes. Herdr's own cap is 80 characters,
    /// so a Korean title stays forty readable characters rather than being
    /// cut to thirteen by a byte rule.
    @Test func aKoreanTitleIsCutByCharacters() {
        let title = ChatTitle.fromMessage(String(repeating: "가", count: 60))
        #expect(title?.count == 40)
    }

    @Test func aBlankMessageEarnsNoTitle() {
        #expect(ChatTitle.fromMessage("  \n ") == nil)
    }

    // MARK: Failure text

    /// R3: the notice carries the step and the reason. The reason Herdr gives
    /// arrives as a JSON envelope, and the live run put the whole envelope in
    /// front of the operator; the sentence inside it is the reason.
    @Test func theCliErrorEnvelopeIsReadDownToItsSentence() {
        let envelope = #"{"error":{"code":"agent_pane_busy","message":"agent target pane w2:p7 is not an available shell"},"id":"cli:agent:start"}"#
        #expect(
            HerdrErrorEnvelope.message(in: envelope)
                == "agent target pane w2:p7 is not an available shell"
        )
    }

    /// Anything that is not one of those envelopes is the CLI talking in
    /// prose, and passing it through unchanged is what keeps a reason the
    /// shape it was written in.
    @Test func textThatIsNotAnEnvelopeIsLeftAlone() {
        #expect(HerdrErrorEnvelope.message(in: "herdr: command not found") == nil)
        #expect(HerdrErrorEnvelope.message(in: #"{"error":"#) == nil)
        #expect(HerdrErrorEnvelope.message(in: #"{"error":{"code":"x"}}"#) == nil)
        #expect(HerdrErrorEnvelope.message(in: "") == nil)
    }

    // MARK: Step order

    /// AC3: the tab is made, then the agent starts, then the first message is
    /// sent, then the title is written. The message step sitting after the
    /// start step is the whole reason the order is asserted: an agent still on
    /// its trust screen swallows anything sent before it is ready.
    @Test func checkoutSubmissionRunsTheFourStepsInOrder() {
        let plan = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
            provider: .claude,
            message: "build me a parser",
            bypassWarnings: false,
            paneID: "s1:p3"
        )

        #expect(plan.map(\.step) == [.createTab, .startAgent, .sendFirstMessage, .writeTitle])
        #expect(plan[0].arguments == [
            "tab", "create",
            "--workspace", "s1",
            "--cwd", "/repo",
            "--label", "hide claude",
            "--focus",
        ])
        #expect(plan[1].arguments == [
            "agent", "start", "hide-claude",
            "--kind", "claude",
            "--pane", "s1:p3",
        ])
        #expect(plan[2].arguments == [
            "agent", "prompt", "s1:p3", "build me a parser",
            "--wait",
            "--timeout", "120000",
        ])
        #expect(plan[3].arguments == [
            "pane", "report-metadata", "s1:p3",
            "--source", "hide",
            "--token", "hide_chat_title=build me a parser",
        ])
    }

    /// AC3: the message crosses verbatim. A pipeline that trimmed, quoted, or
    /// reflowed it would change what the operator asked.
    @Test func theFirstMessageIsSentExactlyAsTyped() {
        let message = "line one\nline two  "
        let plan = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: nil),
            provider: .claude,
            message: message,
            bypassWarnings: false,
            paneID: "s1:p1"
        )
        let prompt = plan.first { $0.step == .sendFirstMessage }
        #expect(prompt?.arguments.count == 7)
        #expect(prompt?.arguments.dropFirst(3).first == message)
        // The title is still the trimmed first line.
        let title = plan.first { $0.step == .writeTitle }
        #expect(title?.arguments.last == "hide_chat_title=line one")
    }

    /// A checkout with no live Herdr workspace creates one instead of adding a
    /// tab to nothing. Herdr drops a workspace with its last pane, so this is
    /// an ordinary state rather than a first-run one.
    @Test func checkoutWithoutALiveWorkspaceCreatesOne() {
        let plan = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: nil),
            provider: .claude,
            message: "hello",
            bypassWarnings: false,
            paneID: "s2:p1"
        )
        #expect(plan[0].arguments == [
            "workspace", "create",
            "--cwd", "/repo",
            "--label", "hide claude",
            "--focus",
        ])
    }

    /// AC8/SC2: a project submission carries that checkout's directory and
    /// that checkout's live Herdr workspace, and takes the same four steps.
    @Test func aProjectSubmissionCarriesTheCheckoutDirectoryAndWorkspace() {
        let plan = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(
                id: "checkout-1",
                path: "/repo/feature",
                workspaceID: "w7"
            ),
            provider: .codex,
            message: "review this diff",
            bypassWarnings: false,
            paneID: "w7:p2"
        )

        #expect(plan.map(\.step) == [.createTab, .startAgent, .sendFirstMessage, .writeTitle])
        #expect(plan[0].arguments.contains("--workspace"))
        #expect(plan[0].arguments.contains("w7"))
        #expect(plan[0].arguments.contains("--cwd"))
        #expect(plan[0].arguments.contains("/repo/feature"))
    }

    @Test func aProjectWithoutALiveHerdrWorkspaceCreatesOne() {
        let plan = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo/feature", workspaceID: nil),
            provider: .claude,
            message: "hello",
            bypassWarnings: false,
            paneID: "w9:p1"
        )
        #expect(plan[0].arguments == [
            "workspace", "create",
            "--cwd", "/repo/feature",
            "--label", "hide claude",
            "--focus",
        ])
    }

    // MARK: Bypass

    /// AC9: on means each provider's own flag, off means neither. The defect
    /// this blocks is a flag that survives into a launch the operator turned
    /// it off for, or one that goes missing from a launch they turned it on
    /// for.
    @Test func bypassAddsEachProvidersOwnFlagAndNothingWhenOff() {
        let claudeOn = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
            provider: .claude,
            message: "hello",
            bypassWarnings: true,
            paneID: "s1:p1"
        )
        #expect(claudeOn[1].arguments.suffix(2) == ["--", "--dangerously-skip-permissions"])

        let codexOn = ChatLaunchPlan.steps(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
            provider: .codex,
            message: "hello",
            bypassWarnings: true,
            paneID: "s1:p1"
        )
        #expect(
            codexOn[1].arguments.suffix(2)
                == ["--", "--dangerously-bypass-approvals-and-sandbox"]
        )

        for provider in AgentProvider.allCases {
            let off = ChatLaunchPlan.steps(
                destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
                provider: provider,
                message: "hello",
                bypassWarnings: false,
                paneID: "s1:p1"
            )
            #expect(!off[1].arguments.contains("--"))
            #expect(!off[1].arguments.contains(provider.bypassFlag))
        }
    }

    // MARK: Destination

    @Test func onlyACheckoutDestinationCarriesACheckoutIdentity() {
        #expect(ChatDestination.scratch(path: "/scratch", workspaceID: "s1").checkoutID == nil)
        #expect(
            ChatDestination.checkout(id: "checkout-1", path: "/repo", workspaceID: nil)
                .checkoutID == "checkout-1"
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
}
