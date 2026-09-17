import Foundation

/// The one relationship navigation intent currently awaiting Herdr.
///
/// It belongs to the shell model rather than an individual sheet so a sheet,
/// a parent Return control, and the retained source canvas cannot disagree
/// after the visible tab starts moving (PRD B24).
struct PaneSelectionOperation: Equatable {
    enum Phase: Equatable {
        case pending
        case failed(reason: String, retryable: Bool)
    }

    let requestID: String
    let sourcePaneID: String
    let targetPaneID: String
    let targetLabel: String
    let phase: Phase

    var isPending: Bool { phase == .pending }

    func isFor(sourcePaneID: String, targetPaneID: String) -> Bool {
        self.sourcePaneID == sourcePaneID && self.targetPaneID == targetPaneID
    }
}

enum PaneSelectionStart: Equatable {
    case unchanged(PaneSelectionOperation)
    case dispatch(PaneSelectionOperation)
    case failed(PaneSelectionOperation)
}

enum PaneSelectionResolution: Equatable {
    case pending
    case succeeded
    case failed(reason: String, retryable: Bool)
}

/// Pure decisions for the B24 pending, unavailable, failure, and retry states.
/// The shell performs the returned effect; the policy never dispatches one.
enum PaneSelectionPolicy {
    static func start(
        current: PaneSelectionOperation?,
        sourcePaneID: String,
        targetPaneID: String,
        targetLabel: String,
        availablePaneIDs: Set<String>
    ) -> PaneSelectionStart {
        if let current, current.isPending {
            return .unchanged(current)
        }
        guard availablePaneIDs.contains(targetPaneID) else {
            return .failed(PaneSelectionOperation(
                requestID: UUID().uuidString,
                sourcePaneID: sourcePaneID,
                targetPaneID: targetPaneID,
                targetLabel: targetLabel,
                phase: .failed(
                    reason: "\(targetLabel) is no longer available.",
                    retryable: true
                )
            ))
        }
        return .dispatch(PaneSelectionOperation(
            requestID: UUID().uuidString,
            sourcePaneID: sourcePaneID,
            targetPaneID: targetPaneID,
            targetLabel: targetLabel,
            phase: .pending
        ))
    }

    static func resolve(
        _ operation: PaneSelectionOperation,
        outcome: CorePaneFocusRequest?
    ) -> PaneSelectionResolution {
        guard operation.isPending else { return .pending }
        guard let outcome, outcome.requestID == operation.requestID else {
            return .pending
        }
        guard outcome.targetPaneID == operation.targetPaneID else {
            return .failed(
                reason: "Hide received a pane-focus result for a different target.",
                retryable: false
            )
        }
        switch outcome.phase {
        case "pending":
            return .pending
        case "succeeded":
            return .succeeded
        case "failed":
            return .failed(
                reason: outcome.message ?? "Could not open \(operation.targetLabel).",
                retryable: outcome.retryable
            )
        default:
            return .failed(
                reason: "Hide received an unknown pane-focus outcome: \(outcome.phase).",
                retryable: false
            )
        }
    }
}
