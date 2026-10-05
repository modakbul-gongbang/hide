#!/usr/bin/env python3
"""Turn the flaky tests of one CI run into GitHub issues.

Playwright and cargo-nextest retry a failed test once in CI and classify a test
that passes the retry as flaky; the run passes. Both write that classification
into a report (Playwright's JSON, nextest's JUnit), and this script is the only
thing built on top: it reads the report and files each flaky test as an issue
labelled `quarantine` with a seven-day deadline, or adds the run to the open
issue that already tracks the test. docs/TESTING.md, "Flaky tests", owns the
policy.

    ci-flaky-report.py --suite web --playwright web/e2e-report.json
    ci-flaky-report.py --suite rust --junit target/nextest/ci/junit.xml

It talks to GitHub through `gh` (GH_TOKEN, GITHUB_REPOSITORY and the run's
GITHUB_* variables come from Actions) and needs `issues: write`. A step that
runs it must not fail the lane: the flaky run already passed, and the report is
a notice, so a GitHub error costs one missed issue the next flaky run files.
"""
import argparse
from datetime import date, timedelta
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

LABEL = "quarantine"
EXPIRY_DAYS = 7
# A run with more flaky tests than this is a broken environment, not flakiness;
# the rest are counted in the log, not filed.
MAX_PER_RUN = 20
ANSI = re.compile(r"\x1b\[[0-9;]*m")
# Hand-written issues quote a title cut short ("Agent tabs shrink in stages...").
MATCH_PREFIX = 20


def playwright_flaky(report, suite):
    found = []

    def walk(node, trail):
        for spec in node.get("specs", []):
            for test in spec.get("tests", []):
                if test.get("status") != "flaky":
                    continue
                first = (test.get("results") or [{}])[0]
                found.append({
                    "file": f"{suite}/e2e/{spec['file']}",
                    "name": " > ".join(trail + [spec["title"]]),
                    "error": (first.get("error") or {}).get("message", ""),
                })
        for child in node.get("suites", []):
            walk(child, trail + [child["title"]])

    for top in report.get("suites", []):
        walk(top, [])
    return found


def nextest_flaky(root):
    found = []
    for case in root.iter("testcase"):
        flake = case.find("flakyFailure")
        if flake is None:
            flake = case.find("flakyError")
        if flake is not None:
            found.append({
                "file": None,
                "name": f"{case.get('classname')} {case.get('name')}",
                "error": (flake.text or "").strip() or flake.get("message") or "",
            })
    return found


def clean(text):
    return ANSI.sub("", text).strip()[:600] or "(no message)"


def tracks(issue, test):
    """Whether an open issue is about this test: its file and the start of its name appear in the body."""
    body = issue.get("body") or ""
    if test["file"] and test["file"] not in body:
        return False
    name = test["name"] if not test["file"] else test["name"].split(" > ")[-1][:MATCH_PREFIX]
    return name in body


def run_facts(env):
    server, repo, run = env["GITHUB_SERVER_URL"], env["GITHUB_REPOSITORY"], env["GITHUB_RUN_ID"]
    pull = re.fullmatch(r"refs/pull/(\d+)/merge", env.get("GITHUB_REF", ""))
    return {
        "marker": f"<!-- flaky-run:{run}:{env.get('GITHUB_RUN_ATTEMPT', '1')} -->",
        "run": f"{server}/{repo}/actions/runs/{run}",
        "change": f"pull request #{pull.group(1)}" if pull else env.get("GITHUB_REF", "unknown ref"),
        "sha": env.get("GITHUB_SHA", "unknown")[:12],
    }


def label_of(test):
    return f"`{test['file']}` \"{test['name']}\"" if test["file"] else f"`{test['name']}`"


def new_issue(test, system, facts, today):
    expires = (today + timedelta(days=EXPIRY_DAYS)).isoformat()
    return {
        "title": f"Flaky test: {test['name']}"[:200],
        "labels": [LABEL, "bug"],
        "body": (
            "CI found this test flaky: it failed, then passed on the one retry in the same job, so the run passed.\n\n"
            f"- Test: {label_of(test)}\n- System: {system}\n"
            f"- First seen: {facts['run']} ({facts['change']}, commit {facts['sha']})\n- Expires: {expires}\n\n"
            f"First attempt's failure:\n\n```\n{clean(test['error'])}\n```\n\n"
            "Fix the cause or delete the test by the expiry date; a flaky test is not left running. "
            "A longer timeout, more retries or a lower expectation does not end this issue. "
            "Each later flaky run adds a comment here.\n"
        ),
    }


def comment(test, system, facts, expired):
    late = "\n\nThis issue is past its expiry date. Decide now: fix it, delete the test, or say here why not." if expired else ""
    return (
        f"{facts['marker']}\nFlaky again on {system}: {facts['run']} ({facts['change']}, commit {facts['sha']}).\n\n"
        f"```\n{clean(test['error'])}\n```{late}"
    )


def gh(args, body=None):
    done = subprocess.run(
        ["gh", "api", *args], input=None if body is None else json.dumps(body), capture_output=True, text=True,
    )
    if done.returncode:
        raise RuntimeError(f"gh api {' '.join(args)}: {done.stderr.strip()}")
    return json.loads(done.stdout) if done.stdout.strip() else None


def open_issues(repo, call=gh):
    issues = call([f"repos/{repo}/issues?labels={LABEL}&state=open&per_page=100"])
    if len(issues) == 100:
        print("::warning::100 open quarantine issues; later ones are not searched for a duplicate")
    return [issue for issue in issues if "pull_request" not in issue]


def report(tests, system, env, call=gh, today=None):
    today = today or date.today()
    repo, facts = env["GITHUB_REPOSITORY"], run_facts(env)
    if len(tests) > MAX_PER_RUN:
        print(f"::warning::{len(tests)} flaky tests in one run; filing the first {MAX_PER_RUN}")
    issues = open_issues(repo, call)
    for test in tests[:MAX_PER_RUN]:
        issue = next((item for item in issues if tracks(item, test)), None)
        if issue is None:
            created = call([f"repos/{repo}/issues", "--input", "-"], new_issue(test, system, facts, today))
            issues.append({**created, "body": created.get("body") or new_issue(test, system, facts, today)["body"]})
            print(f"filed {created['html_url']} for {test['name']}")
            continue
        number = issue["number"]
        comments = call([f"repos/{repo}/issues/{number}/comments?per_page=100"])
        if any(facts["marker"] in (item.get("body") or "") for item in comments):
            continue
        match = re.search(r"^- Expires: (\d{4}-\d\d-\d\d)", issue.get("body") or "", re.M)
        expired = bool(match) and date.fromisoformat(match.group(1)) < today
        call([f"repos/{repo}/issues/{number}/comments", "--input", "-"], {"body": comment(test, system, facts, expired)})
        print(f"added the run to #{number} for {test['name']}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--suite", required=True, choices=("web", "desktop", "rust"))
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--playwright", type=Path, help="a Playwright JSON report")
    source.add_argument("--junit", type=Path, help="a cargo-nextest JUnit report")
    parser.add_argument("--system", default=os.environ.get("RUNNER_OS", "unknown"))
    args = parser.parse_args(argv)
    path = args.playwright or args.junit
    if not path.is_file():
        print(f"no report at {path}; nothing to file")
        return 0
    tests = playwright_flaky(json.loads(path.read_text(encoding="utf-8")), args.suite) if args.playwright else nextest_flaky(ET.parse(path).getroot())
    if not tests:
        print("no flaky test in this run")
        return 0
    try:
        report(tests, args.system, os.environ)
    except (RuntimeError, KeyError, ValueError) as error:
        print(f"::warning::the flaky report failed, so {len(tests)} flaky test(s) are not filed: {error}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
