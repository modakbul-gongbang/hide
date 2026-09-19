import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Browser pane opener")
struct BrowserPaneOpenerTests {
    private let host = BrowserPaneOpener.Host(
        node: URL(fileURLWithPath: "/opt/node/bin/node"),
        script: URL(fileURLWithPath: "/Applications/hide.app/Contents/Resources/browser-pane/browser-pane.mjs")
    )
    private let file = URL(fileURLWithPath: "/Users/example/repo/docs/한글 index.html")

    private func profiles(_ entries: String) -> String {
        #"{"profiles":[],"entries":[\#(entries)]}"#
    }

    /// D-06, B10: the running profile whose state changed last wins; stopped
    /// profiles are never chosen however recent they are.
    @Test func theMostRecentlyStartedRunningProfileIsChosen() {
        let listed: [BrowserPaneOpener.Profile] = [
            .init(name: "default", running: true, stateModifiedAt: "2026-09-19T01:00:00.000Z"),
            .init(name: "modakbul", running: true, stateModifiedAt: "2026-09-19T02:30:00.000Z"),
            .init(name: "stopped", running: false, stateModifiedAt: "2026-09-19T03:00:00.000Z"),
            .init(name: "undated", running: true, stateModifiedAt: nil),
        ]
        #expect(BrowserPaneOpener.selectProfile(from: listed) == .success("modakbul"))
        #expect(BrowserPaneOpener.selectProfile(from: [listed[3]]) == .success("undated"))
        #expect(BrowserPaneOpener.selectProfile(from: [listed[2]]) == .failure(.noRunningProfile))
        #expect(BrowserPaneOpener.Failure.noRunningProfile.message == "No running chromux profile. Launch one with chromux launch <name>.")
    }

    /// D-07: the key is the file's, within the host's identifier rule.
    @Test func theBindingKeyFollowsTheFileAndFitsTheHostsIdentifierRule() {
        let key = BrowserPaneOpener.bindingKey(for: file)
        #expect(key == BrowserPaneOpener.bindingKey(for: URL(fileURLWithPath: "/Users/example/repo/docs/../docs/한글 index.html")))
        #expect(key != BrowserPaneOpener.bindingKey(for: URL(fileURLWithPath: "/Users/example/repo/docs/other.html")))
        #expect(key.count <= 80)
        #expect(key.hasPrefix("file-"))
        #expect(key.range(of: "^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$", options: .regularExpression) != nil)
    }

    /// B8: profiles then open, both through the bundled script with the
    /// chosen profile, the focused pane, the file URL and the file's key.
    @Test @MainActor func aSuccessfulOpenRunsProfilesThenOpenAgainstTheFocusedPane() async throws {
        let recorded = Recorder()
        var result: Result<Void, BrowserPaneOpener.Failure>?
        BrowserPaneOpener.open(
            file: file, targetPane: "w1:p2", host: host, environment: ["PATH": "/opt/node/bin"],
            runner: { executable, arguments, environment, timeout, completion in
                recorded.append(executable: executable, arguments: arguments, environment: environment, timeout: timeout)
                if arguments.last == "profiles" {
                    completion(.exited(status: 0, stdout: self.profiles(#"{"name":"work","running":true,"state_modified_at":"2026-09-19T01:00:00.000Z"}"#), stderr: ""))
                } else {
                    completion(.exited(status: 0, stdout: #"{"ok":true,"pane_id":"w1:p9","reused":false}"#, stderr: ""))
                }
            },
            completion: { result = $0 }
        )
        try await eventually { result != nil }
        #expect(failure(of: result) == nil)
        let calls = recorded.calls
        #expect(calls.count == 2)
        #expect(calls.allSatisfy { $0.executable == host.node })
        #expect(calls.allSatisfy { $0.environment["PATH"] == "/opt/node/bin" })
        #expect(calls.allSatisfy { $0.timeout > 0 && $0.timeout <= BrowserPaneOpener.deadline })
        #expect(calls[0].arguments == [host.script.path, "profiles"])
        #expect(file.standardizedFileURL.absoluteString.hasPrefix("file:///Users/example/repo/docs/"))
        #expect(file.standardizedFileURL.absoluteString.hasSuffix("%20index.html"))
        #expect(calls[1].arguments == [
            host.script.path, "open",
            "--profile", "work",
            "--target-pane", "w1:p2",
            "--url", file.standardizedFileURL.absoluteString,
            "--key", BrowserPaneOpener.bindingKey(for: file),
        ])
    }

    /// B11, B13: no running profile stops before the open; a host refusal
    /// reaches the caller as the host's own sentence; a deadline is its own
    /// message.
    @Test @MainActor func failuresReachTheCallerAsTheSentenceTheNoticeShows() async throws {
        func outcome(profiles: BrowserPaneOpener.CommandOutcome, open: BrowserPaneOpener.CommandOutcome) async throws -> Result<Void, BrowserPaneOpener.Failure> {
            var result: Result<Void, BrowserPaneOpener.Failure>?
            let opened = Recorder()
            BrowserPaneOpener.open(
                file: file, targetPane: "w1:p2", host: host, environment: [:],
                runner: { _, arguments, _, _, completion in
                    if arguments.last == "profiles" { completion(profiles) } else { opened.append(executable: host.node, arguments: arguments, environment: [:], timeout: 0); completion(open) }
                },
                completion: { result = $0 }
            )
            try await eventually { result != nil }
            if case .exited(_, _, _) = profiles, profiles.isSuccess == false {
                #expect(opened.calls.isEmpty, "open must not run after profiles failed")
            }
            return try #require(result)
        }
        let none = try await outcome(
            profiles: .exited(status: 0, stdout: profiles(#"{"name":"work","running":false,"state_modified_at":null}"#), stderr: ""),
            open: .exited(status: 0, stdout: "{}", stderr: "")
        )
        #expect(failure(of: none) == .noRunningProfile)

        let linkedElsewhere = try await outcome(
            profiles: .exited(status: 0, stdout: profiles(#"{"name":"work","running":true,"state_modified_at":null}"#), stderr: ""),
            open: .exited(status: 1, stdout: "", stderr: "Browser host diagnostics: /tmp/x.log\n{\"ok\":false,\"error\":\"hide.browser is linked to another installation. Unlink it explicitly before using this build.\"}\n")
        )
        #expect(failure(of: linkedElsewhere) == .host("hide.browser is linked to another installation. Unlink it explicitly before using this build."))

        let plainStderr = try await outcome(
            profiles: .exited(status: 0, stdout: profiles(#"{"name":"work","running":true,"state_modified_at":null}"#), stderr: ""),
            open: .exited(status: 2, stdout: "", stderr: "node: cannot find module 'chromux'\n\n")
        )
        #expect(failure(of: plainStderr) == .host("node: cannot find module 'chromux'"))

        let silent = try await outcome(
            profiles: .exited(status: 3, stdout: "", stderr: ""),
            open: .exited(status: 0, stdout: "{}", stderr: "")
        )
        #expect(failure(of: silent) == .host("Browser pane host exited 3 without a message"))

        let late = try await outcome(
            profiles: .exited(status: 0, stdout: profiles(#"{"name":"work","running":true,"state_modified_at":null}"#), stderr: ""),
            open: .timedOut
        )
        #expect(failure(of: late) == .timedOut)
        #expect(BrowserPaneOpener.Failure.timedOut.message == "Browser pane did not open in time")
    }

    /// D-09: a real process that outlives the deadline is ended and reported
    /// once; the exit that follows the termination does not report again.
    @Test @MainActor func aHostThatNeverAnswersIsEndedAtTheDeadline() async throws {
        var results: [Result<Void, BrowserPaneOpener.Failure>] = []
        let sleeper = BrowserPaneOpener.Host(node: URL(fileURLWithPath: "/bin/sleep"), script: URL(fileURLWithPath: "30"))
        BrowserPaneOpener.open(
            file: file, targetPane: "w1:p2", host: sleeper, environment: [:],
            runner: { executable, arguments, environment, _, completion in
                // The deadline under test is the runner's own; a second is
                // enough to prove the termination path without waiting 30.
                try BrowserPaneOpener.runCommand(executable: executable, arguments: ["30"], environment: environment, timeout: 1, completion: completion)
            },
            completion: { results.append($0) }
        )
        try await eventually(timeout: .seconds(5)) { !results.isEmpty }
        try await Task.sleep(for: .milliseconds(300))
        #expect(results.count == 1)
        #expect(failure(of: results.first) == .timedOut)
    }

    /// `Result<Void, _>` is not Equatable, so the failure is what is compared;
    /// nil means success.
    private func failure(of result: Result<Void, BrowserPaneOpener.Failure>?) -> BrowserPaneOpener.Failure? {
        guard let result, case .failure(let failure) = result else { return nil }
        return failure
    }

    @MainActor private func eventually(timeout: Duration = .seconds(5), _ predicate: @MainActor () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: timeout)
        while !predicate(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        #expect(predicate())
    }

    private final class Recorder: @unchecked Sendable {
        struct Call {
            let executable: URL
            let arguments: [String]
            let environment: [String: String]
            let timeout: TimeInterval
        }

        private let lock = NSLock()
        private var stored: [Call] = []

        var calls: [Call] {
            lock.lock()
            defer { lock.unlock() }
            return stored
        }

        func append(executable: URL, arguments: [String], environment: [String: String], timeout: TimeInterval) {
            lock.lock()
            defer { lock.unlock() }
            stored.append(Call(executable: executable, arguments: arguments, environment: environment, timeout: timeout))
        }
    }
}

private extension BrowserPaneOpener.CommandOutcome {
    var isSuccess: Bool {
        if case .exited(let status, _, _) = self { return status == 0 }
        return false
    }
}
