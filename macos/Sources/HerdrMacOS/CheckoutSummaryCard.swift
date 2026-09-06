import SwiftUI

/// The right panel's summary card for the selected checkout.
///
/// The card is symbols and numbers by decision: what a branch is, how far it
/// is from its base, whether a pull request is open, what is running in it,
/// and what it costs on disk - each as a short row, never as a sentence. The
/// only prose allowed is an exception state, and there is at most one line of
/// it (G1).
struct CheckoutSummaryCard: View {
    @EnvironmentObject private var model: ShellModel

    private var checkout: CoreCheckoutSnapshot? { model.focusedCheckout }
    private var card: CoreCheckoutCard { model.card }
    private var workspace: CoreWorkspaceSnapshot? { model.focusedWorkspace }

    var body: some View {
        if let checkout, !model.isRemoteContext {
            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                header(checkout)
                rows(checkout)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(HideTheme.elevated.opacity(0.5))
            .overlay(alignment: .bottom) {
                Rectangle().fill(HideTheme.divider).frame(height: HideTheme.Layout.hairlineWidth)
            }
            .accessibilityIdentifier("checkout-card")
        }
    }

    // MARK: - Header

    /// Two lines: what this checkout is, and what it is measured against.
    /// The totals sit at the trailing edge of each so they line up as a
    /// column rather than trailing the names at whatever width those are.
    @ViewBuilder
    private func header(_ checkout: CoreCheckoutSnapshot) -> some View {
        let isGit = workspace?.isGit ?? false
        HStack(spacing: HideTheme.spacingSM) {
            Text(isGit ? (checkout.branch ?? checkout.label) : checkout.label)
                .hideFont(size: 12, weight: .semibold)
                .foregroundStyle(HideTheme.primary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: HideTheme.spacingXS)
            if isGit, checkout.addedLines > 0 || checkout.removedLines > 0 {
                lineDelta(added: checkout.addedLines, removed: checkout.removedLines)
                    .accessibilityLabel(
                        "\(checkout.addedLines) lines added, \(checkout.removedLines) removed since \(checkout.baseBranch ?? "the base branch")"
                    )
            }
        }
        .help(checkout.path)

        if isGit, let base = checkout.baseBranch {
            HStack(spacing: HideTheme.spacingSM) {
                Text("→ \(base)")
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
                    .lineLimit(1)
                Spacer(minLength: HideTheme.spacingXS)
                Text("↑\(checkout.ahead) ↓\(checkout.behind)")
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
                    .help("\(checkout.ahead) commits ahead of \(base), \(checkout.behind) behind")
                    .accessibilityLabel(
                        "\(checkout.ahead) commits ahead of \(base), \(checkout.behind) behind"
                    )
            }
        }
    }

    private func lineDelta(added: Int, removed: Int) -> some View {
        HStack(spacing: HideTheme.spacingXS) {
            Text("+\(added)")
                .foregroundStyle(HideTheme.diffAdded)
            Text("-\(removed)")
                .foregroundStyle(HideTheme.diffRemoved)
        }
        .hideFont(size: 10, design: .monospaced)
    }

    // MARK: - Rows

    @ViewBuilder
    private func rows(_ checkout: CoreCheckoutSnapshot) -> some View {
        let isGit = workspace?.isGit ?? false
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            if isGit {
                unpushedRow(checkout)
                changedFilesRow(checkout)
                pullRequestRow(checkout)
            }
            agentRow()
            portsRow()
            diskRow()
            if let notice = CheckoutCardPresentation.githubNotice(card.github), isGit {
                // The one sentence the card is allowed: gh is missing, logged
                // out, or the lookup failed and this is why.
                Text(notice)
                    .hideFont(size: 10)
                    .foregroundStyle(HideTheme.warning)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("checkout-card-github-notice")
            }
            removeRow(checkout)
        }
    }

    /// Present only when the branch tracks a remote. A branch that tracks none
    /// has nowhere to push, which is a different fact from having nothing to
    /// push, so the row is absent rather than showing a zero.
    @ViewBuilder
    private func unpushedRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        if let unpushed = checkout.unpushed {
            CardRow(icon: "arrow.up.circle", identifier: "checkout-card-unpushed") {
                Text("↑\(unpushed.count) \(unpushed.remote)")
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(unpushed.count > 0 ? HideTheme.warning : HideTheme.secondary)
            }
            .help("\(unpushed.count) commits not pushed to \(unpushed.remote)")
            .accessibilityLabel("\(unpushed.count) commits not pushed to \(unpushed.remote)")
        }
    }

    @ViewBuilder
    private func changedFilesRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        if checkout.changedFileCount > 0 {
            Button { model.selectRightPanelSection(.changes) } label: {
                CardRow(icon: "doc.badge.ellipsis", identifier: "checkout-card-changes") {
                    Text("\(checkout.changedFileCount)")
                        .hideFont(size: 10, design: .monospaced)
                        .foregroundStyle(HideTheme.warning)
                }
            }
            .buttonStyle(.plain)
            .help("\(checkout.changedFileCount) files changed and not committed")
            .accessibilityLabel("\(checkout.changedFileCount) uncommitted files. Opens the changes view")
        }
    }

    /// The pull request, its badge, and when the lookup last succeeded.
    /// A first lookup shows a spinner; a failed one keeps the previous answer
    /// and says how old it is.
    @ViewBuilder
    private func pullRequestRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        if card.github.loading {
            CardRow(icon: "arrow.triangle.pull", identifier: "checkout-card-pull-request-loading") {
                ProgressView().controlSize(.small).tint(HideTheme.secondary)
            }
            .accessibilityLabel("Looking up the pull request")
        } else if let pullRequest = checkout.pullRequest {
            Button { model.openPullRequest(pullRequest) } label: {
                CardRow(icon: "arrow.triangle.pull", identifier: "checkout-card-pull-request") {
                    Text("#\(pullRequest.number)")
                        .hideFont(size: 10, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                    CardBadge(
                        label: CheckoutCardPresentation.badgeLabel(
                            pullRequest.badge,
                            review: pullRequest.review
                        ),
                        color: CheckoutCardPresentation.badgeColor(
                            pullRequest.badge,
                            review: pullRequest.review
                        ),
                        dimmed: card.github.stale
                    )
                    if let merged = pullRequest.mergedAtUnixMS {
                        Text(CheckoutCardPresentation.relativeAge(fromUnixMS: merged))
                            .hideFont(size: 9)
                            .foregroundStyle(HideTheme.muted)
                    }
                    if let stale = CheckoutCardPresentation.staleNotice(card.github) {
                        Text(stale)
                            .hideFont(size: 9)
                            .foregroundStyle(HideTheme.muted)
                            .accessibilityIdentifier("checkout-card-stale")
                    }
                }
            }
            .buttonStyle(.plain)
            .help(pullRequest.url)
            .accessibilityLabel(
                "Pull request \(pullRequest.number), \(CheckoutCardPresentation.badgeLabel(pullRequest.badge, review: pullRequest.review)). Opens it in the browser"
            )
        }
    }

    @ViewBuilder
    private func agentRow() -> some View {
        let agents = model.agents(in: checkout)
        if !agents.isEmpty {
            CardRow(icon: "cpu", identifier: "checkout-card-agents") {
                Circle()
                    .fill(agentColor(agents))
                    .frame(width: 6, height: 6)
                Text("\(agents.count)")
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
            }
            .accessibilityLabel(agents.count == 1 ? "1 agent running here" : "\(agents.count) agents running here")
        }
    }

    private func agentColor(_ agents: [SidebarAgent]) -> Color {
        if agents.contains(where: { $0.demand == "error" }) { return HideTheme.danger }
        if agents.contains(where: { $0.group == AgentGroup.needsYou.rawValue }) {
            return HideTheme.warning
        }
        if agents.contains(where: { $0.group == AgentGroup.working.rawValue }) { return HideTheme.accent }
        return HideTheme.secondary
    }

    @ViewBuilder
    private func portsRow() -> some View {
        let ports = model.portsInFocusedCheckout
        if !ports.isEmpty {
            CardRow(icon: "network", identifier: "checkout-card-ports") {
                ForEach(ports, id: \.self) { port in
                    Button { model.openPanePort(port) } label: {
                        Text(":\(String(port))")
                            .hideFont(size: 9, weight: .semibold)
                            .foregroundStyle(HideTheme.accent)
                            .padding(.horizontal, HideTheme.spacingXS)
                            .frame(height: HideTheme.Layout.panelCollapseControlSize)
                            .background(
                                RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                                    .fill(HideTheme.elevated)
                            )
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("Open http://localhost:\(String(port))")
                    .accessibilityLabel("Open port \(String(port))")
                }
            }
        }
    }

    /// One checkout is measured at a time, so the number is either this
    /// checkout's or still being taken - never the last one's.
    @ViewBuilder
    private func diskRow() -> some View {
        CardRow(icon: "internaldrive", identifier: "checkout-card-disk") {
            if card.diskMeasuring {
                Text("measuring…")
                    .hideFont(size: 10)
                    .foregroundStyle(HideTheme.muted)
                    .accessibilityIdentifier("checkout-card-disk-measuring")
            } else if let reason = card.disk.unavailableReason {
                Text(reason)
                    .hideFont(size: 10)
                    .foregroundStyle(HideTheme.warning)
            } else if let total = card.disk.totalBytes {
                Text(CheckoutCardPresentation.formattedBytes(total))
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
                if let name = card.disk.largestChildName, let bytes = card.disk.largestChildBytes {
                    Text("\(name) \(CheckoutCardPresentation.formattedBytes(bytes))")
                        .hideFont(size: 9)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: HideTheme.spacingXS)
            Button(action: model.refreshCheckoutCard) {
                Image(systemName: "arrow.clockwise")
                    .hideFont(size: 9, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Read the pull request, the counts, and the size again")
            .accessibilityLabel("Refresh this checkout")
            .accessibilityIdentifier("checkout-card-refresh")
        }
    }

    /// All deletion surfaces read the same core gate.
    @ViewBuilder
    private func removeRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        if let gate = card.deletionGate {
            VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                Button { model.requestDeleteWorktree(checkout) } label: {
                    Text(gate.buttonLabel)
                        .hideFont(size: 10, weight: .medium)
                        .foregroundStyle(
                            gate.blockedReason == nil ? HideTheme.danger : HideTheme.muted
                        )
                        .padding(.horizontal, HideTheme.spacingSM)
                        .frame(height: 22)
                        .background(
                            RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                                .fill(HideTheme.panel)
                        )
                        .overlay {
                            RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                                .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(gate.blockedReason != nil)
                .accessibilityIdentifier("checkout-card-remove")
                if let reason = gate.blockedReason {
                    Text(reason)
                        .hideFont(size: 9)
                        .foregroundStyle(HideTheme.muted)
                        .accessibilityIdentifier("checkout-card-remove-blocked")
                }
            }
            .padding(.top, HideTheme.spacingXXS)
        }
    }
}

/// One card row: a fixed-width symbol and whatever the row shows beside it.
/// The symbol column is what lets the rows read as a list rather than as
/// ragged sentences.
private struct CardRow<Content: View>: View {
    let icon: String
    let identifier: String
    @ViewBuilder let content: Content

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Image(systemName: icon)
                .hideFont(size: 9)
                .foregroundStyle(HideTheme.muted)
                .frame(width: 12)
            content
            Spacer(minLength: 0)
        }
        .frame(minHeight: 18)
        // A container, not a merged element: merging folds the row's
        // buttons - refresh, a port, the pull request - into one element
        // whose press runs whichever came first, so assistive tools could
        // not reach the others at all.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(identifier)
    }
}

private struct CardBadge: View {
    let label: String
    let color: Color
    let dimmed: Bool

    var body: some View {
        Text(label)
            .hideFont(size: 8, weight: .medium)
            .foregroundStyle(dimmed ? color.opacity(0.5) : color)
            .padding(.horizontal, 5)
            .frame(height: 16)
            .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
    }
}
