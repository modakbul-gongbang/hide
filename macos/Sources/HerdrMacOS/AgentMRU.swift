import Foundation

struct AgentMRU: Equatable {
    private(set) var paneIDs: [String] = []

    mutating func observe(focusedPaneID: String?, availablePaneIDs: [String]) {
        let availableSet = Set(availablePaneIDs)
        paneIDs.removeAll { !availableSet.contains($0) }
        for paneID in availablePaneIDs where !paneIDs.contains(paneID) {
            paneIDs.append(paneID)
        }
        guard let focusedPaneID, availableSet.contains(focusedPaneID) else { return }
        paneIDs.removeAll { $0 == focusedPaneID }
        paneIDs.insert(focusedPaneID, at: 0)
    }
}

enum AgentSwitcherDirection {
    case forward
    case backward
}

struct AgentSwitcherCycle: Equatable {
    let originalPaneID: String?
    let paneIDs: [String]
    private(set) var selectedIndex: Int

    /// Index 0 is the agent already focused, so opening the switcher skips it:
    /// forward lands on the previous agent, backward on the least recent one.
    init?(
        originalPaneID: String?,
        paneIDs: [String],
        direction: AgentSwitcherDirection = .forward
    ) {
        let unique = paneIDs.reduce(into: [String]()) { result, paneID in
            if !result.contains(paneID) { result.append(paneID) }
        }
        guard unique.count > 1 else { return nil }
        self.originalPaneID = originalPaneID
        self.paneIDs = unique
        selectedIndex = direction == .forward ? 1 : unique.count - 1
    }

    var selectedPaneID: String { paneIDs[selectedIndex] }

    mutating func advance() {
        selectedIndex = (selectedIndex + 1) % paneIDs.count
    }

    /// Option+Shift+Tab walks the cycle the other way, the direction the system
    /// switcher established. Wrapping past the first entry lands on the last.
    mutating func retreat() {
        selectedIndex = (selectedIndex + paneIDs.count - 1) % paneIDs.count
    }

    func committedPaneID(availablePaneIDs: Set<String>) -> String? {
        availablePaneIDs.contains(selectedPaneID) ? selectedPaneID : nil
    }
}
