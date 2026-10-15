# hide introduction site

This directory contains the static introduction-site source prepared for issue #338 and the intended `withhide.dev` domain.
It has no build step, packages, framework, webfont service, analytics, cookies, or application environment variables.
Bricolage Grotesque (headings and body) and Geist Mono (labels and drawn panels) are served from `assets/fonts/` under the SIL Open Font License (`OFL-BricolageGrotesque.txt`, `OFL-Geist.txt`), copied unchanged from the `@fontsource-variable/bricolage-grotesque` 5.3.0 and `geist` 1.4.2 npm packages.
Serving this directory locally does not publish it or configure the domain.

## Preview and checks

From the repository root, run the focused download-policy tests:

```sh
node --test site/tests/downloads.test.mjs
```

Use Node.js 22.7 or later, which recognizes the browser ES module in `downloads.js` without a package manifest.
For a local preview, choose a free loopback port and serve only this directory:

```sh
python3 -m http.server 8080 --bind 127.0.0.1 --directory site
```

Open `http://127.0.0.1:8080/` in the browser and stop the server with Ctrl+C when finished.
Browser modules require HTTP; opening `index.html` through `file:` is not the preview procedure.
Check desktop at 1440 pixels and mobile at 390 pixels, including keyboard navigation, text wrapping, manual OS and processor selection, loading, unavailable and retry states.
Run evidence belongs under ignored `agents/runs/public-readiness/site-338/`, never in this directory or a commit.

## Download policy

The browser reads the unauthenticated GitHub `releases/latest` API for `modakbul-gongbang/hide`.
It requests no credentials, permits one request at a time, and cancels a request after ten seconds or when leaving the page.
There is no polling or persistent release cache.
Only a release with explicit `draft: false`, `prerelease: false`, a stable `v<major>.<minor>.<patch>` tag and that tag's exact repository release URL is eligible.
All three archives below and their individual `.sha256` sidecars must exist exactly once, be uploaded, have a positive integer size, and have exact repository download URLs for the same release tag.

| Setup | Required archive |
| --- | --- |
| macOS, Apple Silicon | `hide-v<version>-macos-arm64.zip` |
| Windows, x64 | `hide-v<version>-windows-x64.zip` |
| Linux, x64 | `hide-v<version>-linux-x64.tar.gz` |

Any incomplete pair withholds downloads for every platform.
The browser's OS suggestion is only a hint; the visitor can change it and must choose a processor.
Unsupported combinations offer no archive.
The archive, checksum and version-specific release links appear together only after validation and a supported selection.
The site links the published checksum for the visitor to verify; it does not download and cryptographically verify the archive itself.
API errors, timeouts, invalid JSON, no public release and ineligible releases leave useful GitHub Releases and source-install links available.
The same links remain usable without JavaScript.
Browser QA should simulate GitHub responses at that external boundary for ready, incomplete, draft, loading, rate-limit and network-error cases, without changing the shipped source or following test download URLs.

## Content and artwork authority

The brand line, approved owl mark, palette and voice come from [BRAND.md](../docs/BRAND.md).
`assets/hide-mark.png` is an unchanged copy of `design/brand/hide-mark.png`; keep its full square and proportions.
The page leads with the person's attention: "Run more agents. Hold less." Its sections, in order, are the hero, one night with hide (`#night`), three pillars, the terminal band, the Software Factory (Soon), pricing, the FAQ, and a closing that carries the download chooser.
Each section's claims follow the owning guide: the supported agents and their tiers [agent-hooks.md](../docs/agent-hooks.md), the four groups and status marks [status-model.md](../docs/status-model.md), agents handing work to each other [delivery.md](../docs/delivery.md), the core, devices and the phone [ARCHITECTURE.md](../docs/ARCHITECTURE.md) and [INSTALL.md](../docs/INSTALL.md), Hide AI [AI_PROVIDERS.md](../docs/AI_PROVIDERS.md), the Factory [factory.md](../docs/factory.md), and adopting an existing Herdr server [INSTALL.md](../docs/INSTALL.md), which the FAQ answers.
The three pillar panels and the night timeline are small HTML drawings of the app's own surfaces (the sidebar's four groups, the lineage tree, the core and its screens) with invented example work; they are illustrations, not captures, and use the app's status marks and colors.
The night timeline sits in `#night-stage` as a static, complete fallback that reads without script; the visible words avoid developer jargon on purpose ("a test broke", "ready for review").
`night.js` replaces that fallback with a picture of the same night, `assets/night/night-wide.svg` or, below 720 px, `night-tall.svg`, inlined so the page's fonts apply: your room stays dark until morning while agents on machines that stay on pass notes, ask, fix and check, and at 07:00 two things wait for you.
The six moments are a list beside it that reads without script; `assets/night/night.css` lights each row on the picture's 17-second timeline.
The night starts when the section comes into view, pauses off screen and in a hidden tab, rests on the morning with a button to play it again, and under `prefers-reduced-motion` shows only the morning with every moment listed.
Every section owl is an `<img class="owl" data-owl="...">`, so a pose is replaced by changing its `src`.
Supported agents are named in text; third-party agent marks stay inside the app, where `web/src/assets/agents/manifest.json` records their terms.
`site.js` runs the one-time scroll reveals, and nothing else on the page repeats; under `prefers-reduced-motion` the reveals show at once.
The Factory is marked Soon and has no sign-up form, because the site has no backend to receive one: its action is to watch the repository's releases on GitHub.
Pro and Team carry no price, because none is decided; free covers everything a person runs themselves.
The workflow and feature descriptions follow [UI_BEHAVIOR.md](../docs/UI_BEHAVIOR.md), with current main's agent launcher, project overview and Workspace components as implementation references.
Supported platforms, signing limitations, prerequisites and installation links follow [INSTALL.md](../docs/INSTALL.md).
The FAQ's speed answer quotes no figure and links the dated candidate measurements in [PERFORMANCE_RESULTS.md](../docs/PERFORMANCE_RESULTS.md); if a figure is ever quoted on the page again, give its sample count and boundary beside it, and never describe internal buffer/frame endpoints as physical display latency.
The recorded comparison attempt did not complete a matching Orca workload, so the site makes no relative performance claim.
The site has its own small CSS scale and brand palette; it does not consume or replace the product's generated UI tokens.
`assets/hide-workspace.webp` is a render of the design library, not a capture of a running app.
The workspace render is assembled from `design/hide-screens.pen` parts (the Projects sidebar, the Workspace toolbar and Agent column, the Sessions panel) with two terminal panes and example session titles written for the page.
It was built in a scratch copy through the Pen CLI and exported at 2x; the committed `.pen` files are unchanged.
`assets/owls/` holds the owl character's poses, generated with the approved mark as the style reference and keyed to transparency; [BRAND.md](../docs/BRAND.md#the-owl-character) owns what the character may and may not do, and the mark stays the unchanged `hide-mark.png` in the header and favicon.
The page is dark by default, on the Owl Field palette: the mark's blue-gray field taken down to night, lime as the one primary accent and the field color as the secondary on kickers, numbers and markers.
It follows `prefers-color-scheme` for a light variant that keeps the same primary and secondary.

## Remaining publication work

Source preparation does not complete issue #338's publication acceptance criteria.
The workspace render and the drawings show invented example work and should be compared with the shipped app before publication; a recorded demo is pending the related product fixes and capture review.
No video or placeholder demo link is included here.
DNS ownership, hosting, HTTPS and live `withhide.dev` behavior have not been verified or changed by this source work.
A complete public stable release must exist before real archive downloads can be verified end to end.
Before publication, refresh feature and platform copy against current main, independently review the site and any media, test the live release links and checksums, and authorize deployment separately.
The page copy describes current main, which is ahead of the latest public release (v0.3.0 names Claude Code and Codex only and carries no Factory); publish it with or after the release that ships those features.
