from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PACKAGE_ROOT))
sys.path.insert(0, str(PACKAGE_ROOT.parent / "t1-preflight"))

from t14_e2e.model import (  # noqa: E402
    ContractError,
    E2EManifest,
    FixtureResource,
    compare_snapshots,
    load_manifest,
    validate_exact_cleanup,
)
from t14_e2e.runtime import E2ERunner, _sanitize_snapshot  # noqa: E402
from t1_preflight.macos import (  # noqa: E402
    screen_window_position,
    select_retina_screen,
)
from t1_preflight.model import ContractError as T1ContractError  # noqa: E402


def write_fixture(root: Path, *, mutate: Any | None = None) -> tuple[Path, Path]:
    scenario = {
        "schema": "herdr.ide.t14-e2e.scenario.v1",
        "scenario_id": "test-run",
        "run_id": "test-run",
        "verification_profile": "test-profile",
        "actions": [
            {"id": "ax", "action": "ax.snapshot"},
            {"id": "digit-eight", "action": "native_input", "key_code": 28, "modifiers": [], "expected_bytes_hex": "38"},
            {"id": "shot", "action": "screenshot.capture", "window_id": 9},
            {"id": "after", "action": "herdr.snapshot"},
        ],
    }
    scenario_path = root / "scenario.json"
    manifest = {
        "schema": "herdr.ide.t14-e2e.manifest.v1",
        "run_id": "test-run",
        "verification_profile": "test-profile",
        "working_directory": ".",
        "harness": "tools/t14-e2e/t14-e2e",
        "app": {
            "path": "bundle.app",
            "executable": "bundle.app/Contents/MacOS/app",
            "bundle_kind": "installed",
            "bundle_identifier": "dev.herdr.test",
        },
        "scenario": {"path": "scenario.json"},
        "output_dir": "evidence/test-run",
        "ports": [45791, 45792],
        "herdr_protocol": 21,
        "owned_resources": [
            {"kind": "workspace", "id": "workspace-test-run", "cleanup": "explicit-confirmation"},
            {"kind": "pane", "id": "pane-test-run", "cleanup": "explicit-confirmation"},
        ],
    }
    if mutate is not None:
        mutate(manifest, scenario)
    scenario_path.write_text(json.dumps(scenario), encoding="utf-8")
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    return manifest_path, scenario_path


class FakeProcess:
    def __init__(self, pid: int = 701) -> None:
        self.pid = pid
        self.exit_code: int | None = None

    def poll(self) -> int | None:
        return self.exit_code

    def wait(self, timeout: float | None = None) -> int:
        self.exit_code = 0
        return 0


class FakeInventory:
    def __init__(self, *, pids: list[int] | None = None) -> None:
        self.pids = list(pids or [])
        self.path: Path | None = None

    def pids_for_executable(self, executable: Path) -> list[int]:
        return list(self.pids)

    def executable_for_pid(self, pid: int) -> Path | None:
        return self.path


class FakeProcessController:
    def __init__(self, inventory: FakeInventory) -> None:
        self.inventory = inventory
        self.launched: FakeProcess | None = None
        self.cleaned = 0

    def launch(self, executable: Path, *, cwd: Path, arguments: tuple[str, ...]) -> FakeProcess:
        self.launched = FakeProcess()
        self.inventory.pids = [self.launched.pid]
        return self.launched

    def terminate_owned(self, process: FakeProcess) -> dict[str, Any]:
        self.cleaned += 1
        process.exit_code = 0
        self.inventory.pids = []
        return {"status": "terminated-owned", "pid": process.pid, "exit_code": 0}


class FakeNative:
    def __init__(self, *, focus: dict[str, Any] | None = None) -> None:
        self.focus = focus or {
            "app_frontmost": True,
            "key_window": True,
            "render_view_first_responder": True,
            "frontmost_identity": {"bundle_identifier": "dev.herdr.test", "process_identifier": 701},
        }
        self.keys: list[tuple[int, list[str]]] = []

    def activate(self, pid: int) -> dict[str, Any]:
        return {"pid": pid, "status": "activated"}

    def focus_state(self, pid: int) -> dict[str, Any]:
        return dict(self.focus)

    def inject_key(self, pid: int, *, key_code: int, modifiers: list[str]) -> dict[str, Any]:
        self.keys.append((key_code, modifiers))
        return {"surface": "appkit", "pid": pid, "key_code": key_code, "modifiers": modifiers, "bytes_hex": "38"}

    def ax_snapshot(self, pid: int, destination: Path) -> dict[str, Any]:
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text('{"role":"window"}\n', encoding="utf-8")
        return {"path": str(destination), "sha256": "ax"}

    def screenshot(self, pid: int, destination: Path, *, window_id: int) -> dict[str, Any]:
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(b"PNG")
        return {"path": str(destination), "window_id": window_id, "sha256": "shot"}

    def owned_window_id(self, pid: int) -> int:
        return 9


class FakeSnapshots:
    def __init__(self) -> None:
        self.calls = 0

    def capture(self) -> dict[str, Any]:
        self.calls += 1
        return {
            "protocol": 21,
            "host": {"host_id": "local"},
            "workspaces": ["workspace-test-run"],
            "tabs": ["tab-test-run"],
            "panes": ["pane-test-run"],
            "agents": [],
            "lineage": [],
        }


class ContractTests(unittest.TestCase):
    def test_retina_screen_selection_prefers_main_and_converts_window_origin(self) -> None:
        inventory = {
            "screens": [
                {
                    "screen_index": 0,
                    "localized_name": "Built-in Retina Display",
                    "is_main": True,
                    "backing_scale_factor": 2.0,
                    "frame": {"x": 0.0, "y": 0.0, "width": 1728.0, "height": 1117.0},
                },
                {
                    "screen_index": 1,
                    "localized_name": "External",
                    "is_main": False,
                    "backing_scale_factor": 1.0,
                    "frame": {"x": -1920.0, "y": 0.0, "width": 1920.0, "height": 1080.0},
                },
            ]
        }
        selected = select_retina_screen(inventory)
        self.assertEqual(selected["localized_name"], "Built-in Retina Display")
        self.assertEqual(screen_window_position(selected, inventory), {"x": 24, "y": 24})

    def test_retina_screen_selection_fails_closed_without_scale_two_display(self) -> None:
        inventory = {
            "screens": [
                {
                    "screen_index": 0,
                    "localized_name": "External",
                    "is_main": True,
                    "backing_scale_factor": 1.0,
                    "frame": {"x": 0.0, "y": 0.0, "width": 1920.0, "height": 1080.0},
                }
            ]
        }
        with self.assertRaises(T1ContractError) as context:
            select_retina_screen(inventory)
        self.assertEqual(context.exception.code, "screen.retina_missing")

    def test_manifest_loads_exact_paths_and_hashes_scenario(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path, _ = write_fixture(root)
            manifest = load_manifest(manifest_path, root=root)
            self.assertEqual(manifest.run_id, "test-run")
            self.assertEqual(len(manifest.scenario_sha256), 64)
            self.assertEqual(manifest.exact_command[-1], "run")
            self.assertEqual(len(manifest.owned_resources), 2)

    def test_manifest_rejects_absolute_and_glob_paths(self) -> None:
        for field, value in (("output_dir", "/tmp/evidence"), ("app.path", "bundle-*.app")):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)

                def mutate(manifest: dict[str, Any], scenario: dict[str, Any]) -> None:
                    if field == "output_dir":
                        manifest[field] = value
                    else:
                        manifest["app"]["path"] = value

                manifest_path, _ = write_fixture(root, mutate=mutate)
                with self.assertRaises(ContractError):
                    load_manifest(manifest_path, root=root)

    def test_manifest_keeps_exact_app_path_when_generated_target_is_sibling_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as cache:
            root = Path(directory)
            (root / "target").symlink_to(Path(cache), target_is_directory=True)
            manifest_path, _ = write_fixture(root, mutate=lambda manifest, scenario: manifest["app"].update({
                "path": "target/bundle.app",
                "executable": "target/bundle.app/Contents/MacOS/app",
            }))
            manifest = load_manifest(manifest_path, root=root)
            self.assertEqual(manifest.app_path, root.resolve() / "target/bundle.app")

    def test_manifest_rejects_duplicate_fixture_and_action_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def mutate(manifest: dict[str, Any], scenario: dict[str, Any]) -> None:
                manifest["owned_resources"].append(manifest["owned_resources"][0])

            manifest_path, _ = write_fixture(root, mutate=mutate)
            with self.assertRaisesRegex(ContractError, "unique"):
                load_manifest(manifest_path, root=root)

    def test_snapshot_comparison_checks_protocol_and_stable_ids(self) -> None:
        before = {"protocol": 21, "host": {"host_id": "local"}, "workspaces": [{"workspace_id": "w"}], "panes": [{"pane_id": "p"}]}
        after = {"protocol": 21, "host": {"host_id": "local"}, "workspaces": [{"workspace_id": "w"}], "panes": [{"pane_id": "p2"}]}
        result = compare_snapshots(before, after, expected_protocol=21)
        self.assertTrue(result["changed_id_sets"]["panes"])
        with self.assertRaisesRegex(ContractError, "protocol"):
            compare_snapshots(before, {"protocol": 19}, expected_protocol=21)

    def test_snapshot_sanitizer_unwraps_herdr_cli_json_rpc_envelope(self) -> None:
        snapshot = _sanitize_snapshot({
            "id": "cli:api:snapshot",
            "result": {
                "snapshot": {
                    "protocol": 21,
                    "host": {"host_id": "local", "display_name": "redacted"},
                    "workspaces": [{"workspace_id": "workspace-test-run"}],
                    "panes": [{"pane_id": "pane-test-run"}],
                }
            },
        })
        self.assertEqual(snapshot["protocol"], 21)
        self.assertEqual(snapshot["workspaces"], ["workspace-test-run"])
        self.assertEqual(snapshot["panes"], ["pane-test-run"])

    def test_exact_cleanup_never_expands_to_prefix_or_glob(self) -> None:
        resource = FixtureResource("pane", "pane-test-run", "explicit-confirmation")
        cleanup = validate_exact_cleanup([resource])
        self.assertFalse(cleanup["automatic_cleanup"])
        self.assertIn("prefix", cleanup["forbidden"])
        with self.assertRaises(ContractError):
            FixtureResource.from_raw({"kind": "pane", "id": "pane-*", "cleanup": "explicit-confirmation"}, 0)

    def test_runner_requires_zero_preexisting_exact_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path, _ = write_fixture(root)
            manifest = load_manifest(manifest_path, root=root)
            inventory = FakeInventory(pids=[12])
            process = FakeProcessController(inventory)
            runner = E2ERunner(
                manifest,
                inventory=inventory,
                process=process,
                native=FakeNative(),
                snapshots=FakeSnapshots(),
                bundle_inspector=lambda _: {"bundle_kind": "installed", "bundle_identifier": "dev.herdr.test", "sha256": "bundle"},
            )
            with self.assertRaisesRegex(ContractError, "already"):
                runner.run(require_hands_off=False)
            self.assertIsNone(process.launched)
            self.assertTrue((manifest.output_dir / "failure.json").is_file())

    def test_runner_checks_focus_before_key_and_cleans_only_owned_pid(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path, _ = write_fixture(root)
            manifest = load_manifest(manifest_path, root=root)
            inventory = FakeInventory()
            inventory.path = manifest.executable
            process = FakeProcessController(inventory)
            native = FakeNative()
            runner = E2ERunner(
                manifest,
                inventory=inventory,
                process=process,
                native=native,
                snapshots=FakeSnapshots(),
                bundle_inspector=lambda _: {"bundle_kind": "installed", "bundle_identifier": "dev.herdr.test", "sha256": "bundle"},
            )
            result = runner.run(require_hands_off=False)
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(native.keys, [(28, [])])
            self.assertEqual(process.cleaned, 1)
            self.assertEqual(result["fixture_ownership"]["mode"], "exact-identities-only")

    def test_runner_resolves_owned_main_window_before_capture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def mutate(manifest: dict[str, Any], scenario: dict[str, Any]) -> None:
                scenario["actions"][2]["window_id"] = "owned-main"

            manifest_path, _ = write_fixture(root, mutate=mutate)
            manifest = load_manifest(manifest_path, root=root)
            inventory = FakeInventory()
            inventory.path = manifest.executable
            process = FakeProcessController(inventory)
            result = E2ERunner(
                manifest,
                inventory=inventory,
                process=process,
                native=FakeNative(),
                snapshots=FakeSnapshots(),
                bundle_inspector=lambda _: {"bundle_kind": "installed", "bundle_identifier": "dev.herdr.test", "sha256": "bundle"},
            ).run(require_hands_off=False)
            screenshot = next(action for action in result["actions"] if action["action"] == "screenshot.capture")
            self.assertEqual(screenshot["evidence"]["requested_window_id"], "owned-main")
            self.assertEqual(screenshot["evidence"]["window_id"], 9)

    def test_runner_fails_closed_on_focus_interference_before_injection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path, _ = write_fixture(root)
            manifest = load_manifest(manifest_path, root=root)
            inventory = FakeInventory()
            inventory.path = manifest.executable
            process = FakeProcessController(inventory)
            native = FakeNative(focus={"app_frontmost": False, "key_window": True, "render_view_first_responder": True})
            runner = E2ERunner(
                manifest,
                inventory=inventory,
                process=process,
                native=native,
                snapshots=FakeSnapshots(),
                bundle_inspector=lambda _: {"bundle_kind": "installed", "bundle_identifier": "dev.herdr.test", "sha256": "bundle"},
            )
            with self.assertRaisesRegex(ContractError, "focus"):
                runner.run(require_hands_off=False)
            self.assertEqual(native.keys, [])
            self.assertTrue((manifest.output_dir / "failure.json").is_file())

    def test_runner_rejects_route_bytes_that_do_not_match_action_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path, _ = write_fixture(root)
            manifest = load_manifest(manifest_path, root=root)
            inventory = FakeInventory()
            inventory.path = manifest.executable
            process = FakeProcessController(inventory)
            native = FakeNative()
            native.inject_key = lambda pid, *, key_code, modifiers: {
                "surface": "appkit",
                "pid": pid,
                "key_code": key_code,
                "modifiers": modifiers,
                "bytes_hex": "1b 38",
            }
            runner = E2ERunner(
                manifest,
                inventory=inventory,
                process=process,
                native=native,
                snapshots=FakeSnapshots(),
                bundle_inspector=lambda _: {"bundle_kind": "installed", "bundle_identifier": "dev.herdr.test", "sha256": "bundle"},
            )
            with self.assertRaisesRegex(ContractError, "expected bytes"):
                runner.run(require_hands_off=False)
            self.assertEqual(process.cleaned, 1)

    def test_runner_rejects_frontmost_identity_mismatch_even_when_focus_booleans_are_true(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path, _ = write_fixture(root)
            manifest = load_manifest(manifest_path, root=root)
            inventory = FakeInventory()
            inventory.path = manifest.executable
            process = FakeProcessController(inventory)
            native = FakeNative(
                focus={
                    "app_frontmost": True,
                    "key_window": True,
                    "render_view_first_responder": True,
                    "frontmost_identity": {"bundle_identifier": "dev.other", "process_identifier": 701},
                }
            )
            runner = E2ERunner(
                manifest,
                inventory=inventory,
                process=process,
                native=native,
                snapshots=FakeSnapshots(),
                bundle_inspector=lambda _: {"bundle_kind": "installed", "bundle_identifier": "dev.herdr.test", "sha256": "bundle"},
            )
            with self.assertRaisesRegex(ContractError, "frontmost"):
                runner.run(require_hands_off=False)
            self.assertEqual(native.keys, [])


if __name__ == "__main__":
    unittest.main()
