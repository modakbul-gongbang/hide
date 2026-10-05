"""What `scripts/ci-flaky-report.py` reads from a report and files as issues."""
from datetime import date
import importlib.util
import json
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

SPEC = importlib.util.spec_from_file_location("ci_flaky_report", Path(__file__).parents[1] / "ci-flaky-report.py")
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

    def test_a_missing_report_is_not_an_error(self):
        self.assertEqual(r.main(["--suite", "web", "--playwright", "/nonexistent/report.json"]), 0)


if __name__ == "__main__":
    unittest.main()
