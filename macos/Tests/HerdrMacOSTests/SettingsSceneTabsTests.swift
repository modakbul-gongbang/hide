import AppKit
import Foundation
import SwiftUI
import Testing

@testable import HerdrMacOS

/// The menu bar's Settings scene is a second window with no ancestor of its
/// own: a tab, or a control a tab shares with the main window, that reads the
/// model from an environment the scene does not provide traps the first time
/// it is shown. Every tab is rendered here exactly as that scene renders it.
@Test @MainActor func everySettingsTabRendersFromTheSettingsScene() throws {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("settings-scene-\(UUID().uuidString)", isDirectory: true)
    let stateURL = root.appendingPathComponent("state.json")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }

    let bridge = CoreBridge(arguments: [
        "HerdrMacOS",
        "--verification-ui-fixture",
        "--workspace-root", root.path,
        "--state-path", stateURL.path,
    ])
    let model = ShellModel(core: bridge)

    for tab in HideSettingsTab.allCases {
        let host = NSHostingView(rootView: HideSettingsScene(model: model, initialTab: tab))
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: HideTheme.settingsSheetSize),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        // AppKit releases a closed window on its own; ARC must not release it
        // a second time when this scope ends.
        window.isReleasedWhenClosed = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        host.displayIfNeeded()
        #expect(host.fittingSize.height > 0, "\(tab) rendered nothing")
        window.close()
    }
}
