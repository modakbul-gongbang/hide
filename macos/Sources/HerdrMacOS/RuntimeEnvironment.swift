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
    static let bundledSHA256 = "bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328"
    private static let loginShellTimeout: TimeInterval = 2

    static func loginShellPath() -> String? {
        let process = Process()
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-ilc", "printf '%s' \"$PATH\""]
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

    static func childEnvironment(
        inherited: [String: String],
        loginPath: String?
    ) -> [String: String] {
        var environment: [String: String] = [
            "HOME": inherited["HOME"] ?? NSHomeDirectory(),
            "USER": inherited["USER"] ?? NSUserName(),
            "PATH": loginPath ?? inherited["PATH"] ?? "/usr/bin:/bin",
        ]
        if let socket = inherited["SSH_AUTH_SOCK"], !socket.isEmpty {
            environment["SSH_AUTH_SOCK"] = socket
        }
        if let config = inherited["HERDR_CONFIG_PATH"], !config.isEmpty {
            environment["HERDR_CONFIG_PATH"] = config
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

private struct HerdrCommandResult {
    let status: Int32
    let output: Data
    let error: Data
}

/// Starts an agent through the selected Herdr runtime. hide only supplies
/// non-secret routing arguments; authentication remains entirely owned by the
/// selected CLI and Herdr server.
enum HerdrAgentLauncher {
    static func launch(
        herdrPath: String,
        agent: String,
        checkoutPath: String,
        paneID: String?,
        bypassWarnings: Bool
    ) -> AgentLaunchResult {
        guard AgentCLIAvailability.isUsable(agent) else {
            return AgentLaunchResult(
                succeeded: false,
                message: "\(agent) is not installed on this Mac. Install it, then try again."
            )
        }

        let targetPaneID: String
        if let paneID, !paneID.isEmpty {
            targetPaneID = paneID
        } else if let createdPaneID = createRootPane(
            herdrPath: herdrPath,
            checkoutPath: checkoutPath,
            agent: agent
        ) {
            targetPaneID = createdPaneID
        } else {
            return AgentLaunchResult(
                succeeded: false,
                message: "Hide could not create a Herdr pane for this checkout. Check Herdr status and retry."
            )
        }

        let idempotencyKey = "hide-\(agent)-\(UUID().uuidString.lowercased())"
        var arguments = [
            "agent", "new", "hide-\(agent)",
            "--kind", agent,
            "--pane", targetPaneID,
            "--idempotency-key", idempotencyKey,
            "--cwd", checkoutPath,
            "--no-focus",
        ]
        if bypassWarnings {
            arguments.append("--")
            arguments.append(contentsOf: agent == "claude"
                ? ["--dangerously-skip-permissions"]
                : ["--dangerously-bypass-approvals-and-sandbox"])
        }

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

    private static func createRootPane(
        herdrPath: String,
        checkoutPath: String,
        agent: String
    ) -> String? {
        let result = run(
            herdrPath: herdrPath,
            arguments: [
                "workspace", "create",
                "--cwd", checkoutPath,
                "--label", "hide \(agent)",
                "--no-focus",
            ]
        )
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
