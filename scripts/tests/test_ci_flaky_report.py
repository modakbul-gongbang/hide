"""What `scripts/ci-flaky-report.py` reads from a report and files as issues."""
from datetime import date
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

SCRIPT = Path(__file__).parents[1] / "ci-flaky-report.py"
SPEC = importlib.util.spec_from_file_location("ci_flaky_report", SCRIPT)
r = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(r)

TODAY = date(2026, 10, 5)
ENV = {
    "GITHUB_SERVER_URL": "https://github.com", "GITHUB_REPOSITORY": "o/r", "GITHUB_RUN_ID": "77",
    "GITHUB_RUN_ATTEMPT": "1", "GITHUB_REF": "refs/pull/9/merge", "GITHUB_SHA": "abcdef0123456789",
}


def spec(title, status, message="boom"):
    return {"title": title, "file": "a.spec.ts", "tests": [{"status": status, "results": [{"error": {"message": message}}]}]}


class Reading(unittest.TestCase):
    def test_only_a_flaky_playwright_test_is_listed_with_its_describe_path(self):
        report = {"suites": [{"title": "a.spec.ts", "specs": [spec("ok", "expected"), spec("bad", "unexpected"), spec("wobbles", "flaky")],
                              "suites": [{"title": "group", "specs": [spec("inner", "flaky")]}]}]}
        found = r.playwright_flaky(report, "web")
        self.assertEqual([(t["file"], t["name"]) for t in found], [("web/e2e/a.spec.ts", "wobbles"), ("web/e2e/a.spec.ts", "group > inner")])

    def test_only_a_flaky_nextest_case_is_listed(self):
        root = ET.fromstring(
            '<testsuites><testsuite name="p::t">'
            '<testcase name="steady" classname="p::t"/>'
            '<testcase name="wobbles" classname="p::t"><flakyFailure message="m">panicked at x</flakyFailure></testcase>'
            "</testsuite></testsuites>"
        )
        self.assertEqual(r.nextest_flaky(root), [{"file": None, "name": "p::t wobbles", "error": "panicked at x"}])

    def test_a_nextest_case_that_hung_once_is_listed_as_a_timeout(self):
        # The shape cargo-nextest 0.9.143 writes for a test ended by slow-timeout that passed its retry.
        root = ET.fromstring(
            '<testsuites><testsuite name="p">'
            '<testcase name="hangs" classname="p"><flakyFailure time="120.003" type="test timeout">'
            "\n<system-out>running 1 test\n</system-out><system-err></system-err></flakyFailure></testcase>"
            "</testsuite></testsuites>"
        )
        self.assertEqual(r.nextest_flaky(root), [{"file": None, "name": "p hangs", "error": "test timeout"}])


class Matching(unittest.TestCase):
    TEST = {"file": "web/e2e/tab-strip-fit.spec.ts", "name": "Agent tabs shrink in stages, the selected one keeping its title longest", "error": ""}

    def test_a_hand_written_issue_with_a_cut_title_tracks_the_test(self):
        body = '`web/e2e/tab-strip-fit.spec.ts:62`("Agent tabs shrink in stages...") flakes'
        self.assertTrue(r.tracks({"body": body}, self.TEST))

    def test_another_file_or_another_test_is_not_tracked(self):
        self.assertFalse(r.tracks({"body": '`web/e2e/other.spec.ts` "Agent tabs shrink in stages"'}, self.TEST))
        self.assertFalse(r.tracks({"body": "`web/e2e/tab-strip-fit.spec.ts` something else"}, self.TEST))

    def test_a_rust_test_is_tracked_by_its_whole_name(self):
        test = {"file": None, "name": "hide-platform::ipc a_burst", "error": ""}
        self.assertTrue(r.tracks({"body": "`hide-platform::ipc a_burst` flakes"}, test))
        self.assertFalse(r.tracks({"body": "`hide-platform::ipc another`"}, test))


class Filing(unittest.TestCase):
    def setUp(self):
        self.calls = []

    def call(self, existing=(), comments=()):
        def call(args, body=None):
            self.calls.append((args, body))
            path = args[0]
            if "/issues?labels=" in path:
                return list(existing)
            if path.endswith("/comments?per_page=100"):
                return list(comments)
            if path.endswith("/issues"):
                return {"number": 50, "html_url": "https://example/50", "body": body["body"]}
            return {}
        return call

    def test_a_new_flaky_test_is_filed_with_run_pull_request_and_a_week(self):
        test = {"file": "web/e2e/a.spec.ts", "name": "wobbles", "error": "\x1b[31mboom\x1b[0m"}
        r.report([test], "Linux", ENV, self.call(), TODAY)
        (args, body), = [c for c in self.calls if c[0][0].endswith("/issues")]
        self.assertEqual(body["labels"], ["quarantine", "bug"])
        for text in ("actions/runs/77", "pull request #9", "abcdef012345", "Expires: 2026-10-12", "boom", "Linux"):
            self.assertIn(text, body["body"])
        self.assertNotIn("\x1b", body["body"])

    def test_a_tracked_test_gets_a_comment_and_no_second_issue(self):
        test = {"file": "web/e2e/a.spec.ts", "name": "wobbles", "error": "boom"}
        issue = {"number": 5, "body": '`web/e2e/a.spec.ts` "wobbles"'}
        r.report([test], "Linux", ENV, self.call([issue]), TODAY)
        posts = [c for c in self.calls if c[1] is not None]
        self.assertEqual([c[0][0] for c in posts], ["repos/o/r/issues/5/comments"])

    def test_the_same_run_is_not_added_twice(self):
        test = {"file": "web/e2e/a.spec.ts", "name": "wobbles", "error": "boom"}
        issue = {"number": 5, "body": '`web/e2e/a.spec.ts` "wobbles"'}
        r.report([test], "Linux", ENV, self.call([issue], [{"body": "<!-- flaky-run:77:1 -->"}]), TODAY)
        self.assertEqual([c for c in self.calls if c[1] is not None], [])

    def test_a_flake_past_its_expiry_says_so(self):
        test = {"file": "web/e2e/a.spec.ts", "name": "wobbles", "error": "boom"}
        issue = {"number": 5, "body": '`web/e2e/a.spec.ts` "wobbles"\n- Expires: 2026-10-01\n'}
        r.report([test], "Linux", ENV, self.call([issue]), TODAY)
        self.assertIn("past its expiry", [c for c in self.calls if c[1] is not None][0][1]["body"])

    def test_a_run_files_at_most_a_cap(self):
        tests = [{"file": None, "name": f"t{n}", "error": ""} for n in range(r.MAX_PER_RUN + 5)]
        r.report(tests, "Linux", ENV, self.call(), TODAY)
        self.assertEqual(len([c for c in self.calls if c[0][0].endswith("/issues")]), r.MAX_PER_RUN)

    def test_a_failed_filing_leaves_the_tests_in_the_job_summary(self):
        import os, tempfile
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        report = Path(folder.name) / "junit.xml"
        report.write_text('<testsuites><testsuite name="p"><testcase name="wobbles" classname="p"><flakyFailure message="m">boom</flakyFailure></testcase></testsuite></testsuites>')
        summary = Path(folder.name) / "summary.md"
        saved = dict(os.environ)
        os.environ.update({"GITHUB_STEP_SUMMARY": str(summary), "PATH": "/nonexistent"})
        self.addCleanup(lambda: (os.environ.clear(), os.environ.update(saved)))
        os.environ.update(ENV)
        self.assertEqual(r.main(["--suite", "rust", "--junit", str(report), "--system", "Linux"]), 1)
        self.assertIn("p wobbles", summary.read_text())
        self.assertIn("not filed", summary.read_text())

    def test_several_junit_reports_are_read_together(self):
        import tempfile
        folder = Path(tempfile.mkdtemp())
        for n in (1, 2):
            (folder / f"junit.{n}.xml").write_text(f'<testsuites><testsuite name="p"><testcase name="t{n}" classname="p"><flakyFailure message="m">x</flakyFailure></testcase></testsuite></testsuites>')
        seen = []
        saved, r.report = r.report, lambda tests, system, env, **kw: seen.extend(t["name"] for t in tests)
        self.addCleanup(setattr, r, "report", saved)
        self.assertEqual(r.main(["--suite", "rust", "--junit", str(folder / "junit.1.xml"), str(folder / "junit.2.xml")]), 0)
        self.assertEqual(seen, ["p t1", "p t2"])

    def test_a_missing_report_is_not_an_error(self):
        self.assertEqual(r.main(["--suite", "web", "--playwright", "/nonexistent/report.json"]), 0)

    def test_only_a_run_with_a_flaky_test_says_so_in_its_step_output(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        output = Path(folder.name) / "output"
        saved = dict(os.environ)
        os.environ["GITHUB_OUTPUT"] = str(output)
        self.addCleanup(lambda: (os.environ.clear(), os.environ.update(saved)))
        saved_report, r.report = r.report, lambda *args, **kwargs: None
        self.addCleanup(setattr, r, "report", saved_report)
        for name, status in (("steady", "expected"), ("failed", "unexpected"), ("flaky", "flaky")):
            report = Path(folder.name) / f"{name}.json"
            report.write_text(json.dumps({"suites": [{"title": "a.spec.ts", "specs": [spec(name, status)]}]}))
            self.assertEqual(r.main(["--suite", "web", "--playwright", str(report)]), 0)
            self.assertEqual(output.read_text() if output.exists() else "", "flaky=true\n" if status == "flaky" else "")


class Workflows(unittest.TestCase):
    def test_every_e2e_log_upload_keeps_a_flaky_run_as_it_keeps_a_failed_one(self):
        # A flaky run passed, but its first attempt failed, and the daemon,
        # Herdr and input logs of that attempt are the only evidence its issue
        # gets. Each job that reports flaky Playwright tests and keeps those
        # logs reads the output of its own report.
        uploads = 0
        for workflow in sorted((SCRIPT.parents[1] / ".github" / "workflows").glob("*.yml")):
            for job in re.split(r"\n  (?=[\w-]+:\n)", workflow.read_text()):
                steps = re.split(r"\n      - ", job)
                if not any(re.search(r"ci-flaky-report\.py --suite (web|desktop)", step) for step in steps):
                    continue
                reports = [step for step in steps if "ci-flaky-report.py" in step and "\n        id: flaky\n" in step]
                for step in steps:
                    if "upload-artifact" not in step or "hide-e2e" not in step:
                        continue
                    uploads += 1
                    self.assertIn("if: ${{ failure() || steps.flaky.outputs.flaky == 'true' }}", step, workflow.name)
                    self.assertEqual(len(reports), 1, f"{workflow.name}: the job's report step has id flaky")
        self.assertEqual(uploads, 3)


class Gh(unittest.TestCase):
    def test_an_answer_is_read_as_utf8_whatever_the_runner_locale(self):
        # A child's text is read in the locale's encoding unless one is named, which is
        # cp1252 on a Windows runner: an open issue with a Korean title broke every report there.
        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp) / "gh"
            fake.write_text("#!/bin/sh\nprintf '[{\"title\": \"\\355\\203\\255\"}]'\n")
            fake.chmod(0o755)
            load = (
                "import importlib.util, sys; s = importlib.util.spec_from_file_location('r', sys.argv[1]); "
                "m = importlib.util.module_from_spec(s); s.loader.exec_module(m); print(ascii(m.gh(['x'])))"
            )
            done = subprocess.run(
                [sys.executable, "-X", "utf8=0", "-c", load, str(SCRIPT)],
                env={"PATH": f"{tmp}{os.pathsep}{os.environ['PATH']}", "LC_ALL": "C", "PYTHONCOERCECLOCALE": "0"},
                capture_output=True, text=True, encoding="utf-8",
            )
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), ascii([{"title": "탭"}]))


if __name__ == "__main__":
    unittest.main()
