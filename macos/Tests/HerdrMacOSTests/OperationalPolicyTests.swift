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
