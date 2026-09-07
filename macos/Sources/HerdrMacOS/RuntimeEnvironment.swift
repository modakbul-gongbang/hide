import Foundation
import OSLog

enum HideLaunchTrace {
    private static let logger = Logger(subsystem: "me.grab.hide", category: "launch")
    private static let onceLock = NSLock()
    nonisolated(unsafe) private static var alreadyMarked: Set<String> = []

    /// When this process was started, read from the kernel rather than from
    /// the first line of Swift that runs. A launch interval measured against
    /// it means the same thing inside the process as it does to a stopwatch
    /// started outside it.
    static let processStart = processStartDate()

    static func mark(_ event: String, detail: String = "", durationMilliseconds: Int? = nil) {
        let duration = durationMilliseconds.map(String.init) ?? "-"
        logger.info(
            "event=\(event, privacy: .public) detail=\(detail, privacy: .public) duration_ms=\(duration, privacy: .public)"
        )
    }

    /// Marks a launch milestone that happens once, carrying the milliseconds
    /// since the process started. Later calls for the same event are dropped,
    /// so the recorded value always names the first occurrence.
    static func markOnce(_ event: String, detail: String = "") {
        onceLock.lock()
        let isFirst = alreadyMarked.insert(event).inserted
        onceLock.unlock()
        guard isFirst else { return }
        mark(
            event,
            detail: detail,
            durationMilliseconds: Int(Date().timeIntervalSince(processStart) * 1_000)
        )
    }

    private static func processStartDate() -> Date {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var name: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&name, 4, &info, &size, nil, 0) == 0 else { return Date() }
        let started = info.kp_proc.p_starttime
        return Date(
            timeIntervalSince1970: Double(started.tv_sec)
                + Double(started.tv_usec) / 1_000_000
        )
    }
}

/// The Herdr binary this launch runs: always the one shipped inside the app,
/// verified against the manifest digest before anything is started.
struct HerdrRuntimeSelection: Equatable, Sendable {
    let path: String
    let version: String
    let sha256: String
}

enum HideStartupDiagnostic {
    static let initializing = "Starting Herdr…"
    static let runtimeUnavailable =
        "hide's bundled Herdr runtime is missing or failed verification. Reinstall hide, then reopen it."

    static func serverStartFailed(_ reason: String) -> String {
        "Herdr could not start: \(reason). Check the runtime installation and reopen hide."
    }

    static func serverExited(status: Int32) -> String {
        "Herdr stopped before connecting (exit status \(status)). Check the runtime installation and reopen hide."
    }
}

/// The Finder launch contract is intentionally small: only the login-shell
/// PATH and non-secret process routing values are carried into child tools.
/// No key, password, token, or passphrase is read, stored, or displayed.
enum HideRuntimeEnvironment {
    private static let loginShellTimeout: TimeInterval = 2
    private static let resolvedLoginShellPath: String? = resolveLoginShellPath()

    static func loginShellPath() -> String? {
        resolvedLoginShellPath
    }

    private static func resolveLoginShellPath() -> String? {
        let process = Process()
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        // Login mode reads the user's PATH setup without running the
        // interactive-only .zshrc hooks that can block Finder startup.
        process.arguments = ["-lc", "printf '%s' \"$PATH\""]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            HideLaunchTrace.mark("login_shell_path.failed", detail: "launch_error")
            return nil
        }
        let deadline = Date().addingTimeInterval(loginShellTimeout)
        while process.isRunning, Date() < deadline {
            Thread.sleep(forTimeInterval: 0.01)
        }
        if process.isRunning {
            process.terminate()
            HideLaunchTrace.mark("login_shell_path.failed", detail: "timeout")
            return nil
        }
        guard process.terminationStatus == 0 else {
            HideLaunchTrace.mark("login_shell_path.failed", detail: "exit_\(process.terminationStatus)")
            return nil
        }
        let path = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !path.isEmpty else {
            HideLaunchTrace.mark("login_shell_path.failed", detail: "empty")
            return nil
        }
        HideLaunchTrace.mark("login_shell_path.ready", detail: "available")
        return path
    }

    static func pathEntries(loginPath: String?) -> [String] {
        var entries = (loginPath ?? "")
            .split(separator: ":", omittingEmptySubsequences: true)
            .map(String.init)
        for fallback in [
            NSHomeDirectory() + "/.local/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
        ] {
            if !entries.contains(fallback) { entries.append(fallback) }
        }
        return entries
    }

    static func childEnvironment() -> [String: String] {
        childEnvironment(
            inherited: ProcessInfo.processInfo.environment,
            loginPath: loginShellPath()
        )
    }

    /// Routing values a child tool needs and the shell can only pass on if it
    /// has one itself. Every key here is a path or a socket, never a secret.
    ///
    /// This list, not a chain of hand-written `if let` blocks, is what decides
    /// the forwarded set. The socket is not on it because it is never merely
    /// forwarded: `childEnvironment` always sets `HERDR_SOCKET_PATH` to the
    /// path the core reads, so the server hide starts and every `herdr` it
    /// spawns bind to that socket whatever the launch environment carried.
    /// Without that, `XDG_CONFIG_HOME` moves the server's default socket while
    /// the core keeps watching `~/.config/herdr/herdr.sock`, and an override
    /// that is forwarded only when present once left the spawned tools on the
    /// default socket while the core read the override, so panes appeared in
    /// a session nobody asked for.
    static let forwardedRoutingKeys = [
        "SSH_AUTH_SOCK",
        "HERDR_CONFIG_PATH",
    ]

    /// Values every child needs, with what the shell substitutes when the
    /// launch environment (a Finder launch, notably) does not carry one.
    /// The socket is substituted too: the core's default rule fills it in.
    static let substitutedKeys = ["HOME", "USER", "PATH", "HERDR_SOCKET_PATH"]

    static func childEnvironment(
        inherited: [String: String],
        loginPath: String?,
        homeDirectory: String = NSHomeDirectory()
    ) -> [String: String] {
        var environment: [String: String] = [
            "HOME": inherited["HOME"] ?? homeDirectory,
            "USER": inherited["USER"] ?? NSUserName(),
            "PATH": loginPath ?? inherited["PATH"] ?? "/usr/bin:/bin",
            "HERDR_SOCKET_PATH": herdrSocketPath(environment: inherited, homeDirectory: homeDirectory),
        ]
        for key in forwardedRoutingKeys {
            if let value = inherited[key], !value.isEmpty {
                environment[key] = value
            }
        }
        return environment
    }

    /// Puts the login PATH on this process, so tools the core runs find the
    /// same binaries the shell's own child processes do.
    ///
    /// A Finder launch inherits `/usr/bin:/bin:/usr/sbin:/sbin`. `git` and
    /// `lsof` happen to live there; `gh` lives in `/opt/homebrew/bin` and does
    /// not. Without this the core would report "gh is not installed" on every
    /// machine that installed it with Homebrew, which is all of them - and the
    /// message would be true of the process while being false of the machine.
    ///
    /// The shell already computes this environment for its child tools; this
    /// applies the same PATH to the process itself rather than teaching the
    /// core a second copy of where binaries live.
    static func applyPathToProcess() {
        let path = pathEntries(loginPath: loginShellPath()).joined(separator: ":")
        guard !path.isEmpty else {
            HideLaunchTrace.mark("process_path.failed", detail: "empty")
            return
        }
        setenv("PATH", path, 1)
        HideLaunchTrace.mark("process_path.ready", detail: "applied")
    }

    static func resolveExecutable(named name: String) -> String? {
        pathEntries(loginPath: loginShellPath())
            .map { URL(fileURLWithPath: $0).appendingPathComponent(name).path }
            .first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    /// The socket the shell, the core and every child `herdr` share. The
    /// core's environment registry applies the same rule to the same
    /// variable, so an override moves all three together; the shell once read
    /// only the default here and started a server on a socket the core was
    /// not watching.
    static func herdrSocketPath(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        homeDirectory: String = NSHomeDirectory()
    ) -> String {
        if let override = environment["HERDR_SOCKET_PATH"], override.hasPrefix("/") {
            return override
        }
        return homeDirectory + "/.config/herdr/herdr.sock"
    }
}

/// hide runs the Herdr it ships. There is no installed-CLI search and no
/// version floor: the manifest names one digest, and the binary inside the
/// bundle either carries it or is not started.
enum HerdrRuntimeResolver {
    private static let probeTimeout: TimeInterval = 2

    static func resolve(bundle: Bundle = .main) -> HerdrRuntimeSelection? {
        resolve(
            bundlePath: bundle.path(forResource: "herdr", ofType: nil, inDirectory: "herdr-runtime"),
            pin: HerdrRuntimePinLoader.pinned
        )
    }

    /// Nil means no runtime may be started, and the reason has already been
    /// written as a diagnostic; the caller shows the startup message.
    static func resolve(bundlePath: String?, pin: HerdrRuntimePin?) -> HerdrRuntimeSelection? {
        // Without the shipped pin there is no digest to verify a bundled
        // binary against. The loader has already reported why on stderr.
        guard let pin else { return nil }
        guard let bundlePath, FileManager.default.isExecutableFile(atPath: bundlePath) else {
            HideDiagnostic.emit(
                component: "runtime",
                kind: "bundle.missing",
                message: "no executable Herdr at \(bundlePath ?? "<no herdr-runtime resource>")"
            )
            HideLaunchTrace.mark("runtime_resolve.failed", detail: "bundle_missing")
            return nil
        }
        let digest = sha256(of: bundlePath)
        guard digest == pin.sha256 else {
            HideDiagnostic.emit(
                component: "runtime",
                kind: "bundle.digest_mismatch",
                message: "bundled Herdr at \(bundlePath) has digest \(digest ?? "<unreadable>"), the pin is \(pin.sha256)"
            )
            HideLaunchTrace.mark("runtime_resolve.failed", detail: "bundle_digest_mismatch")
            return nil
        }
        return HerdrRuntimeSelection(path: bundlePath, version: pin.version, sha256: pin.sha256)
    }

    static func startServerIfNeeded(
        selection: HerdrRuntimeSelection?,
        socketPath: String,
        environment: [String: String]? = nil
    ) -> HerdrServerStartResult {
        guard !FileManager.default.fileExists(atPath: socketPath) else { return .notNeeded }
        guard let selection else { return .failed(HideStartupDiagnostic.runtimeUnavailable) }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: selection.path)
        process.arguments = ["server"]
        process.environment = environment ?? HideRuntimeEnvironment.childEnvironment()
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            return .started(process)
        } catch {
            return .failed(HideStartupDiagnostic.serverStartFailed(error.localizedDescription))
        }
    }

    private static func sha256(of path: String) -> String? {
        let process = Process()
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/shasum")
        process.arguments = ["-a", "256", path]
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            guard waitForExit(process, operation: "sha256_probe") else { return nil }
        } catch {
            HideLaunchTrace.mark("sha256_probe.failed", detail: "launch_error")
            return nil
        }
        guard process.terminationStatus == 0 else {
            HideLaunchTrace.mark("sha256_probe.failed", detail: "exit_\(process.terminationStatus)")
            return nil
        }
        return String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .split(separator: " ")
            .first
            .map(String.init)
    }

    private static func waitForExit(_ process: Process, operation: String) -> Bool {
        let deadline = Date().addingTimeInterval(probeTimeout)
        while process.isRunning, Date() < deadline {
            Thread.sleep(forTimeInterval: 0.01)
        }
        guard !process.isRunning else {
            process.terminate()
            HideLaunchTrace.mark("\(operation).failed", detail: "timeout")
            return false
        }
        return true
    }

}

enum HerdrServerStartResult {
    case notNeeded
    case started(Process)
    case failed(String)
}

enum AgentCLIAvailability {
    static func executable(for agent: String) -> String? {
        switch agent {
        case "claude", "codex":
            return HideRuntimeEnvironment.resolveExecutable(named: agent)
        default:
            return nil
        }
    }

    /// Deliberately presence-only. hide does not impose a lower-bound version
    /// gate on agent CLIs; the selected CLI owns its own compatibility policy.
    static func isUsable(_ agent: String) -> Bool {
        executable(for: agent) != nil
    }
}

struct TerminalLaunchResult: Sendable {
    let succeeded: Bool
    let message: String
    let paneID: String?
}

/// One Herdr invocation's answer.
struct HerdrCommandResult: Sendable {
    let status: Int32
    let output: Data
    let error: Data
}

/// How the chat launcher reaches Herdr.
///
/// A closure rather than a hard-wired `Process` so a failure at any of the
/// four steps can be injected in a test. The failure paths are the ones worth
/// proving: a step that fails silently and reads as success is exactly the
/// defect the step names exist to prevent, and it cannot be reached by running
/// a real server successfully.
typealias HerdrCommandRunner = @Sendable (_ arguments: [String]) -> HerdrCommandResult

/// The two agent CLIs Hide can start.
enum AgentProvider: String, CaseIterable, Equatable, Sendable {
    case claude
    case codex

    var bypassFlag: String {
        switch self {
        case .claude:
            "--dangerously-skip-permissions"
        case .codex:
            "--dangerously-bypass-approvals-and-sandbox"
        }
    }
}

enum AgentLaunchArguments {
    static func build(
        provider: AgentProvider,
        paneID: String,
        bypassWarnings: Bool
    ) -> [String] {
        let agent = provider.rawValue
        var arguments = [
            "agent", "start", "hide-\(agent)",
            "--kind", agent,
            "--pane", paneID,
        ]
        if bypassWarnings {
            arguments.append("--")
            arguments.append(provider.bypassFlag)
        }
        return arguments
    }
}

enum AgentRootPaneArguments {
    /// `cwd` rather than a checkout path: Scratch is a folder with no
    /// checkout, and it opens its tabs through exactly this call.
    static func build(
        workspaceID: String?,
        cwd: String,
        agent: String
    ) -> [String] {
        if let workspaceID, !workspaceID.isEmpty {
            return [
                "tab", "create",
                "--workspace", workspaceID,
                "--cwd", cwd,
                "--label", "hide \(agent)",
                "--focus",
            ]
        }
        return [
            "workspace", "create",
            "--cwd", cwd,
            "--label", "hide \(agent)",
            "--focus",
        ]
    }
}

enum HerdrLiveWorkspaceIdentity {
    /// The checkout's workspace id belongs to Hide's catalog domain. The live
    /// Herdr workspace id is encoded by every attached tab as `<workspace>:tN`.
    /// Keep that domain crossing explicit so callers never accidentally pass a
    /// catalog id to `herdr tab create --workspace`.
    static func workspaceID(for tabs: [CoreTabSnapshot]) -> String? {
        tabs.lazy
            .compactMap(\.id)
            .compactMap(workspaceID(fromTabID:))
            .first
    }

    static func workspaceID(fromTabID tabID: String) -> String? {
        guard let separator = tabID.lastIndex(of: ":") else { return nil }
        let workspaceID = tabID[..<separator]
        let tabComponent = tabID[tabID.index(after: separator)...]
        guard !workspaceID.isEmpty,
              tabComponent.first == "t",
              tabComponent.dropFirst().allSatisfy(\.isNumber),
              tabComponent.count > 1
        else {
            return nil
        }
        return String(workspaceID)
    }
}

/// Starts a chat: one tab, one agent, its first message, and its title.
///
/// Four Herdr calls in a fixed order. The message is sent only after the agent
/// reports itself ready, because an agent still on its trust or update screen
/// would swallow it. Each step reports its own name on failure, and every
/// failure after the tab exists leaves that tab alive as a shell prompt rather
/// than tidying away work the operator can still use.
///
/// hide supplies non-secret routing arguments only; authentication stays with
/// the selected CLI and the Herdr server.
enum HerdrChatLauncher {
    /// Starts only the agent-owned half after another owner has created the
    /// pane. Scratch uses this entry point because Rust owns its folder,
    /// workspace, and tab creation.
    static func startInPane(
        paneID: String,
        path: String,
        provider: AgentProvider,
        message: String?,
        bypassWarnings: Bool,
        agentIsInstalled: Bool,
        run: HerdrCommandRunner
    ) -> ChatLaunchResult {
        guard agentIsInstalled else {
            return ChatLaunchResult(
                succeeded: false,
                failedStep: .startAgent,
                message: "\(provider.rawValue) is not installed on this Mac. Install it, then try again.",
                paneID: paneID
            )
        }
        var remaining: [(step: ChatLaunchStep, arguments: [String])] = [
            (.startAgent, AgentLaunchArguments.build(
                provider: provider,
                paneID: paneID,
                bypassWarnings: bypassWarnings
            ))
        ]
        if let message {
            remaining += ChatLaunchPlan.stepsAfterTab(
                provider: provider,
                message: message,
                bypassWarnings: bypassWarnings,
                paneID: paneID
            ).dropFirst()
        }
        for entry in remaining {
            let result = run(entry.arguments)
            let succeeded = result.status == 0
            trace(step: entry.step, paneID: paneID, succeeded: succeeded)
            guard succeeded else {
                return ChatLaunchResult(
                    succeeded: false,
                    failedStep: entry.step,
                    message: failureMessage(
                        step: entry.step,
                        detail: detail(from: result),
                        fallback: "Herdr refused the \(entry.step.rawValue) step."
                    ),
                    paneID: paneID
                )
            }
        }
        return ChatLaunchResult(
            succeeded: true,
            failedStep: nil,
            message: "Started \(provider.rawValue) in \(URL(fileURLWithPath: path).lastPathComponent).",
            paneID: paneID
        )
    }

    static func start(
        herdrPath: String,
        destination: CheckoutChatDestination,
        provider: AgentProvider,
        message: String,
        bypassWarnings: Bool
    ) -> ChatLaunchResult {
        start(
            destination: destination,
            provider: provider,
            message: message,
            bypassWarnings: bypassWarnings,
            agentIsInstalled: AgentCLIAvailability.isUsable(provider.rawValue),
            run: { arguments in run(herdrPath: herdrPath, arguments: arguments) }
        )
    }

    static func start(
        destination: CheckoutChatDestination,
        provider: AgentProvider,
        message: String,
        bypassWarnings: Bool,
        agentIsInstalled: Bool,
        run: HerdrCommandRunner
    ) -> ChatLaunchResult {
        guard agentIsInstalled else {
            return ChatLaunchResult(
                succeeded: false,
                failedStep: .startAgent,
                message: "\(provider.rawValue) is not installed on this Mac. Install it, then try again.",
                paneID: nil
            )
        }

        let created = run(ChatLaunchPlan.createTab(destination: destination, provider: provider))
        guard created.status == 0,
              let paneID = rootPaneID(from: created.output),
              !paneID.isEmpty
        else {
            trace(step: .createTab, paneID: nil, succeeded: false)
            return ChatLaunchResult(
                succeeded: false,
                failedStep: .createTab,
                message: failureMessage(
                    step: .createTab,
                    detail: detail(from: created),
                    fallback: "Hide could not create a Herdr tab. Check Herdr status and retry."
                ),
                paneID: nil
            )
        }
        trace(step: .createTab, paneID: paneID, succeeded: true)
        return startInPane(
            paneID: paneID,
            path: destination.path,
            provider: provider,
            message: message,
            bypassWarnings: bypassWarnings,
            agentIsInstalled: true,
            run: run
        )
    }

    /// One line per step: which step, which pane, and whether it worked.
    ///
    /// Never the message. What the operator asked an agent is theirs, and a
    /// diagnostic that carried it would put it in every log this app writes.
    private static func trace(step: ChatLaunchStep, paneID: String?, succeeded: Bool) {
        HideLaunchTrace.mark(
            succeeded ? "chat.launch.step" : "chat.launch.step_failed",
            detail: "step=\(step.rawValue) pane=\(paneID ?? "none")"
        )
    }

    private static func failureMessage(
        step: ChatLaunchStep,
        detail: String,
        fallback: String
    ) -> String {
        let reason = detail.isEmpty ? fallback : detail
        return "Chat could not \(step.rawValue): \(reason)"
    }

    private static func detail(from result: HerdrCommandResult) -> String {
        let text = String(decoding: result.error, as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return HerdrErrorEnvelope.message(in: text) ?? text
    }

    /// Both `tab.create` and `workspace.create` answer with the root pane the
    /// tab was opened at, which is the pane the agent then starts in. Starting
    /// it there keeps the new tab at exactly one pane.
    private static func rootPaneID(from output: Data) -> String? {
        (try? JSONSerialization.jsonObject(with: output))
            .flatMap { $0 as? [String: Any] }
            .flatMap { $0["result"] as? [String: Any] }
            .flatMap { $0["root_pane"] as? [String: Any] }
            .flatMap { $0["pane_id"] as? String }
    }

    static func run(herdrPath: String, arguments: [String]) -> HerdrCommandResult {
        let process = Process()
        let output = Pipe()
        let error = Pipe()
        process.executableURL = URL(fileURLWithPath: herdrPath)
        process.arguments = arguments
        process.environment = HideRuntimeEnvironment.childEnvironment()
        process.standardOutput = output
        process.standardError = error
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return HerdrCommandResult(
                status: 127,
                output: Data(),
                error: Data(error.localizedDescription.utf8)
            )
        }
        return HerdrCommandResult(
            status: process.terminationStatus,
            output: output.fileHandleForReading.readDataToEndOfFile(),
            error: error.fileHandleForReading.readDataToEndOfFile()
        )
    }
}

/// Starts the first terminal for a checkout that has no live Herdr pane yet.
/// The command creates a dedicated Herdr workspace rooted at the checkout;
/// herdr-core then reconciles that pane back onto the selected checkout by its
/// returned pane ID, with cwd remaining the catalog identity fallback.
/// Checkout selection keeps the operation no-focus, while the explicit New
/// Tab command requests focus for the root pane it creates.
enum HerdrTerminalLauncher {
    static func launch(
        herdrPath: String,
        checkoutPath: String,
        checkoutLabel: String,
        focus: Bool
    ) -> TerminalLaunchResult {
        var arguments = [
            "workspace", "create",
            "--cwd", checkoutPath,
            "--label", "hide \(checkoutLabel)",
        ]
        arguments.append(focus ? "--focus" : "--no-focus")
        let result = run(
            herdrPath: herdrPath,
            arguments: arguments
        )
        guard result.status == 0 else {
            let detail = String(decoding: result.error, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return TerminalLaunchResult(
                succeeded: false,
                message: detail.isEmpty
                    ? "Hide could not start a terminal for this checkout. Check Herdr status and retry."
                    : detail,
                paneID: nil
            )
        }

        let paneID = (try? JSONSerialization.jsonObject(with: result.output))
            .flatMap { $0 as? [String: Any] }
            .flatMap { $0["result"] as? [String: Any] }
            .flatMap { $0["root_pane"] as? [String: Any] }
            .flatMap { $0["pane_id"] as? String }

        return TerminalLaunchResult(
            succeeded: true,
            message: "Started a terminal in \(URL(fileURLWithPath: checkoutPath).lastPathComponent).",
            paneID: paneID
        )
    }

    private static func run(herdrPath: String, arguments: [String]) -> HerdrCommandResult {
        let process = Process()
        let output = Pipe()
        let error = Pipe()
        process.executableURL = URL(fileURLWithPath: herdrPath)
        process.arguments = arguments
        process.environment = HideRuntimeEnvironment.childEnvironment()
        process.standardOutput = output
        process.standardError = error
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return HerdrCommandResult(
                status: 127,
                output: Data(),
                error: Data(error.localizedDescription.utf8)
            )
        }
        return HerdrCommandResult(
            status: process.terminationStatus,
            output: output.fileHandleForReading.readDataToEndOfFile(),
            error: error.fileHandleForReading.readDataToEndOfFile()
        )
    }
}
