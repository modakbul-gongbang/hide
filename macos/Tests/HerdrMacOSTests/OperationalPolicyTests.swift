import AppKit
import CoreGraphics
import Foundation
import Testing
@testable import HerdrMacOS

@Test func loginShellPathIsTheChildToolPATH() {
    let loginPath = HideRuntimeEnvironment.loginShellPath()
    let childEnvironment = HideRuntimeEnvironment.childEnvironment()

    #expect(loginPath != nil)
    #expect(childEnvironment["PATH"] == loginPath)
}

@Test func childToolEnvironmentHasOnlyNonSecretRoutingValues() {
    let allowedKeys = Set(
        HideRuntimeEnvironment.substitutedKeys + HideRuntimeEnvironment.forwardedRoutingKeys
    )

    #expect(Set(HideRuntimeEnvironment.childEnvironment().keys).isSubset(of: allowedKeys))
}

@Test func finderLikeEnvironmentUsesAVisibleSafePATHFallback() {
    let allowedKeys = Set(
        HideRuntimeEnvironment.substitutedKeys + HideRuntimeEnvironment.forwardedRoutingKeys
    )
    let environment = HideRuntimeEnvironment.childEnvironment(
        inherited: ["HOME": "/tmp/hide-finder", "USER": "tester"],
        loginPath: nil
    )

    #expect(environment["PATH"] == "/usr/bin:/bin")
    #expect(Set(environment.keys).isSubset(of: allowedKeys))
}

@Test func childToolsReachTheSameHerdrSessionTheShellReads() {
    let environment = HideRuntimeEnvironment.childEnvironment(
        inherited: [
            "HOME": "/tmp/hide-routing",
            "USER": "tester",
            "HERDR_SOCKET_PATH": "/private/tmp/hide-worktree/herdr.sock",
            "HERDR_CONFIG_PATH": "/private/tmp/hide-worktree/config",
        ],
        loginPath: "/usr/bin:/bin"
    )

    // A child that reaches the default socket while the core reads an override
    // creates panes in a session the user is not looking at.
    #expect(environment["HERDR_SOCKET_PATH"] == "/private/tmp/hide-worktree/herdr.sock")
    #expect(environment["HERDR_CONFIG_PATH"] == "/private/tmp/hide-worktree/config")
}

@Test func anEmptyRoutingValueIsNotForwardedAsIfItWereConfigured() {
    let environment = HideRuntimeEnvironment.childEnvironment(
        inherited: ["HOME": "/tmp/hide-routing", "USER": "tester", "HERDR_SOCKET_PATH": ""],
        loginPath: "/usr/bin:/bin"
    )

    #expect(environment["HERDR_SOCKET_PATH"] == nil)
}

@Test func perWorktreeInstancesKeepSeparateUIState() {
    let release = CoreBridge.defaultStatePath(
        bundleIdentifier: CoreBridge.releaseBundleIdentifier
    )
    let worktree = CoreBridge.defaultStatePath(bundleIdentifier: "me.grab.hide.workbench")
    let otherWorktree = CoreBridge.defaultStatePath(bundleIdentifier: "me.grab.hide.ux")

    // The installed app keeps the path it already writes, so an upgrade does
    // not silently start from an empty workspace.
    #expect(release.hasSuffix("/hide/state.json"))
    #expect(worktree != release)
    #expect(worktree != otherWorktree)
    #expect(worktree.hasSuffix("/hide/instances/me.grab.hide.workbench/state.json"))
}

@Test func anUnidentifiedBundleFallsBackToTheReleaseStatePath() {
    // A `swift test` process and a plain executable run have no bundle
    // identifier; they must not invent a third state file.
    #expect(
        CoreBridge.defaultStatePath(bundleIdentifier: nil)
            == CoreBridge.defaultStatePath(bundleIdentifier: CoreBridge.releaseBundleIdentifier)
    )
}

@Test func missingRuntimeReturnsAVisibleServerStartFailure() {
    let result = HerdrRuntimeResolver.startServerIfNeeded(
        selection: nil,
        socketPath: "/private/tmp/hide-missing-runtime-verification.sock",
        environment: [:]
    )

    guard case .failed(let message) = result else {
        Issue.record("A missing runtime must return a visible failure instead of a silent no-op.")
        return
    }
    #expect(message == HideStartupDiagnostic.runtimeUnavailable)
}

@Test func unlaunchableRuntimeReturnsItsLaunchFailureToTheCaller() {
    let result = HerdrRuntimeResolver.startServerIfNeeded(
        selection: HerdrRuntimeSelection(
            path: "/private/tmp/hide-runtime-does-not-exist",
            version: "0.0.0-test",
            sha256: String(repeating: "0", count: 64)
        ),
        socketPath: "/private/tmp/hide-unlaunchable-runtime-verification.sock",
        environment: [:]
    )

    guard case .failed(let message) = result else {
        Issue.record("An unlaunchable runtime must return a visible launch failure.")
        return
    }
    #expect(message.hasPrefix("Herdr could not start:"))
}

/// A launch creates one core, and it cannot be created before the Herdr
/// binary it needs is resolved. Until then the bridge holds no core and says
/// what it is waiting for, rather than standing up a throwaway core whose
/// empty navigator reads as "no workspaces" and whose teardown blocks the
/// main thread joining its half-finished session sync.
@Test @MainActor func coreBridgeWaitsForTheRuntimeAndSaysSoBeforeItsOnlyCore() {
    let bridge = CoreBridge(arguments: [
        "HerdrMacOS",
        "--state-path",
        "/tmp/hide-p0-startup-test-state.json",
    ])

    #expect(bridge.snapshot == nil)
    #expect(bridge.runtimeSelection == nil)
    #expect(bridge.bridgeError == HideStartupDiagnostic.initializing)
}

@Test @MainActor func mainWindowPresentationIsVisibleBeforeRuntimeWorkStarts() {
    let application = NSApplication.shared
    let window = NSWindow(
        contentRect: CGRect(x: 0, y: 0, width: 480, height: 320),
        styleMask: [.titled, .closable],
        backing: .buffered,
        defer: false
    )

    MainWindowPresentation.present(window, application: application)

    #expect(window.isVisible)
    #expect(application.windows.contains { $0 === window })
    // Closing the last test window asks the xctest-host application to
    // terminate while the remaining Swift Testing cases are still running.
    // Ordering it out keeps the test isolated without changing host
    // lifecycle state.
    window.orderOut(nil)
}

@Test func paneLessCheckoutSelectionRequestsAnAutomaticTerminal() {
    let checkout = CoreCheckoutSnapshot(
        id: "checkout-empty",
        workspaceID: "workspace",
        label: "feature/empty",
        path: "/tmp/hide-empty-checkout",
        branch: "feature/empty",
        isWorktree: true,
        exists: true,
        temporary: false,
        tabs: []
    )

    #expect(CheckoutSelectionPolicy.action(for: checkout) == .startTerminal)
}

@Test func checkoutWithAExistingPaneOnlyChangesFocus() {
    let pane = CorePaneSnapshot(
        id: "pane-existing",
        herdrLabel: "terminal",
        cwd: "/tmp/hide-existing-checkout",
        statusLabel: "Attached",
        summary: nil,
        activityAt: nil
    )
    let checkout = CoreCheckoutSnapshot(
        id: "checkout-existing",
        workspaceID: "workspace",
        label: "main",
        path: "/tmp/hide-existing-checkout",
        branch: "main",
        isWorktree: false,
        exists: true,
        temporary: false,
        tabs: [CoreTabSnapshot(
            id: "tab-existing",
            workspaceID: "workspace",
            checkoutID: "checkout-existing",
            label: "1",
            empty: false,
            panes: [pane]
        )]
    )

    #expect(CheckoutSelectionPolicy.action(for: checkout) == .focusExisting)
}

@Test func connectedHerdrStatusDoesNotLookLikeItIsStillWaiting() {
    #expect(HerdrStatusPresentation.localMessage(
        startupDiagnostic: nil,
        bridgeError: nil,
        state: "connected",
        providerMessage: nil
    ) == "Connected to Herdr")
    #expect(HerdrStatusPresentation.localMessage(
        startupDiagnostic: nil,
        bridgeError: nil,
        state: "not_connected",
        providerMessage: nil
    ) == "Waiting for Herdr")
    #expect(HerdrStatusPresentation.localMessage(
        startupDiagnostic: nil,
        bridgeError: "Bridge failed",
        state: "connected",
        providerMessage: "Provider failed"
    ) == "Bridge failed")
    #expect(HerdrStatusPresentation.localMessage(
        startupDiagnostic: nil,
        bridgeError: nil,
        state: "stale",
        providerMessage: "Provider failed"
    ) == "Provider failed")
}

/// A pane action that fails while Herdr is disconnected is a symptom; the
/// status bar keeps naming the connection failure, which is what to fix.
/// A launch that cannot proceed at all still comes first.
@Test func aConnectionFailureOutranksTheLastActionErrorUntilHerdrConnects() {
    #expect(HerdrStatusPresentation.localMessage(
        startupDiagnostic: nil,
        bridgeError: "pane.focus_failed: pane w1:p1 not found",
        state: "protocol_mismatch",
        providerMessage: "The running Herdr speaks protocol 20; this hide needs protocol 21."
    ) == "The running Herdr speaks protocol 20; this hide needs protocol 21.")
    #expect(HerdrStatusPresentation.localMessage(
        startupDiagnostic: HideStartupDiagnostic.runtimeUnavailable,
        bridgeError: "pane.focus_failed: pane w1:p1 not found",
        state: "socket_missing",
        providerMessage: "Herdr socket is missing"
    ) == HideStartupDiagnostic.runtimeUnavailable)
}

@Test func coreRemoteSessionCarriesContextAndPaneCwd() throws {
    let data = Data("""
    {
      "workspaces": [{
        "id": "remote:device:mini:workspace:w1",
        "label": "remote-repo",
        "path": "/private/tmp/hide-remote-repo",
        "remote_target_id": "device:mini",
        "device_id": "device:mini",
        "checkouts": [{
          "id": "remote:device:mini:checkout:w1",
          "workspace_id": "remote:device:mini:workspace:w1",
          "label": "remote-repo",
          "path": "/private/tmp/hide-remote-repo",
          "is_worktree": false,
          "exists": true,
          "temporary": false,
          "strip": [{
            "id": "herdr:w1:t1",
            "kind": "herdr",
            "source_id": "w1:t1",
            "label": "Tab 1"
          }],
          "next_tab_label": "Tab 2",
          "tabs": [{
            "id": "w1:t1",
            "workspace_id": "remote:device:mini:workspace:w1",
            "checkout_id": "remote:device:mini:checkout:w1",
            "label": "1",
            "empty": false,
            "panes": [{
              "id": "w1:p1",
              "label": "remote-repo",
              "cwd": "/private/tmp/hide-remote-repo",
              "status_label": "Attached", "requires_close_confirmation": false
            }]
          }]
        }]
      }],
      "agents": [],
      "active_tab_ids": {"remote:device:mini:workspace:w1": "w1:t1"},
      "focused_workspace_id": "remote:device:mini:workspace:w1",
      "focused_checkout_id": "remote:device:mini:checkout:w1",
      "focused_tab_id": "w1:t1",
      "focused_pane_id": "w1:p1",
      "pane_layouts": []
    }
    """.utf8)

    let session = try JSONDecoder().decode(CoreRemoteSessionSnapshot.self, from: data)
    let projection = RemoteNavigationSnapshot(
        deviceID: "device:mini",
        targetLabel: "Mac mini",
        core: session
    )
    let workspace = try #require(projection.workspaces.first)
    let checkout = try #require(workspace.checkouts.first)
    let tab = try #require(checkout.tabs.first)
    let pane = try #require(tab.panes.first)

    #expect(projection.focusedWorkspaceID == workspace.id)
    #expect(projection.focusedCheckoutID == checkout.id)
    #expect(projection.focusedTabID == tab.id)
    #expect(projection.focusedPaneID == pane.id)
    #expect(checkout.path == "/private/tmp/hide-remote-repo")
    #expect(pane.cwd == checkout.path)

    let selected = projection.focused(workspaceID: workspace.id, checkoutID: checkout.id)
    #expect(selected.focusedTabID == tab.id)
    #expect(selected.focusedPaneID == pane.id)
}

@Test func remoteStatusDecodesCoreOwnedSFTPFileState() throws {
    let data = Data("""
    {
      "target_id": "mini",
      "state": "connected",
      "message": null,
      "session": null,
      "files": {
        "root_path": "/private/tmp/project",
        "state": "ready",
        "entries": [{
          "path": "/private/tmp/project/Sources",
          "name": "Sources",
          "is_directory": true,
          "size_bytes": 96
        }],
        "message": null,
        "generation": 7
      }
    }
    """.utf8)

    let status = try JSONDecoder().decode(CoreRemoteStatus.self, from: data)
    let entry = try #require(status.files.entries.first)
    #expect(status.files.rootPath == "/private/tmp/project")
    #expect(status.files.state == "ready")
    #expect(status.files.generation == 7)
    #expect(entry.name == "Sources")
    #expect(entry.isDirectory)
    #expect(entry.sizeBytes == 96)
}

@Test func offscreenPetOriginClampsIntoPrimaryVisibleFrame() {
    let visible = CGRect(x: 0, y: 25, width: 1_440, height: 875)
    let resolved = PetPlacement.clampedOrigin(
        requested: CGPoint(x: 1_000_000, y: 1_000_000),
        windowSize: CGSize(width: 92, height: 92),
        visibleFrames: [visible]
    )
    #expect(resolved == CGPoint(x: 1_348, y: 808))
}

@Test func petPlacementIsStableWhenRepeated() {
    let visible = CGRect(x: -1_920, y: 0, width: 1_920, height: 1_080)
    let first = PetPlacement.clampedOrigin(
        requested: CGPoint(x: -200, y: 100),
        windowSize: CGSize(width: 92, height: 92),
        visibleFrames: [visible]
    )
    let second = PetPlacement.clampedOrigin(
        requested: first,
        windowSize: CGSize(width: 92, height: 92),
        visibleFrames: [visible]
    )
    #expect(first == second)
}

@Test func petHitRegionKeepsCenterInteractive() {
    let frame = CGRect(x: 100, y: 200, width: 92, height: 92)

    #expect(!PetHitRegion.ignoresMouseEvents(
        screenPoint: CGPoint(x: frame.midX, y: frame.midY),
        windowFrame: frame
    ))
}

@Test func petHitRegionKeepsCircularEdgeInteractive() {
    let frame = CGRect(x: 100, y: 200, width: 92, height: 92)
    let edge = CGPoint(x: frame.maxX - PetHitRegion.contentInset, y: frame.midY)

    #expect(!PetHitRegion.ignoresMouseEvents(screenPoint: edge, windowFrame: frame))
}

@Test func petHitRegionMakesTransparentCornerClickThrough() {
    let frame = CGRect(x: 100, y: 200, width: 92, height: 92)
    let transparentCorner = CGPoint(x: frame.maxX - 1, y: frame.maxY - 1)

    #expect(PetHitRegion.ignoresMouseEvents(screenPoint: transparentCorner, windowFrame: frame))
}

@Test func petHitRegionDecisionIsStableWhenRepeated() {
    let frame = CGRect(x: 100, y: 200, width: 92, height: 92)
    let point = CGPoint(x: frame.minX + 1, y: frame.minY + 1)
    let first = PetHitRegion.ignoresMouseEvents(screenPoint: point, windowFrame: frame)
    let second = PetHitRegion.ignoresMouseEvents(screenPoint: point, windowFrame: frame)

    #expect(first)
    #expect(second == first)
}

@Test func workingPaneNoticeNamesTheTerminationConsequence() {
    let target = DestructiveTarget(
        id: "herdr-ide-verify-working",
        label: "Verification",
        statusLabel: "Working",
        requiresCloseConfirmation: true,
        summary: "Long-running fixture"
    )
    let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
    #expect(notice.requiresConfirmation)
    #expect(notice.consequence.contains("terminates its running process"))
    #expect(notice.affected == [target])
}

@Test func idlePaneCloseDoesNotAddAnUnnecessaryConfirmation() {
    let target = DestructiveTarget(
        id: "herdr-ide-verify-idle",
        label: "Verification",
        statusLabel: "Idle",
        requiresCloseConfirmation: false,
        summary: "Completed fixture"
    )
    let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
    #expect(!notice.requiresConfirmation)
    #expect(notice.affected == [target])
}

@Test func everyPaneTheCoreMarksRiskyRequiresCloseConfirmation() {
    for label in ["Approval", "Question", "Error", "Done", "Working"] {
        let target = DestructiveTarget(
            id: "herdr-ide-verify-\(label)",
            label: "Verification",
            statusLabel: label,
            requiresCloseConfirmation: true,
            summary: "Attention fixture"
        )
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        #expect(notice.requiresConfirmation)
        #expect(notice.affected == [target])
    }
}

@Test func workspaceAndTabWarningsAggregateOnlyActiveOrAttentionPanes() {
    let targets = [
        DestructiveTarget(
            id: "w", label: "A", statusLabel: "Working",
            requiresCloseConfirmation: true, summary: "Build"
        ),
        DestructiveTarget(
            id: "q", label: "B", statusLabel: "Question",
            requiresCloseConfirmation: true, summary: "Needs input"
        ),
        DestructiveTarget(
            id: "i", label: "C", statusLabel: "Idle",
            requiresCloseConfirmation: false, summary: "Done"
        ),
    ]
    let workspace = ConsequencePolicy.notice(kind: .workspace, targets: targets)
    let tab = ConsequencePolicy.notice(kind: .tab, targets: targets)
    #expect(workspace.affected.map(\.id) == ["w", "q"])
    #expect(tab.affected.map(\.id) == ["w", "q"])
}

@Test func worktreeNoticeStatesTheCheckoutLossBoundary() {
    let notice = ConsequencePolicy.notice(kind: .worktree, targets: [])
    #expect(!notice.requiresConfirmation)
    #expect(notice.consequence.contains("registration"))
    #expect(notice.consequence.contains("disk"))
}

@Test func cdpTabTitleDecodesCharacterReferencesAfterStructuredJSONParsing() throws {
    let payload = Data(
        #"[{"url":"chrome://profile-picker/","title":"Who&#39;s using Chrome?","type":"page"}]"#.utf8
    )

    let tabs = try ChromuxTabDecoder.decode(payload)

    #expect(tabs.count == 1)
    #expect(tabs[0].url == "chrome://profile-picker/")
    #expect(tabs[0].title == "Who's using Chrome?")
    #expect(tabs[0].type == "page")
}

@Test func chromuxPathAbsenceStopsBeforeAnyProcessLaunch() async {
    let receipt = await ChromuxExecutor.inspectAndOpen(
        profile: "default",
        shouldOpen: true,
        pathState: "absent"
    )

    #expect(receipt.phase == .unavailable)
    #expect(receipt.action == "unavailable")
    #expect(receipt.message.contains("PATH"))
}

/// The app runs only the Herdr it ships, verified against the pinned digest.
/// A bundle whose binary is absent or altered yields no runtime rather than
/// a runtime that happens to be on the machine.
@Test func theResolverStartsOnlyABundledBinaryCarryingThePinnedDigest() throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("hide-resolver-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let binary = directory.appendingPathComponent("herdr")
    try Data("#!/bin/sh\nexit 0\n".utf8).write(to: binary)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
    // shasum -a 256 of the script above.
    let digest = "1d2f2b1a4b1e0b6a0a4a3d3a0f9a5e9e3a7f1b5a0c2d8e6f4a1b3c5d7e9f0a1b"
    let actual = try #require(
        String(decoding: try Process.output("/usr/bin/shasum", ["-a", "256", binary.path]), as: UTF8.self)
            .split(separator: " ").first
    )

    let matching = HerdrRuntimePin(version: "0.0.0-test", sha256: String(actual))
    #expect(
        HerdrRuntimeResolver.resolve(bundlePath: binary.path, pin: matching)
            == HerdrRuntimeSelection(path: binary.path, version: "0.0.0-test", sha256: String(actual))
    )

    let other = HerdrRuntimePin(version: "0.0.0-test", sha256: digest)
    #expect(HerdrRuntimeResolver.resolve(bundlePath: binary.path, pin: other) == nil)
    #expect(HerdrRuntimeResolver.resolve(bundlePath: nil, pin: matching) == nil)
    #expect(HerdrRuntimeResolver.resolve(bundlePath: directory.path + "/absent", pin: matching) == nil)
    #expect(HerdrRuntimeResolver.resolve(bundlePath: binary.path, pin: nil) == nil)
}

/// The shell, the core and every child herdr read one socket path by one
/// rule: an absolute HERDR_SOCKET_PATH wins, anything else is the default.
@Test func theSocketPathFollowsTheCoreOverrideRule() {
    #expect(
        HideRuntimeEnvironment.herdrSocketPath(environment: [:], homeDirectory: "/Users/t")
            == "/Users/t/.config/herdr/herdr.sock"
    )
    #expect(
        HideRuntimeEnvironment.herdrSocketPath(
            environment: ["HERDR_SOCKET_PATH": "/private/tmp/hide-e2e/herdr.sock"],
            homeDirectory: "/Users/t"
        ) == "/private/tmp/hide-e2e/herdr.sock"
    )
    #expect(
        HideRuntimeEnvironment.herdrSocketPath(
            environment: ["HERDR_SOCKET_PATH": "relative/herdr.sock"],
            homeDirectory: "/Users/t"
        ) == "/Users/t/.config/herdr/herdr.sock"
    )
    #expect(
        HideRuntimeEnvironment.herdrSocketPath(
            environment: ["HERDR_SOCKET_PATH": ""],
            homeDirectory: "/Users/t"
        ) == "/Users/t/.config/herdr/herdr.sock"
    )
}

private extension Process {
    static func output(_ executable: String, _ arguments: [String]) throws -> Data {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardOutput = pipe
        try process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return data
    }
}
