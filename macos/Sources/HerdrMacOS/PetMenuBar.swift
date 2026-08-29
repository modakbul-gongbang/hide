import AppKit

/// The pet's menu bar item.
///
/// Two entries only: the shared visibility toggle and a way into its
/// settings. There is deliberately no Quit - in the retired pet app that
/// closed the pet, but here it would close the whole IDE (D-23).
@MainActor
final class PetMenuBarController: NSObject, NSMenuDelegate {
    private let statusItem: NSStatusItem
    private let model: ShellModel
    private let openSettings: () -> Void
    private let toggleItem = NSMenuItem(
        title: "Hide Pet",
        action: #selector(togglePet),
        keyEquivalent: ""
    )

    init(model: ShellModel, openSettings: @escaping () -> Void) {
        self.model = model
        self.openSettings = openSettings
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        super.init()

        // A template image so the icon follows the menu bar's own appearance
        // in light and dark.
        let icon = NSImage(
            systemSymbolName: "pawprint.fill",
            accessibilityDescription: "Herdr Pet"
        )
        icon?.isTemplate = true
        statusItem.button?.image = icon
        statusItem.button?.setAccessibilityIdentifier("pet-menu-bar-item")

        let menu = NSMenu()
        menu.delegate = self
        toggleItem.target = self
        menu.addItem(toggleItem)
        menu.addItem(.separator())
        let settingsItem = NSMenuItem(
            title: "Pet Settings…",
            action: #selector(showSettings),
            keyEquivalent: ""
        )
        settingsItem.target = self
        menu.addItem(settingsItem)
        statusItem.menu = menu
        refresh()
    }

    /// Keeps the menu's wording on the one shared visibility state, so the
    /// menu bar never disagrees with the Settings toggle.
    func refresh() {
        let visible = model.core.pet?.visible ?? true
        toggleItem.title = visible ? "Hide Pet" : "Show Pet"
        statusItem.button?.appearsDisabled = !visible
        statusItem.button?.toolTip = visible ? "Herdr Pet is showing" : "Herdr Pet is hidden"
    }

    func menuWillOpen(_ menu: NSMenu) {
        refresh()
    }

    @objc private func togglePet() {
        model.core.togglePetVisible()
        refresh()
    }

    @objc private func showSettings() {
        openSettings()
    }
}
