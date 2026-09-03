import Foundation

/// How the sidebar splits agents between the attention list and the space
/// tree. Kept apart from `ShellModel` so both rules can be exercised without
/// a live core runtime.
enum SidebarGrouping {
    /// Agents that are waiting on the operator.
    ///
    /// This is the one thing the space tree cannot answer: it says what is
    /// waiting right now across every space, while the tree says what is in
    /// one space. Membership is the core's `needs_you` group, decided once in
    /// the projection, so no view keeps a second copy of the vocabulary.
    static func needingAttention(_ agents: [SidebarAgent]) -> [SidebarAgent] {
        agents.filter { $0.group == "needs_you" }
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
