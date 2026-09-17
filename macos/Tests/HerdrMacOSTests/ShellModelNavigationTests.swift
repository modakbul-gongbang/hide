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
        item: navigationEditorItem(kind: .file)
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
        item: navigationEditorItem(kind: .diff)
    )

    #expect(surface.contextLabel == "Hide")
    #expect(surface.symbol == "doc.text.magnifyingglass")
}
