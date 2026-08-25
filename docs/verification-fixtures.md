# Verification Fixtures

The shared Rust tests use serialized herdr snapshot-shaped JSON fixtures in memory.

They cover token-based question detection, status parsing, duplicate event suppression, attention-first aggregation, injected-clock escalation, DND suppression, theme state validation, and the pre-send response guard.

No automated check sends keys to a real user agent pane.

Browser verification uses deterministic fixture data rendered by `web/app.js` so the question, error, working, disconnected, grouping, filter, DND, command-palette, and guard-note states are observable without external credentials.

