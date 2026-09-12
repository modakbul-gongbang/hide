import SwiftUI

/// Every application-menu chord the shell claims, declared once.
///
/// The defect this exists for: the operator pressed `⌘⇧B` expecting the right
/// panel to toggle, and nothing was bound to it, so the chord fell through to
/// the terminal and was swallowed. Nothing in the shell could answer "which
/// chords are claimed" without reading the menu builder by eye. Declaring them
/// here makes that question a test rather than a reading, and makes a chord
/// claimed twice a build-time-checkable collision instead of a mystery.
///
/// Per-pane commands are not here: they are user-rebindable and live in
/// `PaneCommand`, which owns its own defaults and its own settings surface.
///
/// One chord is declared here without an application menu item: `⌘⌫` moves
/// the Explorer's selected item to the Trash, and it is answered only by
/// the tree's own `keyDown` while the tree has the keyboard, because the
/// same chord elsewhere must reach the terminal untouched. It is in the
/// catalog so the collision checks see it and so a pane command cannot be
/// rebound onto it; `scope` says which commands the application menu
/// builds and which it deliberately leaves out.
enum ShellMenuCommand: String, CaseIterable, Identifiable, Sendable {
    case recentTab = "recent_tab"
    case previousRecentTab = "previous_recent_tab"
    case recentProject = "recent_project"
    case previousRecentProject = "previous_recent_project"
    case newTab = "new_tab"
    case newChat = "new_chat"
    case newWorkspace = "new_workspace"
    case search
    case openFile = "open_file"
    case closeTab = "close_tab"
    case toggleLeftSidebar = "toggle_left_sidebar"
    case toggleSidebarView = "toggle_sidebar_view"
    case toggleRightPanel = "toggle_right_panel"
    case findInPane = "find_in_pane"
    case moveToTrash = "move_to_trash"

    var id: String { rawValue }

    /// Where the chord is answered. The application menu builds every
    /// `.applicationMenu` command and none of the others.
    enum Scope: Equatable {
        case applicationMenu
        case explorerTree
    }

    var scope: Scope {
        switch self {
        case .moveToTrash: .explorerTree
        default: .applicationMenu
        }
    }

    var title: String {
        switch self {
        case .recentTab: "Next Recent Panel"
        case .previousRecentTab: "Previous Recent Panel"
        case .recentProject: "Next Recent Project"
        case .previousRecentProject: "Previous Recent Project"
        case .newTab: "New Tab"
        case .newChat: "New Chat"
        case .newWorkspace: "New Workspace"
        case .search: "Search"
        case .openFile: "Open File"
        case .closeTab: "Close Tab"
        case .toggleLeftSidebar: "Toggle Left Sidebar"
        case .toggleSidebarView: "Toggle Sidebar View"
        case .toggleRightPanel: "Toggle Right Panel"
        case .findInPane: "Find in Pane"
        case .moveToTrash: "Move to Trash"
        }
    }

    var shortcut: PaneShortcut {
        switch self {
        case .recentTab: PaneShortcut(key: "tab", modifiers: [.control])
        case .previousRecentTab: PaneShortcut(key: "tab", modifiers: [.control, .shift])
        case .recentProject: PaneShortcut(key: "tab", modifiers: [.option])
        case .previousRecentProject: PaneShortcut(key: "tab", modifiers: [.option, .shift])
        case .newTab: PaneShortcut(key: "t", modifiers: [.command])
        case .newChat: PaneShortcut(key: "n", modifiers: [.command])
        case .newWorkspace: PaneShortcut(key: "n", modifiers: [.command, .shift])
        case .search: PaneShortcut(key: "k", modifiers: [.command])
        case .openFile: PaneShortcut(key: "p", modifiers: [.command])
        case .closeTab: PaneShortcut(key: "w", modifiers: [.command])
        case .toggleLeftSidebar: PaneShortcut(key: "b", modifiers: [.command])
        case .toggleSidebarView: PaneShortcut(key: "e", modifiers: [.command])
        case .toggleRightPanel: PaneShortcut(key: "b", modifiers: [.command, .shift])
        case .findInPane: PaneShortcut(key: "f", modifiers: [.command])
        case .moveToTrash: PaneShortcut(key: "delete", modifiers: [.command])
        }
    }

    /// The chord as the operator reads it, for help text and accessibility.
    var displayShortcut: String { shortcut.displayString }
}
