import Foundation

struct AgentMRU: Equatable {
    private(set) var paneIDs: [String] = []

    mutating func observe(focusedPaneID: String?, availablePaneIDs: Set<String>) {
        paneIDs.removeAll { !availablePaneIDs.contains($0) }
        guard let focusedPaneID, availablePaneIDs.contains(focusedPaneID) else { return }
        paneIDs.removeAll { $0 == focusedPaneID }
        paneIDs.insert(focusedPaneID, at: 0)
    }
}

struct AgentSwitcherCycle: Equatable {
    let originalPaneID: String?
    let paneIDs: [String]
    private(set) var selectedIndex: Int

    init?(originalPaneID: String?, paneIDs: [String]) {
        let unique = paneIDs.reduce(into: [String]()) { result, paneID in
            if !result.contains(paneID) { result.append(paneID) }
        }
        guard unique.count > 1 else { return nil }
        self.originalPaneID = originalPaneID
        self.paneIDs = unique
        selectedIndex = 1
    }

    var selectedPaneID: String { paneIDs[selectedIndex] }

    mutating func advance() {
        selectedIndex = (selectedIndex + 1) % paneIDs.count
    }

    func committedPaneID(availablePaneIDs: Set<String>) -> String? {
        availablePaneIDs.contains(selectedPaneID) ? selectedPaneID : nil
    }
}
