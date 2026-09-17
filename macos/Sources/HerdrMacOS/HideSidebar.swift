import AppKit
import SwiftUI

struct HideSidebar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HideBrandHeader()
            SidebarContentPicker()
            SidebarCommandBar()

            if model.sidebarContent == .agents {
                AgentScopePicker()
            }

            SidebarList {
                switch model.sidebarContent {
                case .projects:
                    projectsContent
                case .agents:
                    agentsContent
                }
            }

            SidebarUtilityBar(
                devices: model.devices,
                usages: model.core.snapshot?.navigator.providerUsage ?? []
            )
        }
        .background(HideTheme.sidebar)
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(HideTheme.divider)
                .frame(width: 1)
        }
        .accessibilityIdentifier("hide-sidebar")
    }

    @ViewBuilder
    private var projectsContent: some View {
        // The one thing the operator does most often is at the top, above
        // everything that is only a place to look.
        NewChatRow()

        // What is waiting, then what finished while the operator was away,
        // come before where things live: they are the only parts that ask for
        // an action. An empty group is not drawn at all.
        ForEach(model.raisedAgentSections) { section in
            HideSectionLabel(title: section.group.title, count: section.agents.count)
            ForEach(section.agents) { agent in
                AgentNavigatorRow(agent: agent, showsWorkspace: true)
            }
        }

        ScratchSection()

        HideSectionLabel(title: "Projects · Recent activity", count: model.workspaces.count)
        if model.workspaces.isEmpty {
            EmptySidebarRow(
                systemImage: "square.stack.3d.up",
                title: "No projects yet",
                detail: "Add a folder to create your first project."
            )
        } else {
            ForEach(model.sidebarProjectRows) { row in
                switch row {
                case .workspace(let workspace, let hierarchyLevel):
                    WorkspaceNavigatorRow(workspace: workspace, hierarchyLevel: hierarchyLevel)
                case .inactiveProjects(let group, let folded):
                    InactiveFoldRow(
                        title: "Inactive projects",
                        count: folded.count,
                        itemName: "project",
                        expanded: group.expanded,
                        accessibilityID: "hide-inactive-projects-\(group.deviceID)",
                        hierarchyLevel: .root,
                        action: { model.toggleInactiveProjects(in: group) }
                    )
                    .padding(.bottom, group.expanded ? HideTheme.spacingXXS : HideTheme.spacingSM)
                }
            }
        }
    }

    @ViewBuilder
    private var agentsContent: some View {
        if model.core.snapshot == nil {
            HideSectionLabel(title: "Agents", count: 0)
            EmptySidebarRow(
                systemImage: "clock",
                title: "Loading agents",
                detail: "Waiting for the first runtime snapshot."
            )
        } else if !model.agentsConnected {
            HideSectionLabel(title: "Agents", count: model.visibleAgentList.count)
            EmptySidebarRow(
                systemImage: "bolt.slash",
                title: "Agents unavailable",
                detail: "The last known rows may be stale while Herdr reconnects."
            )
            agentSectionRows
        } else if model.agents.isEmpty {
            HideSectionLabel(title: "Agents", count: 0)
            EmptySidebarRow(
                systemImage: "person.2",
                title: "No agents running",
                detail: "Start an agent from a project to see it here."
            )
        } else if model.visibleAgentList.isEmpty {
            HideSectionLabel(title: "My Work", count: 0)
            Button {
                model.agentListScope = .all
            } label: {
                EmptySidebarRow(
                    systemImage: "person.2.badge.gearshape",
                    title: "Only delegated work is active",
                    detail: "Show All to inspect the agents being supervised."
                )
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .accessibilityIdentifier("agents-show-all-empty-state")
        } else {
            agentSectionRows
        }
    }

    @ViewBuilder
    private var agentSectionRows: some View {
        // The four group boundaries, in the order the core sorted them.
        // Membership and order are the core's answer; this only draws it.
        ForEach(model.agentSections) { section in
            HideSectionLabel(title: section.group.title, count: section.agents.count)
            ForEach(section.agents) { agent in
                AgentNavigatorRow(agent: agent, showsWorkspace: true)
            }
        }
    }
}
/// The agent scope is navigation chrome, not a row in the scrollable result.
/// Keeping it above `SidebarList` prevents AppKit's list row clipping from
/// collapsing the segmented control and keeps both scopes reachable while the
/// operator scrolls a long agent list.
private struct AgentScopePicker: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HideChoiceGroup(
            label: "Agent scope",
            values: AgentListScope.allCases,
            selection: $model.agentListScope,
            title: { $0.title },
            identifier: { "agents-scope-\($0.rawValue)" },
            optionHelp: {
                $0 == .mine
                    ? "Show work currently owned by you. Escalated and orphaned work remains visible."
                    : "Show all work, including delegated agents."
            },
            equalWidth: true
        )
        .fixedSize(horizontal: false, vertical: true)
        .padding(.horizontal, HideTheme.spacingSM)
        .padding(.bottom, HideTheme.spacingXS)
    }
}

/// The row that starts a chat. First in the list, because starting one is
/// the most frequent thing done here and every other row is a place rather
/// than an action.
private struct NewChatRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent

    var body: some View {
        Button(action: { model.openComposer() }) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: "plus.bubble")
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(accent)
                Text("New chat")
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .foregroundStyle(HideTheme.primary)
                Spacer(minLength: HideTheme.spacingXS)
                HideKeycap(command: .menu(.newChat), emphasized: model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.command])
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(maxWidth: .infinity, minHeight: HideTheme.IconButton.standardSize.height)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .accessibilityIdentifier("hide-new-chat")
    }
}

/// The Scratch section: a header that is always drawn, and the tabs under it
/// once the operator opens it.
///
/// The header stays at a count of zero on purpose. Scratch is a permanent
/// place, and a section that disappeared when it emptied would make the space
/// look like something that has to be created.
private struct ScratchSection: View {
    @EnvironmentObject private var model: ShellModel

    private var scratch: CoreScratchSnapshot { model.scratch }

    var body: some View {
        Button(action: model.toggleScratchExpanded) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: scratch.expanded ? "chevron.down" : "chevron.right")
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                Text(scratch.label.uppercased())
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                Text("\(scratch.tabs.count)")
                    .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.top, HideTheme.spacingMD)
            .padding(.bottom, HideTheme.spacingSM)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .accessibilityLabel(scratch.expanded ? "Collapse Scratch" : "Expand Scratch")
        .accessibilityIdentifier("hide-scratch-header")

        if scratch.expanded {
            if scratch.tabs.isEmpty {
                EmptySidebarRow(
                    systemImage: "tray",
                    title: "Nothing in Scratch",
                    detail: "\(HideCommand.menu(.newChat).displayString(bindings: model.paneShortcuts)) starts a chat that belongs to no project."
                )
            } else {
                ForEach(model.scratchTabsBelowRaisedSections) { tab in
                    ScratchRow(tab: tab)
                }
            }
        }
    }
}

/// One Scratch row: an agent row when the tab holds an agent, a tab row when
/// it does not. The agent row is the sidebar's own component, so a Scratch
/// chat and a project chat read as the same kind of thing.
private struct ScratchRow: View {
    @EnvironmentObject private var model: ShellModel
    let tab: CoreScratchTabSnapshot

    var body: some View {
        if let agent = model.scratchAgent(for: tab) {
            AgentRow(
                presentation: AgentRowPresentation(
                    agent: agent,
                    title: tab.displayName,
                    connected: model.core.snapshot?.status.herdr.state == "connected"
                ),
                style: .shell(density: .compact),
                density: .compact,
                isFocused: model.focusedPaneID == agent.paneID,
                action: { model.selectAgent(agent) }
            )
            .accessibilityIdentifier("hide-scratch-agent-\(agent.paneID)")
        } else {
            Button(action: { model.focusScratchTab(tab) }) {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: "terminal")
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .foregroundStyle(HideTheme.muted)
                    Text(tab.displayName)
                        .hideFont(size: HideTheme.Typography.body)
                        .foregroundStyle(HideTheme.primary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, HideTheme.spacingMD + HideTheme.spacingSM)
                .frame(maxWidth: .infinity, minHeight: 28)
                .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .accessibilityIdentifier("hide-scratch-tab-\(tab.id)")
        }
    }
}

private struct SidebarContentPicker: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HideChoiceGroup(
            label: "Sidebar view",
            values: SidebarContent.allCases,
            selection: Binding(
                get: { model.sidebarContent },
                set: { model.showSidebarContent($0) }
            ),
            title: { $0.title },
            appearance: .segmented,
            identifier: { "hide-sidebar-view-\($0.rawValue)" },
            optionHelp: { "Show \($0.title)" },
            optionHelpCommand: { _ in .menu(.toggleSidebarView) },
            equalWidth: true
        )
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingSM)
        .accessibilityIdentifier("hide-sidebar-view-switcher")
    }
}

private struct SidebarCommandBar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Button(action: model.openSearch) {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: "magnifyingglass")
                        .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    Text("Search")
                        .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    Spacer(minLength: 4)
                    HideKeycap(command: .menu(.search), emphasized: model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.command])
                }
                .foregroundStyle(HideTheme.secondary)
                .padding(.horizontal, HideTheme.spacingMD)
                .frame(maxWidth: .infinity, minHeight: HideTheme.IconButton.standardSize.height)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .accessibilityLabel("Search projects and agents")
            .hideTooltip(ShellMenuCommand.search.title, command: .menu(.search), inline: true)

            HideIconButton(
                systemImage: "folder.badge.plus",
                help: "New project",
                accessibilityLabel: "New project",
                command: .menu(.newWorkspace),
                action: model.openNewWorkspace
            )
            HideIconButton(
                systemImage: "plus",
                help: "New chat",
                accessibilityLabel: "New chat",
                command: .menu(.newChat),
                action: { model.openComposer() }
            )
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingMD)
    }
}

private struct SidebarUtilityBar: View {
    @EnvironmentObject private var model: ShellModel
    @State private var showingUsage = false

    let devices: [CoreDeviceSnapshot]
    let usages: [CoreProviderUsageSnapshot]

    private var selectedDevice: CoreDeviceSnapshot? {
        devices.first(where: { $0.id == model.selectedDeviceID }) ?? devices.first
    }

    private func availablePercent(for usage: CoreProviderUsageSnapshot) -> Double? {
        guard ["available", "stale", "fallback"].contains(usage.state) else { return nil }
        return usage.usedPercent
    }

    private func usageColor(for usage: CoreProviderUsageSnapshot) -> Color {
        guard let percent = availablePercent(for: usage) else { return HideTheme.muted }
        if percent >= 90 { return HideTheme.danger }
        if percent >= 70 { return HideTheme.warning }
        return HideTheme.success
    }

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            Menu {
                ForEach(devices) { device in
                    Button {
                        model.selectDevice(device)
                    } label: {
                        Label(
                            device.agentCount > 0
                                ? "\(device.label), \(device.agentCount) agents"
                                : device.label,
                            systemImage: device.id == model.selectedDeviceID ? "checkmark" : "circle"
                        )
                    }
                }
            } label: {
                HStack(spacing: HideTheme.spacingSM) {
                    Circle()
                        .fill(selectedDevice.map(deviceStatusColor) ?? HideTheme.muted)
                        .frame(width: 6, height: 6)
                    Text(selectedDevice?.label ?? "No device")
                        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                        .lineLimit(1)
                    if let agentCount = selectedDevice?.agentCount, agentCount > 0 {
                        Text("\(agentCount)")
                            .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    Image(systemName: "chevron.up.chevron.down")
                        .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                        .foregroundStyle(HideTheme.muted)
                }
                .foregroundStyle(HideTheme.secondary)
                .padding(.horizontal, HideTheme.spacingSM)
                .frame(height: 30)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .hideTooltip("Choose device")

            Spacer(minLength: 0)

            Button {
                showingUsage.toggle()
            } label: {
                HStack(spacing: HideTheme.spacingXS) {
                    ForEach(usages) { usage in
                        HStack(spacing: HideTheme.spacingXXS) {
                            HideProviderMark(
                                usage: usage,
                                isMuted: availablePercent(for: usage) == nil
                            )
                            if let percent = availablePercent(for: usage) {
                                Text("\(Int(percent.rounded()))%")
                                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold, design: .monospaced)
                            }
                        }
                        .foregroundStyle(usageColor(for: usage))
                        .padding(.horizontal, HideTheme.spacingXS)
                        .frame(minHeight: 30)
                        .background(
                            HideTheme.elevated,
                            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        )
                    }
                }
                .frame(minWidth: 30, minHeight: 30)
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .hideTooltip("Weekly provider usage")
            .accessibilityLabel("Weekly provider usage")
            .popover(isPresented: $showingUsage, arrowEdge: .bottom) {
                HideUsagePopover(usages: usages)
            }
            .onChange(of: showingUsage) { _, open in
                model.core.setUsagePopoverOpen(open)
            }

            HideIconButton(
                systemImage: "gearshape",
                help: "Settings",
                accessibilityLabel: "Settings",
                variant: .toolbar,
                action: { model.showSettings = true }
            )
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingSM)
        .overlay(alignment: .top) {
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: HideTheme.Layout.hairlineWidth)
        }
    }

    private func deviceStatusColor(_ device: CoreDeviceSnapshot) -> Color {
        device.state == "ready" || device.state == "available"
            ? HideTheme.success
            : HideTheme.warning
    }
}

private struct HideProviderMark: View {
    let usage: CoreProviderUsageSnapshot
    let isMuted: Bool

    var body: some View {
        Group {
            if let mark = AgentMark.image(for: usage.provider) {
                Image(nsImage: mark)
                    .renderingMode(isMuted ? .template : .original)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .foregroundStyle(HideTheme.muted)
                    .padding(HideTheme.spacingXXS)
            } else {
                Text(usage.label.prefix(1))
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold, design: .rounded)
                    .foregroundStyle(isMuted ? HideTheme.muted : HideTheme.secondary)
            }
        }
        .frame(width: HideTheme.spacingLG, height: HideTheme.spacingLG)
        .background(
            HideTheme.elevated,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
        )
    }
}

private struct HideUsagePopover: View {
    let usages: [CoreProviderUsageSnapshot]

    var body: some View {
        TimelineView(.periodic(from: .now, by: 60)) { context in
            VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
                HStack(spacing: HideTheme.spacingSM) {
                    Text("Weekly Usage")
                        .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Spacer()
                    Text("7 days")
                        .hideFont(size: HideTheme.Typography.micro, weight: .semibold, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                }

                ForEach(usages) { usage in
                    VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                        HideWeeklyUsageRow(usage: usage, bucket: nil, now: context.date)
                        ForEach(usage.buckets) { bucket in
                            HideWeeklyUsageRow(usage: usage, bucket: bucket, now: context.date)
                        }
                    }
                }
            }
            .padding(HideTheme.spacingLG)
            .frame(width: 250)
            .background(HideTheme.panel)
            .preferredColorScheme(.dark)
            .accessibilityIdentifier("hide-weekly-usage")
        }
    }
}

private struct HideWeeklyUsageRow: View {
    let usage: CoreProviderUsageSnapshot
    let bucket: CoreProviderUsageBucketSnapshot?
    let now: Date

    private var label: String { bucket?.label ?? usage.label }
    private var state: String { bucket?.state ?? usage.state }
    private var usedPercent: Double? { bucket?.usedPercent ?? usage.usedPercent }
    private var resetsAtUnixSeconds: UInt64? { bucket?.resetsAtUnixSeconds ?? usage.resetsAtUnixSeconds }
    private var message: String? { bucket?.message ?? usage.message }
    private var hasValue: Bool {
        ["available", "stale", "fallback"].contains(state) && usedPercent != nil
    }

    private var clampedProgress: Double {
        guard hasValue else { return 0 }
        return min(max(usedPercent ?? 0, 0), 100) / 100
    }

    private var usageColor: Color {
        guard let percent = usedPercent, hasValue else {
            return HideTheme.muted
        }
        if percent >= 90 { return HideTheme.danger }
        if percent >= 70 { return HideTheme.warning }
        return HideTheme.success
    }

    private var valueLabel: String {
        if state == "loading" {
            return "…"
        }
        guard let percent = usedPercent, hasValue else { return "Unavailable" }
        return "\(Int(percent.rounded()))%"
    }

    private var relativeReset: String? {
        guard let reset = resetsAtUnixSeconds else { return nil }
        let seconds = max(0, Int(Date(timeIntervalSince1970: TimeInterval(reset)).timeIntervalSince(now)))
        let days = seconds / 86_400
        let hours = (seconds % 86_400) / 3_600
        let minutes = max(1, (seconds % 3_600) / 60)
        if days > 0 { return "in \(days)d \(hours)h" }
        if hours > 0 { return "in \(hours)h \(minutes)m" }
        return "in \(minutes)m"
    }

    private var absoluteReset: String? {
        guard let reset = resetsAtUnixSeconds else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(reset))
        return "Resets \(date.formatted(date: .abbreviated, time: .shortened))"
    }

    private var helpText: String {
        if state == "fallback", let source = usage.lastSuccessAtUnixMilliseconds {
            let date = Date(timeIntervalSince1970: TimeInterval(source) / 1_000)
            return "From last Codex session · \(date.formatted(date: .abbreviated, time: .shortened))"
        }
        if let message {
            if bucket != nil, let absoluteReset {
                return "\(label) · \(message) · \(absoluteReset)"
            }
            return message
        }
        return [bucket == nil ? nil : label, absoluteReset]
            .compactMap { $0 }
            .joined(separator: " · ")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            HStack(spacing: HideTheme.spacingSM) {
                if bucket != nil {
                    Text("└")
                        .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                        .frame(width: HideTheme.spacingSM)
                }
                HideProviderMark(usage: usage, isMuted: !hasValue)

                Text(label)
                    .hideFont(
                        size: bucket == nil ? HideTheme.Typography.body : HideTheme.Typography.caption,
                        weight: .medium
                    )
                    .foregroundStyle(HideTheme.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                if let relativeReset {
                    Text("· \(relativeReset)")
                        .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                }
                Spacer(minLength: HideTheme.spacingXS)
                Text(valueLabel)
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold, design: .monospaced)
                    .foregroundStyle(hasValue ? usageColor : HideTheme.muted)
                    .fixedSize(horizontal: true, vertical: false)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                        .fill(HideTheme.divider)
                    RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                        .fill(usageColor)
                        .frame(width: geometry.size.width * clampedProgress)
                }
            }
            .frame(height: 3)
            .accessibilityHidden(true)
        }
        .hideTooltip(helpText)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(label)
        .accessibilityValue(valueLabel)
        .accessibilityIdentifier(
            bucket == nil
                ? "hide-weekly-usage-\(usage.provider)"
                : "hide-weekly-usage-\(usage.provider)-bucket"
        )
    }
}

private struct HideBrandHeader: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text("hide")
                .hideFont(size: HideTheme.Typography.headline, weight: .bold, design: .rounded)
                .tracking(-0.6)
                .foregroundStyle(HideTheme.primary)
            Circle()
                .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                .frame(width: 6, height: 6)
            Spacer()
            Text(model.isRemoteContext ? model.remote.targetLabel : (model.core.runtimeSelection?.version ?? "offline"))
                .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
                .foregroundStyle(HideTheme.muted)
                .lineLimit(1)
                .truncationMode(.middle)
                .hideTooltip(model.isRemoteContext ? model.remote.targetLabel : (model.core.runtimeSelection?.version ?? "offline"))
            HideIconButton(
                systemImage: "sidebar.left",
                help: "Hide left sidebar",
                accessibilityLabel: "Hide left sidebar",
                variant: .toolbar,
                command: .menu(.toggleLeftSidebar),
                action: model.toggleLeftSidebar
            )
            .accessibilityIdentifier("hide-toggle-left-sidebar")
        }
        // The wordmark starts after the traffic lights rather than under them.
        .padding(.leading, HideTheme.Layout.trafficLightInset)
        .padding(.trailing, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingMD)
        // This is the window's top left corner while the sidebar is open, and
        // with the titlebar gone it is where the window is grabbed. The
        // sidebar toggle takes its own clicks; everything else here is the
        // handle.
        .background(WindowDragArea())
        .accessibilityIdentifier("hide-brand")
    }
}

private struct HideSectionLabel: View {
    let title: String
    let count: Int?

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text(title)
                .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
            if let count {
                Text("\(count)")
                    .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            Spacer()
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .padding(.top, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingSM)
    }
}

private struct EmptySidebarRow: View {
    let systemImage: String
    let title: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: HideTheme.spacingSM) {
            Image(systemName: systemImage)
                .foregroundStyle(HideTheme.muted)
                .frame(width: 16)
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                Text(title)
                    .hideFont(size: HideTheme.Typography.subhead, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                Text(detail)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .padding(.vertical, HideTheme.spacingMD)
    }
}

private struct WorkspaceNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let workspace: CoreWorkspaceSnapshot
    let hierarchyLevel: SidebarHierarchyLevel

    private var isFocusedWorkspace: Bool {
        model.focusedWorkspace?.id == workspace.id
    }

    private var presentation: SidebarWorkspacePresentation {
        SidebarWorkspacePresentation(workspace: workspace, agents: model.agents)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
            HStack(spacing: HideTheme.spacingNone) {
                Button {
                    model.toggleWorkspace(workspace)
                } label: {
                    HStack(spacing: HideTheme.spacingSM) {
                        Image(systemName: workspace.expanded ? "chevron.down" : "chevron.right")
                            .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(width: 12, height: 20)
                        Image(systemName: workspace.isGit ? "folder.badge.gearshape" : "folder")
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .foregroundStyle(
                                workspace.temporary
                                    ? HideTheme.warning
                                    : (isFocusedWorkspace ? accent : HideTheme.secondary)
                            )
                            .frame(width: 16)
                        Text(workspace.label)
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .foregroundStyle(isFocusedWorkspace ? HideTheme.primary : HideTheme.secondary)
                            .lineLimit(1)
                            // The project's name is what the row is for. The
                            // trailing detail gained a time token, so the name
                            // takes the width it needs first and the detail
                            // truncates in a narrow sidebar instead.
                            .layoutPriority(1)
                        Spacer(minLength: 0)
                        Text(presentation.activityLabel)
                            .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                            .lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, minHeight: 34)
                    .contentShape(Rectangle())
                }
                .buttonStyle(HideInteractiveButtonStyle())
                .accessibilityLabel(workspace.expanded ? "Collapse \(workspace.label)" : "Expand \(workspace.label)")
                .accessibilityIdentifier("hide-workspace-disclosure-\(workspace.id)")
                Menu {
                    if workspace.isGit && workspace.remoteTargetID == nil {
                        Button("Refresh GitHub status") { model.requestGithubStatus(workspace, refresh: true) }
                    }
                    Button(WorktreeMenuPolicy.newWorktree) { model.requestNewWorktree(workspace) }
                        .disabled(!workspace.isGit || workspace.remoteTargetID != nil)
                    Divider()
                    if workspace.registered {
                        Button(WorktreeMenuPolicy.removeRegistration, role: .destructive) {
                            model.requestRemoveWorkspace(workspace)
                        }
                    }
                } label: {
                    Image(systemName: "ellipsis")
                        .hideFont(size: HideTheme.Typography.body, weight: .bold)
                        .foregroundStyle(HideTheme.muted)
                        .frame(width: 22, height: 22)
                        .contentShape(Rectangle())
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .frame(width: 24, height: 28)
            }
            .padding(.leading, hierarchyLevel.contentLeadingInset)
            .padding(.trailing, HideTheme.spacingSM)

            if workspace.expanded {
                ForEach(model.activeCheckouts(in: workspace)) { checkout in
                    checkoutGroup(checkout, hierarchyLevel: hierarchyLevel.childLevel)
                }
                let inactive = model.inactiveCheckouts(in: workspace)
                if !inactive.isEmpty {
                    InactiveFoldRow(
                        title: "Inactive",
                        count: inactive.count,
                        itemName: "checkout",
                        expanded: workspace.inactiveCheckouts.expanded,
                        accessibilityID: "hide-inactive-checkouts-\(workspace.id)",
                        hierarchyLevel: hierarchyLevel.childLevel,
                        action: { model.toggleInactiveCheckouts(in: workspace) }
                    )
                    if workspace.inactiveCheckouts.expanded {
                        ForEach(inactive) { checkout in
                            checkoutGroup(
                                checkout,
                                hierarchyLevel: hierarchyLevel.childLevel.childLevel
                            )
                        }
                    }
                }
            }
        }
        .padding(.bottom, HideTheme.spacingSM)
        .onAppear { model.requestGithubStatus(workspace) }
    }

    private func checkoutGroup(
        _ checkout: CoreCheckoutSnapshot,
        hierarchyLevel: SidebarHierarchyLevel
    ) -> some View {
        let isFocused = model.focusedCheckout?.id == checkout.id
        let visibleAgents = SidebarGrouping.tree(model.agents, checkoutID: checkout.id,
            ownedPaneIDs: Set(checkout.tabs.flatMap(\.panes).map(\.id)))
        let checkoutPresentation = SidebarCheckoutPresentation(
            workspace: workspace,
            checkout: checkout,
            agents: model.agents,
            connected: model.agentsConnected
        )

        return VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            CheckoutNavigatorRow(
                workspace: workspace,
                checkout: checkout,
                presentation: checkoutPresentation,
                isFocused: isFocused,
                hasAgents: !visibleAgents.isEmpty
            )
            if model.isCheckoutExpanded(checkout) {
                // The connector needs the shape of the run, not just each
                // row's depth, so it is derived once for the whole visible
                // preorder rather than guessed per row.
                let guides = SidebarGrouping.lineageGuides(visibleAgents)
                ForEach(Array(visibleAgents.enumerated()), id: \.element.id) { index, agent in
                    AgentNavigatorRow(
                        agent: agent,
                        showsWorkspace: false,
                        guide: guides[index]
                    )
                }
            }
        }
        .background(
            isFocused ? HideTheme.elevated.opacity(HideTheme.Opacity.secondary) : .clear,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
        )
        .overlay {
            if isFocused {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
        }
        .padding(.leading, hierarchyLevel.selectionLeadingInset)
        .padding(.trailing, HideTheme.spacingSM)
    }
}

/// One disclosure pattern for both inactive levels. It uses the sidebar's
/// existing interactive feedback and tokens; only the core decides which rows
/// belong behind it.
private struct InactiveFoldRow: View {
    let title: String
    let count: Int
    let itemName: String
    let expanded: Bool
    let accessibilityID: String
    let hierarchyLevel: SidebarHierarchyLevel
    let action: () -> Void

    var body: some View {
        let countedItemName = count == 1 ? itemName : "\(itemName)s"
        Button(action: action) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: expanded ? "chevron.down" : "chevron.right")
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                    .frame(width: 12, height: 20)
                Text("\(title) \(count)")
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                Spacer(minLength: HideTheme.spacingXS)
            }
            .padding(.horizontal, HideTheme.spacingSM)
            .frame(maxWidth: .infinity, minHeight: HideTheme.IconButton.standardSize.height)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .padding(.leading, hierarchyLevel.selectionLeadingInset)
        .padding(.trailing, HideTheme.spacingSM)
        .accessibilityLabel(
            "\(title), \(count) \(countedItemName), \(expanded ? "expanded" : "collapsed")"
        )
        .accessibilityValue(expanded ? "Expanded" : "Collapsed")
        .accessibilityIdentifier(accessibilityID)
    }
}

/// A compact read-only summary; the surrounding Workspace row owns disclosure.
private struct WorkspaceAgentSummary: View {
    let presentation: SidebarCheckoutPresentation

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            if let status = presentation.status {
                AgentStatusMark(symbol: status.symbol, color: status.color)
            }
            AgentBadge(
                agentKind: presentation.representativeAgentKind ?? "terminal",
                stateColor: presentation.status?.color ?? HideTheme.secondary,
                size: HideTheme.lineageChevronWidth
            )
            if presentation.agentCount > 1 {
                Text("+\(presentation.agentCount - 1)")
                    .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
            }
        }
        .padding(.horizontal, HideTheme.spacingXS)
        .frame(height: HideTheme.IconButton.toolbarSize.height)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        .fixedSize()
        .accessibilityHidden(true)
    }
}

/// GitHub details are a separate action over the row's disclosure hit area.
private struct WorkspacePullRequestControl: View {
    @EnvironmentObject private var model: ShellModel
    let workspace: CoreWorkspaceSnapshot
    let checkout: CoreCheckoutSnapshot
    @State private var isPresented = false

    private var request: CorePullRequest? { checkout.pullRequest }
    private var icon: Image { CheckoutCardPresentation.pullRequestIcon(request) }
    private var color: Color { CheckoutCardPresentation.pullRequestColor(request) }

    var body: some View {
        HideIconButton(image: icon, imageSize: HideTheme.PullRequest.iconSize, color: color, help: request.map { "PR #\($0.number): \(CheckoutCardPresentation.pullRequestState($0))" }
            ?? "GitHub status for \(checkout.label)", variant: .toolbar, isSelected: isPresented,
            action: { isPresented.toggle() })
            .accessibilityIdentifier("hide-pull-request-\(checkout.id)")
            .popover(isPresented: $isPresented, arrowEdge: .trailing) {
                VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
                    HStack(spacing: HideTheme.spacingSM) {
                        icon.resizable().frame(width: HideTheme.PullRequest.iconSize, height: HideTheme.PullRequest.iconSize)
                            .foregroundStyle(color)
                        Text(request.map { "PR #\($0.number)" } ?? "GitHub")
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                        Spacer()
                        HideIconButton(systemImage: "arrow.clockwise", help: "Refresh GitHub status", variant: .toolbar,
                            action: { model.requestGithubStatus(workspace, refresh: true) })
                            .disabled(checkout.github.loading)
                        if let request {
                            HideIconButton(systemImage: "arrow.up.right.square", help: "Open PR #\(request.number) on GitHub",
                                variant: .toolbar, action: { model.openPullRequest(request) })
                        }
                    }
                    if let request {
                        Text(request.title.flatMap { $0.isEmpty ? nil : $0 } ?? "Pull request #\(request.number)")
                            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .fixedSize(horizontal: false, vertical: true)
                        HStack(spacing: HideTheme.spacingSM) {
                            HideBadge(label: "State: \(CheckoutCardPresentation.pullRequestState(request))",
                                color: color)
                            HideBadge(label: "Checks: \(CheckoutCardPresentation.checksLabel(request.checks))",
                                color: CheckoutCardPresentation.checksColor(request.checks))
                        }
                        Text("\(request.headBranch) → \(request.baseBranch)")
                            .hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.secondary)
                    }
                    if checkout.github.loading {
                        HStack { ProgressView().controlSize(.small); Text("Updating GitHub status…") }
                    } else if let notice = CheckoutCardPresentation.githubNotice(checkout.github) {
                        Text(notice).foregroundStyle(HideTheme.warning)
                    } else if request == nil {
                        Text("No pull request for this branch").foregroundStyle(HideTheme.secondary)
                    }
                    if let stale = CheckoutCardPresentation.staleNotice(checkout.github, now: Date()) {
                        Text("Last known status · \(stale)").foregroundStyle(HideTheme.warning)
                    }
                }
                .hideFont(size: HideTheme.Typography.body)
                .foregroundStyle(HideTheme.primary)
                .padding(HideTheme.spacingLG)
                .frame(width: HideTheme.Layout.pullRequestPopoverWidth)
                .background(HideTheme.panel)
                .hideOverlayHost()
                .environmentObject(model)
                .preferredColorScheme(.dark)
            }
    }
}

private struct CheckoutNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    let workspace: CoreWorkspaceSnapshot
    let checkout: CoreCheckoutSnapshot
    let presentation: SidebarCheckoutPresentation
    let isFocused: Bool
    let hasAgents: Bool

    var body: some View {
        ZStack {
            Button {
                if hasAgents { model.toggleCheckoutExpansion(checkout) }
                else { model.selectCheckout(checkout) }
            } label: {
                Color.clear.contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .hideTooltip(presentation.detailTooltip)
            .accessibilityIdentifier("hide-checkout-\(checkout.id)")
            .accessibilityLabel(CheckoutCardPresentation.rowAccessibilityLabel(
                repoName: workspace.repoName, checkout: checkout, agentCount: presentation.agentCount
            ) + (presentation.status.map { ". \($0.label)" } ?? ""))
            .accessibilityValue(hasAgents
                ? (model.isCheckoutExpanded(checkout) ? "Expanded" : "Collapsed")
                : (isFocused ? "Selected" : "Not selected"))
            .accessibilityHint(hasAgents ? "Show or hide agents in this workspace" : "Open this workspace")

            HStack(spacing: HideTheme.spacingSM) {
                Group {
                    Color.clear.frame(width: HideTheme.agentMarkWidth)
                    Image(systemName: workspace.isGit ? "arrow.triangle.branch" : "folder")
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .foregroundStyle(HideTheme.secondary)
                        .frame(width: HideTheme.checkoutIconWidth)
                    Text(checkout.label)
                        .hideFont(size: HideTheme.Typography.subhead, weight: isFocused ? .semibold : .medium)
                        .foregroundStyle(HideTheme.primary)
                        .lineLimit(1)
                    if presentation.isDetached { HideBadge(label: "detached", color: HideTheme.secondary) }
                    if !checkout.exists { HideBadge(label: "missing", color: HideTheme.danger) }
                    else if checkout.temporary { HideBadge(label: "temporary", color: HideTheme.warning) }
                    if presentation.isPrimary { HideBadge(label: "primary", color: HideTheme.secondary) }
                }
                .allowsHitTesting(false)
                .accessibilityHidden(true)
                if presentation.isPrimary, case .warning(let branch, _) = MainWorktreePresentation.state(
                    branch: checkout.branch, base: model.baseBranch(for: workspace)
                ) {
                    HideIconButton(systemImage: "exclamationmark.triangle", help: "Move \(branch) to a worktree",
                        variant: .toolbar, action: { model.requestBranchMigration(workspace: workspace, checkout: checkout) })
                }
                if checkout.dirty {
                    Circle().fill(HideTheme.warning).frame(width: 5, height: 5)
                        .hideTooltip("\(checkout.changedFileCount) uncommitted changes")
                        .allowsHitTesting(false)
                }
                Spacer(minLength: 0).allowsHitTesting(false)
                if workspace.isGit && workspace.remoteTargetID == nil && checkout.branch != nil,
                   checkout.pullRequest != nil || checkout.github.loading || checkout.github.unavailableReason != nil {
                    WorkspacePullRequestControl(workspace: workspace, checkout: checkout)
                }
                if hasAgents {
                    Group {
                        // Expanded, every agent's own row carries its state;
                        // the collapsed summary would repeat it beside them.
                        if !model.isCheckoutExpanded(checkout) {
                            WorkspaceAgentSummary(presentation: presentation)
                        }
                        Image(systemName: model.isCheckoutExpanded(checkout) ? "chevron.down" : "chevron.right")
                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                            .foregroundStyle(HideTheme.secondary)
                            .frame(width: HideTheme.IconButton.toolbarSize.width)
                    }
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                }
            }
            .padding(.horizontal, HideTheme.spacingSM)
        }
        .frame(height: HideTheme.checkoutRowHeight)
        .contextMenu {
            Button(WorktreeMenuPolicy.newWorktree, systemImage: "plus") { model.requestNewWorktree(workspace) }
            if checkout.isWorktree {
                Button(WorktreeMenuPolicy.startAgentHere, systemImage: "terminal") { model.openComposer(checkoutID: checkout.id) }
            }
            if let branch = checkout.branch {
                Button(WorktreeMenuPolicy.setBaseBranch, systemImage: "arrow.triangle.branch") { model.setBaseBranch(checkout, in: workspace) }
                    .disabled(branch == model.baseBranch(for: workspace))
            }
            Divider()
            Button(WorktreeMenuPolicy.copyPath, systemImage: "doc.on.doc") { model.copyCheckoutPath(checkout) }
            Menu(WorktreeMenuPolicy.openIn, systemImage: "arrow.up.forward.app") {
                Button("Finder") { model.revealCheckout(checkout) }
                Button("Default editor") { model.openCheckoutInDefaultEditor(checkout) }
            }
            if checkout.isWorktree {
                Divider()
                Button(checkout.worktree?.deletionGate.buttonLabel ?? "Delete worktree…", role: .destructive) {
                    model.requestDeleteWorktree(checkout)
                }
                .disabled(checkout.worktree?.deletionGate.blockedReason != nil || checkout.worktree == nil)
                if let reason = checkout.worktree?.deletionGate.blockedReason { Text(reason) }
            }
        }
    }
}

/// A sidebar agent row: the one agent row plus the sidebar's focus state and
/// its direct-select shortcut hint.
private struct AgentNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    let agent: SidebarAgent
    /// Under a checkout the project name is the heading above the row, so
    /// repeating it wastes the line the summary needs.
    let showsWorkspace: Bool
    /// Where this row sits in the visible run, for the connector. The flat
    /// views draw no tree and pass the default.
    var guide = SidebarGrouping.LineageGuide()

    private var density: AgentRowDensity { showsWorkspace ? .prominent : .compact }
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var shortcutVisible: Bool {
        model.shortcutHintState.reveals(.agent(1), bindings: model.paneShortcuts)
    }

    private var relationshipLeadingInset: CGFloat {
        guard showsWorkspace else { return HideTheme.spacingNone }
        return density.leadingPadding + HideTheme.agentMarkWidth + density.badgeSize
            + density.iconSpacing * 2
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
            HStack(alignment: .top, spacing: HideTheme.spacingNone) {
                if !showsWorkspace {
                    Button {
                        model.core.dispatch(kind: "agent_tree_toggle", payload: ["pane_id": agent.paneID])
                    } label: {
                        Image(systemName: agent.lineageCollapsed ? "chevron.right" : "chevron.down")
                            .foregroundStyle(HideTheme.muted)
                    }
                    .buttonStyle(HideInteractiveButtonStyle())
                    .frame(width: HideTheme.lineageChevronWidth, height: HideTheme.compactAgentBadgeSize)
                    .padding(.top, HideTheme.compactAgentRowVerticalPadding)
                    // The toggle sits directly above the line it opens, so
                    // the branch starts at its own control instead of
                    // floating a column away from it.
                    .padding(.leading, HideTheme.compactAgentLeadingInset)
                    .opacity(agent.lineageChildPaneIDs.isEmpty ? 0 : 1)
                    .disabled(agent.lineageChildPaneIDs.isEmpty)
                    .accessibilityHidden(agent.lineageChildPaneIDs.isEmpty)
                    .hideTooltip(agent.lineageCollapsed ? "Expand descendants" : "Collapse descendants")
                }
                AgentRow(
                    presentation: AgentRowPresentation(
                        agent: agent,
                        density: density,
                        connected: model.agentsConnected,
                        // The same instrumentation the pane header resolved,
                        // read off the pane rather than judged again here.
                        children: model.paneMetadata(for: agent.paneID)?.children
                    ),
                    style: .shell(density: density),
                    density: density,
                    isFocused: model.focusedPaneID == agent.paneID,
                    shortcutNumber: model.agentShortcutNumber(paneID: agent.paneID),
                    shortcutVisible: shortcutVisible,
                    leadingInset: showsWorkspace ? nil : HideTheme.spacingNone,
                    action: { model.selectAgent(agent) }
                )
            }
            if !showsWorkspace, let badge = agent.lineageWorktreeBadge {
                HideBadge(label: badge, color: HideTheme.secondary)
            }
            if let hint = showsWorkspace ? agent.raisedHint : agent.lineageHint {
                let fallbackParentLabel = hint.replacingOccurrences(of: "↳ from ", with: "")
                let parentLabel = agent.spawnOriginPaneID.flatMap(model.paneIdentity(for:))
                    ?? fallbackParentLabel
                // Keep the relationship in the same leading metadata slot as
                // the agent identity instead of drawing a debug-looking line
                // at the sidebar edge. When the origin remains live this is
                // also the direct return action (PRD B7, B9, B23).
                if let origin = agent.spawnOriginPaneID {
                    Button {
                        model.selectAgent(paneID: origin)
                    } label: {
                        HStack(spacing: HideTheme.spacingXXS) {
                            Image(systemName: "arrow.turn.up.left")
                            Text(parentLabel)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                        .hideFont(size: HideTheme.Typography.micro, weight: .medium)
                        .foregroundStyle(HideTheme.secondary)
                    }
                    .buttonStyle(HideInteractiveButtonStyle())
                    .padding(.leading, relationshipLeadingInset)
                    .accessibilityLabel("Return to parent \(parentLabel)")
                    .hideTooltip("Return to parent \(parentLabel)")
                } else {
                    HStack(spacing: HideTheme.spacingXXS) {
                        Image(systemName: "arrow.turn.up.left")
                        Text(parentLabel)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    .hideFont(size: HideTheme.Typography.micro)
                    .foregroundStyle(HideTheme.muted)
                    .padding(.leading, relationshipLeadingInset)
                }
            }
        }
        .padding(.leading, showsWorkspace ? HideTheme.spacingNone : HideTheme.lineageInset(depth: agent.lineageDepth))
        // Drawn over the padded row, so the guide's own geometry and the
        // row's inset are measured from the same leading edge and the elbow
        // lands on the child's mark rather than near it.
        .overlay(alignment: .leading) {
            if !showsWorkspace && (agent.lineageDepth > 0 || guide.startsChildren || !guide.continuing.isEmpty) {
                LineageGuideView(
                    depth: agent.lineageDepth,
                    guide: guide,
                    hasToggle: !agent.lineageChildPaneIDs.isEmpty
                )
            }
        }

        .animation(.easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)), value: (shortcutVisible))
        .accessibilityIdentifier("hide-agent-\(agent.id)")
        .hideTooltip(agent.identityLabel, command: model.agentShortcutNumber(paneID: agent.paneID).map(HideCommand.agent), inline: true)
    }
}
