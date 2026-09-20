import AppKit
import SwiftUI

enum SessionsListPresentationState: Equatable {
    case remote
    case loading
    case unavailable(String)
    case empty
    case noMatches
    case rows
}

enum MemoryListPresentationState: Equatable {
    case off
    case empty
    case noMatches
    case rows
}

enum SessionsPresentation {
    static func sessions(_ snapshot: CoreSessionsSnapshot, remote: Bool) -> SessionsListPresentationState {
        if remote { return .remote }
        if snapshot.loading && snapshot.totalSessionCount == 0 { return .loading }
        if let reason = snapshot.unavailableReason { return .unavailable(reason) }
        if snapshot.rows.isEmpty { return snapshot.totalSessionCount == 0 ? .empty : .noMatches }
        return .rows
    }

    static func memory(_ snapshot: CoreSessionsSnapshot) -> MemoryListPresentationState {
        if !snapshot.memoryEnabled { return .off }
        if snapshot.memories.isEmpty { return snapshot.query.isEmpty ? .empty : .noMatches }
        return .rows
    }

    static func attachedLabel(_ count: Int) -> String? {
        count > 0 ? "Memory attached \(count)" : nil
    }

    static func readyLabel(_ count: Int) -> String? {
        count > 0 ? "Project Memory ready · \(count)" : nil
    }

    static func analysisLabel(_ analysis: CoreMemoryAnalysisSnapshot) -> String? {
        if analysis.state.isEmpty || analysis.state == "idle" {
            return nil
        }
        if analysis.state == "complete", analysis.analyzed == 0, analysis.failed == 0 {
            return nil
        }
        if analysis.state == "analyzing", analysis.discovered > 0 {
            return "Analyzing \(analysis.analyzed) of \(analysis.discovered) sessions"
        }
        return analysis.message ?? "Analysis paused"
    }

    static func sessionAccessibility(_ row: CoreSessionRowSnapshot) -> String {
        "\(row.providerLabel), \(row.firstHumanRequest ?? row.title ?? "Untitled session"), \(URL(fileURLWithPath: row.checkoutPath).lastPathComponent), \(sessionTime(row.updatedAtUnixMS)), \(row.unavailableReason == nil ? "Available" : "Session unavailable")"
    }

    static func memoryAccessibility(_ row: CoreMemoryRowSnapshot) -> String {
        "\(row.body), \(row.sourceCount) sources, \(row.lifecycle)"
    }
}

struct SessionsPanel: View {
    @EnvironmentObject private var model: ShellModel
    @State private var query = ""
    @State private var selection = HideSearchSelection()
    @State private var showHookConfirmation = false
    @State private var showDeleteConfirmation = false

    private var state: CoreSessionsSnapshot { model.sessions }
    private var resultIDs: [String] {
        state.mode == .sessions ? state.rows.map(\.id) : state.memories.map(\.id)
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HideChoiceGroup(
                label: "Session archive mode",
                values: CoreSessionsMode.allCases.map(\.rawValue),
                selection: Binding(
                    get: { state.mode.rawValue },
                    set: { value in
                        if let mode = CoreSessionsMode(rawValue: value) { model.setSessionsMode(mode) }
                    }
                ),
                title: { CoreSessionsMode(rawValue: $0)?.title ?? $0 },
                appearance: .segmented,
                identifier: { "sessions-mode-\($0)" },
                equalWidth: true
            )
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)

            if state.mode == .sessions {
                sessionControls
            } else if state.memoryEnabled {
                memoryControls
            }

            Rectangle().fill(HideTheme.divider).frame(height: 1)

            Group {
                if state.mode == .sessions { sessionsBody }
                else { memoryBody }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .task {
            query = state.query
            model.refreshSessions()
        }
        .onChange(of: state.query) { _, value in
            if query != value { query = value }
        }
        .alert("Update agent hooks?", isPresented: $showHookConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Update agent hooks") { model.applyMemoryAction("update_hooks") }
        } message: {
            Text("Hide will update only its marked SessionStart and UserPromptSubmit entries in the local Codex and Claude Code config files. Other settings and hook entries are preserved.")
        }
        .alert("Delete Project Memory data?", isPresented: $showDeleteConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Delete Memory data", role: .destructive) { model.applyMemoryAction("delete") }
        } message: {
            Text("This permanently deletes this Project's derived sources, locators, cursors, memories, revisions, search data, and provided-history receipts. Raw Codex and Claude Code sessions remain. This cannot be undone.")
        }
        .accessibilityIdentifier("sessions-panel")
    }

    private var sessionControls: some View {
        VStack(spacing: HideTheme.spacingSM) {
            HideChoiceGroup(
                label: "Session provider",
                values: CoreSessionsProviderFilter.allCases.map(\.rawValue),
                selection: Binding(
                    get: { state.providerFilter.rawValue },
                    set: { value in
                        if let filter = CoreSessionsProviderFilter(rawValue: value) {
                            updateFilter(filter, query: query)
                        }
                    }
                ),
                title: { CoreSessionsProviderFilter(rawValue: $0)?.title ?? $0 },
                appearance: .segmented,
                identifier: { "sessions-provider-\($0)" },
                equalWidth: true
            )
            HideSearchField(
                placeholder: "Search sessions",
                text: Binding(get: { query }, set: { updateFilter(state.providerFilter, query: $0) }),
                selection: $selection,
                resultIDs: resultIDs,
                activate: activateSearchSelection,
                dismiss: clearFilters
            )
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingSM)
    }

    private var memoryControls: some View {
        VStack(spacing: HideTheme.spacingSM) {
            HStack(spacing: HideTheme.spacingSM) {
                HideBadge(label: "Memory on", color: HideTheme.success)
                if !state.thisTurnMemoryIDs.isEmpty {
                    HideBadge(label: "This turn", color: HideTheme.accent)
                    Button("Show all") { model.openMemoryForTurn([]) }
                        .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                }
                Text("\(state.memoryActiveCount) active")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.secondary)
                Spacer(minLength: 0)
                Button("Turn off") { model.applyMemoryAction("disable") }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
            }
            HideSearchField(
                placeholder: "Search memories",
                text: Binding(get: { query }, set: { updateFilter(state.providerFilter, query: $0) }),
                selection: $selection,
                resultIDs: resultIDs,
                activate: activateSearchSelection,
                dismiss: { updateFilter(state.providerFilter, query: "") }
            )
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingSM)
    }

    @ViewBuilder
    private var sessionsBody: some View {
        switch SessionsPresentation.sessions(state, remote: model.isRemoteContext) {
        case .remote:
            notice("Sessions are local only", systemImage: "externaldrive", detail: "Choose a local Project to browse Codex and Claude Code sessions.")
        case .loading:
            loading("Loading sessions")
        case let .unavailable(reason):
            notice("Sessions unavailable", systemImage: "exclamationmark.triangle", detail: reason)
        case .empty:
            notice("No sessions yet", systemImage: "text.bubble", detail: "Start an agent in this Project to create the first session.")
        case .noMatches:
            VStack(spacing: HideTheme.spacingMD) {
                HideEmptyState { Label("No matching sessions", systemImage: "magnifyingglass") } description: { Text("Clear filters to show this Project's sessions.") }
                Button("Clear filters", action: clearFilters).buttonStyle(HideTextButtonStyle())
            }
        case .rows:
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    ForEach(state.rows) { row in
                        SessionArchiveRow(
                            row: row,
                            activate: { model.openSession(row.id) },
                            retry: { model.refreshSessions() }
                        )
                    }
                }
                .padding(.vertical, HideTheme.spacingXS)
            }
        }
    }

    @ViewBuilder
    private var memoryBody: some View {
        if SessionsPresentation.memory(state) == .off {
            MemoryDisclosure(
                keptCount: state.memoryActiveCount,
                turnOn: { model.applyMemoryAction("enable") },
                updateHooks: { showHookConfirmation = true },
                analysis: state.analysis
            )
        } else {
            VStack(spacing: HideTheme.spacingNone) {
                if state.memoryConflictCount > 0 {
                    actionNotice("\(state.memoryConflictCount) memory needs review", action: "Review") {
                        if let row = state.memories.first(where: { $0.lifecycle == "conflicting" }) {
                            model.openMemory(row.id)
                        }
                    }
                }
                if state.memoryCapacityReached {
                    actionNotice("Memory capacity reached", action: "Review memories") {
                        model.openMemoryForTurn([])
                        updateFilter(state.providerFilter, query: "")
                    }
                }
                if !state.analysis.state.isEmpty && state.analysis.state != "idle" {
                    analysisNotice
                }
                if let noticeState = state.notice {
                    actionNotice(noticeState.message, action: noticeState.undoBatchID == nil ? nil : "Undo") {
                        if let batch = noticeState.undoBatchID { model.applyMemoryAction("undo", batchID: batch) }
                    }
                }
                switch SessionsPresentation.memory(state) {
                case .empty:
                    notice("No memories yet", systemImage: "brain.head.profile", detail: "New durable decisions and rules appear here after session analysis.")
                case .noMatches:
                    notice("No matching memories", systemImage: "brain.head.profile", detail: "Clear the search to show Project Memory.")
                case .rows:
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                            ForEach(state.memories) { row in
                                MemoryArchiveRow(row: row, activate: { model.openMemory(row.id) })
                            }
                        }
                        .padding(.vertical, HideTheme.spacingXS)
                    }
                case .off:
                    EmptyView()
                }
                HStack {
                    Button("Delete Memory data…", role: .destructive) { showDeleteConfirmation = true }
                        .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    Spacer()
                }
                .padding(HideTheme.spacingSM)
            }
        }
    }

    @ViewBuilder
    private var analysisNotice: some View {
        if let progress = SessionsPresentation.analysisLabel(state.analysis) {
            actionNotice(progress, action: state.analysis.action.map(actionTitle)) {
                if state.analysis.action == "update_hooks" { showHookConfirmation = true }
                else if state.analysis.action == "open_settings" || state.analysis.action == "sign_in" { model.showSettings = true }
                else { model.applyMemoryAction("retry") }
            }
        }
    }

    private func actionTitle(_ action: String) -> String {
        switch action {
        case "update_hooks": "Update hooks"
        case "open_settings": "Open Settings"
        case "sign_in": "Sign in"
        default: "Retry"
        }
    }

    private func actionNotice(_ text: String, action: String?, perform: @escaping () -> Void) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text(text).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning).lineLimit(2)
            Spacer(minLength: 0)
            if let action { Button(action, action: perform).buttonStyle(HideTextButtonStyle(appearance: .quiet)) }
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingXS)
        .background(HideTheme.elevated)
    }

    private func updateFilter(_ filter: CoreSessionsProviderFilter, query: String) {
        self.query = query
        model.setSessionsFilter(filter, query: query)
    }

    private func clearFilters() { updateFilter(.all, query: "") }

    private func activateSearchSelection() {
        if state.mode == .sessions, let row = selection.entry(in: state.rows) { model.openSession(row.id) }
        if state.mode == .memory, let row = selection.entry(in: state.memories) { model.openMemory(row.id) }
    }

    private func loading(_ label: String) -> some View {
        VStack(spacing: HideTheme.spacingSM) {
            ProgressView().controlSize(.small).tint(HideTheme.secondary)
            Text(label).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.muted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func notice(_ title: String, systemImage: String, detail: String) -> some View {
        HideEmptyState { Label(title, systemImage: systemImage) } description: { Text(detail) }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
    }
}

private struct MemoryDisclosure: View {
    let keptCount: Int
    let turnOn: () -> Void
    let updateHooks: () -> Void
    let analysis: CoreMemoryAnalysisSnapshot

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                Label(keptCount > 0 ? "Memory off · \(keptCount) memories kept" : "Project Memory is off", systemImage: "brain.head.profile")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                Text("Hide can carry durable decisions and rules into later work in this Project.")
                disclosure("Session content is sent to your selected Background AI provider and may use your subscription.")
                disclosure("That provider's retention and deletion terms apply. Hide cannot guarantee deletion after content is sent.")
                disclosure("Derived memories stay on this Mac until you Forget them or delete this Project's Memory data.")
                disclosure("Known credential patterns are excluded from stored memories and search data.")
                if analysis.state == "hooks_need_update" {
                    Button("Update agent hooks", action: updateHooks).buttonStyle(HideTextButtonStyle(appearance: .prominent, density: .regular))
                } else {
                    Button("Turn on Memory", action: turnOn)
                        .buttonStyle(HideTextButtonStyle(appearance: .prominent, density: .regular))
                }
            }
            .hideFont(size: HideTheme.Typography.subhead)
            .foregroundStyle(HideTheme.secondary)
            .padding(HideTheme.spacingLG)
        }
        .accessibilityIdentifier("memory-disclosure")
    }

    private func disclosure(_ text: String) -> some View {
        Label(text, systemImage: "checkmark.circle")
            .labelStyle(.titleAndIcon)
            .fixedSize(horizontal: false, vertical: true)
    }
}

private struct SessionArchiveRow: View {
    let row: CoreSessionRowSnapshot
    let activate: () -> Void
    let retry: () -> Void

    var body: some View {
        Button(action: activate) {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                HStack(spacing: HideTheme.spacingSM) {
                    HideBadge(label: row.providerLabel, color: row.provider == "codex" ? HideTheme.accent : HideTheme.secondary, dimmed: row.unavailableReason != nil)
                    Text(sessionTime(row.updatedAtUnixMS)).hideFont(size: HideTheme.Typography.micro).foregroundStyle(HideTheme.muted)
                    Spacer(minLength: 0)
                }
                Text(row.firstHumanRequest ?? row.title ?? "Untitled session")
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(row.unavailableReason == nil ? HideTheme.primary : HideTheme.muted)
                    .lineLimit(2)
                    .truncationMode(.tail)
                HStack(spacing: HideTheme.spacingXS) {
                    Text(URL(fileURLWithPath: row.checkoutPath).lastPathComponent)
                    if row.unavailableReason != nil { Text("Session unavailable") }
                }
                .hideFont(size: HideTheme.Typography.micro)
                .foregroundStyle(row.unavailableReason == nil ? HideTheme.secondary : HideTheme.warning)
                .lineLimit(1)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .hideTooltip(row.firstHumanRequest ?? row.title ?? row.locator)
        .accessibilityLabel(SessionsPresentation.sessionAccessibility(row))
        .contextMenu {
            if row.unavailableReason != nil { Button("Retry", action: retry) }
            Button("Copy source location") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(row.locator, forType: .string)
            }
        }
    }
}

private struct MemoryArchiveRow: View {
    let row: CoreMemoryRowSnapshot
    let activate: () -> Void

    var body: some View {
        Button(action: activate) {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                Text(row.body).hideFont(size: HideTheme.Typography.body).foregroundStyle(HideTheme.primary).lineLimit(3).truncationMode(.tail)
                HStack(spacing: HideTheme.spacingSM) {
                    Text("\(row.sourceCount) sources")
                    Text(sessionTime(row.updatedAtUnixMS))
                    if row.lifecycle == "conflicting" { Text("Needs review").foregroundStyle(HideTheme.warning) }
                    Spacer(minLength: 0)
                }
                .hideFont(size: HideTheme.Typography.micro)
                .foregroundStyle(HideTheme.secondary)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .hideTooltip(row.body)
        .accessibilityLabel(SessionsPresentation.memoryAccessibility(row))
    }
}

func sessionTime(_ milliseconds: UInt64) -> String {
    Date(timeIntervalSince1970: TimeInterval(milliseconds) / 1_000)
        .formatted(date: .abbreviated, time: .shortened)
}
