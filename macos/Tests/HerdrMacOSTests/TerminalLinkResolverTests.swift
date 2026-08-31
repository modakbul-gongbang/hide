import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Terminal link resolver")
struct TerminalLinkResolverTests {
    @Test func explicitURLsUseTheMacOSExternalHandlerPolicy() throws {
        let httpsURL = try #require(URL(string: "https://example.com/docs"))
        let mailURL = try #require(URL(string: "mailto:hello@example.com"))
        let customURL = try #require(URL(string: "hide-preview://open/item"))

        #expect(TerminalLinkResolver.parse("https://example.com/docs") == .external(httpsURL))
        #expect(TerminalLinkResolver.parse("mailto:hello@example.com") == .external(mailURL))
        #expect(TerminalLinkResolver.parse("hide-preview://open/item") == .external(customURL))
        #expect(TerminalLinkResolver.parse("https:///missing-host") == .invalid(
            "The terminal URL has no host and cannot be opened."
        ))
    }

    @Test func fileURLsAndSourceLocationsStayInsideHide() throws {
        #expect(TerminalLinkResolver.parse("file:///tmp/App.swift:19:4") == .file(
            path: "/tmp/App.swift",
            line: 19,
            column: 4
        ))
        #expect(TerminalLinkResolver.parse("'Sources/App.swift:27'") == .file(
            path: "Sources/App.swift",
            line: 27,
            column: nil
        ))
    }

    @Test func paneWorkingDirectoryWinsBeforeCheckoutRoot() throws {
        let fixture = try LocalFileFixture()
        defer { fixture.remove() }
        let nested = fixture.root.appendingPathComponent("Sources", isDirectory: true)
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        let rootFile = fixture.root.appendingPathComponent("target.txt")
        let nestedFile = nested.appendingPathComponent("target.txt")
        try Data("root".utf8).write(to: rootFile)
        try Data("nested".utf8).write(to: nestedFile)

        #expect(TerminalLinkResolver.resolveLocalFile(
            path: "target.txt",
            paneCWD: nested.path,
            checkoutRoot: fixture.root
        ) == .file(nestedFile.resolvingSymlinksInPath()))
    }

    @Test func missingRootsFoldersUnreadableAndOutsidePathsReturnVisibleFailures() throws {
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

        #expect(TerminalLinkResolver.resolveLocalFile(
            path: "README.md",
            paneCWD: fixture.root.path,
            checkoutRoot: nil
        ) == .failure("Hide cannot open this path because the selected checkout has no local root."))
        #expect(TerminalLinkResolver.resolveLocalFile(
            path: "Assets",
            paneCWD: fixture.root.path,
            checkoutRoot: fixture.root
        ) == .failure("Assets is a folder. Terminal links currently open files in Workbench."))
        #expect(TerminalLinkResolver.resolveLocalFile(
            path: "secret.txt",
            paneCWD: fixture.root.path,
            checkoutRoot: fixture.root
        ) == .failure("Hide found secret.txt, but it is not readable."))
        #expect(TerminalLinkResolver.resolveLocalFile(
            path: fixture.root.deletingLastPathComponent().appendingPathComponent("outside.txt").path,
            paneCWD: fixture.root.path,
            checkoutRoot: fixture.root
        ) == .failure("Hide could not find \(fixture.root.deletingLastPathComponent().appendingPathComponent("outside.txt").path) inside the selected checkout."))
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
