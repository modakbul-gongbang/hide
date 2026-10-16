// Reading and driving a native browser page from the test, the way hardware and the page itself would:
// the page is found by the address it shows, input is sent as trusted events, and an expression runs in
// its own world. Shared by the specs that look at what is drawn in a page.
import type { ElectronApplication } from "@playwright/test";

/** The addresses the window's native pages show now. */
export async function pageUrls(app: ElectronApplication): Promise<string[]> {
  return app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.contentView.children
    .flatMap((view) => {
      const contents = (view as { webContents?: Electron.WebContents }).webContents;
      return contents && !contents.isDestroyed() ? [contents.getURL()] : [];
    }));
}

/** Sends the page shown at `url` input the way a person's hardware would: the page reads it as trusted. */
export async function sendInput(app: ElectronApplication, url: string, events: Electron.MouseInputEvent[] | Electron.KeyboardInputEvent[]): Promise<void> {
  await app.evaluate(({ BrowserWindow }, { url, events }) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children
      .map((view) => (view as { webContents?: Electron.WebContents }).webContents)
      .find((page) => page && !page.isDestroyed() && page.getURL() === url);
    if (!contents) throw new Error(`no page shows ${url}`);
    for (const event of events) contents.sendInputEvent(event);
  }, { url, events });
}

/** Evaluates `expression` in the main world of the page shown at `url`. */
export async function evaluateIn<T>(app: ElectronApplication, url: string, expression: string): Promise<T> {
  return app.evaluate(({ BrowserWindow }, { url, expression }) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children
      .map((view) => (view as { webContents?: Electron.WebContents }).webContents)
      .find((page) => page && !page.isDestroyed() && page.getURL() === url);
    if (!contents) throw new Error(`no page shows ${url}`);
    return contents.executeJavaScript(expression) as Promise<T>;
  }, { url, expression });
}
