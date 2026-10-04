"""Cheap libtest-output contracts; no cargo, compiler or process fixture.

Expected transcripts are supplied Rust harness output at the external CLI
boundary. These cases catch empty selection, lost names, ignored/failed tests,
duplicate suites, and lying counts without coupling to product assertions.
Native subprocess ownership and three-OS execution are separate acceptance.
"""
import importlib.util
from pathlib import Path
import sys
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-delivery-os-contract.py"
SPEC = importlib.util.spec_from_file_location("delivery_os_contract", SCRIPT)
GATE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATE
SPEC.loader.exec_module(GATE)

# This small boundary contract is independent of the product's current inventory.
GROUP = GATE.Group("example", "delivery::", False, ("delivery::tests::keeps_pending",))
FIRST = "delivery::tests::keeps_pending"
SECOND = "delivery::tests::refuses_stale_owner"
LISTED = b"delivery::tests::keeps_pending: test\ndelivery::tests::refuses_stale_owner: test\n\n2 tests, 0 benchmarks\n"
PASSED = b"running 2 tests\ntest delivery::tests::refuses_stale_owner ... ok\ntest delivery::tests::keeps_pending ... ok\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 101 filtered out; finished in 0.01s\n"


class DeliveryOutputContract(unittest.TestCase):
    def test_listed_tests_and_future_module_test_must_all_pass(self):
        selected = GATE.parse_listing(b"   Compiling example v1\n" + LISTED, GROUP)
        self.assertEqual(selected, (FIRST, SECOND))
        self.assertEqual(GATE.parse_execution(PASSED, selected, GROUP), 2)

    def test_windows_crlf_and_reordered_results_keep_the_same_contract(self):
        selected = GATE.parse_listing(LISTED.replace(b"\n", b"\r\n"), GROUP)
        self.assertEqual(GATE.parse_execution(PASSED.replace(b"\n", b"\r\n"), selected, GROUP), 2)

    def test_selected_runtime_namespace_runs_but_other_substring_matches_fail(self):
        runtime = "runtime::delivery::tests::refuses_stale_owner"
        group = GATE.Group("example", "delivery::", False, (FIRST,),
                           additional_prefixes=("runtime::delivery::",))
        listing = LISTED.replace(SECOND.encode(), runtime.encode())
        selected = GATE.parse_listing(listing, group)
        self.assertEqual(selected, (FIRST, runtime))
        self.assertEqual(GATE.parse_execution(PASSED.replace(SECOND.encode(), runtime.encode()),
                                             selected, group), 2)
        with self.assertRaises(ValueError):
            GATE.parse_listing(listing.replace(runtime.encode(),
                               b"runtime::unrelated::delivery::tests::refuses_stale_owner"), group)

    def test_empty_missing_required_wrong_namespace_and_duplicate_lists_fail(self):
        invalid = [
            b"0 tests, 0 benchmarks\n",
            b"delivery::tests::other: test\n1 test, 0 benchmarks\n",
            b"unrelated::keeps_pending: test\n1 test, 0 benchmarks\n",
            LISTED.replace(b"delivery::tests::refuses_stale_owner", FIRST.encode()),
        ]
        for output in invalid:
            with self.subTest(output=output):
                with self.assertRaises(ValueError):
                    GATE.parse_listing(output, GROUP)

    def test_missing_duplicate_mismatched_and_benchmark_listing_totals_fail(self):
        for output in [LISTED.replace(b"2 tests, 0 benchmarks\n", b""),
                       LISTED + b"2 tests, 0 benchmarks\n",
                       LISTED.replace(b"2 tests, 0 benchmarks", b"1 test, 0 benchmarks"),
                       LISTED.replace(b"0 benchmarks", b"1 benchmark"),
                       LISTED + b"delivery::tests::bench: benchmark\n"]:
            with self.subTest(output=output):
                with self.assertRaises(ValueError):
                    GATE.parse_listing(output, GROUP)

    def test_exact_selection_refuses_a_similarly_named_test(self):
        exact = GATE.Group("example", FIRST, True, (FIRST,))
        with self.assertRaises(ValueError):
            GATE.parse_listing(LISTED, exact)
        selected = GATE.parse_listing(FIRST.encode() + b": test\n1 test, 0 benchmarks\n", exact)
        self.assertEqual(selected, (FIRST,))

    def test_ignored_failed_and_missing_test_results_fail_even_with_ok_summary(self):
        for output in [PASSED.replace(b"refuses_stale_owner ... ok", b"refuses_stale_owner ... ignored"),
                       PASSED.replace(b"refuses_stale_owner ... ok", b"refuses_stale_owner ... FAILED"),
                       PASSED.replace(b"test delivery::tests::refuses_stale_owner ... ok\n", b""),
                       PASSED.replace(b"refuses_stale_owner", b"other")]:
            with self.subTest(output=output):
                with self.assertRaises(ValueError):
                    GATE.parse_execution(output, (FIRST, SECOND), GROUP)

    def test_duplicate_execution_result_cannot_satisfy_a_missing_name(self):
        with self.assertRaises(ValueError):
            GATE.parse_execution(PASSED.replace(SECOND.encode(), FIRST.encode()), (FIRST, SECOND), GROUP)

    def test_count_mismatch_ignored_failure_and_measured_summary_each_fail(self):
        for before, after in [(b"running 2 tests", b"running 0 tests"),
                              (b"2 passed", b"1 passed"), (b"0 failed", b"1 failed"),
                              (b"0 ignored", b"1 ignored"), (b"0 measured", b"1 measured"),
                              (b"test result: ok", b"test result: FAILED")]:
            with self.subTest(after=after):
                with self.assertRaises(ValueError):
                    GATE.parse_execution(PASSED.replace(before, after), (FIRST, SECOND), GROUP)

    def test_missing_or_second_suite_summary_and_running_count_fail(self):
        summary = PASSED.split(b"test result:", 1)[1]
        for output in [PASSED.split(b"test result:", 1)[0],
                       PASSED + b"test result:" + summary,
                       PASSED + b"running 2 tests\n",
                       PASSED.replace(b"running 2 tests\n", b"")]:
            with self.subTest(output=output):
                with self.assertRaises(ValueError):
                    GATE.parse_execution(output, (FIRST, SECOND), GROUP)

    def test_name_and_input_caps_fail_at_the_first_excess(self):
        exact_cap = [FIRST, *(f"delivery::tests::case_{index}" for index in range(255))]
        output = ("\n".join(name + ": test" for name in exact_cap)
                  + "\n256 tests, 0 benchmarks\n").encode()
        self.assertEqual(len(GATE.parse_listing(output, GROUP)), 256)
        excessive = output.replace(b"256 tests, 0 benchmarks", b"delivery::tests::extra: test\n257 tests, 0 benchmarks")
        oversized_name = (FIRST + "x" * 512 + ": test\n1 test, 0 benchmarks\n").encode()
        for invalid in [excessive, oversized_name, b"x" * (8 * 1024 * 1024 + 1), b"\xff"]:
            with self.subTest(size=len(invalid)):
                with self.assertRaises(ValueError):
                    GATE.parse_listing(invalid, GROUP)


if __name__ == "__main__":
    unittest.main()
