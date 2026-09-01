import Foundation

/// How the sidebar splits agents between the attention list and the space
/// tree. Kept apart from `ShellModel` so both rules can be exercised without
/// a live core runtime.
enum SidebarGrouping {
    /// States in which an agent has stopped and is waiting on the user.
    /// Matches the vocabulary the core publishes in `sidebar.rs`.
    static let attentionStates: Set<String> = [
        "blocked", "question", "approval", "error", "unseen_completion",
    ]

    static func requiresCloseConfirmation(_ state: String) -> Bool {
        state == "working" || attentionStates.contains(state)
    }

    /// Agents that are blocked on the user.
    ///
    /// This is the one thing the space tree cannot answer: it says what is
    /// blocked right now across every space, while the tree says what is in
    /// one space. An agent that is merely working belongs in its space and
    /// not here, so the section disappears whenever nothing is blocked.
    static func needingAttention(_ agents: [SidebarAgent]) -> [SidebarAgent] {
        agents.filter { attentionStates.contains($0.state) }
    }

    /// The agents running in a checkout, found through the panes that checkout
    /// owns. Herdr reports the pane an agent runs in and the checkout already
    /// carries its panes, so no second grouping key is needed.
    static func agents(
        _ agents: [SidebarAgent],
        in checkout: CoreCheckoutSnapshot
    ) -> [SidebarAgent] {
        let paneIDs = Set(checkout.tabs.flatMap(\.panes).map(\.id))
        return agents.filter { paneIDs.contains($0.paneID) }
    }
}
