import Foundation

/// Where a chat starts.
///
/// The two destinations differ only in the directory and the Herdr workspace
/// the first tab is made in; everything after that is the same four steps.
/// Keeping them one type is what makes that true rather than aspirational.
enum ChatDestination: Equatable, Sendable {
    /// The fixed non-project folder. The core decides where it is and puts it
    /// on the snapshot; the composer never computes the path itself.
    case scratch(path: String, workspaceID: String?)
    /// A registered checkout, entered from `Start agent here` or by choosing
    /// the project in the Where chip.
    case checkout(id: String, path: String, workspaceID: String?)

    var path: String {
        switch self {
        case .scratch(let path, _): path
        case .checkout(_, let path, _): path
        }
    }

    /// The live Herdr workspace to add a tab to, or `nil` to create one.
    /// Herdr drops a workspace with its last pane, so `nil` is an ordinary
    /// state rather than a first-run one.
    var workspaceID: String? {
        switch self {
        case .scratch(_, let workspaceID): workspaceID
        case .checkout(_, _, let workspaceID): workspaceID
        }
    }

    /// The checkout this chat belongs to, so the core can anchor its focus.
    /// Scratch has none, which is the whole point of it.
    var checkoutID: String? {
        switch self {
        case .scratch: nil
        case .checkout(let id, _, _): id
        }
    }
}

/// The four steps a submission takes, in order.
///
/// A failure names its step. "Herdr refused the tab", "the agent never became
/// ready", and "the first message was not delivered" are three different
/// problems with three different fixes, and one message for all of them would
/// hide which happened.
enum ChatLaunchStep: String, Equatable, CaseIterable, Sendable {
    case createTab = "create tab"
    case startAgent = "start agent"
    case sendFirstMessage = "send first message"
    case writeTitle = "write title"
}

/// What a submission did.
///
/// `paneID` is present whenever the tab was made, including on failure: the
/// tab stays as a shell prompt the operator can use, and the shell needs its
/// id to focus it.
struct ChatLaunchResult: Equatable, Sendable {
    let succeeded: Bool
    let failedStep: ChatLaunchStep?
    let message: String
    let paneID: String?
}

/// The title a chat carries.
///
/// The first line of the first message, cut to 40 characters. Herdr stores a
/// token value up to 80 characters - characters, not bytes, confirmed against
/// the pinned server - so 40 characters is inside the cap in any script and
/// the cut needs no second, byte-aware rule.
enum ChatTitle {
    static let maximumCharacters = 40
    /// Herdr's pane metadata token that holds it. The core reads the same
    /// name back off `agent.list`.
    static let token = "hide_chat_title"
    /// The `--source` every metadata write from this app carries.
    static let metadataSource = "hide"

    static func fromMessage(_ message: String) -> String? {
        let firstLine = message
            .split(separator: "\n", omittingEmptySubsequences: false)
            .first
            .map(String.init)?
            .trimmingCharacters(in: .whitespaces) ?? ""
        guard !firstLine.isEmpty else { return nil }
        return String(firstLine.prefix(maximumCharacters))
    }
}

/// Whether the composer can send.
///
/// Its own type because three screens' worth of state decide one boolean, and
/// the operator is never asked to work it out: the button is either live or it
/// is not, and the reason is beside it.
enum ChatComposerPolicy {
    static func canSend(
        message: String,
        agentIsInstalled: Bool,
        isSubmitting: Bool
    ) -> Bool {
        guard !isSubmitting, agentIsInstalled else { return false }
        return !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

/// What a submission does before any command runs.
///
/// The remote refusal is the reason this is its own decision: `Run on` may
/// name a device Hide cannot start an agent on, and the answer has to be
/// "nothing was created and here is why" rather than a half-made tab. Deciding
/// it here rather than inside the launcher is what lets that be a test.
enum ChatSubmissionRoute: Equatable {
    case start(ChatDestination)
    case refuse(String)
}

enum ChatSubmissionRouting {
    /// The notice a remote `Run on` gets. Unchanged from the wording the
    /// retired sheet used, because the limit itself has not changed.
    static let remoteRefusal =
        "Remote agent start is delegated to the remote Herdr session in v1. Connect that device first."
    static let scratchUnknown =
        "Hide does not know where Scratch lives yet. Wait for the first Herdr connection and retry."

    static func route(
        deviceID: String,
        checkout: ChatDestination?,
        scratchPath: String,
        scratchWorkspaceID: String?
    ) -> ChatSubmissionRoute {
        guard deviceID == "local" else { return .refuse(remoteRefusal) }
        if let checkout { return .start(checkout) }
        guard !scratchPath.isEmpty else { return .refuse(scratchUnknown) }
        return .start(.scratch(path: scratchPath, workspaceID: scratchWorkspaceID))
    }
}

/// The Herdr arguments for the message step.
enum AgentPromptArguments {
    /// The pane id is the target. It is unique per launch, while the agent
    /// name Hide starts every agent under is not: two chats would both answer
    /// to `hide-claude` and the prompt would reach whichever Herdr resolved
    /// first. The pinned server resolves a pane id as a target - a pane with
    /// no agent is refused as `not_agent_backed` rather than `agent_not_found`.
    static func build(paneID: String, message: String) -> [String] {
        ["agent", "prompt", paneID, message]
    }
}

/// The Herdr arguments for the title step.
enum PaneTitleTokenArguments {
    static func build(paneID: String, title: String) -> [String] {
        [
            "pane", "report-metadata", paneID,
            "--source", ChatTitle.metadataSource,
            "--token", "\(ChatTitle.token)=\(title)",
        ]
    }
}

/// One submission's plan: every command, in order, before any of them runs.
///
/// Built as data so the order and the arguments can be asserted without a
/// Herdr server, which is what makes "the message is sent only after the agent
/// is ready" a test rather than a reading of the launcher.
enum ChatLaunchPlan {
    static func steps(
        destination: ChatDestination,
        provider: AgentProvider,
        message: String,
        bypassWarnings: Bool,
        paneID: String
    ) -> [(step: ChatLaunchStep, arguments: [String])] {
        var steps: [(step: ChatLaunchStep, arguments: [String])] = [
            (
                .createTab,
                AgentRootPaneArguments.build(
                    workspaceID: destination.workspaceID,
                    cwd: destination.path,
                    agent: provider.rawValue
                )
            ),
            (
                .startAgent,
                AgentLaunchArguments.build(
                    provider: provider,
                    paneID: paneID,
                    bypassWarnings: bypassWarnings
                )
            ),
            (
                .sendFirstMessage,
                AgentPromptArguments.build(paneID: paneID, message: message)
            ),
        ]
        if let title = ChatTitle.fromMessage(message) {
            steps.append(
                (.writeTitle, PaneTitleTokenArguments.build(paneID: paneID, title: title))
            )
        }
        return steps
    }
}
