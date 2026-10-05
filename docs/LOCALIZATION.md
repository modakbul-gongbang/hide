# Interface localization

Issue #339 defines the interface language policy.
The core owns one explicit choice, shared by clients of that daemon.
`ui_state.interface_language` is `en`, `ko`, `zh-CN`, `ja`, or null.
Null and an absent field mean each client uses its primary OS or browser language.
Supported regional English, Korean and Japanese locales resolve to their base language.
Chinese locales that resolve to the Hans script use Simplified Chinese (`zh-CN`); other Chinese scripts and unsupported languages use English.
The client does not search a secondary preferred-language list or persist a resolved system language.

## Ownership and persistence

Only `interface_language_set {language}` edits the choice; null explicitly returns to system language.
The parameter is required, and unsupported event values are refused without changing the choice.
A stale `ui_state_update` cannot change the language.
Changes use the existing coalesced UI-state save and snapshot publication, with no new timer, worker or storage file.
Repeating the current choice publishes no change.
An invalid stored value produces `ui_state.interface_language_invalid`, publishes English and remains in the store through unrelated saves.
The diagnostic never includes the invalid value.
Old state files need no migration.

## Translation boundary

`web/src/i18n/locale.ts` owns client locale resolution.
`instance.ts` creates an independent translator from the embedded four-language catalogs, validating keys and interpolation placeholders before initialization.
`client.tsx` owns one in-memory translator per shell page and follows confirmed snapshots and browser `languagechange` events.
It sets the document language and subscribes React surfaces through react-i18next.
Language selection is disabled while disconnected, and displays only the confirmed core value while connected.
A client never translates conversations, terminal bytes, file contents, paths, branch names, device labels, task labels or other user-owned text.
Such values are interpolation data, rendered as text.
Diagnostic payloads and copied diagnostic records remain technical source data.
Missing keys and missing interpolation values fail explicitly; English is a locale-resolution fallback, never a missing-key fallback.

## Current integration coverage

The web shell renders every product-owned sentence of its main screens from the catalogs.
That covers Settings (every tab), the sidebar rows, menus and close sheets, the shared Overview and its request view, Agents graph, Issues board and PRs view, the Workspace toolbar, tab bars and pane chrome, the editor and file views, the Explorer and History, browser displays, the palettes, the start panel, the add-project and workspace dialogs, Project Sessions, disk cleanup and the device rail.
Shortcut keycaps retain their host-specific chord glyphs in every language.
Text that a helper composes outside a render uses the `translate` function of `client.tsx`, which reads the confirmed language at call time; a helper that renders inside React takes the component's `t`.
A module the desktop host imports (`host.ts` and everything it reaches) stays free of the translator and names a label by key, as `revealExternal.ts` does.
Where `UI_BEHAVIOR.md` quotes shipped Korean wording (`요청`, `정리`, `맡기기`), that is the Korean catalog text; English and the other languages read their own catalog entries.
Sentences the core sends as data (a `reason`, `message` or `status_label` in a snapshot) are not translated by the client; they remain a boundary until the core sends codes the catalogs can name.
The desktop host draws its native menu, its role items (Undo, Quit, Services and the rest, labeled explicitly because the system's own follow the OS language), the connection screen, the folder picker and the browser display dialogs from the same catalogs (`native.*`, `commands.*`).
It keeps only the last core-confirmed explicit choice in its profile (`interface-language.json`) and resolves an unset choice on its own host, as this document requires of any native or closed-window consumer; `ARCHITECTURE.md`, The desktop host, owns the details.
Remaining #339 work: phone screens and push notifications.
The issue stays open until those integrations and four-language native evidence are complete.

## Verification owners

`runtime::tests::appearance` checks all four choices, reset, invalid events, restart, stale UI saves, and preservation of invalid stored values.
`web/e2e/interface-language.spec.ts` drives the selector against a private daemon and Herdr, with separate Korean and Japanese browser contexts.
It checks shared choices, four-language headings, refresh, daemon restart, system reset, unsupported-system fallback and invalid-store diagnostics.
It also checks Overview scope labels, sidebar tabs, navigation command labels and unchanged shortcut keycaps in all four languages.
Catalog, translator and locale unit tests remain beside their modules.
`desktop/src/main/language.test.ts` and `menu.test.ts` hold the host's resolution, persistence and four-language menu labels, and `desktop/e2e/language.spec.ts` drives the real app: stored choices in all four languages before any daemon answers, the system fallback, and a choice made in Settings relabeling the menu, persisting and returning to the system language.
Run artifacts stay under `agents/runs/`; browser checks do not establish native menu or phone behavior.
