/** App-menu command ids, main -> renderer (B11). */
export const COMMAND_CHANNEL = "hide:command";

/** The stored macOS pane chords the menu is built from, renderer -> main. */
export const BINDINGS_CHANNEL = "hide:bindings";

/** A folder the shell asks Finder to show (a sidebar row's Reveal in Finder), renderer -> main. */
export const REVEAL_CHANNEL = "hide:reveal";

/**
 * Terminal links (docs/ARCHITECTURE.md, A clicked path is one event): which
 * paths a link names exist on this Mac and what they are (invoke), and a
 * path handed to macOS, opened in its default application or revealed in
 * Finder when opening would run it (renderer -> main).
 */
export const PROBE_PATHS_CHANNEL = "hide:probe-paths";
export const OPEN_PATH_CHANNEL = "hide:open-path";

/** Add a project's Browse folder: the native folder picker, answered with the chosen folder or null (invoke). */
export const PICK_FOLDER_CHANNEL = "hide:pick-folder";

/**
 * Browser displays (issue 155): the shell reports where each browser display
 * of the front Workspace sits (renderer -> main), asks for a still of one
 * (invoke), sends toolbar commands, and hears each page's state back
 * (main -> renderer). Nothing else crosses between the shell and a page.
 */
export const BROWSER_SYNC_CHANNEL = "hide:browser-sync";
export const BROWSER_CAPTURE_CHANNEL = "hide:browser-capture";
export const BROWSER_COMMAND_CHANNEL = "hide:browser-command";
export const BROWSER_EVENT_CHANNEL = "hide:browser-event";
export const BROWSER_CYCLE_END_CHANNEL = "hide:browser-cycle-end";
