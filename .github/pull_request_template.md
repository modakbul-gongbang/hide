<!--
Answer every section. A section that does not apply gets "N/A" and one reason.
Tests and screenshots settle what they settle; the questions below are the
parts a reviewer has to judge. CONTRIBUTING.md lists the gates and how to run
them locally.
-->

## What changed, in one paragraph a new contributor can follow

<!-- Plain words. What a user or a maintainer will notice, and why it was worth doing. -->

## Review boundary

<!-- Each answer is one line. These are the places this codebase has been bitten. -->

- **Runtime mutex**: what does this change do while holding it, if anything?
- **Snapshot wire**: does it add or move a field? Which channel (revisioned `rest`, top-level scalar, cursored stream) and why?
- **Ownership**: does it touch a value Herdr owns (pane, split, zoom, cwd, PTY) or one the core owns (visible tab, focus pane, panels, text scale)? Does it wait for Herdr or move ahead of it?
- **Herdr contract**: does it call a method, parameter, or field that is not in `contracts/herdr-api.schema.json`?
- **Failure path**: what does a user see when this fails, and where is it logged?

## Evidence

<!-- A run directory under agents/runs/<slug>/ stays local. Attach what a reviewer must see; describe what was measured and how. -->

- Verified by:
- Not verified (and why):
- High-frequency path impact (or N/A): added work per input, affected consumers, scaling and queue bounds; regression that catches the old failure; native evidence and remaining gaps.

## Screenshots

<!-- Required for anything a user looks at. Paste images here; GitHub stores them as user-attachments. "N/A - no visual surface changed" otherwise. -->

## AI tooling

<!-- Which parts, if any, were produced with AI tooling, and what you checked by hand. This is a review input, not an attribution line. -->
