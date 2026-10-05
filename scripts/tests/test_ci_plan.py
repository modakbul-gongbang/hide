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
E2E = {"web-e2e", "web-e2e-platform", "remote-mailbox", "windows-e2e", "desktop-e2e"}


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
        self.assertTrue({"web-e2e", "web-e2e-platform", "remote-mailbox", "windows-e2e"} <= lanes)
        lanes = set(plan("web/e2e/new-tab.spec.ts")["lanes"])
        self.assertEqual(lanes, {"policy", "web-checks", "web-e2e"})

    def test_the_remote_mailbox_lane_is_planned_exactly_when_the_macos_platform_lane_is(self):
        for path in (
            "web/e2e/s3.spec.ts", "web/e2e/new-tab.spec.ts", "web/src/host.ts", "web/src/Overview.tsx",
            "web/playwright.config.ts", "desktop/src/main/wirePath.ts", "hide-platform/src/process.rs",
            "hide-session/src/lib.rs", "herdr-core/src/lib.rs", "desktop/src/preload/index.ts", "docs/TESTING.md",
        ):
            with self.subTest(path=path):
                lanes = set(plan(path)["lanes"])
                self.assertEqual("remote-mailbox" in lanes, "web-e2e-platform" in lanes)
        self.assertIn("remote-mailbox", plan("web/e2e/gone.spec.ts", status="D")["lanes"])

    def test_desktop_changes_run_the_desktop_lanes(self):
        result = plan("desktop/src/preload/index.ts")
        self.assertEqual(set(result["lanes"]), {"policy", "desktop-checks", "desktop-e2e"})
        self.assertIn("windows-check", plan("desktop/src/main/index.ts")["lanes"])

    def test_a_platform_crate_reaches_every_os_and_its_consumers(self):
        result = plan("hide-platform/src/process.rs")
        self.assertTrue({"rust", "os-contract", "windows-check", "windows-e2e", "desktop-e2e", "web-e2e", "remote-mailbox"} <= set(result["lanes"]))
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

    def test_the_nightly_call_and_a_failed_comparison_run_everything(self):
        self.assertEqual(ci.plan("schedule", "HEAD^1", "HEAD", ROOT)["lanes"], list(ci.LANES))
        result = ci.plan("pull_request", "0" * 40, "HEAD", ROOT)
        self.assertTrue(result["full"])
        self.assertIn("comparison unavailable", result["reasons"]["rust"][0])

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

    def test_a_merge_group_plans_its_own_change_on_the_groups_ahead(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            def git(*args):
                return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True).stdout.strip()
            def group(name, path, on):
                # The queue builds a group commit by merging the pull request
                # onto its base: main, or the group queued ahead.
                git("checkout", "-qb", f"pr-{name}", main)
                (repo / path).parent.mkdir(parents=True, exist_ok=True)
                (repo / path).write_text(f"{name}\n")
                git("add", ".")
                git("commit", "-qm", name)
                git("checkout", "-q", "--detach", on)
                git("merge", "-q", "--no-ff", "-m", f"group {name}", f"pr-{name}")
                return git("rev-parse", "HEAD")
            git("init", "-q", "-b", "main")
            git("config", "user.email", "ci@example.invalid")
            git("config", "user.name", "ci")
            (repo / "docs").mkdir()
            (repo / "docs/a.md").write_text("a\n")
            git("add", ".")
            git("commit", "-qm", "base")
            main = git("rev-parse", "HEAD")
            # Two pull requests queued in order: a desktop change, then docs.
            # The second group's base is the first group's commit, not main.
            first = group("desktop", "desktop/src/preload/index.ts", main)
            second = group("docs", "docs/b.md", first)
            result = ci.plan("merge_group", main, first, repo, CRATES)
            self.assertFalse(result["full"])
            self.assertEqual(set(result["lanes"]), {"policy", "desktop-checks", "desktop-e2e"})
            # The second group checks only its own change; the desktop lanes
            # were checked by the first group's run on the same desktop files.
            self.assertEqual(ci.plan("merge_group", first, second, repo, CRATES)["lanes"], ["policy"])
            # When the first group fails, the queue rebuilds the second on main.
            rebuilt = group("docs-again", "docs/c.md", main)
            self.assertEqual(ci.plan("merge_group", main, rebuilt, repo, CRATES)["lanes"], ["policy"])
            # A base the checkout does not have runs every lane.
            missing = ci.plan("merge_group", "0" * 40, second, repo, CRATES)
            self.assertTrue(missing["full"])
            self.assertIn("comparison unavailable", missing["reasons"]["rust"][0])

    def test_the_merge_queue_runs_verify_from_the_groups_base(self):
        workflow = (ROOT / ".github/workflows/pr.yml").read_text()
        self.assertRegex(workflow, r"\n  merge_group:\n    types: \[checks_requested\]\n")
        self.assertIn("${{ github.event.merge_group.base_sha || 'HEAD^1' }}", workflow)
        self.assertIn('--base "$BASE"', workflow)
        # Only a pull request's newer push cancels a run; a group's ref is its
        # own, and main's runs queue.
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", workflow)

    def test_a_push_the_merge_queue_verified_plans_no_lane(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            def git(*args):
                return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True).stdout.strip()
            def merge(branch, path):
                git("checkout", "-qb", branch, "main")
                (repo / path).parent.mkdir(parents=True, exist_ok=True)
                (repo / path).write_text(f"{branch}\n")
                git("add", ".")
                git("commit", "-qm", branch)
                git("checkout", "-q", "main")
                git("merge", "-q", "--no-ff", "-m", f"merge {branch}", branch)
                return git("rev-parse", "HEAD")
            def answer(*runs):
                path = repo.parent / f"{repo.name}-runs.json"
                path.write_text(json.dumps({"total_count": len(runs), "workflow_runs": list(runs)}))
                return str(path)
            def queue_run(sha, event="merge_group", conclusion="success"):
                return {"event": event, "conclusion": conclusion, "head_sha": sha, "html_url": f"https://example.invalid/runs/{event}"}
            git("init", "-q", "-b", "main")
            git("config", "user.email", "ci@example.invalid")
            git("config", "user.name", "ci")
            (repo / "docs").mkdir()
            (repo / "docs/a.md").write_text("a\n")
            git("add", ".")
            git("commit", "-qm", "base")
            before = git("rev-parse", "HEAD")
            head = merge("docs", "web/src/Overview.tsx")

            verified = ci.plan("push", before, "HEAD", repo, CRATES, queue_runs=answer(queue_run(head)))
            self.assertEqual(verified["lanes"], [])
            self.assertEqual(verified["verified_by"], "https://example.invalid/runs/merge_group")
            self.assertIn("merge queue verified", ci.summary(verified))

            # Anything short of a successful queue run on this very commit runs
            # every lane and says why.
            for runs, reason in (
                (answer(queue_run(before)), "no merge queue run verified"),
                (answer(queue_run(head, event="pull_request")), "no merge queue run verified"),
                (answer(queue_run(head, conclusion="failure")), "no merge queue run verified"),
                (str(repo.parent / "missing.json"), "merge queue lookup failed"),
                (answer(), "no merge queue run verified"),
                (None, "merge queue lookup not given"),
            ):
                with self.subTest(reason=reason, runs=runs):
                    result = ci.plan("push", before, "HEAD", repo, CRATES, queue_runs=runs)
                    self.assertEqual(result["lanes"], list(ci.LANES))
                    self.assertIn(reason, result["reasons"]["policy"][0])
            for base, reason in (("0" * 40, "comparison unavailable"), ("", "no passing push run on main")):
                with self.subTest(base=base):
                    result = ci.plan("push", base, "HEAD", repo, CRATES, queue_runs=answer(queue_run(head)))
                    self.assertEqual(result["lanes"], list(ci.LANES))
                    self.assertIn(reason, result["reasons"]["policy"][0])

            # A cache key input runs every lane even when the queue verified the
            # commit, and the comparison spans every merge since the last
            # passing push run, here two.
            for index, path in enumerate(("Cargo.lock", "herdr-core/Cargo.toml", "pnpm-lock.yaml", ".github/workflows/web-e2e.yml")):
                with self.subTest(path=path):
                    start = git("rev-parse", "HEAD")
                    merge(f"key-{index}", path)
                    head = merge(f"docs-{index}", f"docs/after-{index}.md")
                    result = ci.plan("push", start, "HEAD", repo, CRATES, queue_runs=answer(queue_run(head)))
                    self.assertEqual(result["lanes"], list(ci.LANES))
                    self.assertIn(f"{path} changes a CI cache key", result["reasons"]["policy"][0])
                    self.assertEqual(ci.plan("push", "HEAD^1", "HEAD", repo, CRATES, queue_runs=answer(queue_run(head)))["lanes"], [])

    def test_every_crate_manifest_is_a_cache_key(self):
        # rust-cache hashes each crate's Cargo.toml; a crate a pattern misses
        # would let a push change the key without saving the cache.
        for directory in CRATES:
            manifest = f"{directory}/Cargo.toml" if directory else "Cargo.toml"
            self.assertTrue(any(ci.fnmatchcase(manifest, pattern) for pattern in ci.CACHE_KEYS), manifest)

    def test_main_asks_the_queue_and_nightly_runs_every_lane(self):
        workflow = (ROOT / ".github/workflows/pr.yml").read_text()
        plan_job = workflow[workflow.index("\n  plan:\n"):workflow.index("\n  policy:\n")]
        self.assertIn("      actions: read\n", plan_job)
        self.assertIn("verified-by: ${{ steps.plan.outputs.verified-by }}", plan_job)
        self.assertIn('runs="repos/$GITHUB_REPOSITORY/actions/workflows/pr.yml/runs"', plan_job)
        self.assertIn("$runs?head_sha=$GITHUB_SHA&event=merge_group&status=success", plan_job)
        self.assertIn('queue=(--queue-runs "$answer")', plan_job)
        # A push compares from the last passing push run, so a run GitHub
        # replaced while it waited still has its merges checked.
        self.assertIn("$runs?branch=main&event=push&status=success&per_page=1", plan_job)
        self.assertNotIn("github.event.before", plan_job)
        # A called run takes the caller's name, so nightly's never shares a
        # concurrency group with a push to main.
        self.assertRegex(workflow, r"\n  workflow_call:\n")
        self.assertIn("group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}", workflow)
        nightly = (ROOT / ".github/workflows/nightly.yml").read_text()
        call = nightly[nightly.index("\n  verify:\n"):nightly.index("\n  package:\n")]
        self.assertIn("uses: ./.github/workflows/pr.yml", call)
        for permission in ("contents: read", "issues: write", "actions: read"):
            self.assertIn(permission, call)
        report = nightly[nightly.index("\n  report:\n"):]
        self.assertRegex(report, r"needs: \[[^\]]*\bverify\b[^\]]*\]")


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
        want = {"policy", "web-checks", "web-e2e", "web-e2e-platform", "remote-mailbox", "windows-e2e", "desktop-checks", "desktop-e2e", "windows-check"}
        for path in ("web/e2e/herdr-fixture.ts", "web/e2e/shims/build.ts", "web/e2e/shims/noop.c", "web/e2e/test-size-baseline.json"):
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), want)

    def test_desktop_e2e_helpers_run_the_desktop_suites_and_windows_unit_tests(self):
        for path in ("desktop/e2e/fixture.ts", "desktop/e2e/fixture-cleanup.unit.ts", "desktop/playwright.config.ts", "desktop/vitest.config.ts"):
            with self.subTest(path=path):
                self.assertEqual(self.lanes(path), {"policy", "desktop-checks", "desktop-e2e", "windows-check"})

    def test_configuration_names_the_lanes_that_read_it(self):
        expected = {
            "web/playwright.config.ts": {"web-checks", "web-e2e", "web-e2e-platform", "remote-mailbox", "windows-e2e"},
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

    def test_a_draft_fails_verify_even_when_every_lane_was_skipped(self):
        needs = {lane: {"result": "skipped"} for lane in ci.LANES}
        needs["plan"] = {"result": "success", "outputs": {"lanes": "[]", "draft": "true"}}
        with self.assertRaisesRegex(ValueError, "draft: lanes not run, mark ready for review"):
            ci.aggregate(needs)
        needs["plan"]["outputs"] = {"lanes": json.dumps(["policy"]), "draft": "false"}
        needs["policy"] = {"result": "success"}
        ci.aggregate(needs)

    def test_a_verified_push_passes_only_with_every_lane_skipped(self):
        needs = {lane: {"result": "skipped"} for lane in ci.LANES}
        needs["plan"] = {"result": "success", "outputs": {"lanes": "[]", "verified-by": "https://example.invalid/runs/1"}}
        ci.aggregate(needs)
        with self.assertRaises(ValueError):
            ci.aggregate({**needs, "rust": {"result": "success"}})
        planned = {**needs, "policy": {"result": "success"}}
        planned["plan"] = {"result": "success", "outputs": {"lanes": json.dumps(["policy"]), "verified-by": "https://example.invalid/runs/1"}}
        with self.assertRaisesRegex(ValueError, "a verified push planned lanes"):
            ci.aggregate(planned)

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
