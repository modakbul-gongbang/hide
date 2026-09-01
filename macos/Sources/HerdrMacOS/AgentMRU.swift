import Foundation

private struct RecentItemMRU: Equatable {
    private(set) var itemIDs: [String] = []

    mutating func observe(focusedItemID: String?, availableItemIDs: [String]) {
        let availableSet = Set(availableItemIDs)
        itemIDs.removeAll { !availableSet.contains($0) }
        for itemID in availableItemIDs where !itemIDs.contains(itemID) {
            itemIDs.append(itemID)
        }
        guard let focusedItemID, availableSet.contains(focusedItemID) else { return }
        itemIDs.removeAll { $0 == focusedItemID }
        itemIDs.insert(focusedItemID, at: 0)
    }
}

struct AgentMRU: Equatable {
    private var items = RecentItemMRU()

    var paneIDs: [String] { items.itemIDs }

    mutating func observe(focusedPaneID: String?, availablePaneIDs: [String]) {
        items.observe(focusedItemID: focusedPaneID, availableItemIDs: availablePaneIDs)
    }
}

/// Tab recency belongs to one project checkout. Moving to another checkout
/// starts a separate ordering instead of leaking tabs from the previous one.
struct TabMRU: Equatable {
    private(set) var contextID: String?
    private var items = RecentItemMRU()

    var tabIDs: [String] { items.itemIDs }

    mutating func observe(
        contextID: String?,
        focusedTabID: String?,
        availableTabIDs: [String]
    ) {
        if self.contextID != contextID {
            self.contextID = contextID
            items = RecentItemMRU()
        }
        items.observe(focusedItemID: focusedTabID, availableItemIDs: availableTabIDs)
    }
}

enum RecentSwitcherDirection {
    case forward
    case backward
}

private struct RecentSwitcherCycle: Equatable {
    let originalItemID: String?
    let itemIDs: [String]
    private(set) var selectedIndex: Int

    /// Index 0 is the agent already focused, so opening the switcher skips it:
    /// forward lands on the previous agent, backward on the least recent one.
    init?(
        originalItemID: String?,
        itemIDs: [String],
        direction: RecentSwitcherDirection = .forward
    ) {
        let unique = itemIDs.reduce(into: [String]()) { result, itemID in
            if !result.contains(itemID) { result.append(itemID) }
        }
        guard unique.count > 1 else { return nil }
        self.originalItemID = originalItemID
        self.itemIDs = unique
        selectedIndex = direction == .forward ? 1 : unique.count - 1
    }

    var selectedItemID: String { itemIDs[selectedIndex] }

    mutating func advance() {
        selectedIndex = (selectedIndex + 1) % itemIDs.count
    }

    /// Option+Shift+Tab walks the cycle the other way, the direction the system
    /// switcher established. Wrapping past the first entry lands on the last.
    mutating func retreat() {
        selectedIndex = (selectedIndex + itemIDs.count - 1) % itemIDs.count
    }

    func committedItemID(availableItemIDs: Set<String>) -> String? {
        availableItemIDs.contains(selectedItemID) ? selectedItemID : nil
    }
}

struct AgentSwitcherCycle: Equatable {
    private var cycle: RecentSwitcherCycle

    init?(
        originalPaneID: String?,
        paneIDs: [String],
        direction: RecentSwitcherDirection = .forward
    ) {
        guard let cycle = RecentSwitcherCycle(
            originalItemID: originalPaneID,
            itemIDs: paneIDs,
            direction: direction
        ) else { return nil }
        self.cycle = cycle
    }

    var originalPaneID: String? { cycle.originalItemID }
    var paneIDs: [String] { cycle.itemIDs }
    var selectedPaneID: String { cycle.selectedItemID }

    mutating func advance() { cycle.advance() }
    mutating func retreat() { cycle.retreat() }

    func committedPaneID(availablePaneIDs: Set<String>) -> String? {
        cycle.committedItemID(availableItemIDs: availablePaneIDs)
    }
}

struct TabSwitcherCycle: Equatable {
    private var cycle: RecentSwitcherCycle

    init?(
        originalTabID: String?,
        tabIDs: [String],
        direction: RecentSwitcherDirection = .forward
    ) {
        guard let cycle = RecentSwitcherCycle(
            originalItemID: originalTabID,
            itemIDs: tabIDs,
            direction: direction
        ) else { return nil }
        self.cycle = cycle
    }

    var originalTabID: String? { cycle.originalItemID }
    var tabIDs: [String] { cycle.itemIDs }
    var selectedTabID: String { cycle.selectedItemID }

    mutating func advance() { cycle.advance() }
    mutating func retreat() { cycle.retreat() }

    func committedTabID(availableTabIDs: Set<String>) -> String? {
        cycle.committedItemID(availableItemIDs: availableTabIDs)
    }
}
