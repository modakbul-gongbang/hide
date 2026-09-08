import Foundation

private struct RecentItemMRU: Equatable {
    private(set) var itemIDs: [String] = []

    mutating func observe(focusedItemID: String?, availableItemIDs: [String]) {
        let availableSet = Set(availableItemIDs)
        itemIDs.removeAll { !availableSet.contains($0) }
        var known = Set(itemIDs)
        for itemID in availableItemIDs where known.insert(itemID).inserted {
            itemIDs.append(itemID)
        }
        guard let focusedItemID, availableSet.contains(focusedItemID), itemIDs.first != focusedItemID else { return }
        itemIDs.removeAll { $0 == focusedItemID }
        itemIDs.insert(focusedItemID, at: 0)
    }
}

struct ProjectMRU: Equatable {
    private var items = RecentItemMRU()

    var projectIDs: [String] { items.itemIDs }

    mutating func observe(focusedProjectID: String?, availableProjectIDs: [String]) {
        items.observe(focusedItemID: focusedProjectID, availableItemIDs: availableProjectIDs)
    }
}

/// Each project retains its own ordering across checkout and project visits.
/// Deleted projects release their history on the next topology observation.
struct TabMRU: Equatable {
    private var contexts: [String: RecentItemMRU] = [:]

    func tabIDs(in contextID: String) -> [String] { contexts[contextID]?.itemIDs ?? [] }

    mutating func observe(contextID: String, focusedTabID: String?, availableTabIDs: [String]) {
        contexts[contextID, default: RecentItemMRU()].observe(
            focusedItemID: focusedTabID, availableItemIDs: availableTabIDs
        )
    }

    mutating func retainContexts(_ available: Set<String>) {
        contexts = contexts.filter { available.contains($0.key) }
    }
}

enum RecentSwitcherDirection {
    case forward
    case backward
}

private struct RecentSwitcherCycle: Equatable {
    let originalItemID: String?
    private(set) var itemIDs: [String]
    private(set) var selectedIndex: Int

    /// Index 0 is the item already focused, so opening the switcher skips it:
    /// forward lands on the previous item, backward on the least recent item.
    init?(
        originalItemID: String?,
        itemIDs: [String],
        direction: RecentSwitcherDirection = .forward
    ) {
        var seen = Set<String>()
        let unique = itemIDs.filter { seen.insert($0).inserted }
        guard unique.count > 1 else { return nil }
        self.originalItemID = originalItemID
        self.itemIDs = unique
        let originalIndex = originalItemID.flatMap { unique.firstIndex(of: $0) }
        selectedIndex = originalIndex.map { ($0 + (direction == .forward ? 1 : unique.count - 1)) % unique.count }
            ?? (direction == .forward ? 0 : unique.count - 1)
    }

    var selectedItemID: String { itemIDs[selectedIndex] }

    mutating func advance() {
        selectedIndex = (selectedIndex + 1) % itemIDs.count
    }

    /// The reverse chord walks the cycle the other way, the direction the system
    /// switcher established. Wrapping past the first entry lands on the last.
    mutating func retreat() {
        selectedIndex = (selectedIndex + itemIDs.count - 1) % itemIDs.count
    }

    /// Prune without reordering a held gesture. A removed highlight advances
    /// to the next surviving entry; no surviving entry cancels the gesture.
    mutating func reconcile(available: Set<String>) -> Bool {
        let selected = selectedItemID
        let heldIDs = itemIDs
        let heldIndex = selectedIndex
        let successor = (0..<heldIDs.count).lazy
            .map { heldIDs[(heldIndex + $0) % heldIDs.count] }
            .first { available.contains($0) }
        guard let successor else { return false }
        itemIDs.removeAll { !available.contains($0) }
        selectedIndex = itemIDs.firstIndex(of: available.contains(selected) ? selected : successor)!
        return true
    }

    /// A bounded window, including the current highlight, for either overlay.
    var visibleItemIDs: [String] {
        let count = min(9, itemIDs.count)
        let start = min(max(0, selectedIndex - count / 2), itemIDs.count - count)
        return Array(itemIDs[start..<(start + count)])
    }

    func committedItemID(availableItemIDs: Set<String>) -> String? {
        availableItemIDs.contains(selectedItemID) ? selectedItemID : nil
    }
}

struct ProjectSwitcherCycle: Equatable {
    private var cycle: RecentSwitcherCycle

    init?(
        originalProjectID: String?,
        projectIDs: [String],
        direction: RecentSwitcherDirection = .forward
    ) {
        guard let cycle = RecentSwitcherCycle(
            originalItemID: originalProjectID,
            itemIDs: projectIDs,
            direction: direction
        ) else { return nil }
        self.cycle = cycle
    }

    var originalProjectID: String? { cycle.originalItemID }
    var projectIDs: [String] { cycle.itemIDs }
    var selectedProjectID: String { cycle.selectedItemID }

    var visibleIDs: [String] { cycle.visibleItemIDs }
    mutating func reconcile(available: Set<String>) -> Bool { cycle.reconcile(available: available) }
    mutating func advance() { cycle.advance() }
    mutating func retreat() { cycle.retreat() }

    func committedProjectID(availableProjectIDs: Set<String>) -> String? {
        cycle.committedItemID(availableItemIDs: availableProjectIDs)
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

    var visibleIDs: [String] { cycle.visibleItemIDs }
    mutating func reconcile(available: Set<String>) -> Bool { cycle.reconcile(available: available) }
    mutating func advance() { cycle.advance() }
    mutating func retreat() { cycle.retreat() }

    func committedTabID(availableTabIDs: Set<String>) -> String? {
        cycle.committedItemID(availableItemIDs: availableTabIDs)
    }
}
