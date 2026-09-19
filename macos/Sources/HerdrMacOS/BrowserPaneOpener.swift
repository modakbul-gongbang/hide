import CryptoKit
import Foundation

/// Shows a local file in a Hide browser pane through the bundled host command.
///
/// This is the same kind of boundary `ExternalFileOpener` and
/// `ExternalBrowser` are: the shell asks an owner outside itself to show
/// something, here `browser-pane/browser-pane.mjs` from the app's own bundle,
/// run with the `node` the login PATH resolves. The host decides the pane,
/// links the plugin, and talks to chromux; the shell decides the profile and
/// the binding key, runs one process per request under one deadline, and
/// turns the host's answer into a result the caller can show (D-06 to D-09).
enum BrowserPaneOpener {
    /// D-09: a request that has not answered by then is ended and reported.
    static let deadline: TimeInterval = 30

    /// The two executables a request needs, resolved by the caller so a test
    /// can name its own and a menu can say which one is missing.
    struct Host: Equatable {
        let node: URL
        let script: URL
    }

    enum Failure: Error, Equatable {
        case noRunningProfile
        case host(String)
        case timedOut
        case launch(String)

        /// The sentence the notice shows; the host's own last line when it is
        /// the host that refused (B13).
        var message: String {
            switch self {
            case .noRunningProfile: "No running chromux profile. Launch one with chromux launch <name>."
            case .host(let reason): reason
            case .timedOut: "Browser pane did not open in time"
            case .launch(let reason): reason
            }
        }
    }

    enum CommandOutcome: Sendable {
        case exited(status: Int32, stdout: String, stderr: String)
        case timedOut
    }

    typealias CommandRunner = @Sendable (
        _ executable: URL,
        _ arguments: [String],
        _ environment: [String: String],
        _ timeout: TimeInterval,
        _ completion: @escaping @Sendable (CommandOutcome) -> Void
    ) throws -> Void

    /// One managed chromux profile as the host's `profiles` command lists it.
    struct Profile: Decodable, Equatable {
        let name: String
        let running: Bool
        let stateModifiedAt: String?

        enum CodingKeys: String, CodingKey {
            case name
            case running
            case stateModifiedAt = "state_modified_at"
        }
    }

    private struct ProfileListing: Decodable {
        let entries: [Profile]
    }

    /// The host command travels inside the app bundle beside the runtime, in
    /// both the installed app and a dev build (`build_dev_app.sh`).
    static func hostScript(in bundle: Bundle = .main) -> URL? {
        bundle.url(forResource: "browser-pane", withExtension: "mjs", subdirectory: "browser-pane")
    }

    static func nodeExecutable() -> URL? {
        HideRuntimeEnvironment.resolveExecutable(named: "node").map { URL(fileURLWithPath: $0) }
    }

    /// The pane's binding key, derived from the file alone so the same file
    /// converges on the same pane (D-07) and so it satisfies the host's
    /// identifier rule: letters, digits, dots, underscores or hyphens, at
    /// most 80 characters, starting with a letter or digit.
    static func bindingKey(for file: URL) -> String {
        let digest = SHA256.hash(data: Data(file.standardizedFileURL.path.utf8))
        let hex = digest.map { String(format: "%02x", $0) }.joined()
        return "file-" + hex.prefix(32)
    }

    /// D-06: the running managed profile whose state changed most recently.
    /// A running profile with an unreadable time still counts, after every
    /// dated one, so a profile is never refused for a clock the host could
    /// not read.
    static func selectProfile(from profiles: [Profile]) -> Result<String, Failure> {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let running = profiles.filter(\.running).map { profile in
            (profile.name, profile.stateModifiedAt.flatMap(formatter.date(from:)) ?? .distantPast)
        }
        guard let chosen = running.max(by: { $0.1 < $1.1 }) else {
            return .failure(.noRunningProfile)
        }
        return .success(chosen.0)
    }

    /// The failure sentence the host printed: its JSON error when the last
    /// line of stderr is one, else that line itself, else the exit status.
    static func hostFailure(status: Int32, stderr: String) -> Failure {
        let lastLine = stderr
            .split(separator: "\n", omittingEmptySubsequences: true)
            .last
            .map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        if let data = lastLine.data(using: .utf8),
           let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let error = object["error"] as? String, !error.isEmpty {
            return .host(error)
        }
        if !lastLine.isEmpty {
            return .host(lastLine)
        }
        return .host("Browser pane host exited \(status) without a message")
    }

    /// Opens `file` beside `targetPane`. Two host runs, `profiles` then
    /// `open`, share one deadline; `completion` is called exactly once, on
    /// the main actor, with the result the notice shows.
    static func open(
        file: URL,
        targetPane: String,
        host: Host,
        environment: [String: String],
        runner: @escaping CommandRunner = Self.runCommand,
        completion: @escaping @MainActor (Result<Void, Failure>) -> Void
    ) {
        let started = Date()
        let finish: @Sendable (Result<Void, Failure>) -> Void = { result in
            Task { @MainActor in completion(result) }
        }
        let run: @Sendable ([String], @escaping @Sendable (CommandOutcome) -> Void) -> Void = { arguments, handle in
            let remaining = deadline - Date().timeIntervalSince(started)
            guard remaining > 0 else {
                handle(.timedOut)
                return
            }
            do {
                try runner(host.node, [host.script.path] + arguments, environment, remaining, handle)
            } catch {
                finish(.failure(.launch("Browser pane host could not start: \(error.localizedDescription)")))
            }
        }
        run(["profiles"]) { outcome in
            let profiles: [Profile]
            switch outcome {
            case .timedOut:
                return finish(.failure(.timedOut))
            case .exited(let status, _, let stderr) where status != 0:
                return finish(.failure(hostFailure(status: status, stderr: stderr)))
            case .exited(_, let stdout, _):
                guard let data = stdout.data(using: .utf8),
                      let listing = try? JSONDecoder().decode(ProfileListing.self, from: data)
                else {
                    return finish(.failure(.host("Browser pane host answered profiles with something other than a profile list")))
                }
                profiles = listing.entries
            }
            let profile: String
            switch selectProfile(from: profiles) {
            case .failure(let failure): return finish(.failure(failure))
            case .success(let name): profile = name
            }
            run([
                "open",
                "--profile", profile,
                "--target-pane", targetPane,
                "--url", file.standardizedFileURL.absoluteString,
                "--key", bindingKey(for: file),
            ]) { outcome in
                switch outcome {
                case .timedOut:
                    finish(.failure(.timedOut))
                case .exited(let status, _, let stderr) where status != 0:
                    finish(.failure(hostFailure(status: status, stderr: stderr)))
                case .exited:
                    finish(.success(()))
                }
            }
        }
    }

    /// Runs one host process with its output captured and a deadline after
    /// which it is terminated; the completion fires once either way.
    static func runCommand(
        executable: URL,
        arguments: [String],
        environment: [String: String],
        timeout: TimeInterval,
        completion: @escaping @Sendable (CommandOutcome) -> Void
    ) throws {
        let process = Process()
        let standardOutput = Pipe()
        let standardError = Pipe()
        process.executableURL = executable
        process.arguments = arguments
        process.environment = environment
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = standardOutput
        process.standardError = standardError
        let settled = HostSettlement()
        // The pipes are drained before termination is observed; a host that
        // fills a pipe and waits for a reader would otherwise never exit.
        let output = DrainedPipes(standardOutput: standardOutput, standardError: standardError)
        process.terminationHandler = { process in
            let (stdout, stderr) = output.finish()
            guard settled.claim() else { return }
            completion(.exited(status: process.terminationStatus, stdout: stdout, stderr: stderr))
        }
        try process.run()
        DispatchQueue.global().asyncAfter(deadline: .now() + timeout) {
            guard process.isRunning, settled.claim() else { return }
            process.terminate()
            completion(.timedOut)
        }
    }

    /// Whichever of the exit and the deadline comes first reports; the other
    /// finds the flag set and stays silent.
    private final class HostSettlement: @unchecked Sendable {
        private let lock = NSLock()
        private var claimed = false

        func claim() -> Bool {
            lock.lock()
            defer { lock.unlock() }
            if claimed { return false }
            claimed = true
            return true
        }
    }

    /// Both pipes read to their end on their own threads; `finish` joins them,
    /// and the group wait is what orders the two writes before the read.
    private final class DrainedPipes: @unchecked Sendable {
        private let group = DispatchGroup()
        private var stdout = Data()
        private var stderr = Data()

        init(standardOutput: Pipe, standardError: Pipe) {
            drain(standardOutput.fileHandleForReading) { [self] in stdout = $0 }
            drain(standardError.fileHandleForReading) { [self] in stderr = $0 }
        }

        private func drain(_ handle: FileHandle, into store: @escaping @Sendable (Data) -> Void) {
            group.enter()
            DispatchQueue.global().async { [group] in
                store(handle.readDataToEndOfFile())
                group.leave()
            }
        }

        func finish() -> (String, String) {
            group.wait()
            return (String(decoding: stdout, as: UTF8.self), String(decoding: stderr, as: UTF8.self))
        }
    }
}
