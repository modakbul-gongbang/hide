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
                        detail: model.recentProjectDetail(id), symbol: "folder", location: project.location)
                }, selectedID: cycle.selectedProjectID,
                identifier: "project-mru-switcher"
            )
        } else if let cycle = presentation.tabCycle {
            RecentSwitcherOverlay(
                title: "RECENT PANELS", command: .recentTab,
                rows: cycle.visibleIDs.compactMap { id in
                    guard let surface = model.recentSurfaces[id] else { return nil }
                    return RecentSwitcherRow(
                        id: id, title: RecentSurfacePresentation.title(surface),
                        detail: surface.contextLabel, symbol: surface.symbol, location: surface.location, dirty: surface.item.dirty,
                        agent: surface.item.focusedAgent,
                        identity: surface.item.agentIdentity.map {
                            RecentSwitcherRow.Identity(
                                symbol: $0.symbol,
                                color: AgentStatusPresentation(
                                    demand: $0.demand, activity: $0.activity, emphasized: $0.emphasized,
                                    symbol: $0.symbol, label: $0.statusLabel, connected: model.agentsConnected
                                ).color,
                                statusLabel: $0.statusLabel
                            )
                        }
                    )
                }, selectedID: cycle.selectedTabID,
                identifier: "tab-mru-switcher"
            )
        }
    }
}
/// What a Recent Panels row is called.
enum RecentSurfacePresentation {
    /// A tab holding exactly one agent is called by that agent's title, as the
    /// core decided (PRD D-16); every other tab keeps its label, so a shell
    /// tab and a two-agent tab read as they always did.
    static func title(_ surface: RecentSurface) -> String {
        surface.item.agentIdentity?.label ?? surface.item.label
    }
}

private struct RecentSwitcherRow: Identifiable {
    /// The status mark that replaces the tab's icon when the tab is named
    /// after its one agent.
    struct Identity {
        let symbol: String
        let color: Color
        let statusLabel: String
    }

    let id: String
    let title: String
    let detail: String
    let symbol: String
    let location: RecentLocation
    var dirty = false
    var agent: SidebarAgent? = nil
    var identity: Identity? = nil
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
                        if let identity = row.identity {
                            // The tab is named after its agent, so it carries
                            // the agent's mark in the agent's colour, never the
                            // colour alone (PRD B18).
                            AgentStatusMark(symbol: identity.symbol, color: identity.color)
                        } else if let agent = row.agent {
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
                    if row.location.isRemote {
                        RecentLocationBadge(location: row.location)
                            .layoutPriority(1)
                    }
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
                // The mark is hidden from VoiceOver, so a named tab says its
                // agent's state in words here (PRD D-10).
                .accessibilityLabel(
                    [row.title, row.identity?.statusLabel, row.detail, row.location.spoken]
                        .compactMap { $0 }.joined(separator: ", ")
                )
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

/// The trailing location mark never replaces an agent's activity or provider.
struct RecentLocationBadge: View {
    let location: RecentLocation

    var body: some View {
        HideBadge(label: "Remote · \(location.label)", color: HideTheme.secondary,
                  maximumWidth: HideTheme.recentLocationMaxWidth)
            .hideTooltip(location.spoken)
            .accessibilityLabel(location.spoken)
    }
}
