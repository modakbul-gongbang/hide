import Foundation

/// One registered checkout, as the resolver needs to see it: an id to hand
/// back to the core and a path to measure a clicked path against.
struct TerminalLinkCheckout: Equatable {
    let id: String
    let workspaceID: String
    let path: String
}

/// Where a resolved filesystem path goes.
///
/// The five branches are the product policy: inside a registered checkout Hide
/// shows the path itself, outside it macOS does, and a path that would run
/// something is revealed rather than opened. Link detection is a guess made
/// over arbitrary agent output, so one wrong click must never start a program.
enum TerminalPathRoute: Equatable {
    /// A file inside a checkout: an editor tab plus the tree reveal.
    case checkoutFile(url: URL, checkout: TerminalLinkCheckout)
    /// A folder inside a checkout: the tree reveal alone.
    case checkoutFolder(url: URL, checkout: TerminalLinkCheckout)
    /// A file outside every checkout, handed to its default application.
    case externalFile(URL)
    /// A folder outside every checkout, opened as a Finder window.
    case externalFolder(URL)
    /// Something outside every checkout that opening would execute - an
    /// executable file, an application bundle, an installer package. Finder
    /// selects it instead.
    case externalReveal(URL)

    var url: URL {
        switch self {
        case .checkoutFile(let url, _), .checkoutFolder(let url, _): url
        case .externalFile(let url), .externalFolder(let url), .externalReveal(let url): url
        }
    }

    var namesAFolder: Bool {
        switch self {
        case .checkoutFolder, .externalFolder: true
        case .checkoutFile, .externalFile, .externalReveal: false
        }
    }

    /// The branch name carried in the trace, so a click's route is answerable
    /// from outside the process.
    var traceName: String {
        switch self {
        case .checkoutFile: "checkout_file"
        case .checkoutFolder: "checkout_folder"
        case .externalFile: "external_file"
        case .externalFolder: "external_folder"
        case .externalReveal: "external_reveal"
        }
    }
}

/// Where a clicked terminal link goes.
enum TerminalLinkRoute: Equatable {
    case web(URL)
    case path(TerminalPathRoute)
    case unresolved(String)
}

/// What a path names on this filesystem. "Found but unusable" is kept apart
/// from "not found" so an unreadable file says which it is instead of being
/// reported as an unresolvable token. A folder is no longer unusable: it is a
/// route of its own.
enum TerminalFileResolution: Equatable {
    case found(url: URL, isDirectory: Bool)
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

    /// Files whose "default application" is the operating system running
    /// them. A click on one reveals it in Finder instead.
    private static let executableExtensions: Set<String> = ["app", "pkg", "dmg"]

    /// The one spelling of a path every other layer uses.
    ///
    /// `URL.resolvingSymlinksInPath()` is not that spelling: it resolves
    /// symlinks and then strips a leading `/private`, so it turns the real
    /// `/private/tmp/x` into `/tmp/x`. The navigator, the checkout ids and the
    /// file tree all carry the physical path, so a resolved link that kept
    /// Foundation's answer named a path the tree had no row for - the reveal
    /// switched the checkout and opened the tab, and the tree did not move.
    /// `realpath` answers with the physical path and no such exception.
    ///
    /// Its answer is used as it stands. `standardizedFileURL` applies the same
    /// `/private` removal on the way back out, so standardizing the result
    /// would undo exactly what this is for; the `.` and `..` it would remove
    /// are already gone, because `realpath` removes them itself.
    static func canonical(_ url: URL) -> URL {
        let standardized = url.standardizedFileURL
        guard let resolved = realpath(standardized.path, nil) else { return standardized }
        defer { free(resolved) }
        return URL(fileURLWithPath: String(cString: resolved))
    }

    static func route(
        _ rawValue: String,
        paneCWD: String,
        checkoutRoot: URL?,
        checkouts: [TerminalLinkCheckout] = []
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
            return route(
                resolution: resolveFile(
                    path: splitSourceLocation(url.path).path,
                    paneCWD: paneCWD,
                    checkoutRoot: checkoutRoot
                ),
                checkouts: checkouts,
                displaying: url.path
            )
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
        // filesystem wins over any reading of its spelling, inside a checkout
        // or outside every one of them.
        switch resolveFile(
            path: splitSourceLocation(value).path,
            paneCWD: paneCWD,
            checkoutRoot: checkoutRoot
        ) {
        case .found(let url, let isDirectory):
            return .path(pathRoute(url: url, isDirectory: isDirectory, checkouts: checkouts))
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

    /// The web reading of a token, decided without touching the filesystem.
    ///
    /// A remote pane prints paths that name files on the other machine, so
    /// resolving them here would either miss or, worse, hit an unrelated local
    /// file of the same name. A web address means the same thing from either
    /// side, so it still opens.
    static func webURL(in rawValue: String) -> URL? {
        let value = stripBalancedBoundaryQuotes(
            rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        guard !value.isEmpty else { return nil }
        if let url = explicitExternalURL(in: value) {
            if let scheme = url.scheme?.lowercased(),
               ["http", "https"].contains(scheme),
               url.host == nil {
                return nil
            }
            return url
        }
        return plausibleWebURL(in: value)
    }

    private static func route(
        resolution: TerminalFileResolution,
        checkouts: [TerminalLinkCheckout],
        displaying path: String
    ) -> TerminalLinkRoute {
        switch resolution {
        case .found(let url, let isDirectory):
            .path(pathRoute(url: url, isDirectory: isDirectory, checkouts: checkouts))
        case .unusable(let message): .unresolved(message)
        case .notFound: .unresolved("Hide could not find \(path).")
        }
    }

    /// Which of the five branches a resolved path takes.
    ///
    /// Ownership is decided by the longest checkout path the resolved path
    /// sits under. Both sides are symlink-resolved before they are compared,
    /// because macOS hands out `/tmp` for a directory it stores at
    /// `/private/tmp`, and a nested checkout must win over the checkout that
    /// contains it or a worktree's files would open in the wrong tree.
    static func pathRoute(
        url: URL,
        isDirectory: Bool,
        checkouts: [TerminalLinkCheckout]
    ) -> TerminalPathRoute {
        if let checkout = owningCheckout(of: url, in: checkouts) {
            return isDirectory
                ? .checkoutFolder(url: url, checkout: checkout)
                : .checkoutFile(url: url, checkout: checkout)
        }
        if executableExtensions.contains(url.pathExtension.lowercased()) {
            return .externalReveal(url)
        }
        if isDirectory {
            return .externalFolder(url)
        }
        if FileManager.default.isExecutableFile(atPath: url.path) {
            return .externalReveal(url)
        }
        return .externalFile(url)
    }

    /// The registered checkout a path belongs to, or `nil` when it belongs to
    /// none of them.
    static func owningCheckout(
        of url: URL,
        in checkouts: [TerminalLinkCheckout]
    ) -> TerminalLinkCheckout? {
        let target = canonical(url).path
        // Containment and specificity are both decided on the canonical root.
        // A checkout spelled through a symlink has a shorter raw path than the
        // directory it resolves to, so ranking on the raw path can hand a
        // nested path to the outer checkout.
        return checkouts
            .compactMap { checkout -> (checkout: TerminalLinkCheckout, root: String)? in
                let root = canonical(
                    URL(fileURLWithPath: checkout.path, isDirectory: true)
                ).path
                guard !root.isEmpty else { return nil }
                if target == root { return (checkout, root) }
                let prefix = root.hasSuffix("/") ? root : root + "/"
                guard target.hasPrefix(prefix) else { return nil }
                return (checkout, root)
            }
            .max { $0.root.count < $1.root.count }?
            .checkout
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

    /// A schemeless host, with or without a path. Either the host is a local
    /// address, or the token carries a URL path after a dotted host, or its
    /// last label is a TLD common in agent output.
    ///
    /// The scheme is chosen from the host rather than fixed: a public name gets
    /// `https`, because every host worth clicking in agent output serves it and
    /// http-only hosts redirect. A local address gets `http`, because a dev
    /// server printed by an agent has no certificate and `https` would fail the
    /// TLS handshake instead of opening the page the user asked for.
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
        if host.lowercased() == "localhost" || isIPv4Literal(host) {
            return URL(string: "http://" + value)
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

    /// A dotted-quad address. Agents print `127.0.0.1:3000` and `0.0.0.0:8080`
    /// as often as they print `localhost`, and the TLD test rejects both
    /// because a numeric last label is not a TLD.
    private static func isIPv4Literal(_ host: String) -> Bool {
        let octets = host.split(separator: ".", omittingEmptySubsequences: false)
        guard octets.count == 4 else { return false }
        return octets.allSatisfy { octet in
            guard octet.count <= 3, octet.allSatisfy(\.isNumber),
                  let number = Int(octet)
            else { return false }
            return number <= 255
        }
    }

    /// Resolves a path against the pane's working directory, the checkout
    /// root, and the filesystem root, with no containment restriction: an
    /// absolute path outside every checkout resolves like any other path.
    ///
    /// A folder resolves rather than being reported unusable: the operator
    /// clicking one asked for the tree, which is a route the caller takes.
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
                return .found(url: candidate, isDirectory: true)
            }
            guard FileManager.default.isReadableFile(atPath: candidate.path) else {
                unusable = unusable ?? "Hide found \(candidate.lastPathComponent), but it is not readable."
                continue
            }
            guard (try? candidate.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else {
                unusable = unusable ?? "Hide found \(candidate.lastPathComponent), but it is not a regular file."
                continue
            }
            return .found(url: candidate, isDirectory: false)
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
            return [canonical(URL(fileURLWithPath: expanded))]
        }
        var bases: [URL] = []
        if !paneCWD.isEmpty {
            bases.append(URL(fileURLWithPath: paneCWD, isDirectory: true))
        }
        if let checkoutRoot, !bases.contains(where: { $0.standardizedFileURL == checkoutRoot.standardizedFileURL }) {
            bases.append(checkoutRoot)
        }
        return bases.map { canonical($0.appendingPathComponent(expanded)) }
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
