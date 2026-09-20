import Foundation
import Testing

@testable import HerdrMacOS

private func navigationEditorItem(kind: CoreEditorTabKind) -> ShellTabItem {
    ShellTabItem(
        id: "file:tab-1",
        label: "Review",
        dirty: kind == .file,
        active: true,
        kind: .editor(
            CoreEditorTabSnapshot(
                id: "file-1",
                workspaceID: "workspace-1",
                checkoutID: "checkout-1",
                path: "/tmp/repo/Review.swift",
                label: "Review.swift",
                kind: kind,
                diffCommitted: nil,
                dirty: kind == .file
            )
        )
    )
}

@Test func recentSurfaceNamesProjectAndCheckoutSeparately() {
    let surface = RecentSurface(
        id: "surface-1",
        projectID: "project-1",
        projectLabel: "Hide",
        deviceID: "local",
        workspaceID: "workspace-1",
        checkoutID: "checkout-1",
        checkoutLabel: "feature/navigation",
        item: navigationEditorItem(kind: .file),
        location: RecentLocation(deviceID: "local", label: "This Mac")
    )

    #expect(surface.contextLabel == "Hide · feature/navigation")
}

@Test func recentSurfaceOmitsDuplicateCheckoutContextAndUsesDiffSymbol() {
    let surface = RecentSurface(
        id: "surface-1",
        projectID: "project-1",
        projectLabel: "Hide",
        deviceID: "local",
        workspaceID: "workspace-1",
        checkoutID: "checkout-1",
        checkoutLabel: "Hide",
        item: navigationEditorItem(kind: .diff),
        location: RecentLocation(deviceID: "local", label: "This Mac")
    )

    #expect(surface.contextLabel == "Hide")
    #expect(surface.symbol == "doc.text.magnifyingglass")
}

@Test func recentLocationDistinguishesHostsWithoutChangingProjectContext() {
    let labels = ["local": "This Mac", "mini": "Mac mini", "work": "작업용 원격 Mac"]
    let local = RecentLocation.resolve(deviceID: "local", labels: labels)
    let mini = RecentLocation.resolve(deviceID: "mini", labels: labels)
    #expect(!local.isRemote)
    #expect(local.spoken == "This Mac")
    #expect(mini.isRemote)
    #expect(mini.spoken == "Remote, Mac mini")
    #expect(RecentLocation.resolve(deviceID: "work", labels: labels).label == "작업용 원격 Mac")
    #expect(RecentLocation.resolve(deviceID: "removed-host", labels: labels).spoken == "Remote, removed-host")
    #expect(RecentLocation.resolve(deviceID: "mini", labels: ["mini": ""]).label == "mini")
}
