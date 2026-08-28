from __future__ import annotations

import json
import ctypes
import os
import plistlib
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Protocol

from .model import (
    ContractError,
    E2EManifest,
    compare_snapshots,
    sha256_tree,
    validate_exact_cleanup,
)


class ProcessHandle(Protocol):
    pid: int

    def poll(self) -> int | None: ...

    def wait(self, timeout: float | None = None) -> int: ...


class ProcessController(Protocol):
    def launch(self, executable: Path, *, cwd: Path, arguments: tuple[str, ...]) -> ProcessHandle: ...

    def terminate_owned(self, process: ProcessHandle) -> dict[str, Any]: ...


class InstanceInventory(Protocol):
    def pids_for_executable(self, executable: Path) -> list[int]: ...

    def executable_for_pid(self, pid: int) -> Path | None: ...


class NativeDriver(Protocol):
    def activate(self, pid: int) -> dict[str, Any]: ...

    def focus_state(self, pid: int) -> dict[str, Any]: ...

    def inject_key(self, pid: int, *, key_code: int, modifiers: list[str]) -> dict[str, Any]: ...

    def ax_snapshot(self, pid: int, destination: Path) -> dict[str, Any]: ...

    def screenshot(self, pid: int, destination: Path, *, window_id: int) -> dict[str, Any]: ...

    def owned_window_id(self, pid: int) -> int: ...


class SnapshotClient(Protocol):
    def capture(self) -> dict[str, Any]: ...


class BundleInspector(Protocol):
    def __call__(self, manifest: E2EManifest) -> dict[str, Any]: ...


@dataclass
class SubprocessHandle:
    process: subprocess.Popen[bytes]
    executable: Path

    @property
    def pid(self) -> int:
        return self.process.pid

    def poll(self) -> int | None:
        return self.process.poll()

    def wait(self, timeout: float | None = None) -> int:
        return self.process.wait(timeout=timeout)


class SubprocessController:
    def launch(self, executable: Path, *, cwd: Path, arguments: tuple[str, ...]) -> SubprocessHandle:
        if not executable.is_file() or not os.access(executable, os.X_OK):
            raise ContractError(
                "process.executable_missing",
                "declared native app executable is missing or not executable",
                executable=str(executable),
            )
        try:
            process = subprocess.Popen(
                [str(executable), *arguments],
                cwd=str(cwd),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        except OSError as error:
            raise ContractError(
                "process.launch_failed",
                "declared native app could not be launched",
                executable=str(executable),
                error=repr(error),
            ) from error
        return SubprocessHandle(process=process, executable=executable.resolve(strict=False))

    def terminate_owned(self, process: ProcessHandle) -> dict[str, Any]:
        if process.poll() is not None:
            return {"status": "already-exited", "pid": process.pid, "exit_code": process.poll()}
        try:
            if isinstance(process, SubprocessHandle):
                process.process.terminate()
                exit_code = process.process.wait(timeout=2)
            else:
                raise ContractError(
                    "process.cleanup_unowned",
                    "cleanup requires the process handle returned by this run",
                    pid=process.pid,
                )
        except subprocess.TimeoutExpired as error:
            # This is still the exact process created by this run. Escalating only
            # that PID avoids the broad kill patterns the fixture contract forbids.
            process.process.kill()
            exit_code = process.process.wait(timeout=2)
            return {
                "status": "killed-owned-timeout",
                "pid": process.pid,
                "exit_code": exit_code,
                "cause": repr(error),
            }
        return {"status": "terminated-owned", "pid": process.pid, "exit_code": exit_code}


class MacOSInstanceInventory:
    def __init__(self) -> None:
        package = Path(__file__).resolve().parents[2] / "t1-preflight"
        if str(package) not in sys.path:
            sys.path.insert(0, str(package))
        try:
            from t1_preflight.macos import executable_path_for_pid, pids_for_executable
        except Exception as error:
            raise ContractError(
                "inventory.adapter_unavailable",
                "the T1 macOS process identity adapter is unavailable",
                error=repr(error),
            ) from error
        self._executable_path_for_pid = executable_path_for_pid
        self._pids_for_executable = pids_for_executable

    def pids_for_executable(self, executable: Path) -> list[int]:
        return self._pids_for_executable(executable)

    def executable_for_pid(self, pid: int) -> Path | None:
        return self._executable_path_for_pid(pid)


def _t1_adapters() -> tuple[Any, ...]:
    package = Path(__file__).resolve().parents[2] / "t1-preflight"
    if str(package) not in sys.path:
        sys.path.insert(0, str(package))
    try:
        from t1_preflight.macos import (
            capture_accessibility,
            capture_window,
            frontmost_application_identity,
            place_window_on_screen,
            screen_inventory,
            select_retina_screen,
            screen_window_position,
        )
        from t1_preflight.model import ContractError as T1ContractError
        from t1_preflight.native_input import NativeInput
    except Exception as error:
        raise ContractError(
            "native.adapter_unavailable",
            "the existing T1 native input/screenshot adapter is unavailable",
            error=repr(error),
        ) from error
    return (
        NativeInput,
        capture_accessibility,
        capture_window,
        frontmost_application_identity,
        place_window_on_screen,
        screen_inventory,
        select_retina_screen,
        screen_window_position,
        T1ContractError,
    )


class MacOSNativeDriver:
    """The T14 native boundary delegates to the already-approved T1 adapters."""

    def __init__(self, manifest: E2EManifest) -> None:
        (
            NativeInput,
            capture_accessibility,
            capture_window,
            frontmost_identity,
            place_window_on_screen,
            screen_inventory,
            select_retina_screen,
            screen_window_position,
            adapter_error_type,
        ) = _t1_adapters()
        self._native = NativeInput()
        self._capture_accessibility = capture_accessibility
        self._capture_window = capture_window
        self._frontmost_identity = frontmost_identity
        self._place_window_on_screen = place_window_on_screen
        self._screen_inventory = screen_inventory
        self._select_retina_screen = select_retina_screen
        self._screen_window_position = screen_window_position
        self._adapter_error_type = adapter_error_type
        self._manifest = manifest
        self._activate_script = Path(__file__).resolve().parents[2] / "t1-preflight/t1_preflight/activate_process.applescript"
        self._place_window_script = Path(__file__).resolve().parents[2] / "t1-preflight/t1_preflight/place_process_window.applescript"
        self._ax_script = Path(__file__).resolve().parents[2] / "t1-preflight/t1_preflight/ax_snapshot.applescript"
        self._core_graphics = ctypes.CDLL(
            "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics"
        )
        self._core_foundation = ctypes.CDLL(
            "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation"
        )
        self._configure_window_inventory()

    def retina_screen_target(self, *, minimum_scale_factor: float = 2.0) -> dict[str, Any]:
        inventory = self._screen_inventory()
        selected = self._select_retina_screen(
            inventory, minimum_scale_factor=minimum_scale_factor
        )
        position = self._screen_window_position(selected, inventory)
        return {
            "inventory": inventory,
            "selected": selected,
            "window_position": position,
            "minimum_scale_factor": minimum_scale_factor,
        }

    def place_on_retina_screen(
        self, pid: int, *, minimum_scale_factor: float = 2.0
    ) -> dict[str, Any]:
        target = self.retina_screen_target(minimum_scale_factor=minimum_scale_factor)
        evidence = self._place_window_on_screen(
            pid,
            target["window_position"],
            self._place_window_script,
        )
        return {
            **target,
            "placement": evidence,
        }

    def _configure_window_inventory(self) -> None:
        cg = self._core_graphics
        cf = self._core_foundation
        cg.CGWindowListCopyWindowInfo.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
        cg.CGWindowListCopyWindowInfo.restype = ctypes.c_void_p
        cf.CFArrayGetCount.argtypes = [ctypes.c_void_p]
        cf.CFArrayGetCount.restype = ctypes.c_long
        cf.CFArrayGetValueAtIndex.argtypes = [ctypes.c_void_p, ctypes.c_long]
        cf.CFArrayGetValueAtIndex.restype = ctypes.c_void_p
        cf.CFDictionaryGetValue.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        cf.CFDictionaryGetValue.restype = ctypes.c_void_p
        cf.CFNumberGetValue.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
        cf.CFNumberGetValue.restype = ctypes.c_bool
        cf.CFRelease.argtypes = [ctypes.c_void_p]
        cf.CFStringGetCString.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_long,
            ctypes.c_uint32,
        ]
        cf.CFStringGetCString.restype = ctypes.c_bool
        self._window_keys = {
            name: ctypes.c_void_p.in_dll(cg, name).value
            for name in (
                "kCGWindowOwnerPID",
                "kCGWindowNumber",
                "kCGWindowLayer",
                "kCGWindowOwnerName",
            )
        }

    def owned_window_id(self, pid: int) -> int:
        windows = self._core_graphics.CGWindowListCopyWindowInfo(3, 0)
        if not windows:
            raise ContractError(
                "screenshot.window_inventory_missing",
                "CoreGraphics returned no on-screen windows for the owned app",
                pid=pid,
            )
        try:
            count = self._core_foundation.CFArrayGetCount(windows)
            candidates: list[int] = []
            for index in range(count):
                item = self._core_foundation.CFArrayGetValueAtIndex(windows, index)
                if not item:
                    continue
                owner_pid = ctypes.c_longlong()
                owner_pid_value = self._core_foundation.CFDictionaryGetValue(
                    item, self._window_keys["kCGWindowOwnerPID"]
                )
                if not owner_pid_value or not self._core_foundation.CFNumberGetValue(
                    owner_pid_value, 4, ctypes.byref(owner_pid)
                ):
                    continue
                if owner_pid.value != pid:
                    continue
                layer = ctypes.c_longlong()
                layer_value = self._core_foundation.CFDictionaryGetValue(
                    item, self._window_keys["kCGWindowLayer"]
                )
                if not layer_value or not self._core_foundation.CFNumberGetValue(
                    layer_value, 4, ctypes.byref(layer)
                ):
                    continue
                if layer.value != 0:
                    continue
                number = ctypes.c_longlong()
                number_value = self._core_foundation.CFDictionaryGetValue(
                    item, self._window_keys["kCGWindowNumber"]
                )
                if number_value and self._core_foundation.CFNumberGetValue(
                    number_value, 4, ctypes.byref(number)
                ) and number.value > 0:
                    candidates.append(int(number.value))
            if not candidates:
                raise ContractError(
                    "screenshot.window_missing",
                    "CoreGraphics found no layer-zero window for the owned app",
                    pid=pid,
                )
            return candidates[0]
        finally:
            self._core_foundation.CFRelease(windows)

    def _invoke_adapter(self, operation: str, callback: Callable[[], Any]) -> Any:
        try:
            return callback()
        except self._adapter_error_type as error:
            details = getattr(error, "details", {})
            raise ContractError(
                "native.adapter_failed",
                "the approved macOS adapter reported a structured failure",
                operation=operation,
                adapter_code=getattr(error, "code", "unknown"),
                adapter_details=details if isinstance(details, dict) else {},
            ) from error

    def _adapter_error(self, operation: str, error: Exception) -> dict[str, Any]:
        details = getattr(error, "details", {})
        return {
            "schema": "herdr.ide.t14-e2e.adapter-error.v1",
            "operation": operation,
            "code": getattr(error, "code", "unknown"),
            "details": details if isinstance(details, dict) else {},
        }

    def activate(self, pid: int) -> dict[str, Any]:
        return self._invoke_adapter(
            "activate", lambda: self._native.activate(pid, self._activate_script)
        )

    def focus_state(self, pid: int) -> dict[str, Any]:
        if self._manifest.telemetry_path is None:
            raise ContractError(
                "focus.telemetry_missing",
                "T14 native input requires the app-owned focus telemetry path",
                pid=pid,
            )
        if not self._manifest.telemetry_path.is_file():
            raise ContractError(
                "focus.telemetry_missing",
                "the app-owned focus telemetry file does not exist",
                path=str(self._manifest.telemetry_path),
            )
        latest: dict[str, Any] | None = None
        for line in self._manifest.telemetry_path.read_text(encoding="utf-8").splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError as error:
                raise ContractError("focus.telemetry_invalid", "focus telemetry is not valid JSONL", error=repr(error)) from error
            if event.get("event") == "input.focus.state" and isinstance(event.get("focus"), dict):
                latest = event
        if latest is None:
            raise ContractError("focus.telemetry_missing", "app has not emitted a focus snapshot", pid=pid)
        focus = dict(latest["focus"])
        try:
            identity = self._frontmost_identity()
        except self._adapter_error_type as error:
            focus["frontmost_identity_error"] = self._adapter_error(
                "frontmost_identity", error
            )
        else:
            focus["frontmost_identity"] = identity.get("identity")
        focus["telemetry_seq"] = latest.get("seq")
        return focus

    def inject_key(self, pid: int, *, key_code: int, modifiers: list[str]) -> dict[str, Any]:
        self._invoke_adapter(
            "inject_key", lambda: self._native.key(key_code, modifiers)
        )
        return {"surface": "appkit", "pid": pid, "key_code": key_code, "modifiers": list(modifiers)}

    def ax_snapshot(self, pid: int, destination: Path) -> dict[str, Any]:
        return self._invoke_adapter(
            "ax_snapshot",
            lambda: self._capture_accessibility(pid, self._ax_script, destination),
        )

    def screenshot(self, pid: int, destination: Path, *, window_id: int) -> dict[str, Any]:
        evidence = self._invoke_adapter(
            "screenshot",
            lambda: self._capture_window(window_id, destination),
        )
        evidence["pid"] = pid
        return evidence


class CommandSnapshotClient:
    """Read a sanitized JSON snapshot through an explicit argv, never a shell."""

    def __init__(self, manifest: E2EManifest) -> None:
        if manifest.herdr_snapshot_command is None:
            raise ContractError(
                "snapshot.command_missing",
                "T14 run mode requires an explicit Herdr snapshot argv",
            )
        self._argv = list(manifest.herdr_snapshot_command)

    def capture(self) -> dict[str, Any]:
        try:
            result = subprocess.run(
                self._argv,
                check=False,
                capture_output=True,
                text=True,
                timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ContractError(
                "snapshot.command_failed",
                "Herdr snapshot command could not complete",
                argv=self._argv,
                error=repr(error),
            ) from error
        if result.returncode != 0:
            raise ContractError(
                "snapshot.command_failed",
                "Herdr snapshot command exited non-zero",
                argv=self._argv,
                exit_code=result.returncode,
                stderr=result.stderr[-1000:],
            )
        try:
            value = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise ContractError("snapshot.invalid_json", "Herdr snapshot command returned invalid JSON", error=repr(error)) from error
        return _sanitize_snapshot(value)


def _sanitize_snapshot(value: Any) -> dict[str, Any]:
    # Herdr's CLI returns the JSON-RPC envelope, while test fixtures may
    # provide the snapshot object directly. Normalize both at this boundary
    # so protocol validation observes the same typed payload.
    if isinstance(value, dict):
        result_value = value.get("result")
        if isinstance(result_value, dict) and isinstance(result_value.get("snapshot"), dict):
            value = result_value["snapshot"]
    if not isinstance(value, dict):
        raise ContractError("snapshot.invalid", "Herdr snapshot must be a JSON object")
    allowed = {"protocol", "protocol_revision", "event_sequence", "host", "focused_workspace_id", "focused_tab_id", "focused_pane_id"}
    result: dict[str, Any] = {key: value[key] for key in allowed if key in value}
    host = value.get("host")
    if isinstance(host, dict):
        result["host"] = {
            key: host[key]
            for key in ("host_id", "kind", "remote", "display_name")
            if key in host and isinstance(host[key], (str, bool, int, float))
        }
    elif host is not None:
        result["host"] = {"kind": "present-but-redacted"}
    for field in ("workspaces", "tabs", "panes", "agents", "lineage"):
        raw = value.get(field, [])
        if not isinstance(raw, list):
            result[field] = []
            continue
        ids: list[str] = []
        for item in raw:
            if isinstance(item, str):
                ids.append(item)
            elif isinstance(item, dict):
                for key in ("id", "workspace_id", "tab_id", "pane_id", "agent_instance_id"):
                    candidate = item.get(key)
                    if isinstance(candidate, str):
                        ids.append(candidate)
                        break
        result[field] = sorted(set(ids))
    return result


class OutputStore:
    MARKER = ".t14-e2e-owner.json"

    def __init__(self, manifest: E2EManifest) -> None:
        self.manifest = manifest
        self.marker_path = manifest.output_dir / self.MARKER
        self.result_path = manifest.output_dir / "result.json"

    def prepare(self) -> dict[str, Any] | None:
        if not self.manifest.output_dir.exists():
            self.manifest.output_dir.mkdir(parents=True)
            self.write(self.MARKER, self.manifest.ownership_marker())
            return None
        if self.manifest.output_dir.is_symlink() or not self.manifest.output_dir.is_dir():
            raise ContractError("output.unsafe_existing_path", "T14 output must be a real directory", path=str(self.manifest.output_dir))
        if not self.marker_path.is_file():
            raise ContractError("output.unowned", "existing T14 output has no ownership marker", path=str(self.manifest.output_dir))
        try:
            marker = json.loads(self.marker_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise ContractError("output.marker_invalid", "T14 ownership marker is unreadable", error=repr(error)) from error
        if marker != self.manifest.ownership_marker():
            raise ContractError("output.identity_mismatch", "existing output belongs to different T14 inputs", path=str(self.manifest.output_dir))
        if not self.result_path.is_file():
            raise ContractError("output.incomplete", "owned output is incomplete; use a new run_id/output_dir", path=str(self.manifest.output_dir))
        try:
            result = json.loads(self.result_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise ContractError("output.result_invalid", "existing T14 result is unreadable", error=repr(error)) from error
        if result.get("run_id") != self.manifest.run_id:
            raise ContractError("output.result_identity_mismatch", "existing result run_id differs from manifest")
        return result

    def write(self, relative: str, value: Any) -> Path:
        relative_path = Path(relative)
        if relative_path.is_absolute() or ".." in relative_path.parts or any(character in relative for character in "*?[]{}"):
            raise ContractError("output.path_invalid", "output artifacts must use exact child paths", path=relative)
        target = (self.manifest.output_dir / relative_path).resolve(strict=False)
        try:
            target.relative_to(self.manifest.output_dir.resolve())
        except ValueError as error:
            raise ContractError("output.path_escape", "output artifact escapes the owned directory", path=relative) from error
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_name(f".{target.name}.tmp-{os.getpid()}")
        temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        os.replace(temporary, target)
        return target

    def finish(self, result: dict[str, Any]) -> Path:
        return self.write("result.json", result)


def _assert_focus(focus: dict[str, Any], *, pid: int, action: str, bundle_identifier: str) -> None:
    required = ("app_frontmost", "key_window", "render_view_first_responder")
    invalid = [field for field in required if not isinstance(focus.get(field), bool)]
    failed = {field: focus.get(field) for field in required if focus.get(field) is not True}
    identity = focus.get("frontmost_identity")
    identity_error = focus.get("frontmost_identity_error")
    identity_mismatch = isinstance(identity, dict) and (
        identity.get("process_identifier") not in (None, pid)
        or identity.get("bundle_identifier") not in (None, bundle_identifier)
    )
    if invalid or failed or identity_mismatch or identity_error is not None:
        raise ContractError(
            "environment_focus_interference",
            "native input was aborted because target focus or frontmost identity was not proven",
            pid=pid,
            action=action,
            invalid_fields=invalid,
            failed_checks=failed,
            frontmost_identity=identity,
            frontmost_identity_error=identity_error,
            expected_bundle_identifier=bundle_identifier,
        )


def inspect_bundle(manifest: E2EManifest) -> dict[str, Any]:
    if sys.platform != "darwin":
        raise ContractError("platform.unsupported", "T14 installed-app inspection requires macOS", platform=sys.platform)
    app_path = manifest.app_path.resolve(strict=True)
    if app_path.suffix != ".app" or not app_path.is_dir():
        raise ContractError("bundle.invalid_path", "app.path must be a .app directory", path=str(app_path))
    info_path = app_path / "Contents/Info.plist"
    executable_name = app_path / "Contents/MacOS" / manifest.executable.name
    if not info_path.is_file() or not executable_name.is_file():
        raise ContractError("bundle.incomplete", "declared app bundle lacks Info.plist or main executable", path=str(app_path))
    try:
        info = plistlib.loads(info_path.read_bytes())
    except (OSError, plistlib.InvalidFileException) as error:
        raise ContractError("bundle.info_invalid", "Info.plist is not valid", error=repr(error)) from error
    actual_identifier = info.get("CFBundleIdentifier")
    actual_executable = info.get("CFBundleExecutable")
    if actual_identifier != manifest.bundle_identifier or actual_executable != manifest.executable.name:
        raise ContractError(
            "bundle.identity_mismatch",
            "installed bundle identity differs from the owned manifest",
            expected={"bundle_identifier": manifest.bundle_identifier, "executable": manifest.executable.name},
            actual={"bundle_identifier": actual_identifier, "executable": actual_executable},
        )
    digest = sha256_tree(app_path)
    if manifest.expected_bundle_sha256 is not None and digest != manifest.expected_bundle_sha256:
        raise ContractError("bundle.hash_mismatch", "installed bundle SHA-256 differs from manifest", expected=manifest.expected_bundle_sha256, actual=digest)
    return {
        "path": str(app_path),
        "bundle_kind": manifest.bundle_kind,
        "bundle_identifier": actual_identifier,
        "main_executable": actual_executable,
        "sha256": digest,
    }


class E2ERunner:
    def __init__(
        self,
        manifest: E2EManifest,
        *,
        inventory: InstanceInventory,
        process: ProcessController,
        native: NativeDriver,
        snapshots: SnapshotClient,
        bundle_inspector: BundleInspector = inspect_bundle,
        clock_ns: Callable[[], int] = time.monotonic_ns,
    ) -> None:
        self.manifest = manifest
        self.inventory = inventory
        self.process = process
        self.native = native
        self.snapshots = snapshots
        self.bundle_inspector = bundle_inspector
        self.clock_ns = clock_ns
        self.output = OutputStore(manifest)
        self.owned_process: ProcessHandle | None = None

    def run(self, *, require_hands_off: bool = True) -> dict[str, Any]:
        if require_hands_off and os.environ.get("HERDR_T14_EXCLUSIVE_HANDS_OFF") != "1":
            raise ContractError(
                "environment_focus_interference",
                "T14 physical mode requires an explicit exclusive hands-off marker",
                required_environment={"HERDR_T14_EXCLUSIVE_HANDS_OFF": "1"},
            )
        reused = self.output.prepare()
        if reused is not None:
            return reused
        started_ns = self.clock_ns()
        bundle: dict[str, Any] | None = None
        before: dict[str, Any] | None = None
        after: dict[str, Any] | None = None
        actions: list[dict[str, Any]] = []
        cleanup: dict[str, Any] = {"status": "not-run"}
        try:
            bundle = self.bundle_inspector(self.manifest)
            existing = self.inventory.pids_for_executable(self.manifest.executable)
            if existing:
                raise ContractError(
                    "instance.preexisting",
                    "the exact target executable already has a running instance",
                    executable=str(self.manifest.executable),
                    pids=existing,
                )
            before = self.snapshots.capture()
            self.owned_process = self.process.launch(
                self.manifest.executable,
                cwd=self.manifest.root,
                arguments=self.manifest.launch_arguments,
            )
            self._assert_owned_instance()
            activation = self.native.activate(self.owned_process.pid)
            actions.append({"action": "activate", "evidence": activation})
            for index, action in enumerate(self.manifest.scenario["actions"]):
                action_name = str(action["action"])
                action_id = str(action.get("id", f"action-{index}"))
                self._assert_owned_instance()
                if action_name == "native_input":
                    focus = self.native.focus_state(self.owned_process.pid)
                    _assert_focus(
                        focus,
                        pid=self.owned_process.pid,
                        action=action_id,
                        bundle_identifier=self.manifest.bundle_identifier,
                    )
                    injected_at = self.clock_ns()
                    route = self.native.inject_key(
                        self.owned_process.pid,
                        key_code=int(action["key_code"]),
                        modifiers=list(action.get("modifiers", [])),
                    )
                    expected_bytes = action.get("expected_bytes_hex")
                    if expected_bytes is not None and (
                        not isinstance(expected_bytes, str)
                        or route.get("bytes_hex") != expected_bytes
                    ):
                        raise ContractError(
                            "input.route_mismatch",
                            "native input route did not prove the scenario's expected bytes",
                            action_id=action_id,
                            expected_bytes_hex=expected_bytes,
                            actual_bytes_hex=route.get("bytes_hex"),
                        )
                    actions.append(
                        {
                            "id": action_id,
                            "action": action_name,
                            "focus": focus,
                            "injected_at_monotonic_ns": injected_at,
                            "route": _safe_route(route),
                        }
                    )
                elif action_name == "ax.snapshot":
                    destination = self.manifest.output_dir / "ax" / f"{action_id}.json"
                    evidence = self.native.ax_snapshot(self.owned_process.pid, destination)
                    actions.append({"id": action_id, "action": action_name, "evidence": evidence})
                elif action_name == "screenshot.capture":
                    window_id = action.get("window_id")
                    if window_id == "owned-main":
                        window_id = self.native.owned_window_id(self.owned_process.pid)
                    elif isinstance(window_id, bool) or not isinstance(window_id, int) or window_id <= 0:
                        raise ContractError(
                            "screenshot.window_id_invalid",
                            "screenshot.capture requires a positive exact window_id or owned-main",
                            action_id=action_id,
                        )
                    destination = self.manifest.output_dir / "screenshots" / f"{action_id}.png"
                    evidence = self.native.screenshot(self.owned_process.pid, destination, window_id=window_id)
                    evidence["requested_window_id"] = action.get("window_id")
                    actions.append({"id": action_id, "action": action_name, "evidence": evidence})
                elif action_name == "herdr.snapshot":
                    snapshot = self.snapshots.capture()
                    actions.append({"id": action_id, "action": action_name, "snapshot": snapshot})
            after = self.snapshots.capture()
            comparison = compare_snapshots(before, after, expected_protocol=self.manifest.expected_protocol)
            cleanup = self._cleanup_owned()
            result = {
                "schema": "herdr.ide.t14-e2e.result.v1",
                "status": "PASS",
                "run_id": self.manifest.run_id,
                "verification_profile": self.manifest.verification_profile,
                "manifest_sha256": self.manifest.digest,
                "scenario_sha256": self.manifest.scenario_sha256,
                "bundle": bundle,
                "preflight": {"target_pids_before_launch": [], "owned_pid": self.owned_process.pid if self.owned_process else None},
                "herdr": {"before": before, "after": after, "comparison": comparison},
                "actions": actions,
                "fixture_ownership": validate_exact_cleanup(self.manifest.owned_resources),
                "cleanup": cleanup,
                "duration_ms": (self.clock_ns() - started_ns) / 1_000_000,
            }
            self.output.finish(result)
            return result
        except ContractError as error:
            cleanup = self._cleanup_owned()
            failure = error.as_dict()
            failure["cleanup"] = cleanup
            failure["run_id"] = self.manifest.run_id
            failure["manifest_sha256"] = self.manifest.digest
            failure["scenario_sha256"] = self.manifest.scenario_sha256
            failure["bundle"] = bundle
            failure["herdr"] = {"before": before, "after": after}
            failure["actions"] = actions
            failure["duration_ms"] = (self.clock_ns() - started_ns) / 1_000_000
            self.output.write("failure.json", failure)
            self.output.finish(
                {
                    "schema": "herdr.ide.t14-e2e.result.v1",
                    "status": "FAIL",
                    "run_id": self.manifest.run_id,
                    "manifest_sha256": self.manifest.digest,
                    "failure": "failure.json",
                }
            )
            raise

    def _assert_owned_instance(self) -> None:
        if self.owned_process is None:
            raise ContractError("instance.owner_missing", "T14 has no owned process handle")
        pids = self.inventory.pids_for_executable(self.manifest.executable)
        if pids != [self.owned_process.pid]:
            raise ContractError(
                "instance.not_exactly_one",
                "T14 requires exactly one running target and it must be this run's PID",
                expected=[self.owned_process.pid],
                actual=pids,
            )
        observed_path = self.inventory.executable_for_pid(self.owned_process.pid)
        if observed_path is not None and observed_path.resolve(strict=False) != self.manifest.executable.resolve(strict=False):
            raise ContractError(
                "instance.identity_mismatch",
                "running PID resolves to a different executable",
                pid=self.owned_process.pid,
                expected=str(self.manifest.executable),
                actual=str(observed_path),
            )
        if self.owned_process.poll() is not None:
            raise ContractError("instance.early_exit", "owned app exited before E2E actions completed", pid=self.owned_process.pid)

    def _cleanup_owned(self) -> dict[str, Any]:
        if self.owned_process is None:
            return {"status": "no-owned-process"}
        pid = self.owned_process.pid
        cleanup = self.process.terminate_owned(self.owned_process)
        remaining = self.inventory.pids_for_executable(self.manifest.executable)
        cleanup["remaining_target_pids"] = remaining
        if remaining:
            raise ContractError(
                "instance.leaked",
                "owned target executable remains after exact cleanup",
                pid=pid,
                remaining=remaining,
            )
        return cleanup


def _safe_route(route: Any) -> dict[str, Any]:
    if not isinstance(route, dict):
        return {"value_type": type(route).__name__}
    allowed = {
        key: route[key]
        for key in ("surface", "pid", "key_code", "modifiers", "event", "bytes_hex", "sequence")
        if key in route
    }
    return allowed
