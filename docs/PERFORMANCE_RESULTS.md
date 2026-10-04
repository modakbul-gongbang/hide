# Performance observations, 2026-10-04

These measurements describe one isolated candidate for [issue #340](https://github.com/modakbul-gongbang/hide/issues/340).
They support the qualified figures in the README and introduction site, not a claim that hide is faster than another application.
The measurement contract and isolation procedure remain [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md).
This dated report is not an acceptance threshold for future builds.

## Candidate and environment

| Item | Measured configuration |
| --- | --- |
| Source | `2c27355016131b9065fd5ec6134fdaf634ef8be6`; no product-source changes |
| Native candidate | Worktree-built, unpackaged Electron host with release `hided` and its embedded production web shell; not an installed release bundle |
| Runtime | Herdr 0.9.1, obtained from this revision's bundle manifest; binary schema checked against the committed contract |
| Native browser engine | Electron 44.4.5, Chromium 152.0.7977.130 |
| Headless browser | Google Chrome 154.0.8037.95; separate disposable profile |
| Headless viewport | 1280 × 813 CSS pixels, scale 1; nominal 60 Hz animation callbacks |
| Machine | Apple M4 Pro, 14 CPU cores, 48 GiB RAM, macOS 15.1 (24B2083), arm64 |
| Display | Built-in 3456 × 2234 display at 120 Hz; native content viewport 1440 × 872 CSS pixels, scale 2; initial terminal 137 columns × 48 rows |
| Other work | Concurrent applications remained running; recorded one-minute system load was about 3.7 to 5.3 during the native trials |

The release daemon and web/desktop builds used `scripts/verify-cargo.sh` and `scripts/verify-web.sh` inside this worktree.
Each fixture had a private HOME, provider/configuration directories, state, desktop/browser profile, socket and Herdr session, with an empty-server check before creation.
The native fixture was an independent synthetic Git repository, not a folder inheriting the surrounding repository's catalog.
The candidate window was shown inactive, identified by the launched PID and exact window ID, and captured in the background.
The native focus guard recorded no activation or window-focus events.
The operator's installed hide, Herdr server and panes were not launched, stopped, focused or used as fixtures.
The headless runner used `--isolated-headless` and did not inspect the operator socket.

## Boundaries and statistics

All p50 and p95 figures use nearest rank: sort every sample and select ranks `ceil(0.50 × n)` and `ceil(0.95 × n)`.
No latency outliers were removed from the completed measurement phases below.
For five trials, p95 is the largest observation; this is a small sample, not a reliable population tail estimate.

The native input probe ends when xterm's write callback finds the marker in its parsed buffer.
Tab and scroll probes end at a requested animation-frame callback after the expected DOM or buffer change.
Neither endpoint proves that the GPU compositor presented the requested pixels on the physical display.
Exact-window screenshots confirmed the synthetic terminal, output, scroll state and three-pane layout at observation points; they do not measure key-to-photon latency.
Keyboard and wheel events were injected into the candidate renderer, not through a physical keyboard or an operating-system input device.
The opt-in probe scans terminal text and adds measurement overhead.

## Native launch and terminal preparation

Five fresh-profile trials each started a new daemon and desktop process against an already-running private Herdr fixture.
Each was followed by a warm relaunch using the same desktop profile, saved Workspace and still-running daemon/Herdr.
Fresh-profile timing starts before spawning `hided`; warm timing starts before launching Electron.
Opening the fixture through the Projects sidebar is included only in the fresh terminal-ready row.

| Boundary | n | p50, ms | p95, ms | Maximum, ms |
| --- | ---: | ---: | ---: | ---: |
| Fresh daemon spawn to healthy HTTP response | 5 | 68.1 | 71.3 | 71.3 |
| Fresh daemon spawn to initial Main/Workspace DOM | 5 | 457.3 | 518.0 | 518.0 |
| Fresh daemon spawn, open fixture, terminal canvas present | 5 | 572.5 | 1183.9 | 1183.9 |
| Warm desktop relaunch to restored terminal canvas | 5 | 459.1 | 474.2 | 474.2 |
| Private Herdr start and terminal preparation, measured separately | 5 | 150.1 | 150.3 | 150.3 |

The first terminal-ready observation in this five-pair sequence was 1183.9 ms; later fresh-profile observations ranged from 559.5 to 598.1 ms.
These are fresh application profiles after builds and preparatory launches had warmed machine caches.
They exclude download, installation, signing/Gatekeeper prompts, login and operating-system cold-cache/reboot behavior.
Git fixture creation is outside these clocks; Herdr workspace/pane preparation is excluded from app startup and listed separately; the fresh and warm rows have different readiness endpoints and daemon lifetimes.

## Native input, tabs, output and scroll

These phases used a warmed one-pane Workspace, with a second tab added for the switching phase.
Input starts at the first renderer keydown for a nine-character ASCII marker followed by Enter, and ends when that whole marker is in the parsed terminal buffer.
The interval includes injection of the remaining characters, dispatch, PTY echo, subscription delivery and xterm parsing.
Tab switching alternates two tabs and waits for the selected canvas, a terminal canvas and the next animation frame; it does not require terminal pixels or restored text to have been painted.
Each output burst launches a synthetic writer through the private Herdr CLI, emits 1,000 numbered 79-byte rows plus a unique end marker, and waits for that marker in the parsed buffer.
Burst timing includes CLI startup and writer startup; it is not a bytes-per-second throughput result or a check of every output row.
Scroll alternates 240-pixel wheel requests over the terminal after those bursts, waiting for changed terminal text and the next animation frame.

| Boundary | n | p50, ms | p95, ms | Maximum, ms |
| --- | ---: | ---: | ---: | ---: |
| First renderer keydown to complete marker in buffer, idle | 50 | 15.1 | 16.3 | 20.9 |
| Tab DOM click to selected canvas and next frame | 50 | 15.4 | 16.8 | 24.0 |
| CLI launch to end marker after 1,000-row output | 10 | 17.5 | 57.0 | 57.0 |
| Renderer wheel to changed buffer and next frame | 50 | 16.7 | 99.5 | 101.1 |

A separate warmed native run drove three visible split panes, each requesting one numbered line every 8 ms for 122 seconds, with `cat` receiving input in the measured pane.
All three panes had live output before input sampling, and the measured pane was at the bottom of its scrollback.
This is synthetic terminal concurrency, not three real coding tasks, provider calls or concurrent builds.

| Driven native observation | n | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Before CLI spawn to marker in buffer, three driven panes | 50 | 12.2 ms | 27.8 ms | 132.9 ms |
| Animation-frame callback interval over 120.002 s | 14,403 | 8.3 ms | 9.9 ms | 10.4 ms |

The driven marker was sent as one CLI text operation rather than separate renderer keystrokes, so its latency cannot be compared directly with the idle keyboard row.
Animation callbacks at the display cadence are an internal scheduling observation, not proof of smooth physical presentation or absence of dropped output.

## CPU and memory

`resources.py` sampled twenty approximately one-second CPU-time deltas and RSS snapshots separately for idle and driven phases.
The native paired resource run began with one idle pane, then split to three driven panes in one tab.
Totals include the owned Electron main, renderer, GPU and utility descendants, `hided` descendants, and the private Herdr process tree including fixture shells and writers.
Measurement-controller processes and unrelated applications are excluded.
CPU is percent of one core, so 100% means one fully busy core, not the whole 14-core machine.
RSS is the sum of resident-set sizes in MiB, not unique physical memory or macOS memory footprint; shared pages can be counted more than once.
CPU-time resolution is coarse at one-second sampling, and short-lived children disappearing between samples can be missed.

| Native total | n | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Idle CPU | 20 | 0.0% | 2.9% | 3.8% |
| Three-pane driven CPU | 20 | 25.9% | 30.7% | 34.7% |
| Idle RSS sum | 20 | 547.7 MiB | 549.0 MiB | 549.1 MiB |
| Three-pane driven RSS sum | 20 | 619.4 MiB | 622.0 MiB | 622.8 MiB |

For the driven run, component RSS p50s were 46.2 MiB for the daemon tree, 75.4 MiB for Herdr and fixture descendants, and 495.9 MiB for Electron descendants.
Component percentiles need not sum to the total percentile because their ranks may come from different seconds.
The daemon's RSS alone is not the application's memory use, and the rounded zero idle CPU median does not prove zero resident work.

## Headless sustained-output observation

The existing web-shell runner also completed a one-pane run with three idle batches of fifty unique markers and a ten-minute output window.
The pane requested one line every 8 ms, with fifty additional markers during the driven phase.
Here latency starts immediately before spawning `herdr pane send-text` and ends at xterm write completion.
It is recomputed from the saved `hops` as `write_ms - t0_ms`, including CLI launch; the historical CLI-return-relative field can be negative and is not used for these public figures.

| Headless one-pane observation | n | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Before CLI spawn to marker in buffer, idle | 150 | 8.4 ms | 9.9 ms | 111.2 ms |
| Same CLI boundary, driven | 50 | 12.3 ms | 117.7 ms | 136.6 ms |
| Idle CPU total | 20 | 1.0% | 14.5% | 25.1% |
| Driven CPU total | 20 | 15.4% | 17.4% | 20.2% |
| Idle RSS total | 20 | 1242.0 MiB | 1287.8 MiB | 1290.6 MiB |
| Driven RSS total | 20 | 1216.2 MiB | 1230.6 MiB | 1231.4 MiB |
| One-minute RSS snapshots over ten minutes | 11 | 807.5 MiB | 1199.2 MiB | 1199.2 MiB |

The ten-minute RSS series started at 1199.2 MiB and ended at 814.1 MiB; one changing RSS series cannot establish a leak-free application.
Headless Chrome is a different executable/process tree and rendering mode from the native host, so its resource and latency figures are not native-app estimates.
Its 36,002 animation callbacks over 600.009 s had p50 and p95 approximately 16.7 ms.
The existing strict `dt > 16.7` frame-budget check failed, with 38.8% above that floating-point boundary, while the maximum rounded to 16.8 ms.
The gate and deadlines were not relaxed; this nominal 60 Hz headless scheduling result does not support a native smoothness claim.

## Headless attached-tab workload

The repaired committed runner also completed `MEASURE_SCENARIO=multi` with five visible split panes, five visited/attached tabs and nine retained terminal instances.
After three fifty-marker idle batches, the same measured pane received the 8 ms output driver and fifty markers for a 120.012-second window; the other eight panes stayed idle.
This exercises retained attachments and mounted terminal instances, rather than nine simultaneously driven tasks.
The CLI-before-spawn boundary and process-tree resource method are the same as the other headless table.

| Headless multi observation | n | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Before CLI spawn to marker in buffer, idle | 150 | 7.2 ms | 11.4 ms | 114.8 ms |
| Same CLI boundary, driven | 50 | 11.0 ms | 27.7 ms | 44.2 ms |
| Idle CPU total | 20 | 1.0% | 11.5% | 19.0% |
| Driven CPU total | 20 | 25.0% | 26.9% | 27.9% |
| Idle RSS total | 20 | 1427.8 MiB | 1428.9 MiB | 1429.0 MiB |
| Driven RSS total | 20 | 1044.1 MiB | 1048.7 MiB | 1063.6 MiB |

Its 7,201 animation callbacks again had p50/p95 approximately 16.7 ms and a maximum rounded to 16.8 ms; the unchanged frame-budget check failed with 24.9% strictly above 16.7 ms.
The fixture completed and its owned processes exited; a completed measurement is not a passing frame gate.
The one-pane and multi runs were not alternated under a controlled machine load, so their different tails and RSS cannot establish scaling improvements or regressions.

## Orca comparison boundary

Orca 1.4.214 was already present on the machine.
It was launched only with disposable home/user-data directories, telemetry disabled and its headful test mode that shows a window inactive.
An exact-PID/window capture confirmed its empty project dashboard without personal projects or a login attempt.
However, the Electron launch driver timed out accessing the renderer document, and a separate direct CDP launch also timed out locating its body despite discovering the renderer target.
The attempted driver deadlines remained unchanged; owned candidate processes were stopped, including a bounded forced cleanup when graceful quit did not finish.
No installation, operator-profile modification, sign-in or operator-window interaction was performed.
The test mode and failed renderer control did not yield a matching terminal workload, so there is no Orca timing/resource comparison and no superiority claim.
The failures are a limitation of this comparison attempt, not a finding about Orca responsiveness or authentication requirements.

## Reproduction and limits

Build the production web shell, release daemon and desktop host through the verification wrappers, then follow the isolation checklist before every native launch.
For the headless sustained run, use `HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/single bash scripts/web-shell-measure/run.sh --isolated-headless --memory-series` with the manifest-resolved Herdr binary and private coordination/configuration roots.
Native automation uses the same private fixture contract, `desktop/e2e/focus-guard.cjs`, the desktop host's inactive-show flags and `?probe=1`; use renderer actions and exact-window captures, never global focus or keyboard actions.
Create the independent fixture repository before its workspace, report its checkout-owner metadata, and confirm the probe's pane identity and live output before sampling.
Keep five fresh/warm launch pairs, fifty input/tab/scroll observations, ten numbered output bursts, the three-pane line-every-8-ms workload and separate twenty-second resource windows when repeating these rows.
Preserve whole distributions and timeout/failure records, not only rounded headline values.
Re-measure rather than carrying these numbers to a new executable, workload, machine or browser mode.

Preparatory launches with an incomplete Electron runtime, fixture-navigation failures, and a native fixture inheriting the surrounding repository catalog were excluded before establishing the independent-fixture measurements.
A preliminary identical-row scroll fixture could not distinguish every scroll change and timed out; the numbered-row fixture above completed all fifty observations.
An initial driven-input attempt while still scrolled back could not see newly echoed markers; it is retained as an invalid visibility precondition rather than a successful latency result or proof of lost input.
Raw samples, scripts, logs, identity checks and screenshots remain only under ignored `agents/runs/`; they are not committed, and personal paths/content are removed from retained measurement exports.
The shared machine was not quiesced, paired builds were not alternated, and no earlier product revision or competitor completed the same workload.
Physical key-to-photon latency, first install/reboot, long native uptime, remote/SSH workloads, provider/network activity, IME, selection, resize, and Windows/Linux performance were not measured here.
The README and site must retain these boundaries and link this report whenever quoting its figures.
