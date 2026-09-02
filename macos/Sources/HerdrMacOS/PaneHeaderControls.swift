import Foundation

/// Which controls a pane header shows, and what each one says it will do.
///
/// The core decides whether a pane can be forked, because that answer depends
/// on the agent's recorded session, which only the core sees. What is decided
/// here is what the header does with that answer: which controls appear, and
/// what a reader is told before activating one.
enum PaneHeaderControls {
    /// Every pane can be closed. A pane running an agent that is working or
    /// waiting on the operator says so first, because closing it ends that
    /// work; the core enforces the same rule and rejects an unconfirmed close.
    static func closeRequiresConfirmation(paneState: String) -> Bool {
        [
            "working",
            "blocked",
            "question",
            "approval",
            "error",
            "unseen_completion",
        ].contains(paneState)
    }

    /// A fork control appears only where the core says a fork could succeed.
    ///
    /// Drawing it anywhere else would offer an action whose only outcome is an
    /// error, which is worse than not offering it.
    static func showsFork(_ fork: CorePaneFork) -> Bool {
        fork.available
    }

    /// The address a port indicator opens.
    ///
    /// A dev server is reached over http on the loopback name, not by the
    /// address the listener bound to: a server on `*:5173` and one on
    /// `127.0.0.1:5173` are both `http://localhost:5173` from here.
    static func portURL(_ port: UInt16) -> URL? {
        URL(string: "http://localhost:\(port)")
    }

    /// A pane that Herdr's lineage records as spawned from another is marked as
    /// a fork. The mark is the visual encoding of that state; the parent's id
    /// rides along so the header can name it on hover rather than in the row.
    static func forkMark(_ fork: CorePaneFork) -> String? {
        guard let parent = fork.forkedFromPaneID?.trimmingCharacters(in: .whitespacesAndNewlines),
              !parent.isEmpty
        else { return nil }
        return parent
    }
}
