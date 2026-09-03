import Foundation
import SwiftTerm
import Testing
@testable import HerdrMacOS

/// What a click in a pane offers as a link.
///
/// `TerminalLinkResolver` decides where a token goes; this decides which token
/// the click even sees, and that is where both reported defects lived. Agent
/// transcripts put prose right after a path, and the inherited Ghostty pattern
/// let a path run across spaces, so `shot.png 가나다라 는` came through as one
/// link and the click reported the whole run as unopenable. The same pattern
/// read every slash command as an absolute path, so `/gen-prd` was underlined
/// and raised a modal of its own.
@Suite("Terminal implicit link span")
struct TerminalImplicitLinkSpanTests {
    /// Feeds one line and asks what a click on `column` would open.
    private func link(in line: String, atColumn column: Int) -> String? {
        let headless = HeadlessTerminal(onEnd: { _ in })
        let terminal = headless.terminal!
        terminal.resize(cols: 200, rows: 24)
        terminal.feed(text: line)
        return terminal.link(
            at: .buffer(Position(col: column, row: 0)),
            mode: .explicitAndImplicit
        )
    }

    /// Column of the first character of `needle`, which is where the operator
    /// clicked in each reported case.
    private func column(of needle: String, in line: String) -> Int {
        guard let range = line.range(of: needle) else { return 0 }
        return line.distance(from: line.startIndex, to: range.lowerBound)
    }

    @Test func aPathStopsAtWhitespaceInsteadOfSwallowingTheProseAfterIt() {
        let line = "docs/screenshots/hide.png 가나다라 는 열 수 없습니다"
        #expect(link(in: line, atColumn: column(of: "docs", in: line)) == "docs/screenshots/hide.png")
    }

    @Test func aSlashCommandIsNotOfferedAsAnAbsolutePath() {
        // The reported line: the command itself must not be clickable, while
        // the real path in its arguments still is.
        let line = "/gen-prd --context agents/interview/project-panel/qa-log.md \"프로젝트 중심\""
        #expect(link(in: line, atColumn: column(of: "/gen-prd", in: line)) == nil)
        #expect(
            link(in: line, atColumn: column(of: "agents/", in: line))
                == "agents/interview/project-panel/qa-log.md"
        )
    }

    @Test func theOutputAgentsActuallyPrintStillResolves() {
        // A source location is the primary case: it has to survive intact,
        // including the `:line` suffix the resolver splits off later.
        let line = "~/projects/herdr-ide/macos/Sources/HerdrMacOS/ShellModel.swift:530 참고"
        #expect(
            link(in: line, atColumn: column(of: "~/", in: line))
                == "~/projects/herdr-ide/macos/Sources/HerdrMacOS/ShellModel.swift:530"
        )

        let relative = "run ./scripts/build_dev_app.sh first"
        #expect(
            link(in: relative, atColumn: column(of: "./", in: relative))
                == "./scripts/build_dev_app.sh"
        )
    }

    /// The whole route for the case the operator asked for: a real README.md
    /// printed with prose after it has to come back as the file alone, and
    /// that token has to resolve to the file the editor opens.
    @Test func aRealReadmePrintedWithProseAfterItResolvesToThatFile() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-readme-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let readme = root.appendingPathComponent("README.md")
        try "# hide\n".write(to: readme, atomically: true, encoding: .utf8)

        let line = "wrote \(readme.path) 가나다라 확인해줘"
        let token = link(in: line, atColumn: column(of: readme.path, in: line))
        #expect(token == readme.path)

        let resolved = TerminalLinkResolver.route(
            try #require(token),
            paneCWD: root.path,
            checkoutRoot: root
        )
        #expect(resolved == .file(readme))
    }

    @Test func aRootedPathStillNeedsMoreThanOneSegmentButAWebAddressIsUntouched() {
        // The narrowing that excludes slash commands also excludes a bare top
        // level directory. That is the accepted cost, asserted so it is a
        // decision rather than a surprise.
        let line = "/etc/hosts and /tmp alone"
        #expect(link(in: line, atColumn: column(of: "/etc", in: line)) == "/etc/hosts")
        #expect(link(in: line, atColumn: column(of: "/tmp", in: line)) == nil)

        let web = "open https://example.com/a/b?q=1 please"
        #expect(
            link(in: web, atColumn: column(of: "https", in: web)) == "https://example.com/a/b?q=1"
        )
    }
}
