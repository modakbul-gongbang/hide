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
    @State private var error: String?
    @State private var loading = true

    private var root: URL? {
        model.focusedCheckout.map { URL(fileURLWithPath: $0.path, isDirectory: true) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: "doc.text.magnifyingglass").foregroundStyle(HideTheme.accent)
                TextField("Open file in selected checkout", text: $query)
                    .textFieldStyle(.plain)
                    .hideFont(size: HideTheme.Typography.headline)
                    .onSubmit { open(matches.first) }
                if loading { ProgressView().controlSize(.small) }
                Text("ESC")
                    .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            .padding(HideTheme.spacingLG)
            .background(
                HideTheme.elevated,
                in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
            )
            .padding(HideTheme.spacingLG)

            if let error {
                ContentUnavailableView("File index unavailable", systemImage: "exclamationmark.triangle", description: Text(error))
            } else {
                ScrollView {
                    LazyVStack(spacing: HideTheme.spacingXXS) {
                        ForEach(matches) { match in
                            Button { open(match) } label: {
                                HStack(spacing: HideTheme.spacingSM) {
                                    SetiFileIconView(url: (root ?? URL(fileURLWithPath: "/")).appendingPathComponent(match.relativePath), size: 13)
                                    Text(match.relativePath)
                                        .hideFont(size: HideTheme.Typography.subhead, design: .monospaced)
                                        .foregroundStyle(HideTheme.primary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                    Spacer()
                                }
                                .padding(.horizontal, HideTheme.spacingMD)
                                .padding(.vertical, HideTheme.spacingSM)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, HideTheme.spacingLG)
                }
            }
        }
        .frame(width: HideTheme.searchSheetSize.width, height: HideTheme.searchSheetSize.height)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .task(id: root) { await load() }
        .task(id: query) { matches = await WorkspaceFileSearchIndex.matches(paths: paths, query: query) }
    }

    private func load() async {
        guard let root else {
            error = "The selected checkout path is unavailable."
            loading = false
            return
        }
        do {
            paths = try await WorkspaceFileSearchIndex.load(root: root)
            matches = await WorkspaceFileSearchIndex.matches(paths: paths, query: query)
        } catch {
            self.error = error.localizedDescription
        }
        loading = false
    }

    private func open(_ match: WorkspaceFileSearchMatch?) {
        guard let root, let match else { return }
        model.openFile(root.appendingPathComponent(match.relativePath))
        dismiss()
    }
}
