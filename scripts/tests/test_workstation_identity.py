"""Exercise the public privacy boundary in real temporary Git repositories."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


CHECK = Path(__file__).resolve().parents[1] / "check-no-workstation-identity.py"


def home(platform, account):
    roots = {"macos": "/Users/", "linux": "/home/", "windows": "C:\\Users\\"}
    return roots[platform] + account


class WorkstationIdentityTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git("init", "--quiet")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], check=True,
                              capture_output=True)

    def tracked(self, path, contents):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(contents.encode() if isinstance(contents, str) else contents)
        self.git("add", "-f", "--", path)

    def check(self, scope="checkout"):
        return subprocess.run([sys.executable, str(CHECK), "--scope", scope],
                              cwd=self.root, capture_output=True, text=True)

    def classes(self, result):
        return [json.loads(line)["class"] for line in result.stderr.splitlines()]

    def test_explicit_neutral_accounts_pass_both_scopes(self):
        accounts = ("example", "alice", "al", "me", "remote", "u")
        self.tracked("fixture.txt", "\n".join(home(p, a) + "/project"
                                              for p in ("macos", "linux", "windows")
                                              for a in accounts))
        for scope in ("checkout", "index"):
            with self.subTest(scope=scope):
                result = self.check(scope)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), {
                    "scope": scope, "tracked_files": 1, "findings": 0, "status": "pass"})

    def test_mixed_neutral_and_private_same_line(self):
        account = "private-fixture-person"
        for platform, classification in (("macos", "macos_home"), ("linux", "linux_home"),
                                         ("windows", "windows_home")):
            with self.subTest(platform=platform):
                self.tracked("fixture.txt", home(platform, "example") + " " + home(platform, account))
                result = self.check("index")
                self.assertEqual(result.returncode, 1)
                self.assertNotIn(account, result.stdout + result.stderr)
                self.assertEqual(json.loads(result.stderr), {
                    "path": "fixture.txt", "line": 1, "class": classification, "scope": "index"})

    def test_neutral_prefix_is_not_an_allow(self):
        self.tracked("fixture.txt", home("macos", "example-private"))
        self.assertEqual(self.classes(self.check()), ["macos_home"])

    def test_file_url_home_is_checked(self):
        self.tracked("fixture.txt", "file://" + home("macos", "private-fixture-person"))
        self.assertEqual(self.classes(self.check()), ["macos_home"])

    def test_index_and_checkout_disagreement_in_both_directions(self):
        private = home("macos", "private-fixture-person")
        neutral = home("macos", "example")
        self.tracked("fixture.txt", private)
        (self.root / "fixture.txt").write_text(neutral)
        self.assertEqual(self.check().returncode, 0)
        self.assertEqual(self.check("index").returncode, 1)
        self.git("add", "--", "fixture.txt")
        (self.root / "fixture.txt").write_text(private)
        self.assertEqual(self.check().returncode, 1)
        self.assertEqual(self.check("index").returncode, 0)

    def test_untracked_files_are_outside_the_claim(self):
        self.tracked("tracked.txt", "neutral\n")
        (self.root / "untracked.txt").write_text(home("macos", "private-fixture-person"))
        result = self.check()
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)["tracked_files"], 1)

    def test_binary_and_attributes_cannot_hide_a_literal(self):
        self.tracked(".gitattributes", "fixture.dat -diff\n")
        self.tracked("fixture.dat", b"\x00" + home("macos", "private-fixture-person").encode())
        self.assertEqual(self.classes(self.check("index")), ["macos_home"])

    def test_named_workstation_is_generic_and_value_free(self):
        suffix = "MacBook-Pro"
        private = "private-fixture-" + suffix
        self.tracked("fixture.txt", private)
        result = self.check()
        self.assertEqual(self.classes(result), ["named_workstation"])
        self.assertNotIn(private, result.stderr + result.stdout)
        (self.root / "fixture.txt").write_text("example-" + suffix)
        self.assertEqual(self.check().returncode, 0)

    def test_workstation_dns_name_is_also_checked(self):
        private = "private-fixture-" + "mbp.tailnet.ts.net"
        self.tracked("fixture.txt", "https://" + private)
        result = self.check("index")
        self.assertEqual(self.classes(result), ["named_workstation"])
        self.assertNotIn(private, result.stderr + result.stdout)

    def test_force_added_evidence_and_profiles_fail_by_path(self):
        paths = {
            "docs/verification/run.txt": "tracked_run_artifact",
            "docs/screenshots/run.png": "tracked_run_artifact",
            "spikes/example/evidence/run.bin": "tracked_run_artifact",
            "profile/Cookies": "browser_profile_artifact",
            "profile/History": "browser_profile_artifact",
            "profile/Login Data": "browser_profile_artifact",
            "profile/Web Data": "browser_profile_artifact",
            "profile/Local State": "browser_profile_artifact",
            "profile/Cookies-journal": "browser_profile_artifact",
            "profile/Preferences": "browser_profile_artifact",
            "profile/metrics.pma": "browser_profile_artifact",
            "browser-profile/arbitrary.bin": "browser_profile_artifact",
            "profile/Session_1234": "browser_profile_artifact",
        }
        self.tracked(".gitignore", "docs/\nspikes/\nprofile/\nbrowser-profile/\n")
        for path in paths:
            self.tracked(path, "private-content-marker")
        for scope in ("checkout", "index"):
            result = self.check(scope)
            self.assertEqual(result.returncode, 1)
            self.assertNotIn("private-content-marker", result.stdout + result.stderr)
            rows = [json.loads(line) for line in result.stderr.splitlines()]
            self.assertEqual({row["path"]: row["class"] for row in rows}, paths)
            self.assertTrue(all(row["line"] == 0 and row["scope"] == scope for row in rows))

    def test_similarly_named_source_is_not_a_profile(self):
        self.tracked("session_search.rs", "// ordinary source\n")
        self.assertEqual(self.check().returncode, 0)

    def test_symlink_target_is_scanned_without_following_it(self):
        target = self.root / "link"
        target.symlink_to(home("macos", "private-fixture-person") + "/missing")
        self.git("add", "--", "link")
        for scope in ("checkout", "index"):
            self.assertEqual(self.classes(self.check(scope)), ["macos_home"])

    def test_missing_checkout_file_fails_instead_of_silent_skip(self):
        self.tracked("fixture.txt", "neutral\n")
        (self.root / "fixture.txt").unlink()
        self.assertEqual(self.classes(self.check()), ["checkout_read_failed"])
        self.assertEqual(self.check("index").returncode, 0)

    def test_dynamic_account_cannot_hide_literal_suffix(self):
        self.tracked("fixture.txt", home("linux", "${provider}") + "/session")
        self.assertEqual(self.check().returncode, 0)
        (self.root / "fixture.txt").write_text(home("linux", ".${provider}") + "/session")
        self.assertEqual(self.check().returncode, 0)
        (self.root / "fixture.txt").write_text(home("linux", "$USER-private"))
        self.assertEqual(self.classes(self.check()), ["linux_home"])

    def test_empty_repository_fails_explicitly(self):
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.classes(result), ["no_tracked_files"])

    def test_blob_limit_fails_with_value_free_diagnostic(self):
        self.tracked("fixture.txt", "neutral\n")
        with (self.root / "fixture.txt").open("r+b") as target:
            target.truncate(16 * 1024 * 1024 + 1)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.classes(result), ["tracked_blob_too_large"])
        self.assertEqual(self.check("index").returncode, 0)

    def test_diagnostic_limit_is_reported_instead_of_truncated_success(self):
        self.tracked("fixture.txt", (home("macos", "private-fixture-person") + "\n") * 201)
        result = self.check("index")
        self.assertEqual(result.returncode, 1)
        rows = [json.loads(line) for line in result.stderr.splitlines()]
        self.assertEqual(len(rows), 201)
        self.assertEqual(rows[-1]["class"], "diagnostic_limit_exceeded")
        self.assertNotIn("private-fixture-person", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
