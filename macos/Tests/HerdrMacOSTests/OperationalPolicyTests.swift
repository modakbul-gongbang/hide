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
    let allowedKeys = Set(["HOME", "USER", "PATH", "SSH_AUTH_SOCK", "HERDR_CONFIG_PATH"])

    #expect(Set(HideRuntimeEnvironment.childEnvironment().keys).isSubset(of: allowedKeys))
}

@Test func finderLikeEnvironmentUsesAVisibleSafePATHFallback() {
    let environment = HideRuntimeEnvironment.childEnvironment(
        inherited: ["HOME": "/tmp/hide-finder", "USER": "tester"],
        loginPath: nil
    )

    #expect(environment["PATH"] == "/usr/bin:/bin")
    #expect(Set(environment.keys).isSubset(of: ["HOME", "USER", "PATH", "SSH_AUTH_SOCK", "HERDR_CONFIG_PATH"]))
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
