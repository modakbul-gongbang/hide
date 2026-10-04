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
        result = plan("docs/ARCHITECTURE.md", "CONTRIBUTING.md", "desktop/AGENTS.md")
        self.assertEqual(result["lanes"], ["policy"])
        self.assertEqual(result["rust_packages"], [])

    def test_a_web_display_change_skips_rust_windows_and_desktop(self):
        result = plan("web/src/components/ui/button.tsx", "web/src/Overview.tsx")
        self.assertEqual(set(result["lanes"]), {"policy", "web-checks", "web-e2e"})

    def test_web_code_the_desktop_host_reads_reaches_desktop_and_windows(self):
        for path in ("web/src/shortcuts.ts", "web/src/host.ts", "web/src/store.ts", "web/src/keys.ts"):
            with self.subTest(path=path):
                lanes = set(plan(path)["lanes"])
                self.assertTrue({"web-checks", "web-e2e", "desktop-checks", "desktop-e2e", "windows-e2e"} <= lanes)
                self.assertNotIn("rust", lanes)

    def test_a_platform_spec_keeps_its_platform_lanes(self):
        lanes = set(plan("web/e2e/s3.spec.ts")["lanes"])
        self.assertTrue({"web-e2e", "web-e2e-platform", "windows-e2e"} <= lanes)
        lanes = set(plan("web/e2e/theme.spec.ts")["lanes"])
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

    def test_a_leaf_crate_tests_its_reverse_dependencies_without_windows(self):
        result = plan("hide-session/src/lib.rs")
        self.assertIn("hide-session", result["rust_packages"])
        self.assertIn("hided", result["rust_packages"])
        self.assertNotIn("hide-platform", result["rust_packages"])
        self.assertNotIn("windows-check", result["lanes"])
        self.assertNotIn("windows-e2e", result["lanes"])
        self.assertEqual(plan("hided/src/lib.rs")["rust_packages"], ["hided"])

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
            [("M", "web/e2e/herdr-fixture.ts")],
            [("M", "web/package.json")],
            [("M", "plugins/hcoord/src/cli.ts")],
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
