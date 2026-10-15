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
`assets/hide-icon-blinking.svg` is an unchanged copy of `design/brand/hide-icon-blinking.svg`, the animated app icon, and `assets/hide-icon.svg` is the same file with its `<animateTransform>` elements removed, which leaves the resting pose.
The page presents hide as an agent development environment in four pillars: multi-agent, multi-device, Hide AI and Software Factory, followed by the Herdr relationship.
Each pillar's claims follow the owning guide: the supported agents and their tiers [agent-hooks.md](../docs/agent-hooks.md), the four groups and status marks [status-model.md](../docs/status-model.md), devices and the phone [ARCHITECTURE.md](../docs/ARCHITECTURE.md) and [INSTALL.md](../docs/INSTALL.md), Hide AI [AI_PROVIDERS.md](../docs/AI_PROVIDERS.md), the Factory [factory.md](../docs/factory.md), and adopting an existing Herdr server [INSTALL.md](../docs/INSTALL.md).
The Factory is marked Coming soon until a public release carries it.
The first three pillars' panels are small HTML drawings of the app's own surfaces (the sidebar's four groups, Settings › Devices, an agent title) with invented example tasks; they are illustrations, not captures, and use the app's status marks and colors.
The Factory pillar has an animated Line drawing above the Factory render described below.
Supported agents are named in text; third-party agent marks stay inside the app, where `web/src/assets/agents/manifest.json` records their terms.
`site.js` runs the one-time scroll reveals, the hero owl's peeking and the sidebar panel's group changes.
`factory.js` plays a scripted Factory line: Tasks move from Intake to Merged, and each question goes where the mode table in [factory.md](../docs/factory.md#factory-ai-the-observer) sends it (Direct, Together, Delegate), so a permission always reaches the desk and a risky-path merge goes to Factory AI only in Delegate.
The three repeating animations sit in different sections, each runs only while it is on screen and the tab is visible, and under `prefers-reduced-motion` each shows one still frame.
The header icon's blink is SMIL inside an `<img>`, which a hidden tab does not paint; under `prefers-reduced-motion` a `<picture>` source shows `hide-icon.svg` instead.
The workflow and feature descriptions follow [UI_BEHAVIOR.md](../docs/UI_BEHAVIOR.md), with current main's agent launcher, project overview and Workspace components as implementation references.
Supported platforms, signing limitations, prerequisites and installation links follow [INSTALL.md](../docs/INSTALL.md).
Performance copy quotes only the dated candidate measurements in [PERFORMANCE_RESULTS.md](../docs/PERFORMANCE_RESULTS.md), with sample counts and boundaries beside the figures.
Refresh that report and both the site and repository README together when changing a performance figure; internal buffer/frame endpoints must not be described as physical display latency.
The recorded comparison attempt did not complete a matching Orca workload, so the site makes no relative performance claim.
The site has its own small CSS scale and brand palette; it does not consume or replace the product's generated UI tokens.
`assets/hide-workspace.webp` and `assets/hide-factory.webp` are renders of the design library, not captures of a running app.
The workspace render is assembled from `design/hide-screens.pen` parts (the Projects sidebar, the Workspace toolbar and Agent column, the Sessions panel) with two terminal panes and example session titles written for the page; the Factory render is the Line sheet of that file with its text switched to the app's English catalog (`web/src/i18n/resources/`) and the example Tasks translated.
Both were built in a scratch copy through the Pen CLI and exported at 2x; the committed `.pen` files are unchanged.
`assets/owls/` holds the owl character's poses, generated with the approved mark as the style reference and keyed to transparency; [BRAND.md](../docs/BRAND.md#the-owl-character) owns what the character may and may not do, the header shows the blinking app icon and the favicon stays the unchanged `hide-mark.png`.
`owl-peek.svg` is the `owl-agents` owl traced into vector parts (ears, body, wings, face) and animated by CSS keyframes inside the file, so it plays as a plain `<img>` with no player script.
Its field spans the window's width with the screenshot's top edge 200 units down, and the owl hides behind straight edges measured on `hide-workspace.webp`: it pops up over the window's top edge, leans out from behind the Sessions panel and rises from behind the sidebar's project box, looks at one place each time, and ducks back.
A new screenshot moves those edges, so the edge positions in the file are measured again with it.
`owl-peek-still.svg` is its first stop held still, which the page shows while the hero is off screen, in a hidden tab and under `prefers-reduced-motion`, because an SVG inside `<img>` does not see the page's media queries in every browser.
The page is dark by default, on the Owl Field palette: the mark's blue-gray field taken down to night, lime as the one primary accent and the field color as the secondary on kickers, numbers and markers.
It follows `prefers-color-scheme` for a light variant that keeps the same primary and secondary.

## Remaining publication work

Source preparation does not complete issue #338's publication acceptance criteria.
The workspace and Factory renders show invented example work and should be compared with the shipped app before publication; a recorded demo is pending the related product fixes and capture review.
No video or placeholder demo link is included here.
DNS ownership, hosting, HTTPS and live `withhide.dev` behavior have not been verified or changed by this source work.
A complete public stable release must exist before real archive downloads can be verified end to end.
Before publication, refresh feature and platform copy against current main, independently review the site and any media, test the live release links and checksums, and authorize deployment separately.
The pillar copy describes current main, which is ahead of the latest public release (v0.3.0 names Claude Code and Codex only and carries no Factory); publish it with or after the release that ships those features.
