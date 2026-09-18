import Foundation
import SwiftUI

struct HideSearchSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.hideAccent) private var accent
    @State private var query = ""
    @State private var selection = HideSearchSelection()

    private var agentGroups: [HideSearchAgentGroup] {
        HideSearchPresentation.agentGroups(
            workspaces: model.workspaces,
            agents: model.agents,
            query: query
        )
    }

    private var checkoutEntries: [HideSearchEntry] {
        let entries = model.workspaces.flatMap { workspace in
            workspace.checkouts.map { checkout in
                HideSearchEntry(
                    id: "checkout-\(checkout.id)",
                    title: "\(workspace.repoName) / \(checkout.label)",
                    subtitle: checkout.path,
                    kind: .checkout(workspace, checkout)
                )
            }
        }
        return HideSearchEntry.filtered(entries, query: query)
    }

    private var projectEntries: [HideSearchEntry] {
        HideSearchPresentation.foldedProjectEntries(
            workspaces: model.workspaces,
            groups: model.inactiveProjectGroups,
            query: query
        )
    }

    private var entries: [HideSearchEntry] {
        agentGroups.flatMap(\.entries) + projectEntries + checkoutEntries
    }

    var body: some View {
        let groups = agentGroups
        let projects = projectEntries
        let checkouts = checkoutEntries
        let resultIDs = (groups.flatMap(\.entries) + projects + checkouts).map(\.id)
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                HideSearchField(
                    placeholder: "Search agents and workspaces",
                    text: $query,
                    selection: $selection,
                    resultIDs: resultIDs,
                    activate: activateSelected,
                    dismiss: { dismiss() }
                )
                .frame(maxWidth: .infinity)
                .accessibilityIdentifier("hide-search-query")
                HideKeycap(command: .label("Esc"), emphasized: false)
            }
            .padding(HideTheme.spacingLG)
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: HideTheme.spacingXXS) {
                        ForEach(groups) { group in
                            Text("\(group.workspace) > AGENTS")
                                .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                                .foregroundStyle(HideTheme.muted)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, HideTheme.spacingMD)
                                .padding(.top, HideTheme.spacingSM)
                            ForEach(group.entries) { entry in
                                searchButton(entry)
                            }
                        }
                        if !projects.isEmpty {
                            Text("WORKSPACES > PROJECTS")
                                .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                                .foregroundStyle(HideTheme.muted)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, HideTheme.spacingMD)
                                .padding(.top, HideTheme.spacingSM)
                            ForEach(projects) { entry in
                                searchButton(entry)
                            }
                        }
                        if !checkouts.isEmpty {
                            Text("WORKSPACES > CHECKOUTS")
                                .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                                .foregroundStyle(HideTheme.muted)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, HideTheme.spacingMD)
                                .padding(.top, HideTheme.spacingSM)
                            ForEach(checkouts) { entry in
                                searchButton(entry)
                            }
                        }
                        if resultIDs.isEmpty {
                            Text("No matching agents or workspaces")
                                .hideFont(size: HideTheme.Typography.subhead)
                                .foregroundStyle(HideTheme.secondary)
                                .padding(HideTheme.spacingXXL)
                        }
                    }
                    .padding(.horizontal, HideTheme.spacingLG)
                }
                .onChange(of: selection.selectedID) { _, id in
                    if let id { proxy.scrollTo(id, anchor: .center) }
                }
            }
        }
        .accessibilityIdentifier("hide-search-sheet")
        .frame(width: HideTheme.searchSheetSize.width, height: HideTheme.searchSheetSize.height)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
    }

    private func searchButton(_ entry: HideSearchEntry) -> some View {
        Button { route(entry) } label: {
            HStack(spacing: HideTheme.spacingMD) {
                Image(systemName: entry.kind.systemImage)
                    .foregroundStyle(accent)
                    .frame(width: 18)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text(entry.title)
                        .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(entry.subtitle)
                        .hideFont(
                            size: HideTheme.Typography.caption,
                            design: entry.match == nil ? .monospaced : .default
                        )
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                Spacer()
                Text("↵")
                    .foregroundStyle(HideTheme.muted)
                    .opacity(selection.selectedID == entry.id ? 1 : 0)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .background(
            selection.selectedID == entry.id ? HideTheme.accent.opacity(HideTheme.Opacity.emphasisFill) : Color.clear,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
        )
        .accessibilityAddTraits(selection.selectedID == entry.id ? .isSelected : [])
        .accessibilityLabel([entry.title, entry.subtitle, entry.match].compactMap { $0 }.joined(separator: ", "))
        .accessibilityValue(selection.selectedID == entry.id ? "Selected" : "Not selected")
        .accessibilityIdentifier("hide-search-result-\(entry.id)")
        .id(entry.id)
    }

    private func activateSelected() {
        guard let entry = selection.entry(in: entries) else {
            selection.reconcile(entries.map(\.id))
            return
        }
        route(entry)
    }

    private func route(_ entry: HideSearchEntry) {
        // A result may retire between its last render and the click/Return.
        guard let entry = entries.first(where: { $0.id == entry.id }) else {
            selection.reconcile(entries.map(\.id))
            return
        }
        switch entry.kind {
        case let .agent(agent): model.selectAgent(agent)
        case let .checkout(_, checkout): model.selectCheckout(checkout)
        case let .project(_, primaryCheckout): model.selectCheckout(primaryCheckout)
        }
        dismiss()
    }
}

struct HideSearchAgentGroup: Identifiable {
    let id: String
    let workspace: String
    let entries: [HideSearchEntry]
}

enum HideSearchPresentation {
    static func foldedProjectEntries(
        workspaces: [CoreWorkspaceSnapshot],
        groups: [CoreInactiveProjectGroupSnapshot],
        query: String
    ) -> [HideSearchEntry] {
        let byID = Dictionary(uniqueKeysWithValues: workspaces.map { ($0.id, $0) })
        let entries = groups
            .filter { !$0.expanded }
            .flatMap(\.projectIDs)
            .compactMap { projectID -> HideSearchEntry? in
                guard let workspace = byID[projectID],
                      let primary = workspace.checkouts.first(where: {
                          !$0.isWorktree && $0.path == workspace.path
                      })
                else { return nil }
                return HideSearchEntry(
                    id: "project-\(workspace.id)",
                    title: workspace.repoName,
                    subtitle: workspace.path,
                    kind: .project(workspace, primary)
                )
            }
        return HideSearchEntry.filtered(entries, query: query)
    }

    /// The line under an agent's name: the sentence the core chose for its
    /// state, else the rolling task when it is not already the title, else
    /// the status word. The pane id left this line for the match field and
    /// the accessibility label (PRD D-15).
    static func agentSubtitle(_ agent: SidebarAgent) -> String {
        if let detail = agent.detail { return detail }
        if let task = agent.task, task != agent.identityLabel { return task }
        return agent.statusLabel
    }

    static func agentGroups(
        workspaces: [CoreWorkspaceSnapshot],
        agents: [SidebarAgent],
        query: String
    ) -> [HideSearchAgentGroup] {
        workspaces.compactMap { workspace in
            let paneIDs = Set(workspace.checkouts.flatMap(\.tabs).flatMap(\.panes).map(\.id))
            let entries = agents
                .filter { paneIDs.contains($0.paneID) }
                .map {
                    HideSearchEntry(
                        id: "agent-\($0.paneID)",
                        title: $0.identityLabel,
                        subtitle: agentSubtitle($0),
                        match: $0.paneID,
                        kind: .agent($0)
                    )
                }
            let filtered = HideSearchEntry.filtered(entries, query: query)
            return filtered.isEmpty
                ? nil
                : HideSearchAgentGroup(
                    id: workspace.id,
                    workspace: workspace.label,
                    entries: filtered
                )
        }
    }
}

struct HideSearchEntry: Identifiable {
    enum Kind {
        case agent(SidebarAgent)
        case checkout(CoreWorkspaceSnapshot, CoreCheckoutSnapshot)
        case project(CoreWorkspaceSnapshot, CoreCheckoutSnapshot)

        var systemImage: String {
            switch self {
            case .agent: "sparkles"
            case .checkout: "rectangle.stack"
            case .project: "folder"
            }
        }
    }

    enum Route: Equatable {
        case agent(paneID: String)
        case checkout(workspaceID: String, checkoutID: String)
        case project(workspaceID: String, primaryCheckoutID: String)
    }

    let id: String
    let title: String
    let subtitle: String
    /// Text the query matches that the row does not show: an agent's pane
    /// id, so typing `w7J:p2P` still finds the row (PRD B17).
    var match: String? = nil
    let kind: Kind

    var route: Route {
        switch kind {
        case let .agent(agent):
            .agent(paneID: agent.paneID)
        case let .checkout(workspace, checkout):
            .checkout(workspaceID: workspace.id, checkoutID: checkout.id)
        case let .project(workspace, primaryCheckout):
            .project(workspaceID: workspace.id, primaryCheckoutID: primaryCheckout.id)
        }
    }

    static func filtered(_ entries: [HideSearchEntry], query: String) -> [HideSearchEntry] {
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !normalized.isEmpty else { return entries }
        return entries.filter { entry in
            [entry.title, entry.subtitle, entry.match]
                .compactMap { $0 }
                .contains { $0.lowercased().contains(normalized) }
        }
    }
}
