"""Which lanes `scripts/ci-plan.py` plans for real changes, and what `verify` accepts."""
import importlib.util
import json
import re
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("ci_plan", Path(__file__).parents[1] / "ci-plan.py")
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)
ROOT = Path(__file__).parents[2]
CRATES = ci.cargo_crates(ROOT)
EVERY_PACKAGE = sorted(crate["name"] for crate in CRATES.values())
# The lanes that take a macOS runner; every other check on macOS is a job of
# `nightly.yml`.
MACOS_LANES = {"os-contract-macos", "desktop-e2e", "package"}
# Every lane but `package`: what a push, the nightly and an unclassified path plan.
FULL = [lane for lane in ci.LANES if lane != "package"]
# The paths a pull request changed when `package.yml` had its own trigger.
PACKAGE_INPUTS = (
    "desktop/scripts/package.mjs", "desktop/scripts/build.mjs", "desktop/package.json", "desktop/resources/entitlements.plist",
    "contracts/herdr-bundle.json", "scripts/fetch-herdr-runtime.sh", "scripts/fetch-herdr-runtime.ps1",
    "scripts/verify-cargo.sh", "scripts/verify-web.sh", "scripts/toolchain-env.sh",
    "hided/build.rs", "hided/src/cli.rs", "hide-kit/src/lib.rs", "hide-agent-hooks/src/main.rs",
    ".github/workflows/package.yml", ".github/workflows/release.yml",
)


def plan(*paths, status="M"):
    return ci.select([(status, path) for path in paths], CRATES, ROOT)


def needs_for(result, **override):
    needs = {lane: {"result": "success" if lane in result["lanes"] else "skipped"} for lane in ci.LANES}
    needs["plan"] = {"result": "success", "outputs": {"lanes": json.dumps(result["lanes"])}}
    for lane, outcome in override.items():
        needs[lane.replace("_", "-")] = {"result": outcome}
    return needs


class Selection(unittest.TestCase):
    def test_documentation_runs_only_the_policy_checks(self):
        result = plan("docs/ARCHITECTURE.md", "CONTRIBUTING.md", "desktop/AGENTS.md", "design/hide-ui.lib.pen")
        self.assertEqual(result["lanes"], ["policy"])
        self.assertEqual(result["rust_packages"], [])

    def test_a_file_another_lane_reads_plans_that_lane(self):
        notes = plan("AGENTS.md")
        self.assertEqual(notes["lanes"], ["policy", "rust"])
        self.assertEqual(notes["rust_packages"], ["herdr-core"])
        self.assertIn("herdr-core", plan("hided/src/core.rs")["rust_packages"])
        self.assertTrue({"web-e2e", "windows-e2e"} <= set(plan("desktop/src/main/wirePath.ts")["lanes"]))
        self.assertTrue({"checks", "web-e2e"} <= set(plan("design/tokens.json")["lanes"]))

    def test_a_web_display_change_skips_windows_and_desktop(self):
        result = plan("web/src/components/ui/button.tsx", "web/src/Overview.tsx")
        # herdr-core's structure tests scan the shell's sources.
        self.assertEqual(set(result["lanes"]), {"policy", "checks", "web-e2e", "rust"})
        self.assertEqual(result["rust_packages"], ["herdr-core"])

    def test_web_code_the_desktop_host_reads_reaches_desktop_and_windows(self):
        for path in ("web/src/shortcuts.ts", "web/src/host.ts", "web/src/store.ts", "web/src/keys.ts"):
            with self.subTest(path=path):
                lanes = set(plan(path)["lanes"])
                self.assertTrue({"checks", "web-e2e", "desktop-e2e", "windows-e2e"} <= lanes)
                self.assertNotIn("windows-check", lanes)
                self.assertNotIn("remote-mailbox", lanes)

    def test_a_platform_spec_keeps_its_platform_lanes_on_linux_and_windows(self):
        lanes = set(plan("web/e2e/s3.spec.ts")["lanes"])
        self.assertEqual(lanes, {"policy", "checks", "web-e2e", "windows-e2e"})
        lanes = set(plan("web/e2e/new-tab.spec.ts")["lanes"])
        self.assertEqual(lanes, {"policy", "checks", "web-e2e"})

    def test_the_remote_mailbox_lane_follows_the_crates_it_builds_and_tests(self):
        # It builds hided, hide, the agent hooks and the host helper and runs
        # herdr-core's remote_delivery test, so a change to one of those crates
        # or to a crate they depend on plans it.
        for path in (
            "herdr-core/src/lib.rs", "hided/src/lib.rs", "hide-agent-hooks/src/lib.rs", "hide-host/src/lib.rs",
            "hide-platform/src/process.rs", "hide-kit/src/lib.rs", "hide-session/src/lib.rs", "herdr-core/tests/remote_delivery.rs",
        ):
            with self.subTest(path=path):
                self.assertIn("remote-mailbox", plan(path)["lanes"])
        # The web shell, its specs and the desktop host do not reach it.
        for path in (
            "web/e2e/s3.spec.ts", "web/e2e/new-tab.spec.ts", "web/src/host.ts", "web/src/Overview.tsx", "web/playwright.config.ts",
            "web/e2e/herdr-fixture.ts", "desktop/src/main/wirePath.ts", "desktop/src/preload/index.ts", "docs/TESTING.md",
        ):
            with self.subTest(path=path):
                self.assertNotIn("remote-mailbox", plan(path)["lanes"])
        # Every workspace crate is built into one of the four binaries or one
        # they depend on, so each crate's change plans the lane.
        for directory in CRATES:
            with self.subTest(crate=directory):
                self.assertIn("remote-mailbox", plan(f"{directory}/src/lib.rs")["lanes"])
        self.assertIn("remote-mailbox", plan(".github/workflows/pr.yml")["lanes"])

    def test_the_remote_mailbox_lane_follows_the_crates_it_builds_and_tests(self):
        for path in (
            "herdr-core/src/lib.rs", "herdr-core/tests/remote_delivery.rs", "hided/src/main.rs",
            "hide-host/src/lib.rs", "hide-agent-hooks/src/lib.rs", "hide-platform/src/process.rs",
            "hide-session/src/lib.rs", "hide-ai/src/lib.rs",
        ):
            with self.subTest(path=path):
                self.assertIn("remote-mailbox", plan(path)["lanes"])
        # Web and desktop files and documentation cannot
        # change it.
        for path in (
            "web/e2e/s3.spec.ts", "web/e2e/new-tab.spec.ts", "web/src/host.ts", "web/src/Overview.tsx",
            "web/playwright.config.ts", "desktop/src/main/wirePath.ts", "desktop/src/preload/index.ts",
            "docs/TESTING.md",
        ):
            with self.subTest(path=path):
                self.assertNotIn("remote-mailbox", plan(path)["lanes"])
        # A change no rule claims plans every lane, this one included.
        self.assertIn("remote-mailbox", plan("scripts/verify-cargo.sh")["lanes"])

    def test_desktop_changes_run_the_desktop_lanes(self):
        result = plan("desktop/src/preload/index.ts")
        self.assertEqual(set(result["lanes"]), {"policy", "checks", "desktop-e2e"})
        self.assertIn("windows-check", plan("desktop/src/main/index.ts")["lanes"])

    def test_a_platform_crate_reaches_every_os_and_its_consumers(self):
        result = plan("hide-platform/src/process.rs")
        self.assertTrue({"rust", "os-contract", "os-contract-macos", "windows-check", "windows-e2e", "web-e2e", "remote-mailbox"} <= set(result["lanes"]))
        self.assertNotIn("desktop-e2e", result["lanes"])
        # hide-project depends on nothing in the workspace, so it is the one
        # crate a platform change does not reach.
        self.assertEqual(result["rust_packages"], [name for name in EVERY_PACKAGE if name != "hide-project"])
        self.assertFalse(result["full"])

    def test_a_leaf_crate_tests_its_reverse_dependencies_and_compiles_on_windows(self):
        result = plan("hide-session/src/lib.rs")
        self.assertIn("hide-session", result["rust_packages"])
        self.assertIn("hided", result["rust_packages"])
        self.assertNotIn("hide-platform", result["rust_packages"])
        self.assertIn("windows-check", result["lanes"])
        self.assertNotIn("windows-e2e", result["lanes"])
        self.assertNotIn("os-contract", result["lanes"])
        self.assertEqual(plan("hided/tests/handshake.rs")["rust_packages"], ["hided"])

    def test_the_macos_os_contract_runs_for_the_two_crates_it_tests(self):
        for path in ("hide-herdr-client/src/lib.rs", "hide-platform/src/process.rs"):
            with self.subTest(path=path):
                lanes = set(plan(path)["lanes"])
                self.assertTrue({"os-contract", "os-contract-macos"} <= lanes)
                self.assertNotIn("desktop-e2e", lanes)

    def test_a_core_or_daemon_change_asks_for_no_macos_runner(self):
        # Their OS differences are checked on Linux and Windows; the nightly
        # runs the macOS leg and the macOS `@platform` tests for them.
        for path in (
            "herdr-core/src/lib.rs", "hided/src/lib.rs", "hide-host/src/lib.rs", "hide-session/src/lib.rs",
            "web/src/Overview.tsx", "web/e2e/s3.spec.ts", "web/playwright.config.ts",
        ):
            with self.subTest(path=path):
                lanes = set(plan(path)["lanes"])
                self.assertFalse(lanes & MACOS_LANES, lanes & MACOS_LANES)
        self.assertEqual(set(plan("herdr-core/src/lib.rs")["lanes"]) & {"os-contract", "windows-e2e"}, {"os-contract", "windows-e2e"})

    def test_what_goes_into_a_package_plans_the_package_lane_and_nothing_else_does(self):
        # The kit and hooks a packaged daemon runs reach macOS only through it.
        for path in PACKAGE_INPUTS:
            with self.subTest(path=path):
                self.assertIn("package", plan(path)["lanes"])
        for path in (
            "hided/src/lib.rs", "hided/src/core.rs", "herdr-core/src/lib.rs", "hide-platform/src/process.rs",
            "desktop/src/main/index.ts", "desktop/e2e/fixture.ts", "web/src/store.ts", "docs/TESTING.md",
            ".github/workflows/pr.yml", "Cargo.lock", "pnpm-lock.yaml",
        ):
            with self.subTest(path=path):
                self.assertNotIn("package", plan(path)["lanes"])
        self.assertEqual(set(plan("hide-kit/src/lib.rs")["lanes"]) & MACOS_LANES, {"package"})
        self.assertEqual(plan(".github/workflows/package.yml")["lanes"], ["policy", "package"])

    def test_only_the_desktop_app_and_the_web_code_it_drives_ask_for_the_desktop_lane(self):
        for path in ("desktop/src/main/index.ts", "desktop/static/icon.png", "desktop/e2e/desktop.spec.ts", "web/src/shortcuts.ts", "web/e2e/herdr-fixture.ts"):
            with self.subTest(path=path):
                self.assertIn("desktop-e2e", plan(path)["lanes"])

    def test_shared_unknown_and_unsafe_changes_run_everything(self):
        for entries in (
            [("M", ".github/workflows/pr.yml")],
            [("M", "scripts/verify-web.sh")],
            [("M", "contracts/herdr-bundle.json")],
            [("M", "Cargo.lock")],
            [("M", "pnpm-lock.yaml")],
            [("M", "scripts/check-runtime-gates.sh")],
            [("M", "contracts/hided-ws.schema.json")],
            [("M", "web/package.json")],
            [("M", "scripts/design-scratch.mjs")],
            [("M", "mystery/file.txt")],
            [("T", "web/src/store.ts")],
            [("M", "docs/README.md"), ("M", "scripts/ci-plan.py")],
            [],
        ):
            with self.subTest(entries=entries):
                result = ci.select(entries, CRATES, ROOT)
                self.assertTrue(result["full"])
                package = any(path in PACKAGE_INPUTS for _, path in entries)
                self.assertEqual(result["lanes"], list(ci.LANES) if package else FULL)
                self.assertEqual(result["rust_packages"], EVERY_PACKAGE)

    def test_a_push_and_a_failed_comparison_run_everything(self):
        self.assertEqual(ci.plan("push", None, None, ROOT)["lanes"], FULL)
        result = ci.plan("pull_request", "0" * 40, "HEAD", ROOT)
        self.assertTrue(result["full"])
        self.assertIn("comparison unavailable", result["reasons"]["rust"][0])

    def test_nightly_calls_verify_with_every_lane(self):
        # The call plans every lane like a push, and a lane it fails reaches
        # the nightly issue through the report job's `needs`. The packages are
        # the nightly's own call, so `verify` leaves them out.
        self.assertEqual(ci.plan("schedule", None, None, ROOT)["lanes"], FULL)
        workflow = (ROOT / ".github/workflows/pr.yml").read_text()
        self.assertRegex(workflow, r"\n  workflow_call:\n")
        # A called run takes the caller's name, so it never takes a push run's
        # pending place in the `verify-refs/heads/main` group.
        self.assertIn("group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}", workflow)
        nightly = (ROOT / ".github/workflows/nightly.yml").read_text()
        call = nightly[nightly.index("\n  verify:\n"):nightly.index("\n  package:\n")]
        self.assertIn("uses: ./.github/workflows/pr.yml", call)
        self.assertIn("issues: write", call)
        report = nightly[nightly.index("\n  report:\n"):]
        self.assertRegex(report, r"needs: \[[^\]]*\bverify\b[^\]]*\]")

    def test_a_draft_plans_no_lane_and_ready_for_review_runs_them(self):
        result = ci.plan("pull_request", "0" * 40, "HEAD", ROOT, draft=True)
        self.assertEqual(result["lanes"], [])
        self.assertEqual(result["rust_packages"], [])
        self.assertIn("draft", ci.summary(result))
        workflow = (ROOT / ".github/workflows/pr.yml").read_text()
        # Without the `ready_for_review` run nothing replaces the draft's `verify`.
        self.assertRegex(workflow, r"types: \[[^\]]*\bready_for_review\b[^\]]*\]")
        self.assertIn('--draft "${{ github.event.pull_request.draft || false }}"', workflow)
        # `verify` runs on a draft and fails there: a skipped one would count as
        # passed for the minutes before the ready run's `verify` starts.
        verify = workflow[workflow.index("\n  verify:\n"):]
        self.assertIn("    if: always()\n", verify)
        self.assertNotIn("draft", verify.split("steps:")[0])

    def test_a_missing_crate_graph_runs_everything(self):
        with tempfile.TemporaryDirectory() as directory:
            result = ci.plan("pull_request", "HEAD^1", "HEAD", Path(directory))
            self.assertEqual(result["lanes"], FULL)
            self.assertIn("crate graph unavailable", result["reasons"]["rust"][0])

    def test_a_pull_request_diff_is_read_from_git(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            def git(*args):
                subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)
            git("init", "-q")
            git("config", "user.email", "ci@example.invalid")
            git("config", "user.name", "ci")
            (repo / "docs").mkdir()
            (repo / "docs/a.md").write_text("a\n")
            git("add", ".")
            git("commit", "-qm", "base")
            (repo / "docs/a.md").write_text("b\n")
            (repo / "docs/b.md").write_text("new\n")
            git("add", ".")
            git("commit", "-qm", "head")
            self.assertEqual(ci.changed_entries("HEAD^1", "HEAD", repo), [("M", "docs/a.md"), ("A", "docs/b.md")])
            # Not the merge commit a pull request checks out: every lane.
            self.assertTrue(ci.plan("pull_request", "HEAD^1", "HEAD", repo, CRATES)["full"])
            git("checkout", "-qb", "topic", "HEAD^1")
            (repo / "docs/c.md").write_text("c\n")
            git("add", ".")
            git("commit", "-qm", "topic")
            git("checkout", "-q", "-")
            git("merge", "-q", "--no-ff", "-m", "merge", "topic")
            result = ci.plan("pull_request", "HEAD^1", "HEAD", repo, CRATES)
            self.assertEqual(result["lanes"], ["policy"])


class NamedPaths(unittest.TestCase):
    """The paths `ci-plan.py` names; every other path still plans every lane."""

    def lanes(self, *paths):
        return set(plan(*paths)["lanes"])

    def test_paths_no_lane_reads_plan_policy_alone(self):
        for path in (
            "site/index.html", "tools/t1-preflight/README.md", "spikes/web-shell/measure/summarize.py", "agents/prd/x/prd.md",
            ".gitignore", "web/.gitignore", ".github/pull_request_template.md", ".github/dependabot.yml",
            ".github/workflows/nightly.yml", ".github/workflows/herdr-update.yml",
            "scripts/tests/test_ci_plan.py", "scripts/nightly-report.cjs", "scripts/check-harness-ignore-anchor.sh",
            "scripts/pen-system.mjs", "scripts/design-review.mjs", "scripts/web-shell-measure/run.sh",
            "contracts/README.md", "site/README.md",
        ):
            with self.subTest(path=path):
                result = plan(path)
                self.assertEqual(result["lanes"], ["policy"])
                self.assertFalse(result["full"])

    def test_web_e2e_helpers_reach_the_desktop_and_windows_lanes_but_not_rust(self):
        want = {"policy", "checks", "web-e2e", "windows-e2e", "desktop-e2e", "windows-check"}
        for path in ("web/e2e/herdr-fixture.ts", "web/e2e/shims/build.ts", "web/e2e/shims/noop.c", "web/e2e/test-size-baseline.json"):
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), want)

    def test_desktop_e2e_helpers_run_the_desktop_suites_and_windows_unit_tests(self):
        for path in ("desktop/e2e/fixture.ts", "desktop/e2e/fixture-cleanup.unit.ts", "desktop/playwright.config.ts", "desktop/vitest.config.ts"):
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), {"policy", "checks", "desktop-e2e", "windows-check"})

    def test_configuration_names_the_lanes_that_read_it(self):
        expected = {
            "web/playwright.config.ts": {"checks", "web-e2e", "windows-e2e"},
            "web/eslint.config.js": {"checks"},
            "web/eslint.e2e.mjs": {"checks"},
            "web/eslint-rules/hide-e2e.mjs": {"checks"},
            "web/scripts/check-e2e-test-size.mjs": {"checks"},
            "web/scripts/gen-types.mjs": {"checks", "web-e2e"},
            "desktop/eslint.config.mjs": {"checks"},
            "desktop/eslint.globals.mjs": {"checks"},
            "desktop/scripts/build.mjs": {"checks", "desktop-e2e", "package"},
            "desktop/scripts/package.mjs": {"checks", "package"},
            "desktop/scripts/smoke-package.mjs": {"checks", "package"},
        }
        for path, lanes in expected.items():
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), lanes | {"policy"})

    def test_everything_else_still_plans_every_lane(self):
        for path in (
            ".github/workflows/pr.yml", ".github/workflows/web-e2e.yml", ".github/workflows/os-contract.yml",
            "scripts/ci-plan.py", "scripts/verify-cargo.sh", "scripts/verify-web.sh", "scripts/ci-flaky-report.py",
            "scripts/fetch-herdr-runtime.sh", "scripts/install-nextest.sh", "scripts/toolchain-env.sh",
            "pnpm-lock.yaml", "web/package.json", "desktop/package.json", "Cargo.lock", ".config/nextest.toml", "clippy.toml",
            "contracts/herdr-bundle.json", "contracts/herdr-api.schema.json", "contracts/hided-ws.schema.json",
            "mystery/file.txt", "scripts/some-new-script.sh", "web/some-new-config.json",
        ):
            with self.subTest(path=path):
                self.assertTrue(plan(path)["full"])
                self.assertEqual(plan(path)["lanes"], list(ci.LANES) if path in PACKAGE_INPUTS else FULL)

    def test_a_crate_readme_is_still_the_crates_change(self):
        # The crate may include it in a doc test; the documentation rule only
        # covers folders no rule claims.
        self.assertTrue({"rust", "windows-check"} <= self.lanes("herdr-core/README.md"))

    def test_a_named_path_does_not_narrow_a_plan_that_has_another_reason(self):
        self.assertEqual(self.lanes("site/index.html", "web/src/Overview.tsx"), self.lanes("web/src/Overview.tsx"))
        self.assertEqual(plan("site/index.html", "Cargo.lock")["lanes"], FULL)

    def test_no_script_named_as_unread_is_called_by_a_lane(self):
        # The claim behind POLICY_ONLY's scripts: nothing but `policy` (and a
        # workflow of its own) names them. `pr.yml`'s policy job may.
        workflows = ROOT / ".github/workflows"
        pr = (workflows / "pr.yml").read_text()
        policy = pr[pr.index("\n  policy:\n"):pr.index("\n  rust:\n")]
        callers = pr.replace(policy, "") + "".join(
            (workflows / name).read_text() for name in ("web-e2e.yml", "os-contract.yml", "package.yml")
        )
        for helper in ("verify-cargo.sh", "verify-web.sh", "toolchain-env.sh", "install-nextest.sh", "fetch-herdr-runtime.sh",
                       "fetch-herdr-runtime.ps1", "ci-flaky-report.py", "check-herdr-schema.py"):
            callers += (ROOT / "scripts" / helper).read_text()
        checked = 0
        for pattern in ci.POLICY_ONLY:
            if not pattern.startswith("scripts/") or pattern.startswith("scripts/tests/"):
                continue
            for found in ROOT.glob(pattern):
                checked += 1
                with self.subTest(script=found.name):
                    self.assertNotIn(found.name, callers)
        self.assertGreater(checked, 8)


def pr_jobs():
    """Each job of `pr.yml`, by id, as the text of its block."""
    workflow = (ROOT / ".github/workflows/pr.yml").read_text()
    body = workflow[workflow.index("\njobs:\n") + len("\njobs:\n"):]
    blocks = re.split(r"\n(?=  [a-z0-9-]+:\n)", "\n" + body)
    return {re.match(r"\s*([a-z0-9-]+):", block).group(1): block for block in blocks if block.strip()}


def runner_jobs(block):
    """How many runner jobs one `pr.yml` job starts: a reusable workflow starts its own."""
    if "uses: ./.github/workflows/web-e2e.yml" in block:
        shards = int(re.search(r"\n      shards: (\d+)", block).group(1))
        # With more than one shard a `build` job precedes them.
        return shards + (1 if shards > 1 else 0)
    if "uses: ./.github/workflows/os-contract.yml" in block:
        return len(json.loads(re.search(r"systems: '(\[[^']*\])'", block).group(1)))
    if "uses: ./.github/workflows/package.yml" in block:
        package = (ROOT / ".github/workflows/package.yml").read_text()
        return len(re.findall(r"\n          - system: ", package)) + ("include-macos: true" in block)
    return 1


class Workflows(unittest.TestCase):
    """`pr.yml` has the jobs the plan names, and the job count TESTING.md states."""

    def test_the_package_lane_is_the_only_way_a_pull_request_reaches_package_yml(self):
        # One list of package paths: `package.yml` has no trigger to keep in step with PACKAGE_PATHS.
        package = (ROOT / ".github/workflows/package.yml").read_text()
        triggers = package[package.index("\non:\n"):package.index("\njobs:\n")]
        self.assertNotIn("pull_request", triggers)
        self.assertNotIn("paths:", triggers)
        # The pull request's call builds the macOS package and runs its specs.
        self.assertIn("include-macos: true", pr_jobs()["package"])

    def test_every_lane_has_a_job_that_asks_the_plan_and_verify_waits_on_it(self):
        jobs = pr_jobs()
        for lane in ci.LANES:
            with self.subTest(lane=lane):
                self.assertIn(f"if: contains(fromJSON(needs.plan.outputs.lanes), '{lane}')", jobs[lane])
        verify = re.search(r"needs: \[([^\]]*)\]", jobs["verify"]).group(1)
        self.assertEqual(sorted(name.strip() for name in verify.split(",")), sorted(["plan", *ci.LANES]))
        self.assertEqual(set(jobs), {"plan", "verify", *ci.LANES})

    def test_the_nightly_runs_each_macos_check_once(self):
        nightly = (ROOT / ".github/workflows/nightly.yml").read_text()
        # The OS contract runs through `verify` (all three legs), not again as a
        # job of its own or a step in the desktop job.
        self.assertNotIn("\n  os-contract:\n", nightly)
        self.assertNotIn("OS contract on macOS", nightly)
        # The macOS web tests are the nightly's own job, not a `verify` lane.
        self.assertIn("\n  web-e2e-macos:\n", nightly)
        self.assertNotIn("web-e2e-platform", nightly)
        # The mailbox lane runs on macOS here and on Linux in `pr.yml`.
        call = nightly[nightly.index("\n  remote-mailbox-macos:\n"):]
        self.assertIn("uses: ./.github/workflows/remote-mailbox.yml", call)
        self.assertIn("runner: macos-15", call)
        self.assertRegex(nightly[nightly.index("\n  report:\n"):], r"needs: \[[^\]]*\bremote-mailbox-macos\b[^\]]*\]")

    def test_the_nightly_runs_only_what_differs_by_system_off_the_systems_that_run_it_whole(self):
        nightly = (ROOT / ".github/workflows/nightly.yml").read_text()
        # Linux's whole web suite is a `verify` lane, so the nightly has no web job for it;
        # macOS and Windows run the web tests tagged `@platform`, one shard each.
        self.assertNotIn("\n  web-e2e-linux:\n", nightly)
        for name in ("web-e2e-macos", "web-e2e-windows"):
            with self.subTest(job=name):
                call = nightly[nightly.index(f"\n  {name}:\n"):].split("\n\n")[0]
                self.assertIn("shards: 1", call)
                self.assertIn('grep: "@platform"', call)
        # The desktop suite runs whole on macOS and only its `@platform` tests elsewhere.
        desktop = nightly[nightly.index("\n  desktop-e2e:\n"):nightly.index("\n  verify:\n")]
        self.assertIn('if [ "$RUNNER_OS" != macOS ]; then', desktop)
        self.assertIn("selection=(--grep @platform)", desktop)
        # A desktop change plans the macOS desktop job; the OS contract's macOS leg does not bring it.
        self.assertIn("desktop-e2e", plan("desktop/e2e/fixture.ts")["lanes"])
        self.assertNotIn("desktop-e2e", plan("hide-platform/src/process.rs")["lanes"])

    def test_only_the_macos_lanes_name_a_macos_runner(self):
        # The macOS jobs are the ones in MACOS_LANES; no other job names a macOS
        # runner, so a plan without those lanes holds no macOS job.
        jobs = pr_jobs()
        for lane, block in jobs.items():
            with self.subTest(lane=lane):
                body = "\n".join(line for line in block.splitlines() if not line.lstrip().startswith("#"))
                names_macos = bool(re.search(r"macos-\d+|\"macos\"|include-macos: true", body))
                self.assertEqual(names_macos, lane in MACOS_LANES)

    def test_the_most_a_pull_request_starts_is_the_count_testing_md_states(self):
        jobs = pr_jobs()
        most = 2 + sum(runner_jobs(jobs[lane]) for lane in ci.LANES)
        self.assertLess(most, 23, "the run before the macOS and merge work started 23 jobs")
        testing = (ROOT / "docs/TESTING.md").read_text()
        self.assertIn(f"starts at most {most} jobs", testing)


class Aggregate(unittest.TestCase):
    def test_planned_success_and_unplanned_skip_pass(self):
        for result in (plan("docs/BUILD.md"), plan("web/src/Overview.tsx"), plan(".github/workflows/pr.yml")):
            ci.aggregate(needs_for(result))

    def test_a_planned_lane_that_did_not_succeed_fails(self):
        result = plan("web/src/shortcuts.ts")
        for outcome in ("skipped", "failure", "cancelled"):
            with self.subTest(outcome=outcome), self.assertRaises(ValueError):
                ci.aggregate(needs_for(result, desktop_e2e=outcome))

    def test_an_unplanned_lane_that_ran_fails(self):
        with self.assertRaises(ValueError):
            ci.aggregate(needs_for(plan("docs/BUILD.md"), rust="success"))

    def test_a_draft_fails_verify_even_when_every_lane_was_skipped(self):
        needs = {lane: {"result": "skipped"} for lane in ci.LANES}
        needs["plan"] = {"result": "success", "outputs": {"lanes": "[]", "draft": "true"}}
        with self.assertRaisesRegex(ValueError, "draft: lanes not run, mark ready for review"):
            ci.aggregate(needs)
        needs["plan"]["outputs"] = {"lanes": json.dumps(["policy"]), "draft": "false"}
        needs["policy"] = {"result": "success"}
        ci.aggregate(needs)

    def test_a_missing_or_unknown_lane_or_a_failed_plan_fails(self):
        needs = needs_for(plan("docs/BUILD.md"))
        del needs["rust"]
        with self.assertRaises(ValueError):
            ci.aggregate(needs)
        with self.assertRaises(ValueError):
            ci.aggregate({**needs_for(plan("docs/BUILD.md")), "extra": {"result": "success"}})
        failed = needs_for(plan("docs/BUILD.md"))
        failed["plan"] = {"result": "failure", "outputs": {}}
        with self.assertRaises(ValueError):
            ci.aggregate(failed)
        empty = needs_for(plan("docs/BUILD.md"))
        empty["plan"]["outputs"]["lanes"] = "[]"
        with self.assertRaises(ValueError):
            ci.aggregate(empty)


class ReverseDependencies(unittest.TestCase):
    def test_consumers_come_from_the_manifests(self):
        crates = {
            "leaf": {"name": "leaf", "consumers": {"middle"}},
            "middle": {"name": "middle", "consumers": {"top"}},
            "top": {"name": "top", "consumers": set()},
            "other": {"name": "other", "consumers": set()},
        }
        self.assertEqual(ci.reverse_closure(crates, "leaf"), {"leaf", "middle", "top"})
        self.assertEqual(ci.reverse_closure(crates, "top"), {"top"})

    def test_the_workspace_graph_has_every_crate(self):
        self.assertIn("hide-platform", CRATES)
        self.assertEqual(CRATES["hide-platform"]["name"], "hide-platform")
        self.assertIn("hided", CRATES["herdr-core"]["consumers"])


if __name__ == "__main__":
    unittest.main()
