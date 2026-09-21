import SwiftUI

struct ArchiveDetailView: View {
    @EnvironmentObject private var model: ShellModel
    let detail: CoreArchiveDetailSnapshot

    var body: some View {
        if let reason = detail.unavailableReason {
            HideEmptyState { Label("Session unavailable", systemImage: "exclamationmark.triangle") } description: { Text(reason) }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if detail.kind == "session" {
            ArchiveSessionDetail(detail: detail)
        } else if let memory = detail.memory {
            ArchiveMemoryDetail(memory: memory)
        } else {
            HideEmptyState { Label("Archive unavailable", systemImage: "archivebox") } description: { Text("The selected item no longer has readable detail.") }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct ArchiveSessionDetail: View {
    @EnvironmentObject private var model: ShellModel
    let detail: CoreArchiveDetailSnapshot

    private var provider: ConversationProvider { detail.provider == "Claude Code" ? .claude : .codex }

    private var projection: (messages: [ConversationMessage], injected: [CoreArchiveEventSnapshot], readyCount: Int) {
        var messages: [ConversationMessage] = []
        var injected: [CoreArchiveEventSnapshot] = []
        var pendingAttached: Int?
        var pendingAttachedIDs: [String] = []
        var readyCount = 0
        for (index, event) in detail.events.enumerated() {
            if event.kind == "injected" {
                injected.append(event)
                if let count = event.memoryAttachedCount, count > 0 {
                    if let messageIndex = messages.indices.last, messages[messageIndex].role == .user {
                        messages[messageIndex].memoryAttachedCount = count
                        messages[messageIndex].memoryAttachedItemIDs = event.memoryAttachedItemIDs ?? []
                    } else if messages.isEmpty {
                        readyCount = count
                    } else {
                        pendingAttached = count
                        pendingAttachedIDs = event.memoryAttachedItemIDs ?? []
                    }
                }
                continue
            }
            guard event.role == "user" || event.role == "assistant" else { continue }
            var message = ConversationMessage(
                line: index + 1,
                role: event.role == "assistant" ? .assistant : .user,
                text: event.text,
                timestamp: Date(timeIntervalSince1970: TimeInterval(event.atUnixMS) / 1_000)
            )
            if message.role == .user, let count = pendingAttached {
                message.memoryAttachedCount = count
                message.memoryAttachedItemIDs = pendingAttachedIDs
                pendingAttached = nil
                pendingAttachedIDs = []
            }
            messages.append(message)
        }
        return (messages, injected, readyCount)
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                HideBadge(label: detail.provider ?? "Session", color: HideTheme.secondary)
                Text(detail.title).hideFont(size: HideTheme.Typography.subhead, weight: .medium).lineLimit(1)
                Spacer(minLength: 0)
                if let label = SessionsPresentation.readyLabel(projection.readyCount) {
                    Text(label)
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(HideTheme.secondary)
                }
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(height: HideTheme.Control.regularHeight)
            Rectangle().fill(HideTheme.divider).frame(height: 1)
            if projection.messages.isEmpty {
                HideEmptyState { Label("No conversation messages", systemImage: "text.bubble") } description: { Text("This session contains no readable human or assistant turns.") }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ConversationLedgerView(
                    messages: projection.messages,
                    provider: provider,
                    activity: "stopped",
                    now: Date(),
                    textScale: CGFloat(model.editorTextScale),
                    isKeyboardFocused: false,
                    openLink: openLedgerLink
                )
            }
            if !projection.injected.isEmpty {
                Text("Injected context · \(projection.injected.count)")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.secondary)
                    .padding(HideTheme.spacingMD)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Injected context, \(projection.injected.count) events")
            }
        }
        .accessibilityIdentifier("archive-session-detail")
    }

    private func openLedgerLink(_ value: String) {
        guard let components = URLComponents(string: value),
              components.scheme == "hide-memory",
              components.host == "this-turn" else { return }
        let raw = components.queryItems?.first(where: { $0.name == "items" })?.value ?? ""
        model.openMemoryForTurn(raw.split(separator: ",").map(String.init))
    }

}

private struct ArchiveMemoryDetail: View {
    @EnvironmentObject private var model: ShellModel
    let memory: CoreMemoryDetailSnapshot
    @State private var editing = false
    @State private var draft = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                HStack(spacing: HideTheme.spacingSM) {
                    HideBadge(label: memory.lifecycle == "active" ? "Active" : memory.lifecycle.capitalized,
                              color: memory.lifecycle == "active" ? HideTheme.success : HideTheme.warning)
                    Text("Revision \(memory.revision)").foregroundStyle(HideTheme.secondary)
                    Text("Learned \(sessionTime(memory.learnedAtUnixMS))").foregroundStyle(HideTheme.secondary)
                    Spacer(minLength: 0)
                    Text("Provided to \(memory.providedSessionCount) sessions").foregroundStyle(HideTheme.secondary)
                }
                .hideFont(size: HideTheme.Typography.caption)

                if editing {
                    HideMultilineEditor(
                        text: $draft,
                        accessibilityLabel: "Memory content",
                        minimumHeight: HideTheme.Editor.memoryMinimumHeight
                    )
                    HStack {
                        Button("Cancel") { editing = false; draft = memory.body }
                            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                        Button("Save") {
                            model.applyMemoryAction("edit", itemID: memory.id, body: draft)
                            editing = false
                        }
                        .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || draft == memory.body)
                    }
                } else {
                    Text(memory.body)
                        .hideFont(size: HideTheme.Typography.title)
                        .foregroundStyle(HideTheme.primary)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                    HStack {
                        Button("Edit") {
                            draft = memory.body
                            editing = true
                            model.keepActiveEditorTabOpen()
                        }
                        .buttonStyle(HideTextButtonStyle())
                        Button("Forget", role: .destructive) { model.applyMemoryAction("forget", itemID: memory.id) }
                            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    }
                }

                if let existing = memory.conflictExistingID, let candidate = memory.conflictCandidateID {
                    VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                        Text("Memory needs review")
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .foregroundStyle(HideTheme.warning)
                        HStack(spacing: HideTheme.spacingSM) {
                            Button("Keep existing") {
                                resolveConflict(existing: existing, candidate: candidate, choice: "keep_existing")
                            }
                            Button("Replace with new") {
                                resolveConflict(existing: existing, candidate: candidate, choice: "replace_with_new")
                            }
                            Button("Forget both", role: .destructive) {
                                resolveConflict(existing: existing, candidate: candidate, choice: "forget_both")
                            }
                        }
                        .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    }
                }

                ArchiveDetailSection(title: "Sources", count: memory.sourceCount) {
                    ForEach(memory.sources) { source in
                        Button {
                            if source.available { model.openSession(source.sessionID) }
                        } label: {
                            HStack(spacing: HideTheme.spacingSM) {
                                HideBadge(label: source.provider == "codex" ? "Codex" : "Claude Code", color: HideTheme.secondary, dimmed: !source.available)
                                Text(source.sessionID).lineLimit(1).truncationMode(.middle)
                                Spacer(minLength: 0)
                                if !source.available { Text("Unavailable").foregroundStyle(HideTheme.warning) }
                            }
                            .hideFont(size: HideTheme.Typography.caption)
                        }
                        .buttonStyle(HideInteractiveButtonStyle())
                        .disabled(!source.available)
                        .accessibilityLabel("\(source.provider), source session \(source.sessionID), \(source.available ? "Available" : "Unavailable")")
                    }
                }

                ArchiveDetailSection(title: "Revision history", count: memory.revisions.count) {
                    ForEach(memory.revisions) { revision in
                        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                            Text("Revision \(revision.revision) · \(revision.lifecycle.capitalized)")
                                .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                                .foregroundStyle(HideTheme.secondary)
                            Text(revision.body).hideFont(size: HideTheme.Typography.body).textSelection(.enabled)
                        }
                    }
                }
            }
            .padding(HideTheme.spacingLG)
        }
        .onAppear { draft = memory.body }
        .accessibilityIdentifier("archive-memory-detail")
    }

    private func resolveConflict(existing: String, candidate: String, choice: String) {
        model.applyMemoryAction(
            "resolve_conflict",
            itemID: existing,
            candidateID: candidate,
            conflictChoice: choice
        )
    }
}

private struct ArchiveDetailSection<Content: View>: View {
    let title: String
    let count: Int
    @ViewBuilder let content: Content

    init(title: String, count: Int, @ViewBuilder content: () -> Content) {
        self.title = title
        self.count = count
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Text("\(title) · \(count)")
                .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
            content
        }
    }
}
