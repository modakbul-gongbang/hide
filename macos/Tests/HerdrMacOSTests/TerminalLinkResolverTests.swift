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
            == .path(.externalFile(TerminalLinkResolver.canonical(nestedFile))))
    }

    /// AC1, R1, R4. The five branches, decided by where the path is and what
    /// it would do if macOS opened it. `/tmp` is a symlink to `/private/tmp`
    /// on macOS, so both sides are resolved before they are compared, and a
    /// checkout nested inside another wins by being the longer prefix.
    @Test func aResolvedPathTakesTheBranchItsLocationAndKindDecide() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let checkout = fixture.root.appendingPathComponent("checkout", isDirectory: true)
        let nestedCheckout = checkout.appendingPathComponent("vendor/inner", isDirectory: true)
        let folder = checkout.appendingPathComponent("Sources", isDirectory: true)
        let outsideFolder = fixture.root.appendingPathComponent("elsewhere", isDirectory: true)
        for directory in [checkout, nestedCheckout, folder, outsideFolder] {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        let inside = folder.appendingPathComponent("App.swift")
        let nestedFile = nestedCheckout.appendingPathComponent("vendored.swift")
        let outsideFile = fixture.root.appendingPathComponent("outside.txt")
        let script = fixture.root.appendingPathComponent("run.sh")
        let bundle = fixture.root.appendingPathComponent("Thing.app", isDirectory: true)
        try FileManager.default.createDirectory(at: bundle, withIntermediateDirectories: true)
        for file in [inside, nestedFile, outsideFile, script] {
            try Data("body".utf8).write(to: file)
        }
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)

        // The checkout roots are spelled through the `/tmp` symlink, the way
        // the navigator carries a path the operator registered.
        let checkouts = [
            TerminalLinkCheckout(id: "c-outer", workspaceID: "w1", path: symlinked(checkout)),
            TerminalLinkCheckout(id: "c-inner", workspaceID: "w2", path: symlinked(nestedCheckout)),
        ]
        let resolve = { (url: URL) in TerminalLinkResolver.canonical(url) }

        #expect(route(inside.path, checkouts: checkouts)
            == .path(.checkoutFile(url: resolve(inside), checkout: checkouts[0])))
        #expect(route(folder.path, checkouts: checkouts)
            == .path(.checkoutFolder(url: resolve(folder), checkout: checkouts[0])))
        // The nested checkout is the longer prefix, so it owns its own files.
        #expect(route(nestedFile.path, checkouts: checkouts)
            == .path(.checkoutFile(url: resolve(nestedFile), checkout: checkouts[1])))
        #expect(route(outsideFile.path, checkouts: checkouts)
            == .path(.externalFile(resolve(outsideFile))))
        #expect(route(outsideFolder.path, checkouts: checkouts)
            == .path(.externalFolder(resolve(outsideFolder))))
        // Opening these would run them, so they are revealed instead (A1).
        #expect(route(script.path, checkouts: checkouts)
            == .path(.externalReveal(resolve(script))))
        #expect(route(bundle.path, checkouts: checkouts)
            == .path(.externalReveal(resolve(bundle))))
    }

    /// With no checkout registered at all, every resolved path is outside.
    @Test func anAbsolutePathOutsideEveryCheckoutIsHandedToMacOS() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let checkout = fixture.root.appendingPathComponent("checkout", isDirectory: true)
        try FileManager.default.createDirectory(at: checkout, withIntermediateDirectories: true)
        let outside = fixture.root.appendingPathComponent("outside.txt")
        try Data("outside".utf8).write(to: outside)

        #expect(route(outside.path, paneCWD: checkout.path, checkoutRoot: checkout)
            == .path(.externalFile(TerminalLinkResolver.canonical(outside))))
    }

    /// AC1, R1. Every layer below the resolver carries the physical path: the
    /// navigator's checkout, the file tree's rows, the core's expanded set.
    /// Foundation's `resolvingSymlinksInPath` does not answer with it - it
    /// strips a leading `/private`, turning the real `/private/tmp/x` into
    /// `/tmp/x` - and a reveal that carried that spelling switched the
    /// checkout and opened the tab while the tree stayed exactly where it was.
    @Test func aResolvedPathIsSpelledTheWayEveryOtherLayerSpellsIt() throws {
        // This regression specifically exercises macOS's /tmp -> /private/tmp
        // alias. A runner may put its general TMPDIR anywhere, so arrange that
        // alias explicitly rather than assuming the runner's directory has it.
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let physical = TerminalLinkResolver.canonical(fixture.root)
        try #require(physical.path.hasPrefix("/private/"), "the fixture root is under /private on macOS")
        let file = physical.appendingPathComponent("target.txt")
        try Data("body".utf8).write(to: file)
        let checkouts = [
            TerminalLinkCheckout(id: "c1", workspaceID: "w1", path: symlinked(physical)),
        ]

        // Spelled either way, the route names the physical path and the
        // checkout that owns it.
        for spelling in [file.path, symlinked(file)] {
            #expect(
                route(spelling, checkouts: checkouts)
                    == .path(.checkoutFile(url: file, checkout: checkouts[0])),
                "\(spelling) did not resolve to the physical path"
            )
        }
    }

    /// The `/tmp` spelling of a real path under `/private/tmp`, which is what
    /// a registered checkout path looks like on macOS.
    /// AC1, R1. Which checkout owns a path is decided on the canonical root,
    /// not on the path the navigator happens to carry. The inner checkout here
    /// is spelled through a symlink, so it is the longer directory while being
    /// the shorter string: ranking on the raw path hands its own file to the
    /// outer checkout.
    @Test func theInnerCheckoutOwnsItsFileEvenWhenItIsSpelledMoreBriefly() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let outer = fixture.root.appendingPathComponent("checkout", isDirectory: true)
        let inner = outer.appendingPathComponent("v", isDirectory: true)
        try FileManager.default.createDirectory(at: inner, withIntermediateDirectories: true)
        let innerFile = inner.appendingPathComponent("vendored.swift")
        try Data("body".utf8).write(to: innerFile)
        let alias = fixture.root.appendingPathComponent("c", isDirectory: true)
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: outer)

        let outerPath = outer.standardizedFileURL.path
        let innerPath = alias.appendingPathComponent("v", isDirectory: true).standardizedFileURL.path
        #expect(innerPath.count < outerPath.count, "the fixture must make the inner root spell shorter")

        let checkouts = [
            TerminalLinkCheckout(id: "c-outer", workspaceID: "w1", path: outerPath),
            TerminalLinkCheckout(id: "c-inner", workspaceID: "w2", path: innerPath),
        ]
        #expect(route(innerFile.path, checkouts: checkouts)
            == .path(.checkoutFile(url: TerminalLinkResolver.canonical(innerFile), checkout: checkouts[1])))
    }

    /// AC2, AC3, SC1 and SC2 failure and recovery. A path printed minutes ago
    /// can be gone by the time it is clicked. It then resolves to nothing at
    /// all, so no reveal is dispatched and the tree, the selection and the tab
    /// strip are left exactly as they were, and the operator is told which
    /// path could not be found. Putting the file or the folder back makes the
    /// same click work, with nothing else done in between.
    @Test func aClickOnAPathThatIsGoneRevealsNothingAndSaysWhichUntilItIsBack() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let checkout = fixture.root.appendingPathComponent("checkout", isDirectory: true)
        let folder = checkout.appendingPathComponent("deep", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let file = folder.appendingPathComponent("target.txt")
        try Data("body".utf8).write(to: file)
        let checkouts = [
            TerminalLinkCheckout(id: "c", workspaceID: "w1", path: checkout.path),
        ]
        let resolve = { (url: URL) in TerminalLinkResolver.canonical(url) }

        // Both resolve while they exist.
        #expect(route(file.path, checkouts: checkouts)
            == .path(.checkoutFile(url: resolve(file), checkout: checkouts[0])))
        #expect(route(folder.path, checkouts: checkouts)
            == .path(.checkoutFolder(url: resolve(folder), checkout: checkouts[0])))

        // The file goes away. The click carries no reveal, and names the path.
        try FileManager.default.removeItem(at: file)
        let missingFile = route(file.path, checkouts: checkouts)
        if case .unresolved(let message) = missingFile {
            #expect(message.hasPrefix("Hide could not resolve "))
            #expect(message.contains(String(file.path.prefix(40))), "the reason names the path that was clicked")
        } else {
            Issue.record("a deleted file must reveal nothing and say so, got \(missingFile)")
        }

        // The folder goes away too, taking the same branch rather than a
        // different one.
        try FileManager.default.removeItem(at: folder)
        let missingFolder = route(folder.path, checkouts: checkouts)
        if case .unresolved = missingFolder {} else {
            Issue.record("a deleted folder must reveal nothing and say so, got \(missingFolder)")
        }

        // Both come back, and the same click resolves again.
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("body".utf8).write(to: file)
        #expect(route(folder.path, checkouts: checkouts)
            == .path(.checkoutFolder(url: resolve(folder), checkout: checkouts[0])))
        #expect(route(file.path, checkouts: checkouts)
            == .path(.checkoutFile(url: resolve(file), checkout: checkouts[0])))
    }

    private func symlinked(_ url: URL) -> String {
        let path = url.standardizedFileURL.path
        guard path.hasPrefix("/private/") else { return path }
        return String(path.dropFirst("/private".count))
    }

    @Test func aSourceLocationSuffixStillResolvesTheFileItNames() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let file = fixture.root.appendingPathComponent("App.swift")
        try Data("code".utf8).write(to: file)

        #expect(route("\(file.path):19:4", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .path(.externalFile(TerminalLinkResolver.canonical(file))))
        #expect(route("'App.swift:27'", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .path(.externalFile(TerminalLinkResolver.canonical(file))))
    }

    /// A folder is a route now, not a refusal: the operator who clicks one
    /// asked for the tree. An unreadable file still says what it is.
    @Test func aFolderIsARouteAndAnUnreadableFileStatesItsOwnReason() throws {
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
            == .path(.externalFolder(TerminalLinkResolver.canonical(folder))))
        #expect(route("secret.txt", paneCWD: fixture.root.path, checkoutRoot: fixture.root)
            == .unresolved("Hide found secret.txt, but it is not readable."))
    }

    @Test func anEmptyLinkIsRejectedRatherThanResolved() {
        #expect(route("   ") == .unresolved("The terminal link is empty."))
    }

    private func route(
        _ value: String,
        paneCWD: String = "",
        checkoutRoot: URL? = nil,
        checkouts: [TerminalLinkCheckout] = []
    ) -> TerminalLinkRoute {
        TerminalLinkResolver.route(
            value,
            paneCWD: paneCWD,
            checkoutRoot: checkoutRoot,
            checkouts: checkouts
        )
    }
}

private struct LocalFileFixture {
    let root: URL

    /// Rooted at `/tmp` rather than at `FileManager.temporaryDirectory`.
    ///
    /// These cases are about the difference between a symlinked spelling and
    /// the physical one, so the fixture has to sit under a symlinked path:
    /// `/tmp` is a link to `/private/tmp` on macOS. `temporaryDirectory`
    /// follows `TMPDIR`, and a runner that points `TMPDIR` at an ordinary
    /// directory left the fixture with no symlink to resolve and the case
    /// failing on where it was run rather than on what it tests.
    init() throws {
        root = URL(fileURLWithPath: "/tmp", isDirectory: true)
            .appendingPathComponent("hide-terminal-links-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    func remove() {
        try? FileManager.default.removeItem(at: root)
    }
}
