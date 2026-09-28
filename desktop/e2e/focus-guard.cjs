// Preloaded into every e2e app's main process (`-r`, before the app's own
// code), so it hears the app come forward even for the first window. Each
// time the app becomes active or a window takes the keyboard, one line goes to
// stderr; `fixture.ts` reads it and fails a test not tagged @needs-focus
// (issue 232).

const { app } = require("electron");

const report = (event) => process.stderr.write(`hide-e2e-focus ${JSON.stringify({ event })}\n`);
app.on("did-become-active", () => report("did-become-active"));
app.on("browser-window-focus", () => report("browser-window-focus"));
