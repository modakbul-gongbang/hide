// Command-line switches the desktop host reads besides Chromium's own. Only a
// test harness passes one: an app opened from Finder, the Dock or `open` gets
// none, so the operator's launch never changes. No Electron import here, so the
// e2e fixture can name the same switch.

/**
 * Windows appear without activating the app (issue 232): each window is shown
 * with `showInactive()` and then sent behind every other window, and a second
 * launch or a Dock click shows it again the same way instead of focusing it,
 * so a desktop e2e run leaves the operator's screen, frontmost app and
 * keyboard alone.
 */
export const SHOW_INACTIVE_SWITCH = "hide-show-inactive";
