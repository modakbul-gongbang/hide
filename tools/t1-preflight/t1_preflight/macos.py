from __future__ import annotations

import ctypes
import hashlib
import json
import os
import plistlib
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable

from .model import ContractError, Manifest, sha256_file


FRONTMOST_IDENTITY_FIELDS = (
    "bundle_identifier",
    "localized_name",
    "process_identifier",
    "executable_name",
)


class _NSPoint(ctypes.Structure):
    _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]


class _NSSize(ctypes.Structure):
    _fields_ = [("width", ctypes.c_double), ("height", ctypes.c_double)]


class _NSRect(ctypes.Structure):
    _fields_ = [("origin", _NSPoint), ("size", _NSSize)]


class _ObjectiveCRuntime:
    """Minimal typed objc_msgSend boundary for the macOS identity query."""

    def __init__(self) -> None:
        if sys.platform != "darwin":
            raise ContractError(
                "frontmost.objc_platform_unavailable",
                "NSWorkspace identity requires the macOS Objective-C runtime",
                platform=sys.platform,
            )
        try:
            self._objc = ctypes.CDLL("/usr/lib/libobjc.A.dylib")
            # Keep both framework handles alive for the duration of every query.
            self._foundation = ctypes.CDLL(
                "/System/Library/Frameworks/Foundation.framework/Foundation"
            )
            self._appkit = ctypes.CDLL(
                "/System/Library/Frameworks/AppKit.framework/AppKit"
            )
        except OSError as error:
            raise ContractError(
                "frontmost.objc_framework_unavailable",
                "Objective-C Foundation/AppKit framework could not be loaded",
                error=repr(error),
            ) from error
        try:
            self._objc_get_class = self._objc.objc_getClass
            self._objc_get_class.argtypes = [ctypes.c_char_p]
            self._objc_get_class.restype = ctypes.c_void_p
            self._sel_register_name = self._objc.sel_registerName
            self._sel_register_name.argtypes = [ctypes.c_char_p]
            self._sel_register_name.restype = ctypes.c_void_p
            self._objc_msg_send_address = ctypes.cast(
                self._objc.objc_msgSend, ctypes.c_void_p
            ).value
        except (AttributeError, TypeError) as error:
            raise ContractError(
                "frontmost.objc_runtime_unavailable",
                "Objective-C runtime entry points are unavailable",
                error=repr(error),
            ) from error
        if not self._objc_msg_send_address:
            raise ContractError(
                "frontmost.objc_runtime_unavailable",
                "objc_msgSend has no callable address",
            )

    def class_handle(self, name: str) -> int:
        handle = self._objc_get_class(name.encode("ascii"))
        if not handle:
            raise ContractError(
                "frontmost.objc_class_missing",
                "Objective-C class is unavailable",
                class_name=name,
            )
        return int(handle)

    def selector(self, name: str) -> int:
        selector = self._sel_register_name(name.encode("ascii"))
        if not selector:
            raise ContractError(
                "frontmost.objc_selector_missing",
                "Objective-C selector is unavailable",
                selector=name,
            )
        return int(selector)

    def send(
        self,
        receiver: int,
        selector: str,
        restype: Any = ctypes.c_void_p,
        *args: Any,
        argtypes: tuple[Any, ...] = (),
    ) -> Any:
        if not receiver:
            raise ContractError(
                "frontmost.objc_receiver_missing",
                "Objective-C message receiver is nil",
                selector=selector,
            )
        selector_handle = self.selector(selector)
        try:
            function_type = ctypes.CFUNCTYPE(
                restype,
                ctypes.c_void_p,
                ctypes.c_void_p,
                *argtypes,
            )
            function = function_type(self._objc_msg_send_address)
            return function(
                ctypes.c_void_p(receiver),
                ctypes.c_void_p(selector_handle),
                *args,
            )
        except (OSError, TypeError, ValueError) as error:
            raise ContractError(
                "frontmost.objc_message_failed",
                "Objective-C message dispatch failed",
                selector=selector,
                error=repr(error),
            ) from error

    def autorelease_pool(self) -> int:
        pool_class = self.class_handle("NSAutoreleasePool")
        pool = self.send(pool_class, "alloc")
        if not pool:
            raise ContractError(
                "frontmost.objc_pool_failed",
                "NSAutoreleasePool allocation returned nil",
            )
        pool = self.send(int(pool), "init")
        if not pool:
            raise ContractError(
                "frontmost.objc_pool_failed",
                "NSAutoreleasePool initialization returned nil",
            )
        return int(pool)


def _objc_utf8(runtime: _ObjectiveCRuntime, value: int, field: str) -> str:
    if not value:
        raise ContractError(
            "frontmost.identity_field_missing",
            "Objective-C string object is nil",
            field=field,
        )
    encoded = runtime.send(value, "UTF8String", ctypes.c_char_p)
    if not encoded:
        raise ContractError(
            "frontmost.identity_field_invalid",
            "NSRunningApplication identity field has no UTF-8 representation",
            field=field,
        )
    try:
        return encoded.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractError(
            "frontmost.identity_field_invalid",
            "NSRunningApplication identity field is not UTF-8",
            field=field,
        ) from error


def _objc_string(
    runtime: _ObjectiveCRuntime, receiver: int, selector: str, field: str
) -> str:
    value = runtime.send(receiver, selector)
    if not value:
        raise ContractError(
            "frontmost.identity_field_missing",
            "NSRunningApplication identity field is nil",
            field=field,
            selector=selector,
        )
    return _objc_utf8(runtime, int(value), field)


def _validate_frontmost_identity(value: dict[str, Any]) -> dict[str, Any]:
    invalid_fields = [
        field
        for field in ("bundle_identifier", "localized_name", "executable_name")
        if not isinstance(value.get(field), str)
    ]
    process_identifier = value.get("process_identifier")
    if isinstance(process_identifier, bool) or not isinstance(process_identifier, int):
        invalid_fields.append("process_identifier")
    if invalid_fields:
        raise ContractError(
            "frontmost.identity_invalid",
            "NSWorkspace identity query omitted required typed fields",
            invalid_fields=invalid_fields,
        )
    return {
        "bundle_identifier": value["bundle_identifier"],
        "localized_name": value["localized_name"],
        "process_identifier": process_identifier,
        "executable_name": value["executable_name"],
    }


def frontmost_identity_from_application(application: Any) -> dict[str, Any]:
    """Read typed NSRunningApplication fields from an injected in-process object."""

    if application is None:
        raise ContractError(
            "frontmost.identity_query_failed",
            "NSWorkspace returned no frontmost application",
        )
    try:
        executable_url = application.executableURL()
        if executable_url is None:
            raise ContractError(
                "frontmost.identity_field_missing",
                "NSRunningApplication executableURL is nil",
                field="executable_name",
            )
        value = {
            "bundle_identifier": application.bundleIdentifier(),
            "localized_name": application.localizedName(),
            "process_identifier": application.processIdentifier(),
            "executable_name": executable_url.lastPathComponent(),
        }
    except ContractError:
        raise
    except Exception as error:
        raise ContractError(
            "frontmost.identity_query_failed",
            "in-process NSRunningApplication query failed",
            error=repr(error),
        ) from error
    return _validate_frontmost_identity(value)


def _objc_frontmost_identity(runtime: _ObjectiveCRuntime) -> dict[str, Any]:
    pool = runtime.autorelease_pool()
    try:
        workspace_class = runtime.class_handle("NSWorkspace")
        workspace = runtime.send(workspace_class, "sharedWorkspace")
        if not workspace:
            raise ContractError(
                "frontmost.identity_query_failed",
                "NSWorkspace.sharedWorkspace returned nil",
            )
        application = runtime.send(int(workspace), "frontmostApplication")
        if not application:
            raise ContractError(
                "frontmost.identity_query_failed",
                "NSWorkspace.frontmostApplication returned nil",
            )
        executable_url = runtime.send(int(application), "executableURL")
        if not executable_url:
            raise ContractError(
                "frontmost.identity_field_missing",
                "NSRunningApplication executableURL returned nil",
                field="executable_name",
            )
        last_path_component = runtime.send(int(executable_url), "lastPathComponent")
        value = {
            "bundle_identifier": _objc_string(
                runtime, int(application), "bundleIdentifier", "bundle_identifier"
            ),
            "localized_name": _objc_string(
                runtime, int(application), "localizedName", "localized_name"
            ),
            "process_identifier": int(
                runtime.send(int(application), "processIdentifier", ctypes.c_int)
            ),
            "executable_name": _objc_utf8(
                runtime,
                int(last_path_component),
                "executable_name",
            ),
        }
        return _validate_frontmost_identity(value)
    finally:
        runtime.send(pool, "drain", None)


def frontmost_application_identity(
    workspace_factory: Callable[[], Any] | None = None,
) -> dict[str, Any]:
    """Capture NSWorkspace identity without creating an external helper process."""

    started_ns = time.monotonic_ns()
    try:
        if workspace_factory is None:
            identity = _objc_frontmost_identity(_ObjectiveCRuntime())
        else:
            workspace = workspace_factory()
            application = workspace.frontmostApplication()
            identity = frontmost_identity_from_application(application)
    except ContractError as error:
        error.details.setdefault(
            "probe_duration_ms", (time.monotonic_ns() - started_ns) / 1_000_000
        )
        raise
    except Exception as error:
        raise ContractError(
            "frontmost.identity_query_failed",
            "in-process NSWorkspace query failed",
            error=repr(error),
            probe_duration_ms=(time.monotonic_ns() - started_ns) / 1_000_000,
        ) from error
    return {
        "identity": identity,
        "observed_monotonic_ns": time.monotonic_ns(),
        "probe_duration_ms": (time.monotonic_ns() - started_ns) / 1_000_000,
        "boundary": (
            "injected.NSWorkspace"
            if workspace_factory is not None
            else "ctypes.objc_msgSend.NSWorkspace"
        ),
    }


def _screen_descriptor(
    runtime: _ObjectiveCRuntime,
    screen: int,
    *,
    index: int,
    main_screen: int,
) -> dict[str, Any]:
    frame = runtime.send(screen, "frame", _NSRect)
    scale = float(runtime.send(screen, "backingScaleFactor", ctypes.c_double))
    if not scale > 0.0 or not scale == scale:
        raise ContractError(
            "screen.scale_invalid",
            "NSScreen.backingScaleFactor returned an invalid value",
            index=index,
            value=scale,
        )
    name = _objc_string(runtime, screen, "localizedName", "localized_name")
    logical_size = {
        "width": frame.size.width,
        "height": frame.size.height,
    }
    return {
        "screen_index": index,
        "localized_name": name,
        "is_main": screen == main_screen,
        "frame": {
            "x": frame.origin.x,
            "y": frame.origin.y,
            "width": frame.size.width,
            "height": frame.size.height,
        },
        "logical_size": logical_size,
        "physical_size": {
            "width": frame.size.width * scale,
            "height": frame.size.height * scale,
        },
        "backing_scale_factor": scale,
    }


def screen_inventory() -> dict[str, Any]:
    """Enumerate typed NSScreen geometry without spawning a helper process."""

    started_ns = time.monotonic_ns()
    runtime = _ObjectiveCRuntime()
    pool = runtime.autorelease_pool()
    try:
        screen_class = runtime.class_handle("NSScreen")
        screens = runtime.send(screen_class, "screens")
        if not screens:
            raise ContractError(
                "screen.inventory_missing",
                "NSScreen.screens returned nil",
            )
        count = int(runtime.send(int(screens), "count", ctypes.c_ulong))
        if count <= 0:
            raise ContractError(
                "screen.inventory_empty",
                "NSScreen.screens returned no displays",
            )
        main_screen = int(runtime.send(screen_class, "mainScreen"))
        if not main_screen:
            raise ContractError(
                "screen.main_missing",
                "NSScreen.mainScreen returned nil",
            )
        values: list[dict[str, Any]] = []
        for index in range(count):
            screen = runtime.send(
                int(screens),
                "objectAtIndex:",
                ctypes.c_void_p,
                ctypes.c_ulong(index),
                argtypes=(ctypes.c_ulong,),
            )
            if not screen:
                raise ContractError(
                    "screen.object_missing",
                    "NSScreen.screens contained a nil entry",
                    index=index,
                )
            values.append(
                _screen_descriptor(
                    runtime,
                    int(screen),
                    index=index,
                    main_screen=main_screen,
                )
            )
        return {
            "schema": "herdr.t1-preflight.screen-inventory.v1",
            "screens": values,
            "boundary": "ctypes.objc_msgSend.NSScreen",
            "probe_duration_ms": (time.monotonic_ns() - started_ns) / 1_000_000,
        }
    except ContractError as error:
        error.details.setdefault(
            "probe_duration_ms", (time.monotonic_ns() - started_ns) / 1_000_000
        )
        raise
    except Exception as error:
        raise ContractError(
            "screen.inventory_failed",
            "in-process NSScreen query failed",
            error=repr(error),
            probe_duration_ms=(time.monotonic_ns() - started_ns) / 1_000_000,
        ) from error
    finally:
        runtime.send(pool, "drain", None)


def select_retina_screen(
    inventory: dict[str, Any], *, minimum_scale_factor: float = 2.0
) -> dict[str, Any]:
    screens = inventory.get("screens")
    if not isinstance(screens, list):
        raise ContractError(
            "screen.inventory_invalid",
            "screen inventory does not contain a typed screen list",
        )
    candidates = [
        screen
        for screen in screens
        if isinstance(screen, dict)
        and isinstance(screen.get("backing_scale_factor"), (int, float))
        and float(screen["backing_scale_factor"]) >= minimum_scale_factor
    ]
    if not candidates:
        raise ContractError(
            "screen.retina_missing",
            "no connected NSScreen meets the Retina backing-scale requirement",
            minimum_scale_factor=minimum_scale_factor,
            available_scales=[
                screen.get("backing_scale_factor")
                for screen in screens
                if isinstance(screen, dict)
            ],
        )
    candidates.sort(key=lambda screen: (not bool(screen.get("is_main")), int(screen.get("screen_index", 0))))
    selected = candidates[0]
    if not isinstance(selected.get("frame"), dict):
        raise ContractError(
            "screen.frame_missing",
            "selected NSScreen has no typed frame",
            screen_index=selected.get("screen_index"),
        )
    return selected


def screen_window_position(
    selected: dict[str, Any], inventory: dict[str, Any], *, margin: int = 24
) -> dict[str, int]:
    """Convert AppKit bottom-left NSScreen frames to AX top-left coordinates."""

    frame = selected.get("frame")
    screens = inventory.get("screens")
    if not isinstance(frame, dict) or not isinstance(screens, list):
        raise ContractError(
            "screen.placement_geometry_missing",
            "screen placement requires typed NSScreen frames",
        )
    try:
        global_top = max(
            float(item["frame"]["y"]) + float(item["frame"]["height"])
            for item in screens
            if isinstance(item, dict) and isinstance(item.get("frame"), dict)
        )
        x = float(frame["x"]) + margin
        y = global_top - (float(frame["y"]) + float(frame["height"])) + margin
    except (KeyError, TypeError, ValueError) as error:
        raise ContractError(
            "screen.placement_geometry_invalid",
            "screen placement frame contains invalid values",
            error=repr(error),
        ) from error
    return {"x": round(x), "y": round(y)}


def place_window_on_screen(
    pid: int, position: dict[str, int], script_path: Path
) -> dict[str, Any]:
    """Move only the owned process window through the existing AX boundary."""

    if pid <= 0:
        raise ContractError(
            "screen.placement_pid_invalid",
            "screen placement requires a positive owned PID",
            pid=pid,
        )
    if not script_path.is_file():
        raise ContractError(
            "screen.placement_script_missing",
            "screen placement AppleScript is missing",
            path=str(script_path),
        )
    try:
        x = int(position["x"])
        y = int(position["y"])
    except (KeyError, TypeError, ValueError) as error:
        raise ContractError(
            "screen.placement_position_invalid",
            "screen placement requires integer x and y coordinates",
            position=position,
            error=repr(error),
        ) from error
    evidence = run_command(
        ["/usr/bin/osascript", str(script_path), str(pid), str(x), str(y)],
        timeout=20,
    )
    return {
        "pid": pid,
        "position": {"x": x, "y": y},
        "command": evidence.as_dict(),
    }


@dataclass(frozen=True)
class CommandEvidence:
    argv: list[str]
    exit_code: int
    stdout: str
    stderr: str
    duration_ms: float

    def as_dict(self) -> dict[str, Any]:
        return {
            "argv": self.argv,
            "exit_code": self.exit_code,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "duration_ms": self.duration_ms,
        }


def run_command(
    argv: list[str], *, timeout: float = 30, check: bool = True
) -> CommandEvidence:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            argv,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ContractError(
            "command.unavailable",
            "external verification command could not complete",
            argv=argv,
            error=repr(error),
        ) from error
    evidence = CommandEvidence(
        argv=argv,
        exit_code=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        duration_ms=(time.monotonic() - started) * 1000,
    )
    if check and completed.returncode != 0:
        raise ContractError(
            "command.failed",
            "external verification command failed",
            **evidence.as_dict(),
        )
    return evidence


def inspect_bundle(app_path: Path, manifest: Manifest) -> dict[str, Any]:
    if sys.platform != "darwin":
        raise ContractError("platform.unsupported", "bundle inspection requires macOS")
    app = app_path.resolve(strict=True)
    if app.suffix != ".app" or not app.is_dir():
        raise ContractError(
            "bundle.invalid_path", "app path must be a .app directory", path=str(app)
        )
    info_path = app / "Contents/Info.plist"
    if not info_path.is_file():
        raise ContractError(
            "bundle.info_missing", "Contents/Info.plist is missing", path=str(info_path)
        )
    try:
        info = plistlib.loads(info_path.read_bytes())
    except (OSError, plistlib.InvalidFileException) as error:
        raise ContractError(
            "bundle.info_invalid", "Info.plist is not valid", path=str(info_path)
        ) from error
    bundle_contract = manifest.bundle
    expected_identifier = bundle_contract["identifier"]
    if info.get("CFBundleIdentifier") != expected_identifier:
        raise ContractError(
            "bundle.identifier_mismatch",
            "CFBundleIdentifier does not match the manifest",
            expected=expected_identifier,
            actual=info.get("CFBundleIdentifier"),
        )
    main_relative = Path(bundle_contract["main_executable"])
    main_name = main_relative.name
    if info.get("CFBundleExecutable") != main_name:
        raise ContractError(
            "bundle.executable_mismatch",
            "CFBundleExecutable does not match the manifest",
            expected=main_name,
            actual=info.get("CFBundleExecutable"),
        )
    if info.get("NSHighResolutionCapable") is False:
        raise ContractError(
            "bundle.retina_disabled", "NSHighResolutionCapable must not be false"
        )

    commands: list[dict[str, Any]] = []
    signature = run_command(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=4", str(app)]
    )
    commands.append(signature.as_dict())
    signature_details = run_command(
        ["/usr/bin/codesign", "-d", "--verbose=4", str(app)], check=True
    )
    commands.append(signature_details.as_dict())

    entitlements_command = run_command(
        ["/usr/bin/codesign", "-d", "--entitlements", ":-", str(app)], check=False
    )
    commands.append(entitlements_command.as_dict())
    entitlement_payload = entitlements_command.stdout or entitlements_command.stderr
    if (
        "com.apple.security.get-task-allow" in entitlement_payload
        and "<true/>" in entitlement_payload
    ):
        raise ContractError(
            "bundle.debug_entitlement",
            "release bundle enables com.apple.security.get-task-allow",
        )

    expected_executables = [Path(value) for value in bundle_contract["executables"]]
    discovered_helpers = discover_helper_executables(app)
    undeclared_helpers = sorted(
        str(item) for item in discovered_helpers - set(expected_executables)
    )
    if undeclared_helpers:
        raise ContractError(
            "bundle.undeclared_helpers",
            "nested helper executables must be declared in the manifest",
            helpers=undeclared_helpers,
        )

    executable_evidence: list[dict[str, Any]] = []
    expected_architectures = set(bundle_contract["architectures"])
    required_rpaths = bundle_contract.get("required_rpaths", {})
    if not isinstance(required_rpaths, dict):
        raise ContractError(
            "manifest.invalid_rpaths", "bundle.required_rpaths must be an object"
        )
    for relative in expected_executables:
        executable = app / relative
        if not executable.is_file() or not os.access(executable, os.X_OK):
            raise ContractError(
                "bundle.executable_missing",
                "declared executable is missing or not executable",
                path=str(relative),
            )
        file_evidence = run_command(["/usr/bin/file", "-b", str(executable)])
        commands.append(file_evidence.as_dict())
        if "Mach-O" not in file_evidence.stdout:
            raise ContractError(
                "bundle.executable_not_macho",
                "declared executable is not Mach-O",
                path=str(relative),
                file=file_evidence.stdout.strip(),
            )
        arch_evidence = run_command(["/usr/bin/lipo", "-archs", str(executable)])
        commands.append(arch_evidence.as_dict())
        executable_signature = run_command(
            [
                "/usr/bin/codesign",
                "--verify",
                "--strict",
                "--verbose=2",
                str(executable),
            ]
        )
        commands.append(executable_signature.as_dict())
        architectures = set(arch_evidence.stdout.split())
        if not expected_architectures.issubset(architectures):
            raise ContractError(
                "bundle.architecture_missing",
                "executable does not include every required architecture",
                path=str(relative),
                expected=sorted(expected_architectures),
                actual=sorted(architectures),
            )
        rpaths, load_command = executable_rpaths(executable)
        commands.append(load_command.as_dict())
        unsafe_rpaths = [
            item
            for item in rpaths
            if not item.startswith(("@loader_path", "@executable_path", "@rpath"))
        ]
        if unsafe_rpaths:
            raise ContractError(
                "bundle.unsafe_rpath",
                "executable contains an absolute or external rpath",
                path=str(relative),
                rpaths=unsafe_rpaths,
            )
        required = required_rpaths.get(str(relative), [])
        if not isinstance(required, list) or any(
            not isinstance(item, str) for item in required
        ):
            raise ContractError(
                "manifest.invalid_rpaths",
                "required rpaths must be arrays of strings",
                path=str(relative),
            )
        missing_rpaths = sorted(set(required) - set(rpaths))
        if missing_rpaths:
            raise ContractError(
                "bundle.rpath_missing",
                "executable is missing required rpaths",
                path=str(relative),
                missing=missing_rpaths,
            )
        dependencies, dependency_command = executable_dependencies(executable)
        commands.append(dependency_command.as_dict())
        unsafe_dependencies = [
            item
            for item in dependencies
            if not item.startswith(
                (
                    "/System/Library/",
                    "/usr/lib/",
                    "@rpath/",
                    "@loader_path/",
                    "@executable_path/",
                )
            )
        ]
        if unsafe_dependencies:
            raise ContractError(
                "bundle.unsafe_dependency",
                "executable links a dependency outside the bundle and system roots",
                path=str(relative),
                dependencies=unsafe_dependencies,
            )
        executable_evidence.append(
            {
                "path": str(relative),
                "sha256": sha256_file(executable),
                "architectures": sorted(architectures),
                "rpaths": rpaths,
                "dependencies": dependencies,
            }
        )

    resource_evidence: list[dict[str, Any]] = []
    for resource in bundle_contract.get("resources", []):
        relative = Path(resource["path"])
        path = app / relative
        if not path.is_file():
            raise ContractError(
                "bundle.resource_missing",
                "required bundle resource is missing",
                path=str(relative),
            )
        actual_digest = sha256_file(path)
        expected_digest = resource.get("sha256")
        if expected_digest is not None and actual_digest != expected_digest:
            raise ContractError(
                "bundle.resource_digest_mismatch",
                "bundle resource digest does not match the manifest",
                path=str(relative),
                expected=expected_digest,
                actual=actual_digest,
            )
        resource_evidence.append(
            {
                "path": str(relative),
                "sha256": actual_digest,
                "size_bytes": path.stat().st_size,
            }
        )

    return {
        "schema": "herdr.t1-preflight.bundle-evidence.v1",
        "app_path": str(app),
        "bundle_identifier": info["CFBundleIdentifier"],
        "bundle_executable": info["CFBundleExecutable"],
        "bundle_version": info.get("CFBundleVersion"),
        "short_version": info.get("CFBundleShortVersionString"),
        "info_plist_sha256": sha256_file(info_path),
        "signature_verified": True,
        "executables": executable_evidence,
        "discovered_helper_executables": sorted(
            str(item) for item in discovered_helpers
        ),
        "resources": resource_evidence,
        "commands": commands,
    }


def discover_helper_executables(app: Path) -> set[Path]:
    helpers: set[Path] = set()
    for info_path in app.glob("Contents/**/*.app/Contents/Info.plist"):
        try:
            info = plistlib.loads(info_path.read_bytes())
        except (OSError, plistlib.InvalidFileException):
            continue
        executable = info.get("CFBundleExecutable")
        if isinstance(executable, str) and executable:
            path = info_path.parent / "MacOS" / executable
            if path.is_file():
                helpers.add(path.relative_to(app))
    for info_path in app.glob("Contents/**/*.xpc/Contents/Info.plist"):
        try:
            info = plistlib.loads(info_path.read_bytes())
        except (OSError, plistlib.InvalidFileException):
            continue
        executable = info.get("CFBundleExecutable")
        if isinstance(executable, str) and executable:
            path = info_path.parent / "MacOS" / executable
            if path.is_file():
                helpers.add(path.relative_to(app))
    return helpers


def macho_rpaths_argv(executable: Path) -> list[str]:
    # Apple's otool interprets parentheses in a normal path as archive-member
    # syntax. CEF's standard helper bundle names contain parentheses, so use the
    # LLVM Mach-O inspector through xcrun for every executable.
    return [
        "/usr/bin/xcrun",
        "llvm-objdump",
        "--macho",
        "--rpaths",
        str(executable),
    ]


def macho_dependencies_argv(executable: Path) -> list[str]:
    return [
        "/usr/bin/xcrun",
        "llvm-objdump",
        "--macho",
        "--dylibs-used",
        str(executable),
    ]


def executable_rpaths(executable: Path) -> tuple[list[str], CommandEvidence]:
    evidence = run_command(macho_rpaths_argv(executable))
    result = [line.strip() for line in evidence.stdout.splitlines()[1:] if line.strip()]
    return result, evidence


def executable_dependencies(executable: Path) -> tuple[list[str], CommandEvidence]:
    evidence = run_command(macho_dependencies_argv(executable))
    dependencies: list[str] = []
    for line in evidence.stdout.splitlines()[1:]:
        stripped = line.strip()
        if stripped:
            dependencies.append(stripped.split(" (compatibility version", 1)[0])
    return dependencies, evidence


def _libproc() -> ctypes.CDLL:
    if sys.platform != "darwin":
        raise ContractError(
            "platform.unsupported", "process identity inspection requires macOS"
        )
    library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    library.proc_listallpids.argtypes = [ctypes.c_void_p, ctypes.c_int]
    library.proc_listallpids.restype = ctypes.c_int
    library.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    library.proc_pidpath.restype = ctypes.c_int
    return library


def all_pids() -> list[int]:
    library = _libproc()
    required = library.proc_listallpids(None, 0)
    if required <= 0:
        raise ContractError(
            "process.list_failed", "proc_listallpids returned no capacity"
        )
    capacity = required + 128
    buffer = (ctypes.c_int * capacity)()
    count = library.proc_listallpids(buffer, ctypes.sizeof(buffer))
    if count < 0:
        raise ContractError(
            "process.list_failed", "proc_listallpids failed", errno=ctypes.get_errno()
        )
    return [pid for pid in buffer[:count] if pid > 0]


def executable_path_for_pid(pid: int) -> Path | None:
    library = _libproc()
    buffer = ctypes.create_string_buffer(4096)
    length = library.proc_pidpath(pid, buffer, ctypes.sizeof(buffer))
    if length <= 0:
        return None
    return Path(os.fsdecode(buffer.value)).resolve(strict=False)


def pids_for_executable(executable: Path) -> list[int]:
    expected = executable.resolve(strict=False)
    return sorted(pid for pid in all_pids() if executable_path_for_pid(pid) == expected)


def process_table() -> dict[int, dict[str, float | int]]:
    evidence = run_command(["/bin/ps", "-axo", "pid=,ppid=,%cpu=,rss="], timeout=10)
    table: dict[int, dict[str, float | int]] = {}
    for line in evidence.stdout.splitlines():
        fields = line.split()
        if len(fields) != 4:
            continue
        try:
            pid, ppid = int(fields[0]), int(fields[1])
            cpu, rss_kb = float(fields[2]), int(fields[3])
        except ValueError:
            continue
        table[pid] = {"ppid": ppid, "cpu_percent": cpu, "rss_kb": rss_kb}
    return table


def descendant_pids(
    root_pid: int, table: dict[int, dict[str, float | int]]
) -> list[int]:
    descendants = {root_pid}
    changed = True
    while changed:
        changed = False
        for pid, row in table.items():
            if pid not in descendants and int(row["ppid"]) in descendants:
                descendants.add(pid)
                changed = True
    return sorted(descendants)


def sample_process_tree(
    root_pid: int,
    *,
    app_path: Path,
    allowed_external_processes: Iterable[str],
) -> dict[str, Any]:
    table = process_table()
    members = descendant_pids(root_pid, table)
    if root_pid not in table:
        raise ContractError(
            "process.root_missing",
            "root process disappeared during sampling",
            pid=root_pid,
        )
    app_root = app_path.resolve(strict=True)
    allowed = {
        Path(value).resolve(strict=False) for value in allowed_external_processes
    }
    member_evidence: list[dict[str, Any]] = []
    total_cpu = 0.0
    total_rss_kb = 0
    for pid in members:
        path = executable_path_for_pid(pid)
        if path is None:
            raise ContractError(
                "process.identity_unavailable",
                "could not resolve process executable",
                pid=pid,
            )
        inside_bundle = False
        try:
            path.relative_to(app_root)
            inside_bundle = True
        except ValueError:
            pass
        if not inside_bundle and path not in allowed:
            raise ContractError(
                "process.undeclared_descendant",
                "process tree contains an executable not owned by the bundle or manifest",
                pid=pid,
                executable=str(path),
            )
        row = table[pid]
        cpu = float(row["cpu_percent"])
        rss_kb = int(row["rss_kb"])
        total_cpu += cpu
        total_rss_kb += rss_kb
        member_evidence.append(
            {
                "pid": pid,
                "ppid": int(row["ppid"]),
                "executable": str(path),
                "cpu_percent": cpu,
                "rss_kb": rss_kb,
            }
        )
    return {
        "monotonic_ns": time.monotonic_ns(),
        "cpu_percent": total_cpu,
        "rss_mb": total_rss_kb / 1024,
        "members": member_evidence,
    }


def capture_window(window_id: int, destination: Path) -> dict[str, Any]:
    if window_id <= 0:
        raise ContractError(
            "screenshot.invalid_window_id",
            "window id must be positive",
            window_id=window_id,
        )
    destination.parent.mkdir(parents=True, exist_ok=True)
    evidence = run_command(
        ["/usr/sbin/screencapture", "-x", "-l", str(window_id), str(destination)],
        timeout=20,
    )
    if not destination.is_file() or destination.stat().st_size == 0:
        raise ContractError(
            "screenshot.missing",
            "screencapture produced no image",
            path=str(destination),
        )
    return {
        "path": str(destination),
        "sha256": sha256_file(destination),
        "size_bytes": destination.stat().st_size,
        "command": evidence.as_dict(),
    }


def capture_accessibility(
    pid: int, script_path: Path, destination: Path
) -> dict[str, Any]:
    evidence = run_command(
        ["/usr/bin/osascript", str(script_path), str(pid)], timeout=30
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(evidence.stdout, encoding="utf-8")
    if not evidence.stdout.strip():
        raise ContractError(
            "ax.empty", "Accessibility query returned no elements", pid=pid
        )
    return {
        "path": str(destination),
        "sha256": hashlib.sha256(evidence.stdout.encode("utf-8")).hexdigest(),
        "raw": evidence.stdout,
        "command": evidence.as_dict(),
    }
