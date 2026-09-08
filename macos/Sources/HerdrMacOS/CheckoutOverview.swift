import SwiftUI

/// Current checkout facts share one section. Historical authorship is never
/// inferred from a pane that happens to occupy this checkout now.
struct CheckoutOverview: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        if model.isRemoteContext {
            ContentUnavailableView("Overview is local only", systemImage: "externaldrive",
                                   description: Text("Git context is read on this Mac."))
        } else if let checkout = model.focusedCheckout {
            ScrollView {
                VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                    CheckoutSummaryCard()
                    VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
                        if let worktree = checkout.worktree {
                            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                                heading("Latest commit")
                                if let sha = worktree.headSHA {
                                    Text(String(sha.prefix(12)))
                                        .hideFont(size: HideTheme.Typography.body, design: .monospaced)
                                        .foregroundStyle(HideTheme.primary)
                                        .textSelection(.enabled)
                                        .hideTooltip(sha)
                                    if let subject = worktree.lastCommitSubject {
                                        Text(subject)
                                            .hideFont(size: HideTheme.Typography.body)
                                            .foregroundStyle(HideTheme.primary)
                                            .fixedSize(horizontal: false, vertical: true)
                                    }
                                    if let seconds = worktree.lastCommitUnixSeconds {
                                        Text(Date(timeIntervalSince1970: seconds), style: .date)
                                            .hideFont(size: HideTheme.Typography.caption)
                                            .foregroundStyle(HideTheme.secondary)
                                    }
                                } else {
                                    Text("No commit available")
                                        .hideFont(size: HideTheme.Typography.body)
                                        .foregroundStyle(HideTheme.muted)
                                }
                                if let reason = worktree.unavailableReason {
                                    Text(reason)
                                        .hideFont(size: HideTheme.Typography.caption)
                                        .foregroundStyle(HideTheme.warning)
                                }
                            }
                            .accessibilityIdentifier("overview-latest-commit")
                        }
                        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                            heading("Connected panes")
                            if model.card.panes.isEmpty {
                                Text("No panes in this checkout")
                                    .hideFont(size: HideTheme.Typography.body)
                                    .foregroundStyle(HideTheme.muted)
                            }
                            ForEach(model.card.panes) { pane in
                                Button { model.focusPane(pane.paneID) } label: {
                                    VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                                        HStack(spacing: HideTheme.spacingSM) {
                                            Text(pane.paneID)
                                                .hideFont(size: HideTheme.Typography.body, design: .monospaced)
                                            Spacer(minLength: HideTheme.spacingXS)
                                            Text(pane.status)
                                                .hideFont(size: HideTheme.Typography.caption)
                                                .foregroundStyle(HideTheme.secondary)
                                        }
                                        Text(pane.title)
                                            .hideFont(size: HideTheme.Typography.body)
                                            .lineLimit(2)
                                        if let parent = pane.parentPaneID {
                                            Text("From \(parent)")
                                                .hideFont(size: HideTheme.Typography.caption)
                                                .foregroundStyle(HideTheme.secondary)
                                        }
                                        if let session = pane.sessionID {
                                            Text("Session \(session)")
                                                .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                                                .foregroundStyle(HideTheme.muted)
                                                .lineLimit(1)
                                                .truncationMode(.middle)
                                        }
                                    }
                                    .foregroundStyle(HideTheme.primary)
                                    .padding(HideTheme.spacingSM)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
                                }
                                .buttonStyle(.plain)
                                .hideTooltip("Focus \(pane.paneID)")
                                .accessibilityIdentifier("overview-pane-\(pane.paneID)")
                            }
                            Text("Current checkout context. Commit authoring panes are not recorded.")
                                .hideFont(size: HideTheme.Typography.caption)
                                .foregroundStyle(HideTheme.muted)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .padding(.horizontal, HideTheme.spacingMD)
                    .padding(.bottom, HideTheme.spacingMD)
                }
            }
            .accessibilityIdentifier("checkout-overview")
        } else {
            ContentUnavailableView("No workspace", systemImage: "folder",
                                   description: Text("Choose a workspace to see its context."))
        }
    }

    private func heading(_ text: String) -> some View {
        Text(text)
            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
            .foregroundStyle(HideTheme.secondary)
    }
}
