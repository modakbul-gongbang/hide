import Foundation

/// Where a clicked terminal link goes.
enum TerminalLinkRoute: Equatable {
    case web(URL)
    case file(URL)
    case unresolved(String)
}

/// What a path names on this filesystem. "Found but unusable" is kept apart
/// from "not found" so a folder or an unreadable file says which it is instead
/// of being reported as an unresolvable token.
enum TerminalFileResolution: Equatable {
    case file(URL)
    case unusable(String)
    case notFound
}

/// Routes a clicked terminal link by what it resolves to, not by how it is
/// spelled.
///
/// The reported defect: anything without `://` or a scheme in a small
/// allowlist fell through to a local-file resolver that also required the path
/// to sit inside the selected checkout, so a schemeless host an agent printed -
/// `docs.anthropic.com/en/docs` - produced "could not find inside the selected
/// checkout". Nothing below the resolver ever imposed that root restriction:
/// the shell's own file open and the core's file event both accept any
/// readable path. The restriction was the resolver's alone, so it is gone.
///
/// SwiftTerm deliberately recognizes a broad, Ghostty-derived class of URL and
/// path shapes. Hide keeps the product policy here instead of growing a second
/// collection of per-extension or per-command regular expressions.
enum TerminalLinkResolver {
    /// TLDs common enough in agent output that a bare dotted name carrying one
    /// is a web address rather than a filename. A dotted name is otherwise
    /// ambiguous - `Foo.swift` and `example.com` have the same shape - and
    /// guessing wrong sends a source file to a browser. A name that resolves
    /// to a real file never reaches this test, because resolution runs first.
    private static let webTopLevelDomains: Set<String> = [
        "ai", "app", "co", "com", "cloud", "dev", "edu", "gov", "gg", "info",
        "io", "me", "net", "org", "page", "sh", "so", "xyz",
    ]

    static func route(
        _ rawValue: String,
        paneCWD: String,
        checkoutRoot: URL?
    ) -> TerminalLinkRoute {
        let value = stripBalancedBoundaryQuotes(
            rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        guard !value.isEmpty else {
            return .unresolved("The terminal link is empty.")
        }

        if let url = URL(string: value), url.scheme?.lowercased() == "file" {
            guard url.isFileURL, !url.path.isEmpty else {
                return .unresolved("The terminal file URL has no usable path.")
            }
            return route(resolution: resolveFile(
                path: splitSourceLocation(url.path).path,
                paneCWD: paneCWD,
                checkoutRoot: checkoutRoot
            ), displaying: url.path)
        }

        if let url = explicitExternalURL(in: value) {
            if let scheme = url.scheme?.lowercased(),
               ["http", "https"].contains(scheme),
               url.host == nil {
                return .unresolved("The terminal URL has no host and cannot be opened.")
            }
            return .web(url)
        }

        // Resolution first: a path that names something real on this
        // filesystem wins over any reading of its spelling, inside the
        // checkout or outside it.
        switch resolveFile(
            path: splitSourceLocation(value).path,
            paneCWD: paneCWD,
            checkoutRoot: checkoutRoot
        ) {
        case .file(let url):
            return .file(url)
        case .unusable(let message):
            return .unresolved(message)
        case .notFound:
            break
        }

        if let url = plausibleWebURL(in: value) {
            return .web(url)
        }

        let displayed = value.count > 120 ? String(value.prefix(117)) + "..." : value
        return .unresolved("Hide could not resolve \(displayed) as a file or a web address.")
    }

    private static func route(
        resolution: TerminalFileResolution,
        displaying path: String
    ) -> TerminalLinkRoute {
        switch resolution {
        case .file(let url): .file(url)
        case .unusable(let message): .unresolved(message)
        case .notFound: .unresolved("Hide could not find \(path).")
        }
    }

    private static func explicitExternalURL(in value: String) -> URL? {
        guard let url = URL(string: value),
              let scheme = url.scheme?.lowercased()
        else { return nil }
        let schemesWithoutSlashes: Set<String> = [
            "ftp", "gemini", "git", "gopher", "ipfs", "ipns", "mailto",
            "magnet", "news", "ssh", "tel",
        ]
        if value.contains("://") || schemesWithoutSlashes.contains(scheme) {
            return url
        }
        return nil
    }

    /// A schemeless host, with or without a path. Either the token carries a
    /// URL path after a dotted host, or its last label is a TLD common in
    /// agent output. `https` is assumed because every host worth clicking in
    /// agent output serves it, and http-only hosts redirect.
    static func plausibleWebURL(in value: String) -> URL? {
        guard !value.contains(" "), !value.hasPrefix("/"), !value.hasPrefix("~"), !value.hasPrefix(".") else {
            return nil
        }
        let hostPart = value.prefix { $0 != "/" && $0 != "?" && $0 != "#" }
        let hasURLTail = hostPart.count < value.count
        var host = String(hostPart)
        if let colon = host.lastIndex(of: ":"),
           Int(host[host.index(after: colon)...]) != nil {
            host = String(host[..<colon])
        }
        if host == "localhost" {
            return URL(string: "https://" + value)
        }
        let labels = host.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2 else { return nil }
        for label in labels {
            guard !label.isEmpty,
                  label.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "-" }),
                  label.first != "-", label.last != "-"
            else { return nil }
        }
        let topLevel = labels[labels.count - 1].lowercased()
        guard topLevel.count >= 2, topLevel.allSatisfy(\.isLetter) else { return nil }
        guard hasURLTail || webTopLevelDomains.contains(topLevel) else { return nil }
        return URL(string: "https://" + value)
    }

    /// Resolves a path against the pane's working directory, the checkout
    /// root, and the filesystem root, with no containment restriction: an
    /// absolute path outside the checkout opens like any other file.
    static func resolveFile(
        path: String,
        paneCWD: String,
        checkoutRoot: URL?
    ) -> TerminalFileResolution {
        var unusable: String?
        for candidate in candidates(path: path, paneCWD: paneCWD, checkoutRoot: checkoutRoot) {
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: candidate.path, isDirectory: &isDirectory) else {
                continue
            }
            if isDirectory.boolValue {
                unusable = unusable ?? "\(candidate.lastPathComponent) is a folder. Terminal links open files."
                continue
            }
            guard FileManager.default.isReadableFile(atPath: candidate.path) else {
                unusable = unusable ?? "Hide found \(candidate.lastPathComponent), but it is not readable."
                continue
            }
            guard (try? candidate.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else {
                unusable = unusable ?? "Hide found \(candidate.lastPathComponent), but it is not a regular file."
                continue
            }
            return .file(candidate)
        }
        if let unusable { return .unusable(unusable) }
        return .notFound
    }

    private static func candidates(
        path: String,
        paneCWD: String,
        checkoutRoot: URL?
    ) -> [URL] {
        guard !path.isEmpty else { return [] }
        let expanded = NSString(string: path).expandingTildeInPath
        if expanded.hasPrefix("/") {
            return [URL(fileURLWithPath: expanded).standardizedFileURL.resolvingSymlinksInPath()]
        }
        var bases: [URL] = []
        if !paneCWD.isEmpty {
            bases.append(URL(fileURLWithPath: paneCWD, isDirectory: true))
        }
        if let checkoutRoot, !bases.contains(where: { $0.standardizedFileURL == checkoutRoot.standardizedFileURL }) {
            bases.append(checkoutRoot)
        }
        return bases.map {
            $0.appendingPathComponent(expanded).standardizedFileURL.resolvingSymlinksInPath()
        }
    }

    private static func stripBalancedBoundaryQuotes(_ value: String) -> String {
        guard value.count >= 2,
              let first = value.first,
              let last = value.last,
              (first == last),
              first == "\"" || first == "'" || first == "`"
        else { return value }
        return String(value.dropFirst().dropLast())
    }

    /// Splits a trailing `:line` or `:line:column` off a path. The location is
    /// discarded by every layer below, so jumping to it is a stated non-goal
    /// this round; the split still has to happen or the path would not resolve.
    static func splitSourceLocation(
        _ value: String
    ) -> (path: String, line: Int?, column: Int?) {
        var pieces = value.split(separator: ":", omittingEmptySubsequences: false)
        guard pieces.count > 1,
              let trailingNumber = positiveInteger(pieces.last)
        else { return (value, nil, nil) }
        pieces.removeLast()

        if pieces.count > 1,
           let line = positiveInteger(pieces.last) {
            pieces.removeLast()
            return (pieces.joined(separator: ":"), line, trailingNumber)
        }
        return (pieces.joined(separator: ":"), trailingNumber, nil)
    }

    private static func positiveInteger(_ value: Substring?) -> Int? {
        guard let value,
              !value.isEmpty,
              value.allSatisfy(\.isNumber),
              let number = Int(value),
              number > 0
        else { return nil }
        return number
    }
}
