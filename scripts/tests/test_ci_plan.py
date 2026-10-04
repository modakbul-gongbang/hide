import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("ci_plan", Path(__file__).parents[1] / "ci-plan.py")
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)
ROOT = Path(__file__).parents[2]


class SelectionContract(unittest.TestCase):
    def plan(self, entries):
        return ci.select(ROOT, entries, {"head": "a" * 40})

    def test_docs_only_exempts_execution_but_has_a_required_check(self):
        plan = self.plan([("M", "docs/ARCHITECTURE.md")])
        self.assertEqual([k for k, v in plan["lanes"].items() if v], ["docs"])
        results = {k: {"result": "success" if v else "skipped"} for k, v in plan["lanes"].items()}
        results["plan"] = {"result": "success"}
        ci.aggregate(plan, results)
        for bad in ("cancelled", "failure", "unknown", "skipped", None):
            with self.subTest(result=bad), self.assertRaises(ValueError):
                ci.aggregate(plan, {**results, "docs": {"result": bad}})
        with self.assertRaises(ValueError):
            ci.aggregate(plan, {k: v for k, v in results.items() if k != "rust"})
        with self.assertRaises(ValueError):
            ci.aggregate(plan, {**results, "unrecognized": {"result": "success"}})
        with self.assertRaises(ValueError):
            ci.aggregate(plan, {**results, "rust": {"result": "success"}})

    def test_unsafe_comparisons_never_remove_coverage(self):
        for entries in ([], [("D", "docs/BUILD.md")], [("A", "web/new-script.ts")], [("R100", "hide-ai/src/lib.rs")], [("M", "mystery/file.md")], [("M", "Cargo.lock")], [("M", ".github/workflows/pr.yml")]):
            with self.subTest(entries=entries):
                self.assertTrue(all(self.plan(entries)["lanes"].values()))
        self.assertTrue(all(ci.select(ROOT, [], {}, "failed diff")["lanes"].values()))

    def test_tests_and_shared_ownership_are_execution_payload(self):
        for path in ("desktop/e2e/fixture.ts", "hide-platform/tests/ipc.rs", "hide-kit/src/hcoord.rs", "hide-agent-hooks/src/lib.rs"):
            with self.subTest(path=path):
                self.assertTrue(all(self.plan([("M", path)])["lanes"].values()))

    def test_display_and_host_changes_select_their_packages(self):
        web = self.plan([("M", "web/src/components/Welcome.tsx")])
        self.assertTrue(web["lanes"]["web-checks"])
        self.assertTrue(web["lanes"]["web-e2e"])
        self.assertFalse(web["lanes"]["windows-e2e"])
        self.assertFalse(web["lanes"]["rust"])
        desktop = self.plan([("M", "desktop/src/preload.ts")])
        self.assertTrue(desktop["lanes"]["desktop-checks"])
        self.assertTrue(desktop["lanes"]["desktop-e2e"])
        self.assertFalse(desktop["lanes"]["web-e2e"])
        self.assertFalse(desktop["full"])
        shared = self.plan([("M", "web/src/store.ts")])
        self.assertTrue(shared["lanes"]["desktop-e2e"])
        self.assertTrue(shared["lanes"]["windows-e2e"])
        for path in ("web/src/host.ts", "web/src/snapshot.ts", "web/src/shortcuts.ts"):
            plan = self.plan([("M", path)])
            for consumer in ("web-checks", "web-e2e", "desktop-checks", "desktop-e2e", "windows-check", "windows-e2e"):
                self.assertTrue(plan["lanes"][consumer], f"omitted {consumer} for {path}")
            reports = {k: {"result": "success" if v else "skipped"} for k, v in plan["lanes"].items()}
            reports["plan"] = {"result": "success"}
            ci.aggregate(plan, reports)
            with self.assertRaises(ValueError):
                ci.aggregate(plan, {**reports, "desktop-e2e": {"result": "skipped"}})

    def test_any_full_reason_overrides_a_partial_rust_package_set(self):
        expected = sorted(m["package"]["name"] for m in ci.cargo_graph(ROOT)[0].values())
        for entries in ([('M', 'hide-ai/src/lib.rs'), ('M', '.github/workflows/pr.yml')], [('M', 'hide-ai/src/lib.rs'), ('D', 'docs/BUILD.md')], [('M', 'herdr-core/src/lib.rs')]):
            self.assertEqual(self.plan(entries)["rust_packages"], expected)

    def test_reverse_dependencies_come_from_manifests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[workspace]\nmembers=["leaf","consumer","independent"]\n')
            for member in ("leaf", "consumer", "independent"):
                (root / member).mkdir()
                manifest = f'[package]\nname="{member}"\nversion="0.1.0"\n'
                if member == "consumer":
                    manifest += '[target."cfg(unix)".dev-dependencies]\nrenamed={package="leaf",path="../leaf"}\n'
                (root / member / "Cargo.toml").write_text(manifest)
            plan = ci.select(root, [("M", "leaf/tests/contract.rs")], {})
            self.assertEqual(plan["rust_packages"], ["consumer", "leaf"])
            self.assertTrue(plan["lanes"]["rust"])
            self.assertFalse(plan["lanes"]["desktop-e2e"])

    def test_schema_and_empty_selection_are_refused(self):
        for plan in ({"version": 999}, {"version": ci.VERSION, "lanes": dict.fromkeys(ci.LANES, False)}):
            with self.assertRaises(ValueError):
                ci.aggregate(plan, {})
