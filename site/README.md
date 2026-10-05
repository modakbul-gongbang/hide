# hide introduction site

This directory contains the static introduction-site source prepared for issue #338 and the intended `withhide.dev` domain.
It has no build step, packages, framework, webfont service, analytics, cookies, or application environment variables.
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
The workflow and feature descriptions follow [UI_BEHAVIOR.md](../docs/UI_BEHAVIOR.md), with current main's agent launcher, project overview and Workspace components as implementation references.
Supported platforms, signing limitations, prerequisites and installation links follow [INSTALL.md](../docs/INSTALL.md).
Performance copy quotes only the dated candidate measurements in [PERFORMANCE_RESULTS.md](../docs/PERFORMANCE_RESULTS.md), with sample counts and boundaries beside the figures.
Refresh that report and both the site and repository README together when changing a performance figure; internal buffer/frame endpoints must not be described as physical display latency.
The recorded comparison attempt did not complete a matching Orca workload, so the site makes no relative performance claim.
The site has its own small CSS scale and brand palette; it does not consume or replace the product's generated UI tokens.
`assets/hide-overview.webp` is a real capture of the app's project overview, cropped to the app window with other projects' rows blurred and resized to 2400 pixels wide; the feature rows show crops of that same file.
Its text is the capturing machine's own project data and Korean-language interface; it is a candidate screenshot, not a recorded demo.
The page is dark by default and follows `prefers-color-scheme` for a light variant that keeps the brand palette.

## Remaining publication work

Source preparation does not complete issue #338's publication acceptance criteria.
The overview screenshot still needs independent privacy review, and a recorded demo is pending the related product fixes and capture review.
No video or placeholder demo link is included here.
DNS ownership, hosting, HTTPS and live `withhide.dev` behavior have not been verified or changed by this source work.
A complete public stable release must exist before real archive downloads can be verified end to end.
Before publication, refresh feature and platform copy against current main, independently review the site and any media, test the live release links and checksums, and authorize deployment separately.
