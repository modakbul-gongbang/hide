import Foundation
import Testing

@testable import HerdrMacOS

/// The Settings sheet sizes itself to the window it is presented over: never
/// smaller than it used to be, never taller than the cap, and with the inset
/// kept clear above and below. The Settings scene passes no window and keeps
/// the old size.

@Test @MainActor func theSettingsSheetTakesWhatAnOrdinaryWindowOffers() {
    // A 900-point window, the size of the default main window: the inset
    // comes off both ends and the rest is the sheet.
    #expect(HideSettingsView.sheetHeight(availableHeight: 860) == 780)
    // A taller window is capped, so the sheet does not become a second window.
    #expect(HideSettingsView.sheetHeight(availableHeight: 1400) == HideTheme.settingsSheetMaxHeight)
}

@Test @MainActor func theSettingsSheetNeverShrinksBelowItsOldSize() {
    #expect(HideSettingsView.sheetHeight(availableHeight: 600) == HideTheme.settingsSheetSize.height)
    #expect(HideSettingsView.sheetHeight(availableHeight: nil) == HideTheme.settingsSheetSize.height)
}
