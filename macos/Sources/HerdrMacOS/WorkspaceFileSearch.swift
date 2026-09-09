import Foundation
import SwiftUI

struct WorkspaceFileSearchMatch: Identifiable, Equatable, Sendable {
    let relativePath: String
    let score: Int

    var id: String { relativePath }
}

enum WorkspaceFileSearchIndex {
    static let resultLimit = 80

    static func load(root: URL) async throws -> [String] {
        try await Task.detached(priority: .userInitiated) {
            if let tracked = try gitFiles(root: root) {
                return tracked
            }
            return try directoryFiles(root: root)
        }.value
    }

    static func matches(paths: [String], query: String, limit: Int = resultLimit) async -> [WorkspaceFileSearchMatch] {
        await Task.detached(priority: .userInitiated) {
            ranked(paths: paths, query: query, limit: limit)
        }.value
    }

    static func ranked(paths: [String], query: String, limit: Int = resultLimit) -> [WorkspaceFileSearchMatch] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if needle.isEmpty {
            return paths.prefix(limit).map { WorkspaceFileSearchMatch(relativePath: $0, score: 0) }
        }
        return paths.compactMap { path -> WorkspaceFileSearchMatch? in
            fuzzyScore(candidate: path.lowercased(), query: needle).map {
                WorkspaceFileSearchMatch(relativePath: path, score: $0)
            }
        }
        .sorted {
            $0.score == $1.score
                ? $0.relativePath.localizedStandardCompare($1.relativePath) == .orderedAscending
                : $0.score > $1.score
        }
        .prefix(limit)
        .map { $0 }
    }

    static func fuzzyScore(candidate: String, query: String) -> Int? {
        guard !query.isEmpty else { return 0 }
        var cursor = candidate.startIndex
        var score = 0
        var previous: String.Index?
        for character in query {
            guard let found = candidate[cursor...].firstIndex(of: character) else { return nil }
            let offset = candidate.distance(from: candidate.startIndex, to: found)
            score += 100 - min(offset, 90)
            if let previous, candidate.index(after: previous) == found { score += 35 }
            if found == candidate.startIndex || "/_- .".contains(candidate[candidate.index(before: found)]) { score += 25 }
            previous = found
            cursor = candidate.index(after: found)
        }
        score -= candidate.count
        return score
    }

    private static func gitFiles(root: URL) throws -> [String]? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", root.path, "ls-files", "--cached", "--others", "--exclude-standard", "-z"]
        let output = Pipe()
        let errors = Pipe()
        process.standardOutput = output
        process.standardError = errors
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { return nil }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        return String(decoding: data, as: UTF8.self)
            .split(separator: "\0")
            .map(String.init)
            .filter { !$0.isEmpty }
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    private static func directoryFiles(root: URL) throws -> [String] {
        guard let enumerator = FileManager.default.enumerator(
            at: root,
            includingPropertiesForKeys: [.isDirectoryKey, .isRegularFileKey, .isSymbolicLinkKey],
            options: [.skipsHiddenFiles, .skipsPackageDescendants]
        ) else {
            throw CocoaError(.fileReadUnknown)
        }
        var paths: [String] = []
        for case let url as URL in enumerator {
            let values = try url.resourceValues(forKeys: [.isDirectoryKey, .isRegularFileKey, .isSymbolicLinkKey])
            if values.isSymbolicLink == true {
                if values.isDirectory == true { enumerator.skipDescendants() }
                continue
            }
            if values.isDirectory == true,
               [".git", ".build", "build", "target", "DerivedData"].contains(url.lastPathComponent) {
                enumerator.skipDescendants()
            } else if values.isRegularFile == true {
                paths.append(String(url.path.dropFirst(root.path.count + 1)))
            }
        }
        return paths.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }
}

struct WorkspaceFileSearchSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var paths: [String] = []
    @State private var matches: [WorkspaceFileSearchMatch] = []
    @State private var selection = HideSearchSelection()
    @State private var matchedQuery: String?
    @State private var loadedRoot: URL?
    @State private var indexRevision: UInt64 = 0
    @State private var error: String?
    @State private var loading = true

    private var root: URL? {
        model.focusedCheckout.map { URL(fileURLWithPath: $0.path, isDirectory: true) }
    }

    private var currentMatches: [WorkspaceFileSearchMatch] {
        !loading && loadedRoot == root && matchedQuery == query ? matches : []
    }

    var body: some View {
        let rows = currentMatches
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                HideSearchField(
                    placeholder: "Open file in selected checkout",
                    text: $query,
                    selection: $selection,
                    resultIDs: rows.map(\.id),
                    activate: { open(selection.entry(in: currentMatches)) },
                    dismiss: { dismiss() }
                )
                .frame(maxWidth: .infinity)
                .accessibilityIdentifier("hide-file-search-query")
                if loading { ProgressView().controlSize(.small) }
                HideKeycap(command: .label("Esc"), emphasized: false)
            }
            .padding(HideTheme.spacingLG)

            if let error {
                HideEmptyState(
                    "File index unavailable",
                    systemImage: "exclamationmark.triangle",
                    description: Text(error)
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: HideTheme.spacingXXS) {
                            ForEach(rows) { match in
                                Button { open(match) } label: {
                                    HStack(spacing: HideTheme.spacingSM) {
                                        if let root {
                                            SetiFileIconView(url: root.appendingPathComponent(match.relativePath), size: 13)
                                        }
                                        Text(match.relativePath)
                                            .hideFont(size: HideTheme.Typography.subhead, design: .monospaced)
                                            .foregroundStyle(HideTheme.primary)
                                            .lineLimit(1)
                                            .truncationMode(.middle)
                                        Spacer()
                                        Text("↵")
                                            .foregroundStyle(HideTheme.muted)
                                            .opacity(selection.selectedID == match.id ? 1 : 0)
                                    }
                                    .padding(.horizontal, HideTheme.spacingMD)
                                    .padding(.vertical, HideTheme.spacingSM)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(HideInteractiveButtonStyle())
                                .background(
                                    selection.selectedID == match.id ? HideTheme.accent.opacity(HideTheme.Opacity.emphasisFill) : Color.clear,
                                    in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                                )
                                .accessibilityAddTraits(selection.selectedID == match.id ? .isSelected : [])
                                .accessibilityValue(selection.selectedID == match.id ? "Selected" : "Not selected")
                                .accessibilityIdentifier("hide-file-search-result-\(match.id)")
                                .id(match.id)
                            }
                            if !loading && rows.isEmpty {
                                Text(query.isEmpty ? "No files in this checkout" : "No matching files")
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
        }
        .frame(width: HideTheme.searchSheetSize.width, height: HideTheme.searchSheetSize.height)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .task(id: root) { await load() }
        .task(id: query) { await updateMatches() }
    }

    private func load() async {
        loading = true
        error = nil
        loadedRoot = nil
        paths = []
        matches = []
        indexRevision &+= 1
        guard let root else {
            error = "The selected checkout path is unavailable."
            loading = false
            return
        }
        do {
            let indexed = try await WorkspaceFileSearchIndex.load(root: root)
            guard !Task.isCancelled, root == self.root else { return }
            paths = indexed
            loadedRoot = root
            indexRevision &+= 1
            await updateMatches()
        } catch {
            guard !Task.isCancelled else { return }
            self.error = error.localizedDescription
        }
        guard !Task.isCancelled, root == self.root else { return }
        loading = false
    }

    private func updateMatches() async {
        let requestedQuery = query
        let revision = indexRevision
        let results = await WorkspaceFileSearchIndex.matches(paths: paths, query: requestedQuery)
        // A cancelled query or a replaced index cannot restore stale rows.
        guard !Task.isCancelled, requestedQuery == query, revision == indexRevision else { return }
        matchedQuery = requestedQuery
        matches = results
    }

    private func open(_ match: WorkspaceFileSearchMatch?) {
        guard let root, let match, currentMatches.contains(where: { $0.id == match.id }) else {
            selection.reconcile(currentMatches.map(\.id))
            return
        }
        model.openFile(root.appendingPathComponent(match.relativePath))
        dismiss()
    }
}
