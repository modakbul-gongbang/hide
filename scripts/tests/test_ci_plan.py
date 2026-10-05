"""Which lanes `scripts/ci-plan.py` plans for real changes, and what `verify` accepts."""
import importlib.util
import json
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
E2E = {"web-e2e", "web-e2e-platform", "windows-e2e", "desktop-e2e"}


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
        self.assertTrue({"web-checks", "web-e2e"} <= set(plan("design/tokens.json")["lanes"]))

    def test_a_web_display_change_skips_windows_and_desktop(self):
        result = plan("web/src/components/ui/button.tsx", "web/src/Overview.tsx")
        # herdr-core's structure tests scan the shell's sources.
        self.assertEqual(set(result["lanes"]), {"policy", "web-checks", "web-e2e", "rust"})
        self.assertEqual(result["rust_packages"], ["herdr-core"])

    def test_web_code_the_desktop_host_reads_reaches_desktop_and_windows(self):
        for path in ("web/src/shortcuts.ts", "web/src/host.ts", "web/src/store.ts", "web/src/keys.ts"):
            with self.subTest(path=path):
                lanes = set(plan(path)["lanes"])
                self.assertTrue({"web-checks", "web-e2e", "desktop-checks", "desktop-e2e", "windows-e2e"} <= lanes)
                self.assertNotIn("windows-check", lanes)

    def test_a_platform_spec_keeps_its_platform_lanes(self):
        lanes = set(plan("web/e2e/s3.spec.ts")["lanes"])
        self.assertTrue({"web-e2e", "web-e2e-platform", "windows-e2e"} <= lanes)
        lanes = set(plan("web/e2e/new-tab.spec.ts")["lanes"])
        self.assertEqual(lanes, {"policy", "web-checks", "web-e2e"})

    def test_desktop_changes_run_the_desktop_lanes(self):
        result = plan("desktop/src/preload/index.ts")
        self.assertEqual(set(result["lanes"]), {"policy", "desktop-checks", "desktop-e2e"})
        self.assertIn("windows-check", plan("desktop/src/main/index.ts")["lanes"])

    def test_a_platform_crate_reaches_every_os_and_its_consumers(self):
        result = plan("hide-platform/src/process.rs")
        self.assertTrue({"rust", "os-contract", "windows-check", "windows-e2e", "desktop-e2e", "web-e2e"} <= set(result["lanes"]))
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

    def test_the_os_contract_brings_its_macos_leg(self):
        result = plan("hide-herdr-client/src/lib.rs")
        self.assertIn("os-contract", result["lanes"])
        self.assertIn("desktop-e2e", result["lanes"])

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
                self.assertEqual(result["lanes"], list(ci.LANES))
                self.assertEqual(result["rust_packages"], EVERY_PACKAGE)

    def test_a_push_and_a_failed_comparison_run_everything(self):
        self.assertEqual(ci.plan("push", None, None, ROOT)["lanes"], list(ci.LANES))
        result = ci.plan("pull_request", "0" * 40, "HEAD", ROOT)
        self.assertTrue(result["full"])
        self.assertIn("comparison unavailable", result["reasons"]["rust"][0])

    def test_a_missing_crate_graph_runs_everything(self):
        with tempfile.TemporaryDirectory() as directory:
            result = ci.plan("pull_request", "HEAD^1", "HEAD", Path(directory))
            self.assertEqual(result["lanes"], list(ci.LANES))
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
            ".github/workflows/nightly.yml", ".github/workflows/package.yml", ".github/workflows/release.yml",
            "scripts/tests/test_ci_plan.py", "scripts/nightly-report.cjs", "scripts/check-harness-ignore-anchor.sh",
            "scripts/pen-system.mjs", "scripts/design-review.mjs", "scripts/web-shell-measure/run.sh",
            "contracts/README.md", "site/README.md",
        ):
            with self.subTest(path=path):
                result = plan(path)
                self.assertEqual(result["lanes"], ["policy"])
                self.assertFalse(result["full"])

    def test_web_e2e_helpers_reach_the_desktop_and_windows_lanes_but_not_rust(self):
        want = {"policy", "web-checks", "web-e2e", "web-e2e-platform", "windows-e2e", "desktop-checks", "desktop-e2e", "windows-check"}
        for path in ("web/e2e/herdr-fixture.ts", "web/e2e/shims/build.ts", "web/e2e/shims/noop.c", "web/e2e/test-size-baseline.json"):
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), want)

    def test_desktop_e2e_helpers_run_the_desktop_suites_and_windows_unit_tests(self):
        for path in ("desktop/e2e/fixture.ts", "desktop/e2e/fixture-cleanup.unit.ts", "desktop/playwright.config.ts", "desktop/vitest.config.ts"):
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), {"policy", "desktop-checks", "desktop-e2e", "windows-check"})

    def test_configuration_names_the_lanes_that_read_it(self):
        expected = {
            "web/playwright.config.ts": {"web-checks", "web-e2e", "web-e2e-platform", "windows-e2e"},
            "web/eslint.config.js": {"web-checks", "desktop-checks"},
            "web/eslint.e2e.mjs": {"web-checks", "desktop-checks"},
            "web/eslint-rules/hide-e2e.mjs": {"web-checks", "desktop-checks"},
            "web/scripts/check-e2e-test-size.mjs": {"web-checks", "desktop-checks"},
            "web/scripts/gen-types.mjs": {"web-checks", "web-e2e"},
            "desktop/eslint.config.mjs": {"desktop-checks"},
            "desktop/eslint.globals.mjs": {"desktop-checks"},
            "desktop/scripts/build.mjs": {"desktop-checks", "desktop-e2e"},
            "desktop/scripts/package.mjs": {"desktop-checks"},
            "desktop/scripts/smoke-package.mjs": {"desktop-checks"},
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
                self.assertEqual(plan(path)["lanes"], list(ci.LANES))

    def test_a_crate_readme_is_still_the_crates_change(self):
        # The crate may include it in a doc test; the documentation rule only
        # covers folders no rule claims.
        self.assertTrue({"rust", "windows-check"} <= self.lanes("herdr-core/README.md"))

    def test_a_named_path_does_not_narrow_a_plan_that_has_another_reason(self):
        self.assertEqual(self.lanes("site/index.html", "web/src/Overview.tsx"), self.lanes("web/src/Overview.tsx"))
        self.assertEqual(plan("site/index.html", "scripts/verify-cargo.sh")["lanes"], list(ci.LANES))

    def test_no_script_named_as_unread_is_called_by_a_lane(self):
        # The claim behind POLICY_ONLY's scripts: nothing but `policy` (and a
        # workflow of its own) names them. `pr.yml`'s policy job may.
        workflows = ROOT / ".github/workflows"
        pr = (workflows / "pr.yml").read_text()
        policy = pr[pr.index("\n  policy:\n"):pr.index("\n  rust:\n")]
        callers = pr.replace(policy, "") + (workflows / "web-e2e.yml").read_text() + (workflows / "os-contract.yml").read_text()
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
