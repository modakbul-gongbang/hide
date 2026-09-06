import Foundation
import Testing
@testable import HerdrMacOS

/// The reported defect: a schemeless host an agent printed went down the
/// local-file path and produced "could not find inside the selected checkout".
/// These pin the routing order - resolution first, spelling second - and pin
/// that a real file outside the checkout opens like any other.
@Suite("Terminal link routing")
struct TerminalLinkResolverTests {
    @Test func explicitURLsUseTheMacOSExternalHandlerPolicy() throws {
        let httpsURL = try #require(URL(string: "https://example.com/docs"))
        let mailURL = try #require(URL(string: "mailto:hello@example.com"))
        let customURL = try #require(URL(string: "hide-preview://open/item"))

        #expect(route("https://example.com/docs") == .web(httpsURL))
        #expect(route("mailto:hello@example.com") == .web(mailURL))
        #expect(route("hide-preview://open/item") == .web(customURL))
        #expect(route("https:///missing-host") == .unresolved(
            "The terminal URL has no host and cannot be opened."
        ))
    }

    @Test func aSchemelessHostOpensOnTheWebRatherThanBeingSearchedForAsAFile() throws {
        let docs = try #require(URL(string: "https://docs.anthropic.com/en/docs"))
        let host = try #require(URL(string: "https://github.com"))
        #expect(route("docs.anthropic.com/en/docs") == .web(docs))
        #expect(route("github.com") == .web(host))
    }

    /// A dev server printed by an agent has no certificate, so assuming
    /// `https` would fail the TLS handshake instead of opening the page. A
    /// dotted-quad address also has to survive the TLD test, which rejects a
    /// numeric last label.
    @Test func aLocalAddressOpensOverHTTPRatherThanFailingTheTLSHandshake() throws {
        let named = try #require(URL(string: "http://localhost:5173/health"))
        let loopback = try #require(URL(string: "http://127.0.0.1:3000"))
        let anyInterface = try #require(URL(string: "http://0.0.0.0:8080/"))
        #expect(route("localhost:5173/health") == .web(named))
        #expect(route("127.0.0.1:3000") == .web(loopback))
        #expect(route("0.0.0.0:8080/") == .web(anyInterface))
    }

    /// A remote pane's paths name files on the other machine, but a web
    /// address means the same thing from either side. The remote branch reads
    /// the token through this entry point, so it must not consult the
    /// filesystem or claim a bare path is a host.
    @Test func theWebReadingOfATokenIsDecidedWithoutTheFilesystem() throws {
        let explicit = try #require(URL(string: "https://example.com/docs"))
        #expect(TerminalLinkResolver.webURL(in: "https://example.com/docs") == explicit)
        #expect(TerminalLinkResolver.webURL(in: "localhost:5173") != nil)
        #expect(TerminalLinkResolver.webURL(in: "/etc/hosts") == nil)
        #expect(TerminalLinkResolver.webURL(in: "Foo.swift") == nil)
        #expect(TerminalLinkResolver.webURL(in: "") == nil)
    }

    /// `Foo.swift` and `example.com` are the same shape. A dotted name whose
    /// last label is not a web TLD and that carries no URL path is neither
    /// opened as a page nor claimed to be missing from the checkout.
    @Test func aDottedNameThatIsNeitherAFileNorAHostIsNamedRatherThanGuessed() {
        let result = route("Foo.swift")
        #expect(result == .unresolved("Hide could not resolve Foo.swift as a file or a web address."))
        if case .unresolved(let message) = result {
            #expect(!message.contains("checkout"))
        }
        #expect(route("src/main.rs") == .unresolved(
            "Hide could not resolve src/main.rs as a file or a web address."
        ))
    }

    @Test func paneWorkingDirectoryWinsBeforeCheckoutRoot() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let nested = fixture.root.appendingPathComponent("Sources", isDirectory: true)
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try Data("root".utf8).write(to: fixture.root.appendingPathComponent("target.txt"))
        let nestedFile = nested.appendingPathComponent("target.txt")
        try Data("nested".utf8).write(to: nestedFile)

        #expect(route("target.txt", paneCWD: nested.path, checkoutRoot: fixture.root)
            == .file(nestedFile.standardizedFileURL.resolvingSymlinksInPath()))
    }

    /// The operator asked for a file outside the checkout to open as a tab.
    /// Nothing below the resolver ever refused one; only the resolver did.
    @Test func anAbsolutePathOutsideTheCheckoutOpensAsAFile() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let checkout = fixture.root.appendingPathComponent("checkout", isDirectory: true)
        try FileManager.default.createDirectory(at: checkout, withIntermediateDirectories: true)
        let outside = fixture.root.appendingPathComponent("outside.txt")
        try Data("outside".utf8).write(to: outside)

        #expect(route(outside.path, paneCWD: checkout.path, checkoutRoot: checkout)
            == .file(outside.standardizedFileURL.resolvingSymlinksInPath()))
    }

    @Test func aSourceLocationSuffixStillResolvesTheFileItNames() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let file = fixture.root.appendingPathComponent("App.swift")
        try Data("code".utf8).write(to: file)

        #expect(route("\(file.path):19:4", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .file(file.standardizedFileURL.resolvingSymlinksInPath()))
        #expect(route("'App.swift:27'", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .file(file.standardizedFileURL.resolvingSymlinksInPath()))
    }

    @Test func aFolderIsRoutedAsADirectoryAndAnUnreadableFileStatesItsReason() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let folder = fixture.root.appendingPathComponent("Assets", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let unreadable = fixture.root.appendingPathComponent("secret.txt")
        try Data("private".utf8).write(to: unreadable)
        try FileManager.default.setAttributes([.posixPermissions: 0o000], ofItemAtPath: unreadable.path)
        defer {
            try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: unreadable.path)
        }

        #expect(route("Assets", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .directory(folder.standardizedFileURL.resolvingSymlinksInPath()))
        #expect(route("Assets/", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .directory(folder.standardizedFileURL.resolvingSymlinksInPath()))
        #expect(route("secret.txt", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .unresolved("Hide found secret.txt, but it is not readable."))
    }

    /// A directory the explorer shows is revealed in it, opened down to its
    /// row; anything the explorer cannot show goes to Finder. The returned
    /// paths are spelled under the explorer root as given, because the outline
    /// names its rows by appending to that root.
    @Test func aDirectoryInsideTheExplorerRootIsRevealedThereAndOneOutsideGoesToFinder() {
        let root = URL(fileURLWithPath: "/Users/me/projects/hide", isDirectory: true)
        let nested = URL(fileURLWithPath: "/Users/me/projects/hide/agents/runs/spec", isDirectory: true)
        #expect(
            TerminalLinkResolver.directoryDestination(nested, explorerRoot: root)
                == .explorer(
                    expand: [
                        "/Users/me/projects/hide/agents",
                        "/Users/me/projects/hide/agents/runs",
                        "/Users/me/projects/hide/agents/runs/spec",
                    ],
                    selectedPath: "/Users/me/projects/hide/agents/runs/spec"
                )
        )
        #expect(
            TerminalLinkResolver.directoryDestination(root, explorerRoot: root)
                == .explorer(expand: [], selectedPath: nil)
        )

        let sibling = URL(fileURLWithPath: "/Users/me/projects/hide.worktrees/spec", isDirectory: true)
        #expect(TerminalLinkResolver.directoryDestination(sibling, explorerRoot: root) == .finder)
        #expect(TerminalLinkResolver.directoryDestination(nested, explorerRoot: nil) == .finder)
    }

    /// The resolver hands back a symlink-resolved directory while the explorer
    /// root may be spelled through a symlink; containment is judged resolved
    /// and the reveal is spelled the way the outline is.
    @Test func aDirectoryUnderASymlinkedExplorerRootIsStillRevealedUnderThatRoot() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let real = fixture.root.appendingPathComponent("real", isDirectory: true)
        try FileManager.default.createDirectory(
            at: real.appendingPathComponent("docs", isDirectory: true),
            withIntermediateDirectories: true
        )
        let alias = fixture.root.appendingPathComponent("alias", isDirectory: true)
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: real)

        let resolved = real.appendingPathComponent("docs", isDirectory: true)
            .standardizedFileURL.resolvingSymlinksInPath()
        #expect(
            TerminalLinkResolver.directoryDestination(resolved, explorerRoot: alias)
                == .explorer(
                    expand: [alias.standardizedFileURL.path + "/docs"],
                    selectedPath: alias.standardizedFileURL.path + "/docs"
                )
        )
    }

    @Test func anEmptyLinkIsRejectedRatherThanResolved() {
        #expect(route("   ") == .unresolved("The terminal link is empty."))
    }

    private func route(
        _ value: String,
        paneCWD: String = "",
        checkoutRoot: URL? = nil
    ) -> TerminalLinkRoute {
        TerminalLinkResolver.route(value, paneCWD: paneCWD, checkoutRoot: checkoutRoot)
    }
}

private struct LocalFileFixture {
    let root: URL

    init() throws {
        root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-terminal-links-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    func remove() {
        try? FileManager.default.removeItem(at: root)
    }
}
