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

    /// A process-boundary failure that happens once, then lets the exact same
    /// checkout action succeed on retry. The checkout directory is owned by
    /// the caller and is deliberately never removed by this fake.
    private final class RetryOnceHerdr: @unchecked Sendable {
        private(set) var calls: [[String]] = []
        private let failingStep: String
        private var failuresRemaining = 1
        private var nextPane = 9

        init(failingStep: String) {
            self.failingStep = failingStep
        }

        var runner: HerdrCommandRunner {
            { [self] arguments in
                calls.append(arguments)
                if failuresRemaining > 0,
                   arguments.joined(separator: " ").contains(failingStep)
                {
                    failuresRemaining -= 1
                    return HerdrCommandResult(
                        status: 1,
                        output: Data(),
                        error: Data("injected \(failingStep) failure".utf8)
                    )
                }
                if arguments.first == "tab" || arguments.first == "workspace" {
                    defer { nextPane += 1 }
                    return HerdrCommandResult(
                        status: 0,
                        output: Data(#"{"result":{"root_pane":{"pane_id":"s1:p\#(nextPane)"}}}"#.utf8),
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
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
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

    @Test func paneCreationFailureKeepsTheCheckoutAndRetryStartsTheAgent() throws {
        let checkout = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-pane-retry-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: checkout, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: checkout) }
        let destination = CheckoutChatDestination(
            id: "checkout-1",
            path: checkout.path,
            workspaceID: "s1"
        )
        let herdr = RetryOnceHerdr(failingStep: "tab create")

        let failed = HerdrChatLauncher.start(
            destination: destination,
            provider: .claude,
            message: "retry me",
            bypassWarnings: false,
            agentIsInstalled: true,
            run: herdr.runner
        )
        #expect(!failed.succeeded)
        #expect(failed.failedStep == .createTab)
        #expect(failed.message.contains("create tab"))
        #expect(FileManager.default.fileExists(atPath: checkout.path))

        let retried = HerdrChatLauncher.start(
            destination: destination,
            provider: .claude,
            message: "retry me",
            bypassWarnings: false,
            agentIsInstalled: true,
            run: herdr.runner
        )
        #expect(retried.succeeded)
        #expect(retried.paneID == "s1:p9")
        #expect(FileManager.default.fileExists(atPath: checkout.path))
        #expect(herdr.calls.count == 5)
    }

    @Test func agentStartFailureKeepsTheCheckoutAndRetryStartsTheAgent() throws {
        let checkout = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-agent-retry-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: checkout, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: checkout) }
        let destination = CheckoutChatDestination(
            id: "checkout-1",
            path: checkout.path,
            workspaceID: "s1"
        )
        let herdr = RetryOnceHerdr(failingStep: "agent start")

        let failed = HerdrChatLauncher.start(
            destination: destination,
            provider: .claude,
            message: "retry me",
            bypassWarnings: false,
            agentIsInstalled: true,
            run: herdr.runner
        )
        #expect(!failed.succeeded)
        #expect(failed.failedStep == .startAgent)
        #expect(failed.message.contains("start agent"))
        #expect(failed.paneID == "s1:p9")
        #expect(FileManager.default.fileExists(atPath: checkout.path))

        let retried = HerdrChatLauncher.start(
            destination: destination,
            provider: .claude,
            message: "retry me",
            bypassWarnings: false,
            agentIsInstalled: true,
            run: herdr.runner
        )
        #expect(retried.succeeded)
        #expect(retried.paneID == "s1:p10")
        #expect(FileManager.default.fileExists(atPath: checkout.path))
        #expect(herdr.calls.count == 6)
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

    /// B33/B35: Herdr without `--wait` acknowledges terminal writes even when
    /// the agent never begins a turn. The launcher must request the observed
    /// lifecycle transition so that non-delivery is a named late failure,
    /// never a successful chat with an empty agent prompt.
    @Test func anAcknowledgedWriteWithoutDeliveryIsNotReportedAsSuccess() {
        final class NonDeliveringHerdr: @unchecked Sendable {
            var runner: HerdrCommandRunner {
                { arguments in
                    if arguments.first == "tab" || arguments.first == "workspace" {
                        return HerdrCommandResult(
                            status: 0,
                            output: Data(#"{"result":{"root_pane":{"pane_id":"s1:p9"}}}"#.utf8),
                            error: Data()
                        )
                    }
                    guard arguments.prefix(2) == ["agent", "prompt"] else {
                        return HerdrCommandResult(status: 0, output: Data(), error: Data())
                    }
                    guard arguments.contains("--wait") else {
                        // This is the pinned CLI's write acknowledgement. It
                        // says nothing about whether the agent began a turn.
                        return HerdrCommandResult(status: 0, output: Data(), error: Data())
                    }
                    return HerdrCommandResult(
                        status: 1,
                        output: Data(),
                        error: Data(
                            #"{"error":{"code":"agent_prompt_stalled","message":"agent did not begin working after prompt submission"},"id":"cli:agent:prompt"}"#.utf8
                        )
                    )
                }
            }
        }

        let result = HerdrChatLauncher.start(
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
            provider: .codex,
            message: "Reply exactly OK.",
            bypassWarnings: false,
            agentIsInstalled: true,
            run: NonDeliveringHerdr().runner
        )

        #expect(!result.succeeded)
        #expect(result.failedStep == .sendFirstMessage)
        #expect(result.paneID == "s1:p9")
        #expect(result.message.contains("send first message"))
        #expect(result.message.contains("agent did not begin working"))
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
            destination: CheckoutChatDestination(id: "checkout-1", path: "/repo", workspaceID: "s1"),
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
