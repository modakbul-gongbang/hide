import Foundation
import Testing

@testable import HerdrMacOS

/// What a submission does when a step fails.
///
/// Every case is a failure that must not read as success. A silent failure
/// here means an operator watching a pane that will never answer, with no
/// notice and no way to tell which of four steps went wrong.
@Suite("Chat launch failures")
struct ChatLaunchFailureTests {
    /// A fake Herdr: it answers the tab step with a real root pane, and fails
    /// whichever later step the case is about.
    private final class Herdr: @unchecked Sendable {
        private(set) var calls: [[String]] = []
        private let failingStep: String?

        init(failingStep: String? = nil) {
            self.failingStep = failingStep
        }

        var runner: HerdrCommandRunner {
            { [self] arguments in
                calls.append(arguments)
                if let failingStep, arguments.joined(separator: " ").contains(failingStep) {
                    return HerdrCommandResult(
                        status: 1,
                        output: Data(),
                        error: Data("herdr refused it".utf8)
                    )
                }
                if arguments.first == "tab" || arguments.first == "workspace" {
                    return HerdrCommandResult(
                        status: 0,
                        output: Data(#"{"result":{"root_pane":{"pane_id":"s1:p9"}}}"#.utf8),
                        error: Data()
                    )
                }
                return HerdrCommandResult(status: 0, output: Data(), error: Data())
            }
        }
    }

    private func start(
        herdr: Herdr,
        agentIsInstalled: Bool = true
    ) -> ChatLaunchResult {
        HerdrChatLauncher.start(
            destination: .scratch(path: "/scratch", workspaceID: "s1"),
            provider: .claude,
            message: "build me a parser",
            bypassWarnings: false,
            agentIsInstalled: agentIsInstalled,
            run: herdr.runner
        )
    }

    @Test func aSuccessfulSubmissionRunsAllFourStepsAndReportsThePane() {
        let herdr = Herdr()
        let result = start(herdr: herdr)

        #expect(result.succeeded)
        #expect(result.failedStep == nil)
        #expect(result.paneID == "s1:p9")
        #expect(herdr.calls.count == 4)
        #expect(herdr.calls[0].first == "tab")
        #expect(herdr.calls[1].prefix(2) == ["agent", "start"])
        #expect(herdr.calls[2].prefix(2) == ["agent", "prompt"])
        #expect(herdr.calls[3].prefix(2) == ["pane", "report-metadata"])
    }

    /// AC3: a failed agent start leaves the tab and names its step. The pane
    /// stays because it is a shell prompt the operator can still type in.
    @Test func aFailedAgentStartKeepsThePaneAndNamesItsStep() {
        let herdr = Herdr(failingStep: "agent start")
        let result = start(herdr: herdr)

        #expect(!result.succeeded)
        #expect(result.failedStep == .startAgent)
        #expect(result.paneID == "s1:p9")
        #expect(result.message.contains("start agent"))
        #expect(result.message.contains("herdr refused it"))
        // Nothing after the failure ran: no message was sent into a pane with
        // no agent, and no title was written for a chat that never started.
        #expect(herdr.calls.count == 2)
    }

    /// AC3: the same for a first message that was not delivered.
    @Test func aFailedFirstMessageKeepsThePaneAndNamesItsStep() {
        let herdr = Herdr(failingStep: "agent prompt")
        let result = start(herdr: herdr)

        #expect(!result.succeeded)
        #expect(result.failedStep == .sendFirstMessage)
        #expect(result.paneID == "s1:p9")
        #expect(result.message.contains("send first message"))
        #expect(herdr.calls.count == 3)
    }

    /// AC3: a tab that was never made leaves nothing behind.
    @Test func aFailedTabCreationLeavesNothing() {
        let herdr = Herdr(failingStep: "tab create")
        let result = start(herdr: herdr)

        #expect(!result.succeeded)
        #expect(result.failedStep == .createTab)
        #expect(result.paneID == nil)
        #expect(herdr.calls.count == 1)
    }

    /// A tab step that exits zero but names no pane is a failure, not a
    /// success with a missing pane. Reading it as success would send the next
    /// three commands at an empty target.
    @Test func aTabResponseWithoutARootPaneIsAFailure() {
        final class NoPane: @unchecked Sendable {
            var runner: HerdrCommandRunner {
                { _ in HerdrCommandResult(status: 0, output: Data("{}".utf8), error: Data()) }
            }
        }
        let result = HerdrChatLauncher.start(
            destination: .scratch(path: "/scratch", workspaceID: "s1"),
            provider: .claude,
            message: "hello",
            bypassWarnings: false,
            agentIsInstalled: true,
            run: NoPane().runner
        )
        #expect(!result.succeeded)
        #expect(result.failedStep == .createTab)
        #expect(result.paneID == nil)
    }

    /// An agent CLI that is not installed is refused before anything runs.
    @Test func aMissingAgentCliRunsNoCommands() {
        let herdr = Herdr()
        let result = start(herdr: herdr, agentIsInstalled: false)

        #expect(!result.succeeded)
        #expect(herdr.calls.isEmpty)
    }

    /// AC3: a remote `Run on` creates nothing and returns the existing
    /// refusal. This is decided before the launcher is reached, so there is no
    /// command to fail partway.
    @Test func aRemoteDeviceSubmissionRefusesBeforeAnyCommand() {
        let route = ChatSubmissionRouting.route(
            deviceID: "device:mini",
            checkout: nil,
            scratchPath: "/scratch",
            scratchWorkspaceID: "s1"
        )
        #expect(route == .refuse(ChatSubmissionRouting.remoteRefusal))
    }

    @Test func aLocalSubmissionWithNoCheckoutGoesToScratch() {
        let route = ChatSubmissionRouting.route(
            deviceID: "local",
            checkout: nil,
            scratchPath: "/scratch",
            scratchWorkspaceID: "s1"
        )
        #expect(route == .start(.scratch(path: "/scratch", workspaceID: "s1")))
    }

    @Test func aLocalSubmissionWithACheckoutGoesToThatCheckout() {
        let destination = ChatDestination.checkout(
            id: "checkout-1",
            path: "/repo",
            workspaceID: "w7"
        )
        let route = ChatSubmissionRouting.route(
            deviceID: "local",
            checkout: destination,
            scratchPath: "/scratch",
            scratchWorkspaceID: "s1"
        )
        #expect(route == .start(destination))
    }

    /// A Scratch path the core has not published yet is refused rather than
    /// used as an empty working directory.
    @Test func anUnknownScratchPathIsRefusedRatherThanUsedEmpty() {
        let route = ChatSubmissionRouting.route(
            deviceID: "local",
            checkout: nil,
            scratchPath: "",
            scratchWorkspaceID: nil
        )
        #expect(route == .refuse(ChatSubmissionRouting.scratchUnknown))
    }
}
