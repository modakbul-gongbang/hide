import SwiftUI

struct RecentNavigationOverlay: View {
    let model: ShellModel
    @ObservedObject var presentation: RecentNavigationPresentation

    var body: some View {
        if let cycle = presentation.projectCycle {
            RecentSwitcherOverlay(
                title: "RECENT PROJECTS", command: .recentProject,
                rows: cycle.visibleIDs.compactMap { id in
                    guard let project = model.recentProjects[id] else { return nil }
                    return RecentSwitcherRow(id: id, title: project.workspace.label,
                        detail: model.recentProjectDetail(id), symbol: "folder")
                }, selectedID: cycle.selectedProjectID,
                identifier: "project-mru-switcher"
            )
        } else if let cycle = presentation.tabCycle {
            RecentSwitcherOverlay(
                title: "RECENT PANELS", command: .recentTab,
                rows: cycle.visibleIDs.compactMap { id in
                    guard let surface = model.recentSurfaces[id] else { return nil }
                    return RecentSwitcherRow(id: id, title: surface.item.label,
                        detail: surface.contextLabel, symbol: surface.symbol, dirty: surface.item.dirty,
                        agent: surface.item.focusedAgent)
                }, selectedID: cycle.selectedTabID,
                identifier: "tab-mru-switcher"
            )
        }
    }
}
private struct RecentSwitcherRow: Identifiable {
    let id: String
    let title: String
    let detail: String
    let symbol: String
    var dirty = false
    var agent: SidebarAgent? = nil
}

/// Both navigation levels share the existing panel, typography and keycaps.
/// The model exposes at most nine rows around the highlight, even in a large session.
private struct RecentSwitcherOverlay: View {
    let title: String
    let command: ShellMenuCommand
    let rows: [RecentSwitcherRow]
    let selectedID: String
    let identifier: String

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            HStack {
                Text(title)
                    .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                Spacer()
                HideKeycap(command: .menu(command))
            }
            ForEach(rows) { row in
                HStack(spacing: HideTheme.spacingMD) {
                    Group {
                        if let agent = row.agent {
                            AgentBadge(agentKind: agent.agentKind,
                                stateColor: HideTheme.secondary,
                                size: HideTheme.checkoutIconWidth)
                        } else {
                            Image(systemName: row.symbol)
                                .hideFont(size: HideTheme.Typography.title, weight: .semibold)
                                .foregroundStyle(HideTheme.secondary)
                        }
                    }
                    .frame(width: HideTheme.checkoutIconWidth)
                    VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                        Text(row.title)
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(row.detail)
                            .hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Spacer()
                    if row.dirty {
                        Image(systemName: "circle.fill")
                            .hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.secondary)
                            .accessibilityLabel("Unsaved changes")
                    }
                }
                .padding(.horizontal, HideTheme.spacingMD)
                .frame(height: HideTheme.formControlHeight)
                .background(
                    row.id == selectedID ? HideTheme.accent.opacity(HideTheme.Opacity.emphasisFill) : Color.clear,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                )
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(row.id == selectedID ? .isSelected : [])
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(width: HideTheme.Hint.tooltipMaxWidth)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusLarge))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusLarge).stroke(HideTheme.divider)
        }
        .accessibilityIdentifier(identifier)
    }
}
