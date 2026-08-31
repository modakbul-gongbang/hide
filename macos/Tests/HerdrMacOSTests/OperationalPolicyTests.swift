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
            source: "test",
            version: "0.8.2",
            sha256: nil,
            guidance: nil
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

@Test @MainActor func coreBridgeHasAnImmediateSnapshotBeforeRuntimeResolution() {
    let bridge = CoreBridge(arguments: [
        "HerdrMacOS",
        "--state-path",
        "/tmp/hide-p0-startup-test-state.json",
    ])

    #expect(bridge.snapshot != nil)
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
        label: "terminal",
        cwd: "/tmp/hide-existing-checkout",
        state: "attached",
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

@Test func terminalLayoutMustBelongToTheFocusedCheckoutBeforeRendering() {
    let checkout = CoreCheckoutSnapshot(
        id: "checkout-b",
        workspaceID: "workspace-b",
        label: "main",
        path: "/tmp/hide-workspace-b",
        branch: "main",
        isWorktree: false,
        exists: true,
        temporary: false,
        tabs: [CoreTabSnapshot(
            id: "tab-b",
            workspaceID: "workspace-b",
            checkoutID: "checkout-b",
            label: "1",
            empty: false,
            panes: [CorePaneSnapshot(
                id: "pane-b",
                label: "pane-b",
                cwd: "/tmp/hide-workspace-b",
                state: "attached",
                summary: nil,
                activityAt: nil
            )]
        )]
    )

    #expect(TerminalLayoutPolicy.belongs(
        workspaceID: "herdr-live-workspace-b",
        tabID: "tab-b",
        paneIDs: ["pane-b"],
        checkout: checkout
    ))
    #expect(!TerminalLayoutPolicy.belongs(
        workspaceID: "workspace-a",
        tabID: "tab-a",
        paneIDs: ["pane-a"],
        checkout: checkout
    ))
}

@Test func remoteSnapshotProjectionCarriesContextAndPaneCwd() throws {
    let data = Data("""
    {
      "result": {
        "snapshot": {
          "focused_workspace_id": "w1",
          "focused_tab_id": "w1:t1",
          "focused_pane_id": "w1:p1",
          "workspaces": [{
            "workspace_id": "w1",
            "label": "remote-repo",
            "active_tab_id": "w1:t1",
            "pane_count": 1,
            "tab_count": 1
          }],
          "tabs": [{
            "tab_id": "w1:t1",
            "workspace_id": "w1",
            "label": "1",
            "pane_count": 1
          }],
          "panes": [{
            "pane_id": "w1:p1",
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "cwd": "/private/tmp/hide-remote-repo",
            "terminal_title": "remote-repo",
            "terminal_title_stripped": "remote-repo"
          }],
          "agents": []
        }
      }
    }
    """.utf8)

    let projection = try RemoteSnapshotProjection.decode(
        data,
        deviceID: "device:mini",
        targetLabel: "Mac mini"
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

@Test func remoteShellCommandQuotesPathsWithoutHandlingCredentials() {
    #expect(RemoteShellCommand.quote("/tmp/remote checkout") == "'/tmp/remote checkout'")
    #expect(RemoteShellCommand.quote("it's safe") == "'it'\\''s safe'")
    #expect(
        RemoteShellCommand.loginShell("herdr api snapshot")
            == "zsh -ilc 'herdr api snapshot'"
    )
    let attach = RemoteShellCommand.attach(paneID: "w1:p2")
    #expect(attach.contains("herdr pane attach 'w1:p2' 2>/dev/null"))
    #expect(attach.contains("remote terminal initialization failed on mini"))
    #expect(!attach.contains("panic"))
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
        state: "working",
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
        state: "idle",
        summary: "Completed fixture"
    )
    let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
    #expect(!notice.requiresConfirmation)
    #expect(notice.affected == [target])
}

@Test func everyAttentionPaneStateRequiresCloseConfirmation() {
    for state in ["question", "approval", "error", "unseen_completion"] {
        let target = DestructiveTarget(
            id: "herdr-ide-verify-\(state)",
            label: "Verification",
            state: state,
            summary: "Attention fixture"
        )
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        #expect(notice.requiresConfirmation)
        #expect(notice.affected == [target])
    }
}

@Test func workspaceAndTabWarningsAggregateOnlyActiveOrAttentionPanes() {
    let targets = [
        DestructiveTarget(id: "w", label: "A", state: "working", summary: "Build"),
        DestructiveTarget(id: "q", label: "B", state: "question", summary: "Needs input"),
        DestructiveTarget(id: "i", label: "C", state: "idle", summary: "Done"),
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
