import Foundation
import OSLog

enum HideLaunchTrace {
    private static let logger = Logger(subsystem: "me.grab.hide", category: "launch")

    static func mark(_ event: String, detail: String = "", durationMilliseconds: Int? = nil) {
        let duration = durationMilliseconds.map(String.init) ?? "-"
        logger.info(
            "event=\(event, privacy: .public) detail=\(detail, privacy: .public) duration_ms=\(duration, privacy: .public)"
        )
    }
}

struct HerdrRuntimeSelection: Equatable, Sendable {
    let path: String
    let source: String
    let version: String
    let sha256: String?
    let guidance: String?
}

enum HideStartupDiagnostic {
    static let initializing = "Starting Herdr…"
    static let runtimeUnavailable = "Herdr is unavailable. Install Herdr or repair hide, then reopen the app."

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
    static let bundledVersion = "0.8.2"
    static let bundledSHA256 = "a5d4f4d504d8b309c91f811050559300faba31258425f53c50852fc96f6ae574"
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
    /// the forwarded set. `HERDR_SOCKET_PATH` was missing for exactly that
    /// reason: the core honoured the override and read one Herdr session while
    /// the `herdr` processes the shell spawned reached the default socket, so
    /// panes appeared in a session nobody asked for.
    static let forwardedRoutingKeys = [
        "SSH_AUTH_SOCK",
        "HERDR_CONFIG_PATH",
        "HERDR_SOCKET_PATH",
    ]

    /// Values every child needs, with what the shell substitutes when the
    /// launch environment (a Finder launch, notably) does not carry one.
    static let substitutedKeys = ["HOME", "USER", "PATH"]

    static func childEnvironment(
        inherited: [String: String],
        loginPath: String?
    ) -> [String: String] {
        var environment: [String: String] = [
            "HOME": inherited["HOME"] ?? NSHomeDirectory(),
            "USER": inherited["USER"] ?? NSUserName(),
            "PATH": loginPath ?? inherited["PATH"] ?? "/usr/bin:/bin",
        ]
        for key in forwardedRoutingKeys {
            if let value = inherited[key], !value.isEmpty {
                environment[key] = value
            }
        }
        return environment
    }

    static func resolveExecutable(named name: String) -> String? {
        pathEntries(loginPath: loginShellPath())
            .map { URL(fileURLWithPath: $0).appendingPathComponent(name).path }
            .first { FileManager.default.isExecutableFile(atPath: $0) }
    }
}

enum HerdrRuntimeResolver {
    private static let probeTimeout: TimeInterval = 2

    static func resolve(bundle: Bundle = .main) -> HerdrRuntimeSelection? {
        resolve(
            bundlePath: bundle.path(forResource: "herdr", ofType: nil, inDirectory: "herdr-runtime")
        )
    }

    static func resolve(bundlePath: String?) -> HerdrRuntimeSelection? {
        let hasLiveSocket = FileManager.default.fileExists(
            atPath: NSHomeDirectory() + "/.config/herdr/herdr.sock"
        )
        let installedCandidates = standardInstalledCandidates(homeDirectory: NSHomeDirectory())
            + HideRuntimeEnvironment.pathEntries(loginPath: HideRuntimeEnvironment.loginShellPath())
                .map { URL(fileURLWithPath: $0).appendingPathComponent("herdr").path }

        var oldInstalledVersion: String?
        var firstInstalled: (path: String, version: String)?
        for path in deduplicated(installedCandidates) where FileManager.default.isExecutableFile(atPath: path) {
            guard let version = version(of: path) else { continue }
            firstInstalled = firstInstalled ?? (path, version)
            if compare(version, with: HideRuntimeEnvironment.bundledVersion) != .orderedAscending {
                let source = hasLiveSocket ? "live-socket" : "installed"
                return HerdrRuntimeSelection(
                    path: path,
                    source: source,
                    version: version,
                    sha256: sha256(of: path),
                    guidance: nil
                )
            }
            oldInstalledVersion = version
        }

        if let bundlePath,
           FileManager.default.isExecutableFile(atPath: bundlePath),
           sha256(of: bundlePath) == HideRuntimeEnvironment.bundledSHA256 {
            let guidance = oldInstalledVersion.map {
                "Installed Herdr \($0) is below \(HideRuntimeEnvironment.bundledVersion); hide is using its bundled runtime."
            }
            return HerdrRuntimeSelection(
                path: bundlePath,
                source: hasLiveSocket ? "live-socket" : "bundled",
                version: HideRuntimeEnvironment.bundledVersion,
                sha256: HideRuntimeEnvironment.bundledSHA256,
                guidance: guidance
            )
        }

        if let firstInstalled {
            return HerdrRuntimeSelection(
                path: firstInstalled.path,
                source: "installed-below-minimum",
                version: firstInstalled.version,
                sha256: sha256(of: firstInstalled.path),
                guidance: "The installed Herdr CLI is below \(HideRuntimeEnvironment.bundledVersion), but the verified bundled runtime is unavailable."
            )
        }
        return nil
    }

    static func standardInstalledCandidates(homeDirectory: String) -> [String] {
        [
            homeDirectory + "/.local/bin/herdr",
            "/opt/homebrew/bin/herdr",
            "/usr/local/bin/herdr",
        ]
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

    private static func version(of path: String) -> String? {
        let process = Process()
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = ["--version"]
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            guard waitForExit(process, operation: "version_probe") else { return nil }
        } catch {
            HideLaunchTrace.mark("version_probe.failed", detail: "launch_error")
            return nil
        }
        guard process.terminationStatus == 0 else {
            HideLaunchTrace.mark("version_probe.failed", detail: "exit_\(process.terminationStatus)")
            return nil
        }
        let line = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return line.split(separator: " ").last.map(String.init)
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

    private static func deduplicated(_ paths: [String]) -> [String] {
        var seen = Set<String>()
        return paths.filter { seen.insert($0).inserted }
    }

    private static func compare(_ left: String, with right: String) -> ComparisonResult {
        let leftParts = left.split(separator: ".").compactMap { Int($0) }
        let rightParts = right.split(separator: ".").compactMap { Int($0) }
        for index in 0 ..< max(leftParts.count, rightParts.count) {
            let leftPart = index < leftParts.count ? leftParts[index] : 0
            let rightPart = index < rightParts.count ? rightParts[index] : 0
            if leftPart != rightPart { return leftPart < rightPart ? .orderedAscending : .orderedDescending }
        }
        return .orderedSame
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

struct AgentLaunchResult: Sendable {
    let succeeded: Bool
    let message: String
}

struct TerminalLaunchResult: Sendable {
    let succeeded: Bool
    let message: String
    let paneID: String?
}

private struct HerdrCommandResult {
    let status: Int32
    let output: Data
    let error: Data
}

enum NewAgentProvider: String, CaseIterable, Equatable {
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

struct NewAgentDraft: Equatable {
    var selectedKind: String
    var selectedCheckoutID: String
    var selectedDeviceID: String
    var bypassWarnings: Bool

    static let empty = NewAgentDraft(
        selectedKind: NewAgentProvider.claude.rawValue,
        selectedCheckoutID: "",
        selectedDeviceID: "local",
        bypassWarnings: false
    )

    static func fresh(
        selectedKind: String,
        selectedCheckoutID: String?,
        focusedCheckoutID: String?,
        selectedDeviceID: String
    ) -> NewAgentDraft {
        NewAgentDraft(
            selectedKind: NewAgentProvider(rawValue: selectedKind)?.rawValue
                ?? NewAgentProvider.claude.rawValue,
            selectedCheckoutID: selectedCheckoutID ?? focusedCheckoutID ?? "",
            selectedDeviceID: selectedDeviceID,
            bypassWarnings: false
        )
    }

    mutating func consumeBypassWarnings() -> Bool {
        let value = bypassWarnings
        bypassWarnings = false
        return value
    }
}

enum AgentLaunchArguments {
    static func build(
        provider: NewAgentProvider,
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
    static func build(
        workspaceID: String?,
        checkoutPath: String,
        agent: String
    ) -> [String] {
        if let workspaceID, !workspaceID.isEmpty {
            return [
                "tab", "create",
                "--workspace", workspaceID,
                "--cwd", checkoutPath,
                "--label", "hide \(agent)",
                "--focus",
            ]
        }
        return [
            "workspace", "create",
            "--cwd", checkoutPath,
            "--label", "hide \(agent)",
            "--focus",
        ]
    }
}

/// Starts an agent through the selected Herdr runtime. hide only supplies
/// non-secret routing arguments; authentication remains entirely owned by the
/// selected CLI and Herdr server.
enum HerdrAgentLauncher {
    static func launch(
        herdrPath: String,
        agent: String,
        checkoutPath: String,
        workspaceID: String?,
        bypassWarnings: Bool
    ) -> AgentLaunchResult {
        guard let provider = NewAgentProvider(rawValue: agent) else {
            return AgentLaunchResult(
                succeeded: false,
                message: "Hide supports only Claude and Codex agent launches."
            )
        }
        guard AgentCLIAvailability.isUsable(agent) else {
            return AgentLaunchResult(
                succeeded: false,
                message: "\(agent) is not installed on this Mac. Install it, then try again."
            )
        }

        let targetPaneID: String
        if let createdPaneID = createRootPane(
            herdrPath: herdrPath,
            checkoutPath: checkoutPath,
            workspaceID: workspaceID,
            agent: agent
        ) {
            targetPaneID = createdPaneID
        } else {
            return AgentLaunchResult(
                succeeded: false,
                message: "Hide could not create a Herdr pane for this checkout. Check Herdr status and retry."
            )
        }

        let arguments = AgentLaunchArguments.build(
            provider: provider,
            paneID: targetPaneID,
            bypassWarnings: bypassWarnings
        )

        let process = Process()
        let output = Pipe()
        let errorPipe = Pipe()
        process.executableURL = URL(fileURLWithPath: herdrPath)
        process.arguments = arguments
        process.environment = HideRuntimeEnvironment.childEnvironment()
        process.standardOutput = output
        process.standardError = errorPipe
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return AgentLaunchResult(succeeded: false, message: error.localizedDescription)
        }
        guard process.terminationStatus == 0 else {
            let detail = String(decoding: errorPipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return AgentLaunchResult(
                succeeded: false,
                message: detail.isEmpty
                    ? "Herdr could not start the \(agent) agent. Check the Herdr status and retry."
                    : detail
            )
        }
        return AgentLaunchResult(
            succeeded: true,
            message: "Started \(agent) in \(URL(fileURLWithPath: checkoutPath).lastPathComponent)."
        )
    }

    /// Opens the single-pane tab an agent will run in.
    ///
    /// A repository Herdr already has a workspace for gets another tab in that
    /// workspace; only a repository Herdr has never seen gets a new workspace.
    /// Creating a workspace per launch left one empty duplicate space behind
    /// for every agent started on a paneless checkout. Starting the agent in
    /// the returned root pane keeps the new tab at exactly one pane.
    private static func createRootPane(
        herdrPath: String,
        checkoutPath: String,
        workspaceID: String?,
        agent: String
    ) -> String? {
        let arguments = AgentRootPaneArguments.build(
            workspaceID: workspaceID,
            checkoutPath: checkoutPath,
            agent: agent
        )
        let result = run(herdrPath: herdrPath, arguments: arguments)
        guard result.status == 0,
              let object = try? JSONSerialization.jsonObject(with: result.output) as? [String: Any],
              let resultObject = object["result"] as? [String: Any],
              let rootPane = resultObject["root_pane"] as? [String: Any],
              let paneID = rootPane["pane_id"] as? String,
              !paneID.isEmpty
        else {
            return nil
        }
        return paneID
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
