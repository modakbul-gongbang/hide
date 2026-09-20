import Combine
import Foundation

enum ShellTabKind {
    case herdr(CoreTabSnapshot)
    case editor(CoreEditorTabSnapshot)
}

struct ShellTabItem: Identifiable {
    let id: String
    let label: String
    let dirty: Bool
    let active: Bool
    let kind: ShellTabKind
    var focusedAgent: SidebarAgent? = nil
    var contextLabel: String? = nil
    /// The one agent a Herdr tab holds, as the core named it. Where a tab is
    /// named rather than drawn as a strip slot - the Recent Panels switcher -
    /// this is the title and the mark; a tab without one keeps `label`.
    var agentIdentity: CoreAgentChip? = nil
    /// The checkout's preview tab: titled in italic, named with "Preview",
    /// promoted by a double-click on the title (PRD editor-preview-tab D-06).
    var preview: Bool = false
}

/// What a preview tab says about itself wherever it is named rather than
/// drawn: the tooltip and the accessibility label carry "Preview" after the
/// file name, and lose it the moment the tab is promoted (B16). The strip's
/// colours, close button and keycap are the ordinary tab's (B17).
enum EditorTabTitlePresentation {
    static let previewSuffix = "Preview"

    static func spoken(label: String, preview: Bool) -> String {
        preview ? "\(label) · \(previewSuffix)" : label
    }

    static func italic(preview: Bool) -> Bool {
        preview
    }
}

/// Only the overlay subscribes to held-key presentation changes. The shell's
/// retained sidebar and strip are not invalidated for each preview step.
@MainActor
final class RecentNavigationPresentation: ObservableObject {
    @Published var projectCycle: ProjectSwitcherCycle?
    @Published var tabCycle: TabSwitcherCycle?
}

struct RecentProject: Identifiable {
    let id: String
    let deviceID: String
    let workspace: CoreWorkspaceSnapshot
    let location: RecentLocation
}

/// Location is separate from agent state and checkout context. It is resolved
/// once per navigation projection, never by scanning devices on held-key steps.
struct RecentLocation: Equatable {
    let deviceID: String
    let label: String

    var isRemote: Bool { deviceID != "local" }
    var spoken: String { isRemote ? "Remote, \(label)" : label }

    static func resolve(deviceID: String, labels: [String: String]) -> Self {
        // A vanished device still has an identity. Show that exact ID instead
        // of presenting an unregistered remote session as local.
        let label = labels[deviceID].flatMap { $0.isEmpty ? nil : $0 } ?? deviceID
        return Self(deviceID: deviceID, label: label)
    }
}

struct RecentSurface: Identifiable {
    let id: String
    let projectID: String
    let projectLabel: String
    let deviceID: String
    let workspaceID: String
    let checkoutID: String
    let checkoutLabel: String
    let item: ShellTabItem
    let location: RecentLocation

    /// The switcher spans projects, so a row names its project unless the
    /// checkout already carries the same name, which is the single-checkout case.
    var contextLabel: String {
        projectLabel == checkoutLabel ? checkoutLabel : "\(projectLabel) · \(checkoutLabel)"
    }

    var symbol: String {
        switch item.kind {
        case .herdr(let tab): tab.panes.contains { $0.content != .terminal } ? "globe" : "terminal"
        case .editor(let tab): tab.kind == .diff ? "doc.text.magnifyingglass" : "doc.text"
        }
    }
}

/// Resolves the core's ordered tab strip into what the strip draws.
///
/// The order is the core's and is used as given. This maps each entry onto the
/// snapshot it stands for, which is where the panes, the dirty mark, and the
/// active mark live: those change far more often than the strip does, so they
/// do not ride the strip.
enum ShellTabStrip {
    static func items(
        strip: [CoreStripTabSnapshot],
        herdrTabs: [CoreTabSnapshot],
        editorTabs: [CoreEditorTabSnapshot],
        activeHerdrTabID: String?,
        activeFileTabID: String?,
        focusedPaneIDsByTab: [String: String] = [:],
        agents: [SidebarAgent] = []
    ) -> [ShellTabItem] {
        let agentsByPane = Dictionary(uniqueKeysWithValues: agents.map { ($0.paneID, $0) })
        let tabsByID = Dictionary(uniqueKeysWithValues: herdrTabs.compactMap { tab in tab.id.map { ($0, tab) } })
        let editorByID = Dictionary(uniqueKeysWithValues: editorTabs.map { ($0.id, $0) })
        return strip.compactMap { entry in
            switch entry.kind {
            case .herdr:
                // The core builds the strip from the same tabs it publishes,
                // so an entry always has one to point at.
                guard let tab = tabsByID[entry.sourceID]
                else { return nil }
                let pane = tab.panes.first { $0.id == focusedPaneIDsByTab[entry.sourceID] }
                let agent = pane.flatMap { agentsByPane[$0.id] }
                let paneTitle = pane.map {
                    PaneHeaderPresentation.title(
                        herdrLabel: $0.herdrLabel, agentSummary: agent?.identityLabel ?? $0.identityLabel,
                        terminalTitle: $0.terminalTitle, workspaceLabel: $0.workspaceLabel, paneID: $0.id
                    )
                } ?? entry.label
                return ShellTabItem(
                    id: entry.id,
                    // The strip names the stable layout. The pane header names
                    // the currently focused work, so split layouts do not
                    // repeat one pane's changing title in both places (B14).
                    label: entry.label,
                    dirty: false,
                    active: activeFileTabID == nil && entry.sourceID == activeHerdrTabID,
                    kind: .herdr(tab),
                    focusedAgent: agent,
                    contextLabel: pane.map { "\(entry.label) · \($0.statusLabel)\n\(paneTitle)" },
                    agentIdentity: entry.agentIdentity
                )
            case .file:
                guard let tab = editorByID[entry.sourceID]
                else { return nil }
                return ShellTabItem(
                    id: entry.id,
                    label: entry.label,
                    dirty: tab.dirty,
                    active: entry.sourceID == activeFileTabID,
                    kind: .editor(tab),
                    preview: entry.preview
                )
            case .diff:
                guard let tab = editorByID[entry.sourceID]
                else { return nil }
                return ShellTabItem(
                    id: entry.id,
                    label: entry.label,
                    dirty: false,
                    active: entry.sourceID == activeFileTabID,
                    kind: .editor(tab),
                    preview: entry.preview
                )
            }
        }
    }
}

/// Direct-select numbering for the tab strip. The number is the tab's
/// position in the strip as drawn, so ⌘1 always reaches the leftmost tab.
/// It mirrors `AgentShortcutNumbering`, which does the same for Option and
/// the agent rows.
enum TabShortcutNumbering {
    /// Only the first nine tabs get a number: ⌘0 is not a tenth slot, it is a
    /// different key, and a two-digit chord is not a shortcut anyone reaches
    /// for without looking.
    static let capacity = 9

    static func number(ofTabID tabID: String, in tabs: [ShellTabItem]) -> Int? {
        guard let index = tabs.firstIndex(where: { $0.id == tabID }),
              index < capacity
        else { return nil }
        return index + 1
    }

    static func tab(atNumber number: Int, in tabs: [ShellTabItem]) -> ShellTabItem? {
        guard number >= 1, number <= capacity, number <= tabs.count else { return nil }
        return tabs[number - 1]
    }
}

/// Where a dragged tab lands when the operator lets go.
///
/// Tabs are as wide as their labels, so the destination cannot be a fixed
/// step: it is decided by how far the drag has carried the tab across the
/// neighbours beside it. A tab has taken a neighbour's slot once it has moved
/// past the middle of that neighbour, which is the point where the two would
/// visually trade places.
enum TabDragPlacement {
    static func destinationIndex(
        from index: Int,
        translation: CGFloat,
        widths: [CGFloat]
    ) -> Int {
        guard widths.indices.contains(index) else { return index }
        var destination = index
        var travelled: CGFloat = 0
        if translation > 0 {
            var candidate = index + 1
            while candidate < widths.count {
                travelled += widths[candidate]
                guard translation >= travelled - widths[candidate] / 2 else { break }
                destination = candidate
                candidate += 1
            }
        } else if translation < 0 {
            var candidate = index - 1
            while candidate >= 0 {
                travelled += widths[candidate]
                guard -translation >= travelled - widths[candidate] / 2 else { break }
                destination = candidate
                candidate -= 1
            }
        }
        return destination
    }
}
