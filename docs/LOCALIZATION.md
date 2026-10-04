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

The first integration covers Settings navigation and General labels, the language selector, sidebar header navigation, and Home navigation, project facts and project-list empty states.
Shared Overview page and modal navigation, its toolbar tooltip and attention count, the sidebar tabs and Overview row, and the three Overview navigation command labels in Settings and the shortcut sheet also follow the confirmed language.
Shortcut keycaps retain their host-specific chord glyphs in every language.
The catalogs also contain translations for the remaining surfaces, but their presence does not mean those surfaces render translated text.
Remaining #339 work includes other Settings bodies and their presentation helpers, sidebar rows and menus, boards and requests, Workspace and pane chrome, editor and file views, dialogs and errors, phone screens and push notifications, and native host menus and connection screens.
Native or closed-window consumers may retain only the last core-confirmed explicit choice; they must resolve an unset choice on their own host.
The issue remains open until these integrations and four-language native evidence are complete.

## Verification owners

`runtime::tests::appearance` checks all four choices, reset, invalid events, restart, stale UI saves, and preservation of invalid stored values.
`web/e2e/interface-language.spec.ts` drives the selector against a private daemon and Herdr, with separate Korean and Japanese browser contexts.
It checks shared choices, four-language headings, refresh, daemon restart, system reset, unsupported-system fallback and invalid-store diagnostics.
It also checks Overview scope labels, sidebar tabs, navigation command labels and unchanged shortcut keycaps in all four languages.
Catalog, translator and locale unit tests remain beside their modules.
Run artifacts stay under `agents/runs/`; browser checks do not establish native menu or phone behavior.
