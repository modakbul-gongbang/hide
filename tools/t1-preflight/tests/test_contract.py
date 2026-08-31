from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PACKAGE_ROOT))

from t1_preflight.model import (  # noqa: E402
    PHASES,
    ContractError,
    OutputStore,
    evaluate_runtime_evidence,
    load_manifest,
    percentile_nearest_rank,
    validate_root_relative_command_contract,
)
from t1_preflight.macos import (  # noqa: E402
    _objc_utf8,
    frontmost_application_identity,
    frontmost_identity_from_application,
    macho_dependencies_argv,
    macho_rpaths_argv,
)
from t1_preflight.native_input import (  # noqa: E402
    K_CG_EVENT_FLAG_MASK_ALTERNATE,
    K_CG_EVENT_FLAG_MASK_COMMAND,
    NativeInput,
    chord_steps,
    layout_invariant_key_codes,
)
from t1_preflight.runtime import EventReader, RuntimeHarness  # noqa: E402


class ManifestFixture:
    def __init__(self, root: Path) -> None:
        example_root = PACKAGE_ROOT / "fixtures"
        self.raw = json.loads(
            (example_root / "example-manifest.json").read_text(encoding="utf-8")
        )
        self.scenario = json.loads(
            (example_root / "example-scenario.json").read_text(encoding="utf-8")
        )
        self.raw["output_dir"] = "output"
        self.raw["runtime"]["scenario"] = "scenario.json"
        self.path = root / "manifest.json"
        self.scenario_path = root / "scenario.json"
        self.write()

    def write(self) -> None:
        self.path.write_text(json.dumps(self.raw), encoding="utf-8")
        self.scenario_path.write_text(json.dumps(self.scenario), encoding="utf-8")


class T5ManifestFixture(ManifestFixture):
    def __init__(self, root: Path) -> None:
        super().__init__(root)
        self.raw["verification_profile"] = "t5-option-meta-v1"
        self.raw["run_id"] = "t5-option-meta-test"
        self.raw["output_dir"] = "t5-output"
        self.raw["runtime"]["scenario"] = "scenario-t5.json"
        self.scenario["verification_profile"] = "t5-option-meta-v1"
        self.scenario["scenario_id"] = "t5-option-meta-test"
        self.scenario["run_id"] = "t5-option-meta-test"
        self.scenario["actions"].insert(
            2,
            {
                "action": "terminal.option_meta",
                "target_point": "terminal",
                "key_code": 3,
                "modifiers": ["option"],
                "repeat": 2,
                "expected_bytes_hex": "1b 66",
                "latency_budget_ms": 50,
            },
        )
        self.path = root / "manifest-t5.json"
        self.scenario_path = root / "scenario-t5.json"
        self.write()


class ContractTests(unittest.TestCase):
    def test_runtime_requires_explicit_exclusive_hands_off_marker(self) -> None:
        marker = "HERDR_T5_EXCLUSIVE_HANDS_OFF"
        previous = os.environ.pop(marker, None)
        try:
            with self.assertRaises(ContractError) as context:
                RuntimeHarness._require_exclusive_hands_off()
            self.assertEqual(context.exception.code, "environment_focus_interference")
            self.assertEqual(
                context.exception.details["required_environment"], {marker: "1"}
            )

            os.environ[marker] = "1"
            RuntimeHarness._require_exclusive_hands_off()
        finally:
            if previous is None:
                os.environ.pop(marker, None)
            else:
                os.environ[marker] = previous

    def test_native_injection_focus_requires_all_appkit_owners(self) -> None:
        NativeInput.assert_injection_focus(
            {
                "app_frontmost": True,
                "key_window": True,
                "render_view_first_responder": True,
            },
            pid=123,
            injection="terminal.option-meta",
        )

        with self.assertRaises(ContractError) as context:
            NativeInput.assert_injection_focus(
                {
                    "app_frontmost": False,
                    "key_window": True,
                    "render_view_first_responder": True,
                },
                pid=123,
                injection="terminal.option-meta",
            )
        self.assertEqual(context.exception.code, "environment_focus_interference")
        self.assertEqual(context.exception.details["failed_checks"], {"app_frontmost": False})

        with self.assertRaises(ContractError) as context:
            NativeInput.assert_injection_focus(
                None,
                pid=123,
                injection="phase.quit",
            )
        self.assertEqual(context.exception.code, "environment_focus_interference")
        self.assertEqual(
            context.exception.details["missing"],
            ["app_frontmost", "key_window", "render_view_first_responder"],
        )

    def test_terminal_probe_uses_physical_keys_stable_under_korean_ime(self) -> None:
        self.assertEqual(
            layout_invariant_key_codes("917-000"),
            [25, 18, 26, 27, 29, 29, 29],
        )
        with self.assertRaisesRegex(ContractError, "layout-invariant"):
            layout_invariant_key_codes("T1PROBE")

    def test_macho_inspector_preserves_cef_helper_parentheses_as_one_argument(
        self,
    ) -> None:
        executable = Path(
            "/Applications/Herdr.app/Contents/Frameworks/"
            "Herdr Helper (Alerts).app/Contents/MacOS/Herdr Helper (Alerts)"
        )
        for argv in (
            macho_rpaths_argv(executable),
            macho_dependencies_argv(executable),
        ):
            self.assertEqual(argv[:2], ["/usr/bin/xcrun", "llvm-objdump"])
            self.assertEqual(argv[-1], str(executable))
            self.assertEqual(len([item for item in argv if "Alerts" in item]), 1)

    def test_frontmost_identity_uses_in_process_injected_nsworkspace_boundary(self) -> None:
        class ExecutableURL:
            def lastPathComponent(self) -> str:
                return "Terminal"

        class Application:
            def bundleIdentifier(self) -> str:
                return "com.apple.Terminal"

            def localizedName(self) -> str:
                return "Terminal"

            def processIdentifier(self) -> int:
                return 123

            def executableURL(self) -> ExecutableURL:
                return ExecutableURL()

        class Workspace:
            def frontmostApplication(self) -> Application:
                return Application()

        identity = frontmost_application_identity(lambda: Workspace())
        self.assertEqual(identity["identity"], {
            "bundle_identifier": "com.apple.Terminal",
            "localized_name": "Terminal",
            "process_identifier": 123,
            "executable_name": "Terminal",
        })
        self.assertEqual(identity["boundary"], "injected.NSWorkspace")
        self.assertIsInstance(identity["probe_duration_ms"], float)
        self.assertNotIn("command", identity)
        self.assertEqual(
            frontmost_identity_from_application(Application())["process_identifier"],
            123,
        )

    def test_frontmost_identity_surfaces_typed_in_process_query_failures(self) -> None:
        class Workspace:
            def frontmostApplication(self) -> None:
                return None

        with self.assertRaises(ContractError) as context:
            frontmost_application_identity(lambda: Workspace())
        self.assertEqual(context.exception.code, "frontmost.identity_query_failed")
        self.assertIn("probe_duration_ms", context.exception.details)

    def test_frontmost_identity_utf8_failure_is_explicit_contract_error(self) -> None:
        class Runtime:
            def send(self, receiver: int, selector: str, restype: object) -> None:
                self.last_query = (receiver, selector, restype)
                return None

        with self.assertRaises(ContractError) as context:
            _objc_utf8(Runtime(), 123, "localized_name")
        self.assertEqual(context.exception.code, "frontmost.identity_field_invalid")
        self.assertEqual(context.exception.details["field"], "localized_name")

    def test_frontmost_probe_has_no_per_query_swift_or_external_command_boundary(self) -> None:
        source = (PACKAGE_ROOT / "t1_preflight" / "macos.py").read_text(encoding="utf-8")
        self.assertIn("objc_msgSend", source)
        self.assertNotIn("frontmost_application_command", source)
        self.assertNotIn("/usr/bin/swift", source)

    def test_native_command_contract_resolves_all_operands_from_worktree_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "tools/t1-preflight/t1-preflight"
            executable.parent.mkdir(parents=True)
            executable.write_text("#!/bin/sh\n", encoding="utf-8")
            executable.chmod(0o755)
            app = root / "spikes/integrated-preflight/target/bundle/preflight.app"
            app.mkdir(parents=True)
            manifest = root / "spikes/integrated-preflight/t5-pathfix.json"
            manifest.write_text("{}\n", encoding="utf-8")

            contract = validate_root_relative_command_contract(
                root,
                working_directory=".",
                executable="tools/t1-preflight/t1-preflight",
                app_path="spikes/integrated-preflight/target/bundle/preflight.app",
                manifest_path="spikes/integrated-preflight/t5-pathfix.json",
            )

            self.assertEqual(contract["contract"], "root-relative")
            self.assertEqual(contract["working_directory"], ".")
            self.assertEqual(
                contract["paths"]["app"],
                "spikes/integrated-preflight/target/bundle/preflight.app",
            )
            self.assertTrue(contract["resolved"]["executable_is_executable"])

    def test_native_command_contract_rejects_old_subdirectory_and_mixed_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ContractError) as context:
                validate_root_relative_command_contract(
                    root,
                    working_directory="tools/t1-preflight",
                    executable="./t1-preflight",
                    app_path="spikes/integrated-preflight/target/bundle/preflight.app",
                    manifest_path="spikes/integrated-preflight/t5-pathfix.json",
                )
            self.assertEqual(context.exception.code, "command.cwd_contract_invalid")

            with self.assertRaises(ContractError) as context:
                validate_root_relative_command_contract(
                    root,
                    working_directory=".",
                    executable="tools/t1-preflight/t1-preflight",
                    app_path=str(root / "preflight.app"),
                    manifest_path="spikes/integrated-preflight/t5-pathfix.json",
                )
            self.assertEqual(context.exception.code, "command.path_style_mismatch")

    def test_valid_manifest_renders_only_readiness_telemetry_arguments(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = ManifestFixture(Path(directory))
            manifest = load_manifest(fixture.path)
            arguments = manifest.render_arguments(
                "warm_closed", Path(directory) / "events.jsonl"
            )
            self.assertIn("warm_closed", arguments)
            self.assertIn("browser-closed", arguments)
            self.assertIn(str(Path(directory) / "events.jsonl"), arguments)
            self.assertNotIn(str(fixture.scenario_path), arguments)

            included_arguments = manifest.render_arguments(
                "browser_included", Path(directory) / "browser-events.jsonl"
            )
            self.assertIn("browser-included", included_arguments)
            self.assertNotEqual(arguments, included_arguments)
            self.assertEqual(
                manifest.runtime["phases"]["browser_included"]["profile_settle_ms"],
                2000,
            )

            fixture.raw["runtime"]["arguments"].extend(
                ["--t1-preflight-action-socket", "{action_socket}"]
            )
            fixture.write()
            manifest = load_manifest(fixture.path)
            action_socket = Path(directory) / "warm.action.sock"
            rendered = manifest.render_arguments(
                "warm_closed",
                Path(directory) / "events.jsonl",
                action_socket=action_socket,
            )
            self.assertIn(str(action_socket), rendered)

    def test_all_closed_phases_require_the_exact_closed_launch_mode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = ManifestFixture(Path(directory))
            fixture.raw["runtime"]["phases"]["relaunch_closed"]["launch_mode"] = (
                "browser-included"
            )
            fixture.write()
            with self.assertRaisesRegex(ContractError, "exact T1 launch mode"):
                load_manifest(fixture.path)

    def test_browser_settle_is_manifest_owned_and_bounded(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = ManifestFixture(Path(directory))
            fixture.raw["runtime"]["phases"]["browser_included"][
                "profile_settle_ms"
            ] = 10001
            fixture.write()
            with self.assertRaisesRegex(ContractError, "at most 10000"):
                load_manifest(fixture.path)

    def test_manifest_cannot_loosen_hard_performance_budgets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = ManifestFixture(Path(directory))
            fixture.raw["budgets"]["browser_closed_rss_mb"] = 201
            fixture.write()
            with self.assertRaisesRegex(ContractError, "hard ceiling"):
                load_manifest(fixture.path)

    def test_unlisted_scenario_action_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = ManifestFixture(Path(directory))
            fixture.scenario["actions"].append({"action": "unsupported.action"})
            fixture.write()
            with self.assertRaisesRegex(ContractError, "unsupported actions"):
                load_manifest(fixture.path)

    def test_t5_manifest_owns_a_distinct_profile_and_exactly_two_option_meta_events(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = T5ManifestFixture(Path(directory))
            manifest = load_manifest(fixture.path)
            self.assertEqual(manifest.verification_profile, "t5-option-meta-v1")
            self.assertEqual(manifest.run_id, "t5-option-meta-test")
            self.assertEqual(manifest.output_dir.name, "t5-output")

            fixture.scenario["actions"][2]["repeat"] = 1
            fixture.write()
            with self.assertRaisesRegex(ContractError, "exactly twice"):
                load_manifest(fixture.path)

    def test_t5_v3_profile_is_distinct_and_requires_matching_scenario_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = T5ManifestFixture(Path(directory))
            raw = json.loads(fixture.path.read_text(encoding="utf-8"))
            scenario = json.loads(fixture.scenario_path.read_text(encoding="utf-8"))
            raw["verification_profile"] = "t5-option-meta-v3"
            raw["run_id"] = "t5-option-meta-v3"
            raw["output_dir"] = "t5-output-v3"
            raw["runtime"]["scenario"] = "scenario-t5-v3.json"
            scenario["verification_profile"] = "t5-option-meta-v3"
            scenario["scenario_id"] = "t5-option-meta-v3"
            scenario["run_id"] = "t5-option-meta-v3"
            v3_manifest = Path(directory) / "manifest-t5-v3.json"
            v3_scenario = Path(directory) / "scenario-t5-v3.json"
            v3_manifest.write_text(json.dumps(raw), encoding="utf-8")
            v3_scenario.write_text(json.dumps(scenario), encoding="utf-8")
            manifest = load_manifest(v3_manifest)
            self.assertEqual(manifest.verification_profile, "t5-option-meta-v3")
            self.assertEqual(manifest.run_id, "t5-option-meta-v3")
            self.assertEqual(manifest.output_dir.name, "t5-output-v3")

    def test_t5_v4_requires_plain_key_control_immediately_before_option_meta(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = T5ManifestFixture(Path(directory))
            raw = json.loads(fixture.path.read_text(encoding="utf-8"))
            scenario = json.loads(fixture.scenario_path.read_text(encoding="utf-8"))
            raw["verification_profile"] = "t5-option-meta-v4"
            raw["run_id"] = "t5-option-meta-v4"
            raw["output_dir"] = "t5-output-v4"
            raw["runtime"]["scenario"] = "scenario-t5-v4.json"
            scenario["verification_profile"] = "t5-option-meta-v4"
            scenario["scenario_id"] = "t5-option-meta-v4"
            scenario["run_id"] = "t5-option-meta-v4"
            scenario["actions"].insert(
                2,
                {
                    "action": "terminal.plain_key_control",
                    "target_point": "terminal",
                    "key_code": 28,
                    "modifiers": [],
                    "repeat": 1,
                    "expected_bytes_hex": "38",
                },
            )
            v4_manifest = Path(directory) / "manifest-t5-v4.json"
            v4_scenario = Path(directory) / "scenario-t5-v4.json"
            v4_manifest.write_text(json.dumps(raw), encoding="utf-8")
            v4_scenario.write_text(json.dumps(scenario), encoding="utf-8")
            manifest = load_manifest(v4_manifest)
            self.assertEqual(manifest.verification_profile, "t5-option-meta-v4")
            self.assertEqual(manifest.run_id, "t5-option-meta-v4")
            self.assertEqual(manifest.output_dir.name, "t5-output-v4")

            scenario["actions"][2], scenario["actions"][3] = (
                scenario["actions"][3],
                scenario["actions"][2],
            )
            v4_scenario.write_text(json.dumps(scenario), encoding="utf-8")
            with self.assertRaisesRegex(ContractError, "immediately precede"):
                load_manifest(v4_manifest)

    def test_t5_semantic_gate_does_not_accept_standalone_unicode_latency(self) -> None:
        budgets = {
            "warm_usable_ms": 1000.0,
            "terminal_input_to_present_p95_ms": 50.0,
            "idle_cpu_percent": 1.0,
            "browser_closed_rss_mb": 200.0,
            "browser_included_rss_mb": 800.0,
        }
        profiles = {
            "browser_closed": [
                {"rss_mb": 100.0, "cpu_percent": 0.1},
                {"rss_mb": 100.0, "cpu_percent": 0.1},
                {"rss_mb": 100.0, "cpu_percent": 0.1},
            ],
            "browser_included": [
                {"rss_mb": 400.0, "cpu_percent": 0.1},
                {"rss_mb": 400.0, "cpu_percent": 0.1},
                {"rss_mb": 400.0, "cpu_percent": 0.1},
            ],
        }
        semantic = {
            "verification_profile": "t5-option-meta-v1",
            "workspace_count": 7,
            "pane_count": 11,
            "browser_closed": True,
            "zoom_before_topology_hash": "same",
            "zoom_after_topology_hash": "same",
            "focus_before": "terminal",
            "focus_after": "editor",
            "resize_scale_factor": 2.0,
            "resize_logical_width": 960,
            "resize_physical_width": 1920,
            "persisted_state_hash": "same",
            "relaunch_state_hash": "same",
            "ax_labels_present": True,
            "native_screenshots_present": True,
            "browser_included_cdp": True,
            "exactly_one_instance": True,
        }
        failed = evaluate_runtime_evidence(
            budgets=budgets,
            warm_usable_ms=200.0,
            terminal_latencies_ms=[1.0] * 20,
            profiles=profiles,
            semantic=semantic,
        )
        self.assertEqual(failed["status"], "FAIL")
        self.assertFalse(failed["semantic_checks"]["option_meta_exactly_twice"])

    def test_t5_native_probe_917_008_completes_before_declared_control(self) -> None:
        events = [
            {"event": "terminal.input_presented", "probe": "917-008", "seq": 40},
            {"event": "input.action.armed", "action": "terminal.plain_key_control", "seq": 41},
        ]
        self.assertEqual(
            RuntimeHarness._require_native_probe_before_action(events, 41), 40
        )
        with self.assertRaisesRegex(ContractError, "917-008"):
            RuntimeHarness._require_native_probe_before_action(
                [{"event": "terminal.input_presented", "probe": "917-007", "seq": 40}],
                41,
            )

    def test_t5_declared_control_proves_appkit_route_and_byte_38(self) -> None:
        budgets = {
            "warm_usable_ms": 1000.0,
            "terminal_input_to_present_p95_ms": 50.0,
            "idle_cpu_percent": 1.0,
            "browser_closed_rss_mb": 200.0,
            "browser_included_rss_mb": 800.0,
        }
        focus = {
            "app_frontmost": True,
            "key_window": True,
            "render_view_first_responder": True,
        }
        control_events = [
            {"event": "input.plain_key_control.appkit", "bytes_hex": "38", "focus": focus},
            {"event": "input.plain_key_control.app-routing", "bytes_hex": "38", "focus": focus},
            {"event": "input.plain_key_control.herdr", "bytes_hex": "38", "focus": focus},
            {"event": "input.plain_key_control.pty", "bytes_hex": "38", "focus": focus},
        ]
        semantic = {
            "verification_profile": "t5-option-meta-v4",
            "workspace_count": 7,
            "pane_count": 11,
            "browser_closed": True,
            "zoom_before_topology_hash": "same",
            "zoom_after_topology_hash": "same",
            "focus_before": "terminal",
            "focus_after": "editor",
            "resize_scale_factor": 2.0,
            "resize_logical_width": 960,
            "resize_physical_width": 1920,
            "persisted_state_hash": "same",
            "relaunch_state_hash": "same",
            "ax_labels_present": True,
            "native_screenshots_present": True,
            "browser_included_cdp": True,
            "exactly_one_instance": True,
            "plain_key_control": {
                "count": 1,
                "expected_count": 1,
                "expected_bytes_hex": "38",
                "source_order": ["appkit", "app-routing", "herdr", "pty"],
                "source_events": control_events,
                "focus": focus,
                "native_probe_917_008_seq": 40,
                "action_armed_seq": 41,
            },
            "option_meta": {
                "count": 2,
                "expected_count": 2,
                "expected_bytes_hex": "1b 66",
                "source_order": ["appkit", "app-routing", "herdr", "pty"],
                "events": [
                    {
                        "bytes_hex": "1b 66",
                        "source_events": [
                            {"event": "input.option_meta.appkit"},
                            {"event": "input.option_meta.app-routing"},
                            {"event": "input.option_meta.herdr"},
                            {"event": "input.option_meta.pty"},
                        ],
                    },
                    {
                        "bytes_hex": "1b 66",
                        "source_events": [
                            {"event": "input.option_meta.appkit"},
                            {"event": "input.option_meta.app-routing"},
                            {"event": "input.option_meta.herdr"},
                            {"event": "input.option_meta.pty"},
                        ],
                    },
                ],
                "latencies_ms": [1.0, 1.0],
                "focus_before": focus,
                "focus_snapshots": [focus, focus],
                "latency_budget_ms": 50,
            },
        }
        profiles = {
            "browser_closed": [{"rss_mb": 100.0, "cpu_percent": 0.1}] * 3,
            "browser_included": [{"rss_mb": 400.0, "cpu_percent": 0.1}] * 3,
        }
        result = evaluate_runtime_evidence(
            budgets=budgets,
            warm_usable_ms=200.0,
            terminal_latencies_ms=[1.0] * 20,
            profiles=profiles,
            semantic=semantic,
        )
        self.assertEqual(result["status"], "PASS")
        self.assertTrue(result["semantic_checks"]["plain_key_control_source_timeline"])
        self.assertTrue(result["semantic_checks"]["plain_key_control_bytes_proven"])
        self.assertTrue(result["semantic_checks"]["plain_key_control_after_native_probe"])

        semantic["plain_key_control"]["source_events"][-1]["bytes_hex"] = "39"
        failed = evaluate_runtime_evidence(
            budgets=budgets,
            warm_usable_ms=200.0,
            terminal_latencies_ms=[1.0] * 20,
            profiles=profiles,
            semantic=semantic,
        )
        self.assertEqual(failed["status"], "FAIL")
        self.assertFalse(failed["semantic_checks"]["plain_key_control_bytes_proven"])

    def test_output_parent_traversal_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = ManifestFixture(Path(directory))
            fixture.raw["output_dir"] = "../escaped"
            fixture.write()
            with self.assertRaisesRegex(ContractError, "parent traversal"):
                load_manifest(fixture.path)

    def test_nearest_rank_p95_uses_the_budget_edge(self) -> None:
        values = [10.0] * 19 + [50.0]
        self.assertEqual(percentile_nearest_rank(values, 0.95), 10.0)
        values[-2] = 51.0
        values[-1] = 51.0
        self.assertEqual(percentile_nearest_rank(values, 0.95), 51.0)

    def test_runtime_verdict_checks_budget_and_semantic_outcomes(self) -> None:
        budgets = {
            "warm_usable_ms": 1000.0,
            "terminal_input_to_present_p95_ms": 50.0,
            "idle_cpu_percent": 1.0,
            "browser_closed_rss_mb": 200.0,
            "browser_included_rss_mb": 800.0,
        }
        profiles = {
            "browser_closed": [
                {"rss_mb": 199.0, "cpu_percent": 1.0},
                {"rss_mb": 200.0, "cpu_percent": 1.0},
                {"rss_mb": 198.0, "cpu_percent": 1.0},
            ],
            "browser_included": [
                {"rss_mb": 800.0, "cpu_percent": 3.0},
                {"rss_mb": 799.0, "cpu_percent": 3.0},
                {"rss_mb": 798.0, "cpu_percent": 3.0},
            ],
        }
        semantic = {
            "workspace_count": 7,
            "pane_count": 11,
            "browser_closed": True,
            "zoom_before_topology_hash": "same",
            "zoom_after_topology_hash": "same",
            "focus_before": "terminal",
            "focus_after": "editor",
            "resize_scale_factor": 2.0,
            "resize_logical_width": 960,
            "resize_physical_width": 1920,
            "persisted_state_hash": "restored",
            "relaunch_state_hash": "restored",
            "ax_labels_present": True,
            "native_screenshots_present": True,
            "browser_included_cdp": True,
            "exactly_one_instance": True,
        }
        verdict = evaluate_runtime_evidence(
            budgets=budgets,
            warm_usable_ms=1000.0,
            terminal_latencies_ms=[50.0] * 20,
            profiles=profiles,
            semantic=semantic,
        )
        self.assertEqual(verdict["status"], "PASS")
        semantic["zoom_after_topology_hash"] = "changed"
        failed = evaluate_runtime_evidence(
            budgets=budgets,
            warm_usable_ms=1000.0,
            terminal_latencies_ms=[50.0] * 20,
            profiles=profiles,
            semantic=semantic,
        )
        self.assertEqual(failed["status"], "FAIL")
        self.assertFalse(failed["semantic_checks"]["zoom_exact_restore"])

    def test_completed_output_is_idempotently_reused_but_incomplete_output_fails_closed(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = ManifestFixture(root)
            manifest = load_manifest(fixture.path)
            store = OutputStore(manifest, root / "Missing.app", "dry-run")
            self.assertIsNone(store.prepare())
            result = {"status": "DRY_RUN", "run_id": manifest.run_id}
            store.finish(result)
            self.assertEqual(
                OutputStore(manifest, root / "Missing.app", "dry-run").prepare(), result
            )

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = ManifestFixture(root)
            manifest = load_manifest(fixture.path)
            incomplete = OutputStore(manifest, root / "Missing.app", "dry-run")
            self.assertIsNone(incomplete.prepare())
            with self.assertRaisesRegex(ContractError, "without a completed result"):
                OutputStore(manifest, root / "Missing.app", "dry-run").prepare()

    def test_output_identity_changes_when_any_bundle_file_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = ManifestFixture(root)
            manifest = load_manifest(fixture.path)
            app = root / "Herdr IDE.app"
            helper = app / "Contents/Frameworks/Helper"
            helper.parent.mkdir(parents=True)
            helper.write_text("first", encoding="utf-8")
            first = OutputStore(manifest, app, "static").identity
            helper.write_text("second", encoding="utf-8")
            second = OutputStore(manifest, app, "static").identity
            self.assertNotEqual(first["app_bundle_sha256"], second["app_bundle_sha256"])

    def test_event_reader_rejects_sequence_gaps(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            events = [
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 1,
                    "monotonic_ns": 10,
                    "event": "app.usable",
                },
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 3,
                    "monotonic_ns": 20,
                    "event": "fixture.ready",
                },
            ]
            path.write_text(
                "\n".join(json.dumps(item) for item in events) + "\n", encoding="utf-8"
            )
            reader = EventReader(path, run_id="t1", phase="warm", pid=123)
            with self.assertRaisesRegex(ContractError, "exactly one"):
                reader.poll()

    def test_modified_chord_brackets_the_key_with_real_modifier_transitions(
        self,
    ) -> None:
        option = K_CG_EVENT_FLAG_MASK_ALTERNATE
        self.assertEqual(
            chord_steps(3, ["option"]),
            [
                (58, True, option),
                (3, True, option),
                (3, False, option),
                (58, False, 0),
            ],
        )
        command = K_CG_EVENT_FLAG_MASK_COMMAND
        self.assertEqual(
            chord_steps(12, ["command", "option"]),
            [
                (55, True, command),
                (58, True, command | option),
                (12, True, command | option),
                (12, False, command | option),
                (58, False, command),
                (55, False, 0),
            ],
        )

    def test_unmodified_key_keeps_the_exact_two_step_injection_shape(self) -> None:
        self.assertEqual(chord_steps(28, []), [(28, True, 0), (28, False, 0)])

    def test_chord_plan_rejects_duplicate_or_unknown_modifiers(self) -> None:
        with self.assertRaisesRegex(ContractError, "same modifier twice"):
            chord_steps(3, ["option", "option"])
        with self.assertRaisesRegex(ContractError, "unsupported modifier"):
            chord_steps(3, ["fn"])

    def test_chord_wait_rejects_focus_loss_before_a_late_target_event(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            base = {
                "schema": "herdr.t1-preflight.telemetry.v1",
                "run_id": "t1",
                "phase": "warm",
                "pid": 123,
            }
            path.write_text(
                "\n".join(
                    [
                        json.dumps(
                            {
                                **base,
                                "seq": 1,
                                "monotonic_ns": 10,
                                "event": "input.focus.state",
                                "focus": {
                                    "app_frontmost": True,
                                    "key_window": True,
                                    "render_view_first_responder": True,
                                },
                            },
                        ),
                        json.dumps(
                            {
                                **base,
                                "seq": 2,
                                "monotonic_ns": 20,
                                "event": "app.lifecycle.resign",
                            }
                        ),
                        json.dumps(
                            {
                                **base,
                                "seq": 3,
                                "monotonic_ns": 30,
                                "event": "input.option_meta.pty",
                            }
                        ),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            reader = EventReader(
                path,
                run_id="t1",
                phase="warm",
                pid=123,
                frontmost_identity_probe=lambda: {
                    "identity": {
                        "bundle_identifier": "com.example.Other",
                        "localized_name": "Other",
                        "process_identifier": 456,
                        "executable_name": "Other",
                    },
                    "boundary": "test.injected",
                },
            )

            class RunningProcess:
                def poll(self) -> None:
                    return None

            with self.assertRaises(ContractError) as caught:
                reader.wait_for(
                    "input.option_meta.pty",
                    timeout_ms=100,
                    process=RunningProcess(),
                    after_seq=1,
                    abort_on_focus_loss_after=1,
                )
            self.assertEqual(caught.exception.code, "environment_focus_interference")
            self.assertEqual(caught.exception.details["interference_seq"], 2)
            self.assertEqual(
                caught.exception.details["frontmost_at_detection"]["identity"][
                    "process_identifier"
                ],
                456,
            )

    def test_focus_probe_failure_does_not_mask_focus_interference(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            base = {
                "schema": "herdr.t1-preflight.telemetry.v1",
                "run_id": "t1",
                "phase": "warm",
                "pid": 123,
            }
            path.write_text(
                "\n".join(
                    [
                        json.dumps(
                            {
                                **base,
                                "seq": 1,
                                "monotonic_ns": 10,
                                "event": "input.focus.state",
                                "focus": {
                                    "app_frontmost": False,
                                    "key_window": False,
                                    "render_view_first_responder": True,
                                },
                            },
                        )
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            reader = EventReader(
                path,
                run_id="t1",
                phase="warm",
                pid=123,
                frontmost_identity_probe=lambda: (_ for _ in ()).throw(
                    ContractError("frontmost.test_failure", "probe unavailable")
                ),
            )

            class RunningProcess:
                def poll(self) -> None:
                    return None

            with self.assertRaises(ContractError) as caught:
                reader.wait_for(
                    "input.option_meta.pty",
                    timeout_ms=100,
                    process=RunningProcess(),
                    after_seq=0,
                    abort_on_focus_loss_after=0,
                )
            self.assertEqual(caught.exception.code, "environment_focus_interference")
            self.assertEqual(
                caught.exception.details["frontmost_at_detection"]["error"]["code"],
                "frontmost.test_failure",
            )

    def test_wait_for_fails_fast_when_focus_is_lost_after_the_injection_boundary(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            base = {
                "schema": "herdr.t1-preflight.telemetry.v1",
                "run_id": "t1",
                "phase": "warm",
                "pid": 123,
            }
            events = [
                {
                    **base,
                    "seq": 1,
                    "monotonic_ns": 10,
                    "event": "input.focus.state",
                    "focus": {
                        "app_frontmost": True,
                        "key_window": True,
                        "render_view_first_responder": True,
                    },
                },
                {
                    **base,
                    "seq": 2,
                    "monotonic_ns": 20,
                    "event": "app.lifecycle.resign",
                    "source": "NSApplicationDelegate",
                    "active": False,
                },
            ]
            path.write_text(
                "\n".join(json.dumps(item) for item in events) + "\n", encoding="utf-8"
            )
            reader = EventReader(path, run_id="t1", phase="warm", pid=123)

            class RunningProcess:
                def poll(self) -> None:
                    return None

            with self.assertRaises(ContractError) as caught:
                reader.wait_for(
                    "input.option_meta.pty",
                    timeout_ms=15000,
                    process=RunningProcess(),
                    after_seq=1,
                    abort_on_focus_loss_after=1,
                )
            self.assertEqual(caught.exception.code, "environment_focus_interference")
            self.assertEqual(
                caught.exception.details.get("interference_event"),
                "app.lifecycle.resign",
            )
            self.assertEqual(caught.exception.details.get("interference_seq"), 2)
            self.assertIn("frontmost_at_detection", caught.exception.details)

    def test_wait_for_fails_fast_with_the_cause_when_the_app_drops_the_emit(
        self,
    ) -> None:
        cause = (
            "stage=preflight.events.serialize event=input.action.armed "
            "cause=reserved-field field=phase"
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            events = [
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 1,
                    "monotonic_ns": 10,
                    "event": "app.usable",
                },
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 2,
                    "monotonic_ns": 20,
                    "event": "telemetry.emit_failed",
                    "target": "input.action.armed",
                    "cause": cause,
                },
            ]
            path.write_text(
                "\n".join(json.dumps(item) for item in events) + "\n", encoding="utf-8"
            )
            reader = EventReader(path, run_id="t1", phase="warm", pid=123)

            class RunningProcess:
                def poll(self) -> None:
                    return None

            with self.assertRaises(ContractError) as caught:
                reader.wait_for(
                    "input.action.armed",
                    timeout_ms=15000,
                    process=RunningProcess(),
                    after_seq=1,
                )
            self.assertEqual(caught.exception.code, "telemetry.emit_failed")
            self.assertEqual(caught.exception.details.get("cause"), cause)
            self.assertEqual(caught.exception.details.get("seq"), 2)

    def test_event_reader_uses_newest_focus_snapshot_before_injection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            events = [
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 1,
                    "monotonic_ns": 10,
                    "event": "app.usable",
                    "focus": {
                        "app_frontmost": True,
                        "key_window": True,
                        "render_view_first_responder": True,
                    },
                },
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 2,
                    "monotonic_ns": 20,
                    "event": "input.focus.state",
                    "focus": {
                        "app_frontmost": False,
                        "key_window": True,
                        "render_view_first_responder": True,
                    },
                },
            ]
            path.write_text(
                "\n".join(json.dumps(item) for item in events) + "\n", encoding="utf-8"
            )
            reader = EventReader(path, run_id="t1", phase="warm", pid=123)
            self.assertEqual(
                reader.latest_focus_state(),
                {
                    "app_frontmost": False,
                    "key_window": True,
                    "render_view_first_responder": True,
                },
            )

    def test_fresh_focus_boundary_blocks_key_after_stale_true_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            events = [
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 1,
                    "monotonic_ns": 10,
                    "event": "input.focus.state",
                    "focus": {
                        "app_frontmost": True,
                        "key_window": True,
                        "render_view_first_responder": True,
                    },
                },
                {
                    "schema": "herdr.t1-preflight.telemetry.v1",
                    "run_id": "t1",
                    "phase": "warm",
                    "pid": 123,
                    "seq": 2,
                    "monotonic_ns": 20,
                    "event": "input.focus.state",
                    "focus": {
                        "app_frontmost": False,
                        "key_window": False,
                        "render_view_first_responder": True,
                    },
                },
            ]
            path.write_text(
                "\n".join(json.dumps(item) for item in events) + "\n", encoding="utf-8"
            )

            class RunningProcess:
                pid = 123

                @staticmethod
                def poll() -> None:
                    return None

            class RecordingNativeInput:
                def __init__(self) -> None:
                    self.keys: list[tuple[int, list[str]]] = []

                def key(self, key_code: int, modifiers: list[str]) -> None:
                    self.keys.append((key_code, modifiers))

            reader = EventReader(path, run_id="t1", phase="warm", pid=123)
            harness = RuntimeHarness.__new__(RuntimeHarness)
            harness.native = RecordingNativeInput()
            harness.event_timeout_ms = 100
            with self.assertRaises(ContractError) as context:
                harness._inject_key(
                    RunningProcess(),
                    reader,
                    3,
                    ["option"],
                    "terminal.option_meta[1]",
                    fresh_focus_after_seq=1,
                )
            self.assertEqual(context.exception.code, "environment_focus_interference")
        self.assertEqual(harness.native.keys, [])

    def test_frontmost_target_mismatch_fails_closed_before_control_key(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "events.jsonl"
            path.write_text(
                json.dumps(
                    {
                        "schema": "herdr.t1-preflight.telemetry.v1",
                        "run_id": "t1",
                        "phase": "warm",
                        "pid": 123,
                        "seq": 1,
                        "monotonic_ns": 10,
                        "event": "input.focus.state",
                        "focus": {
                            "app_frontmost": True,
                            "key_window": True,
                            "render_view_first_responder": True,
                        },
                    }
                )
                + "\n",
                encoding="utf-8",
            )

            class RunningProcess:
                pid = 123

                @staticmethod
                def poll() -> None:
                    return None

            class RecordingNativeInput:
                def __init__(self) -> None:
                    self.keys: list[tuple[int, list[str]]] = []

                def key(self, key_code: int, modifiers: list[str]) -> None:
                    self.keys.append((key_code, modifiers))

            class Manifest:
                bundle = {"identifier": "dev.herdr.integrated-preflight"}

            reader = EventReader(path, run_id="t1", phase="warm", pid=123)
            harness = RuntimeHarness.__new__(RuntimeHarness)
            harness.native = RecordingNativeInput()
            harness.manifest = Manifest()
            harness.event_timeout_ms = 100
            harness._frontmost_application_timeline = []
            harness._current_phase = "warm"
            harness._frontmost_identity_probe = lambda: {
                "identity": {
                    "bundle_identifier": "com.apple.Finder",
                    "localized_name": "Finder",
                    "process_identifier": 456,
                    "executable_name": "Finder",
                },
                "observed_monotonic_ns": 20,
                "probe_duration_ms": 0.3,
                "boundary": "injected.NSWorkspace",
            }
            with self.assertRaises(ContractError) as context:
                harness._inject_key(
                    RunningProcess(),
                    reader,
                    28,
                    [],
                    "terminal.plain_key_control",
                    fresh_focus_after_seq=0,
                    frontmost_target_required=True,
                )
            self.assertEqual(context.exception.code, "environment_focus_interference")
            self.assertEqual(harness.native.keys, [])
            self.assertEqual(
                context.exception.details["expected"]["process_identifier"], 123
            )

    def test_focus_loss_records_in_process_frontmost_identity_with_focus_booleans(self) -> None:
        harness = RuntimeHarness.__new__(RuntimeHarness)
        harness._frontmost_tracking_enabled = True
        harness._frontmost_focus_all_true = True
        harness._frontmost_application_timeline = []
        harness._current_phase = "warm_closed"
        harness._frontmost_identity_probe = lambda: {
            "identity": {
                "bundle_identifier": "com.apple.Terminal",
                "localized_name": "Terminal",
                "process_identifier": 321,
                "executable_name": "Terminal",
            },
            "observed_monotonic_ns": 30,
            "probe_duration_ms": 0.2,
            "boundary": "injected.NSWorkspace",
        }

        harness._observe_focus_event(
            {
                "event": "input.focus.state",
                "seq": 42,
                "monotonic_ns": 40,
                "focus": {
                    "app_frontmost": False,
                    "key_window": False,
                    "render_view_first_responder": True,
                },
            }
        )

        self.assertEqual(len(harness._frontmost_application_timeline), 1)
        entry = harness._frontmost_application_timeline[0]
        self.assertEqual(entry["boundary"], "focus_loss")
        self.assertEqual(entry["event_seq"], 42)
        self.assertEqual(entry["focus"]["app_frontmost"], False)
        self.assertEqual(entry["identity"]["bundle_identifier"], "com.apple.Terminal")

    def test_frontmost_probe_failure_is_recorded_without_masking_focus_loss(self) -> None:
        harness = RuntimeHarness.__new__(RuntimeHarness)
        harness._frontmost_tracking_enabled = True
        harness._frontmost_focus_all_true = True
        harness._frontmost_application_timeline = []
        harness._current_phase = "warm_closed"

        def unavailable_probe() -> dict[str, object]:
            raise ContractError(
                "frontmost.identity_query_failed", "probe unavailable"
            )

        harness._frontmost_identity_probe = unavailable_probe
        focus_event = {
            "event": "input.focus.state",
            "seq": 43,
            "monotonic_ns": 50,
            "focus": {
                "app_frontmost": False,
                "key_window": True,
                "render_view_first_responder": True,
            },
        }

        harness._observe_focus_event(focus_event)

        self.assertEqual(len(harness._frontmost_application_timeline), 1)
        entry = harness._frontmost_application_timeline[0]
        self.assertEqual(entry["event_seq"], 43)
        self.assertEqual(entry["focus"]["app_frontmost"], False)
        self.assertEqual(entry["error"]["code"], "frontmost.identity_query_failed")

    def test_cli_dry_run_records_and_reuses_manifest_owned_result(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = ManifestFixture(root)
            command = [
                sys.executable,
                str(PACKAGE_ROOT / "t1-preflight"),
                "--mode",
                "dry-run",
                "--app",
                str(root / "Missing.app"),
                "--manifest",
                str(fixture.path),
            ]
            first = subprocess.run(command, check=False, capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            result_path = root / "output" / "result.json"
            result = json.loads(result_path.read_text())
            self.assertEqual(result["status"], "DRY_RUN")
            self.assertEqual(
                [phase["phase"] for phase in result["phases"]], list(PHASES)
            )
            self.assertEqual(
                [
                    next(
                        value
                        for value in phase["argv"]
                        if value in {"browser-closed", "browser-included"}
                    )
                    for phase in result["phases"]
                ],
                [
                    "browser-closed",
                    "browser-closed",
                    "browser-included",
                    "browser-closed",
                ],
            )
            second = subprocess.run(
                command, check=False, capture_output=True, text=True
            )
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertIn("t1_preflight.reused", second.stderr)


if __name__ == "__main__":
    unittest.main()
