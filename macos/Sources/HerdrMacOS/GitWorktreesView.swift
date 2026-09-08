import AppKit
import SwiftUI

struct GitWorktreesView: View {
    @EnvironmentObject private var model: ShellModel
    private var project: CoreProjectWorktrees? { model.core.snapshot?.gitWorktrees }
    private var loading: Bool { model.core.snapshot?.gitWorktreesLoading ?? false }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                Text("base: \(project?.baseBranch ?? "—")")
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .hideTooltip(project?.baseBranchFallback ?? "Base source: \(project?.baseSource ?? "unavailable")")
                Spacer(minLength: HideTheme.spacingNone)
                ZStack {
                    HideIconButton(
                        systemImage: HideTheme.GitIcon.refresh,
                        help: loading ? "Refreshing worktrees, disk usage and pull requests" : "Refresh worktrees, disk usage and pull requests",
                        accessibilityLabel: loading ? "Refreshing worktrees, disk usage and pull requests" : "Refresh worktrees, disk usage and pull requests",
                        variant: .toolbar
                    ) {
                        model.core.dispatch(kind: "git_worktrees_refresh", payload: [:])
                    }
                    .opacity(loading ? 0 : 1)
                    .disabled(loading || model.isRemoteContext)
                    if loading {
                        ProgressView()
                            .controlSize(.small)
                            .allowsHitTesting(false)
                            .accessibilityHidden(true)
                    }
                }
                .accessibilityIdentifier("git-worktrees-refresh")
            }
            .foregroundStyle(HideTheme.secondary)
            .padding(HideTheme.spacingMD)
            if let fallback = project?.baseBranchFallback {
                notice(fallback)
            }
            if model.isRemoteContext || model.core.snapshot?.gitWorktreesRemote == true {
                notice("Git worktrees are available for local repositories only.")
            } else if let reason = project?.unavailableReason {
                notice("Repository unavailable: \(project?.rootPath ?? "")\n\(reason)")
            } else if loading && (project == nil || project?.worktrees.isEmpty == true) {
                HStack(spacing: HideTheme.spacingSM) {
                    ProgressView().controlSize(.small)
                    Text("Reading worktrees").hideFont(size: HideTheme.Typography.body)
                }.padding(HideTheme.spacingMD)
            } else if let project {
                ScrollView([.vertical, .horizontal]) {
                    LazyVStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                        ForEach(project.worktrees) { worktree in
                            GitWorktreeRow(worktree: worktree, root: project.rootPath)
                        }
                        if project.worktrees.count == 1 && project.worktrees.first?.isMain == true {
                            notice("No linked worktrees yet")
                        }
                    }
                }
            } else {
                notice("Choose a local Git repository to read its worktrees.")
            }
            Spacer(minLength: HideTheme.spacingNone)
        }
        .accessibilityIdentifier("git-worktrees")
    }
    private func notice(_ text: String) -> some View {
        Text(text).hideFont(size: HideTheme.Typography.caption)
            .foregroundStyle(HideTheme.muted).padding(HideTheme.spacingMD)
    }
}

private struct GitWorktreeRow: View {
    @EnvironmentObject private var model: ShellModel
    let worktree: CoreGitWorktree
    let root: String

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            HStack(spacing: HideTheme.spacingSM) {
                Text(worktree.label)
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .foregroundStyle(HideTheme.primary)
                if worktree.isMain { HideBadge(label: "main worktree", color: HideTheme.secondary) }
                if worktree.branch == nil { HideBadge(label: "detached", color: HideTheme.muted) }
                if worktree.missing { HideBadge(label: "missing on disk", color: HideTheme.danger) }
                Text(worktree.relativePath(root: root)).foregroundStyle(HideTheme.muted)
                    .hideTooltip(worktree.path)
                Text("\(worktree.paneCount) panes").foregroundStyle(HideTheme.secondary)
                if !worktree.missing {
                    Image(systemName: worktree.merged == true ? HideTheme.GitIcon.merged : HideTheme.GitIcon.unmerged)
                        .foregroundStyle(worktree.merged == true ? HideTheme.success : HideTheme.muted)
                        .hideTooltip(worktree.merged.map { $0 ? "merged into \(worktree.baseBranch ?? "base")" : "not merged" } ?? worktree.unavailableReason ?? "Merge status unavailable")
                    Text(worktree.unavailableReason == nil ? "↑\(worktree.ahead) ↓\(worktree.behind)" : "—")
                        .hideTooltip(worktree.unavailableReason ?? "Compared with \(worktree.baseBranch ?? "base")")
                    Image(systemName: worktree.dirty ? HideTheme.GitIcon.dirty : HideTheme.GitIcon.clean)
                        .foregroundStyle(worktree.dirty ? HideTheme.warning : HideTheme.muted)
                        .hideTooltip(worktree.dirty ? "\(worktree.changedFileCount) uncommitted changes" : "clean")
                    Text(worktree.pushedLabel).hideTooltip(worktree.unavailableReason ?? "State of local remote-tracking refs")
                    Text(worktree.lastFetchAtUnixMS.map { "fetched \(CheckoutCardPresentation.relativeAge(fromUnixMS: $0))" } ?? "fetch time unknown")
                        .foregroundStyle(HideTheme.muted)
                    Text(worktree.disk.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "—")
                        .hideTooltip(worktree.disk.unavailableReason ?? "Disk usage")
                    Text(worktree.measuredAtUnixMS.map { "\(max(0, Int((Date().timeIntervalSince1970 * 1000 - $0) / 60000))) min ago" } ?? "not measured")
                        .foregroundStyle(HideTheme.muted)
                    pullRequest
                }
            }
            .hideFont(size: HideTheme.Typography.caption)
            .foregroundStyle(HideTheme.secondary)
            if let error = worktree.openError {
                Text(error).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.danger)
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(worktree.isMain ? HideTheme.elevated : HideTheme.panel)
        .contentShape(Rectangle())
        .contextMenu {
            if !worktree.missing {
                Button("Open") { model.core.dispatch(kind: "git_worktree_open", payload: ["checkout_path": worktree.path]) }
                Button("Reveal in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: worktree.path)]) }
                Button("Copy path") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(worktree.path, forType: .string)
                }
                if let branch = worktree.branch {
                    Button("Set as base branch") {
                        model.core.dispatch(kind: "git_worktree_set_base", payload: [
                            "repository_root": root,
                            "branch": branch,
                        ])
                    }
                }
                Divider()
            }
            Button(worktree.deletionGate.buttonLabel, role: .destructive) { model.requestDeleteWorktree(worktree) }
                .disabled(worktree.deletionGate.blockedReason != nil)
            if let reason = worktree.deletionGate.blockedReason { Text(reason) }
        }
        .accessibilityIdentifier("git-worktree-\(worktree.path)")
    }

    @ViewBuilder private var pullRequest: some View {
        if worktree.branch == nil {
            Text(" ").hideTooltip("no branch")
        } else if let category = worktree.github.failureCategory {
            Text(category).foregroundStyle(HideTheme.warning)
                .hideTooltip(worktree.github.unavailableReason ?? category)
        } else if let request = worktree.pullRequest {
            HideIconButton(
                image: CheckoutCardPresentation.pullRequestIcon(request),
                imageSize: HideTheme.PullRequest.iconSize,
                color: CheckoutCardPresentation.pullRequestColor(request),
                help: "PR #\(request.number): \(CheckoutCardPresentation.badgeLabel(request.badge, review: request.review))",
                variant: .toolbar
            ) {
                if let url = URL(string: request.url) { NSWorkspace.shared.open(url) }
            }
        } else if let reason = worktree.github.unavailableReason {
            Image(systemName: HideTheme.GitIcon.unavailable).foregroundStyle(HideTheme.warning).hideTooltip(reason)
        } else {
            Image(systemName: HideTheme.GitIcon.noPullRequest).foregroundStyle(HideTheme.muted).hideTooltip("No pull request")
        }
    }
}
