from __future__ import annotations

import hashlib
import json
import math
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


SCHEMA = "herdr.t1-preflight.manifest.v1"
RUN_ID_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
PHASES = ("clean_closed", "warm_closed", "browser_included", "relaunch_closed")
REQUIRED_ARGUMENT_PLACEHOLDERS = ("{phase}", "{launch_mode}", "{events_path}")
DEFAULT_VERIFICATION_PROFILE = "t1-native-v1"
T5_OPTION_META_PROFILE = "t5-option-meta-v1"
T5_OPTION_META_V3_PROFILE = "t5-option-meta-v3"
T5_OPTION_META_V4_PROFILE = "t5-option-meta-v4"
T5_OPTION_META_PROFILES = frozenset(
    {T5_OPTION_META_PROFILE, T5_OPTION_META_V3_PROFILE, T5_OPTION_META_V4_PROFILE}
)


def is_t5_option_meta_profile(profile: str) -> bool:
    return profile in T5_OPTION_META_PROFILES


def is_t5_option_meta_v4_profile(profile: str) -> bool:
    return profile == T5_OPTION_META_V4_PROFILE
HARD_BUDGET_CEILINGS = {
    "warm_usable_ms": 1000.0,
    "terminal_input_to_present_p95_ms": 50.0,
    "idle_cpu_percent": 1.0,
    "browser_closed_rss_mb": 200.0,
    "browser_included_rss_mb": 800.0,
}


class ContractError(RuntimeError):
    def __init__(self, code: str, message: str, **details: Any) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details

    def as_dict(self) -> dict[str, Any]:
        return {
            "event": "t1_preflight.failed",
            "code": self.code,
            "message": self.message,
            "details": self.details,
        }


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_tree(path: Path) -> str:
    if not path.is_dir():
        return "missing"
    digest = hashlib.sha256()
    for candidate in sorted(
        path.rglob("*"), key=lambda item: str(item.relative_to(path))
    ):
        relative = str(candidate.relative_to(path))
        if candidate.is_symlink():
            payload = {
                "path": relative,
                "type": "symlink",
                "target": os.readlink(candidate),
            }
        elif candidate.is_file():
            payload = {
                "path": relative,
                "type": "file",
                "sha256": sha256_file(candidate),
            }
        elif candidate.is_dir():
            continue
        else:
            payload = {"path": relative, "type": "other"}
        digest.update(canonical_json(payload))
        digest.update(b"\n")
    return digest.hexdigest()


def require_mapping(value: Any, field: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ContractError(
            "manifest.invalid_type", f"{field} must be an object", field=field
        )
    return value


def require_list(value: Any, field: str) -> list[Any]:
    if not isinstance(value, list):
        raise ContractError(
            "manifest.invalid_type", f"{field} must be an array", field=field
        )
    return value


def require_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractError(
            "manifest.invalid_type", f"{field} must be a non-empty string", field=field
        )
    return value


def require_number(value: Any, field: str, *, minimum: float = 0) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or value < minimum
    ):
        raise ContractError(
            "manifest.invalid_number",
            f"{field} must be a number greater than or equal to {minimum}",
            field=field,
        )
    return float(value)


def safe_relative_path(raw: str, field: str) -> Path:
    path = Path(require_string(raw, field))
    if path.is_absolute() or ".." in path.parts or path == Path("."):
        raise ContractError(
            "manifest.unsafe_path",
            f"{field} must be a non-empty relative path without parent traversal",
            field=field,
            value=raw,
        )
    return path


def resolve_owned_path(base: Path, raw: str, field: str) -> Path:
    relative = safe_relative_path(raw, field)
    base_resolved = base.resolve()
    candidate = (base_resolved / relative).resolve(strict=False)
    try:
        candidate.relative_to(base_resolved)
    except ValueError as error:
        raise ContractError(
            "manifest.path_escape",
            f"{field} escapes the manifest directory",
            field=field,
            value=raw,
        ) from error
    return candidate


def validate_root_relative_command_contract(
    root: Path,
    *,
    working_directory: str,
    executable: str,
    app_path: str,
    manifest_path: str,
) -> dict[str, Any]:
    """Validate the exact command paths before a native run can start.

    The command contract deliberately has one path model: every harness-owned
    operand is relative to the worktree root, and the command runs from that
    root.  This prevents a root-relative app/manifest from being interpreted
    beneath a tools subdirectory, which would fail before the app launches.
    """

    root_resolved = root.resolve(strict=True)
    if working_directory != ".":
        raise ContractError(
            "command.cwd_contract_invalid",
            "native verification command must run from the worktree root",
            expected=".",
            actual=working_directory,
        )

    raw_paths = {
        "executable": executable,
        "app": app_path,
        "manifest": manifest_path,
    }
    resolved: dict[str, Path] = {}
    for field, raw in raw_paths.items():
        if not isinstance(raw, str) or not raw:
            raise ContractError(
                "command.path_invalid",
                "native verification command path must be a non-empty string",
                field=field,
            )
        path = Path(raw)
        if path.is_absolute() or ".." in path.parts:
            raise ContractError(
                "command.path_style_mismatch",
                "native verification executable, app, and manifest must all be root-relative",
                field=field,
                value=raw,
                contract="root-relative",
            )
        resolved[field] = (root_resolved / path).resolve(strict=False)

    executable_path = resolved["executable"]
    if not executable_path.is_file() or not os.access(executable_path, os.X_OK):
        raise ContractError(
            "command.executable_missing",
            "native verification executable is missing or not executable",
            path=executable,
        )
    app = resolved["app"]
    if not app.is_dir() or app.suffix != ".app":
        raise ContractError(
            "command.app_missing",
            "native verification app must resolve to an existing .app directory",
            path=app_path,
        )
    manifest = resolved["manifest"]
    if not manifest.is_file():
        raise ContractError(
            "command.manifest_missing",
            "native verification manifest must resolve to an existing file",
            path=manifest_path,
        )

    return {
        "schema": "herdr.t1-preflight.command-contract.v1",
        "contract": "root-relative",
        "working_directory": ".",
        "paths": {
            field: str(path.relative_to(root_resolved))
            for field, path in resolved.items()
        },
        "resolved": {
            "executable_is_file": True,
            "executable_is_executable": True,
            "app_is_bundle": True,
            "manifest_is_file": True,
        },
    }


@dataclass(frozen=True)
class Manifest:
    path: Path
    raw: dict[str, Any]
    digest: str
    run_id: str
    verification_profile: str
    output_dir: Path
    scenario_path: Path

    @property
    def bundle(self) -> dict[str, Any]:
        return self.raw["bundle"]

    @property
    def runtime(self) -> dict[str, Any]:
        return self.raw["runtime"]

    @property
    def budgets(self) -> dict[str, float]:
        return self.raw["budgets"]

    def render_arguments(
        self,
        phase: str,
        events_path: Path,
        *,
        action_socket: Path | None = None,
    ) -> list[str]:
        if phase not in PHASES:
            raise ContractError(
                "runtime.unknown_phase", "unknown runtime phase", phase=phase
            )
        replacements = {
            "{phase}": phase,
            "{launch_mode}": self.runtime["phases"][phase]["launch_mode"],
            "{events_path}": str(events_path),
        }
        if action_socket is not None:
            replacements["{action_socket}"] = str(action_socket)
        rendered: list[str] = []
        for item in self.runtime["arguments"]:
            value = item
            for marker, replacement in replacements.items():
                value = value.replace(marker, replacement)
            if "{" in value or "}" in value:
                raise ContractError(
                    "runtime.unknown_placeholder",
                    "runtime argument contains an unknown placeholder",
                    value=item,
                )
            rendered.append(value)
        return rendered


def load_manifest(path: Path) -> Manifest:
    manifest_path = path.resolve(strict=True)
    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(
            "manifest.unreadable",
            "manifest is not readable JSON",
            path=str(manifest_path),
        ) from error
    root = require_mapping(raw, "manifest")
    if root.get("schema") != SCHEMA:
        raise ContractError(
            "manifest.unsupported_schema",
            "manifest schema is not supported",
            expected=SCHEMA,
            actual=root.get("schema"),
        )

    run_id = require_string(root.get("run_id"), "run_id")
    if not RUN_ID_PATTERN.fullmatch(run_id):
        raise ContractError(
            "manifest.invalid_run_id",
            "run_id contains unsupported characters",
            run_id=run_id,
        )

    verification_profile = root.get("verification_profile", DEFAULT_VERIFICATION_PROFILE)
    if not isinstance(verification_profile, str) or not verification_profile:
        raise ContractError(
            "manifest.invalid_verification_profile",
            "verification_profile must be a non-empty string",
        )
    if verification_profile not in {
        DEFAULT_VERIFICATION_PROFILE,
        *T5_OPTION_META_PROFILES,
    }:
        raise ContractError(
            "manifest.unsupported_verification_profile",
            "verification_profile is not supported",
            profile=verification_profile,
        )

    output_dir = resolve_owned_path(
        manifest_path.parent,
        require_string(root.get("output_dir"), "output_dir"),
        "output_dir",
    )
    bundle = require_mapping(root.get("bundle"), "bundle")
    require_string(bundle.get("identifier"), "bundle.identifier")
    main_executable = safe_relative_path(
        require_string(bundle.get("main_executable"), "bundle.main_executable"),
        "bundle.main_executable",
    )
    if main_executable.parts[:2] != ("Contents", "MacOS"):
        raise ContractError(
            "manifest.invalid_main_executable",
            "main executable must live under Contents/MacOS",
            value=str(main_executable),
        )
    architectures = require_list(bundle.get("architectures"), "bundle.architectures")
    if not architectures or any(
        not isinstance(item, str) or not item for item in architectures
    ):
        raise ContractError(
            "manifest.invalid_architectures",
            "bundle.architectures must contain strings",
        )
    executables = require_list(bundle.get("executables"), "bundle.executables")
    executable_paths = {
        str(
            safe_relative_path(
                require_string(item, "bundle.executables[]"), "bundle.executables[]"
            )
        )
        for item in executables
    }
    if str(main_executable) not in executable_paths:
        raise ContractError(
            "manifest.main_not_declared",
            "bundle.executables must include bundle.main_executable",
        )
    browser_executables = {
        str(
            safe_relative_path(
                require_string(item, "bundle.browser_executables[]"),
                "bundle.browser_executables[]",
            )
        )
        for item in require_list(
            bundle.get("browser_executables"), "bundle.browser_executables"
        )
    }
    if not browser_executables or not browser_executables.issubset(executable_paths):
        raise ContractError(
            "manifest.invalid_browser_executables",
            "bundle.browser_executables must be a non-empty subset of bundle.executables",
        )
    for item in require_list(bundle.get("resources", []), "bundle.resources"):
        resource = require_mapping(item, "bundle.resources[]")
        safe_relative_path(
            require_string(resource.get("path"), "bundle.resources[].path"),
            "bundle.resources[].path",
        )
        digest = resource.get("sha256")
        if digest is not None and (
            not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest)
        ):
            raise ContractError(
                "manifest.invalid_sha256",
                "resource sha256 must be 64 lowercase hexadecimal characters",
                path=resource.get("path"),
            )

    runtime = require_mapping(root.get("runtime"), "runtime")
    arguments = require_list(runtime.get("arguments"), "runtime.arguments")
    if not arguments or any(
        not isinstance(item, str) or not item for item in arguments
    ):
        raise ContractError(
            "manifest.invalid_arguments", "runtime.arguments must contain strings"
        )
    joined_arguments = "\n".join(arguments)
    missing = [
        marker
        for marker in REQUIRED_ARGUMENT_PLACEHOLDERS
        if marker not in joined_arguments
    ]
    if missing:
        raise ContractError(
            "manifest.missing_placeholders",
            "runtime.arguments is missing required placeholders",
            missing=missing,
        )
    phases = require_mapping(runtime.get("phases"), "runtime.phases")
    if set(phases) != set(PHASES):
        raise ContractError(
            "manifest.invalid_phases",
            "runtime.phases must declare the exact T1 launch phases",
            expected=list(PHASES),
            actual=sorted(phases),
        )
    for phase in PHASES:
        phase_contract = require_mapping(phases[phase], f"runtime.phases.{phase}")
        launch_mode = require_string(
            phase_contract.get("launch_mode"), f"runtime.phases.{phase}.launch_mode"
        )
        allowed_fields = {"launch_mode"}
        if phase == "browser_included":
            allowed_fields.add("profile_settle_ms")
            settle_ms = require_number(
                phase_contract.get("profile_settle_ms"),
                "runtime.phases.browser_included.profile_settle_ms",
            )
            if settle_ms > 10000:
                raise ContractError(
                    "manifest.invalid_profile_settle",
                    "browser_included profile_settle_ms must be at most 10000",
                    actual=settle_ms,
                )
        unknown_fields = sorted(set(phase_contract) - allowed_fields)
        if unknown_fields:
            raise ContractError(
                "manifest.unknown_phase_fields",
                "runtime phase contains unsupported fields",
                phase=phase,
                fields=unknown_fields,
            )
        expected_mode = (
            "browser-included" if phase == "browser_included" else "browser-closed"
        )
        if launch_mode != expected_mode:
            raise ContractError(
                "manifest.invalid_launch_mode",
                "runtime phase must use its exact T1 launch mode",
                phase=phase,
                expected=expected_mode,
                actual=launch_mode,
            )
    if phases["clean_closed"]["launch_mode"] != phases["warm_closed"]["launch_mode"]:
        raise ContractError(
            "manifest.closed_mode_mismatch",
            "clean_closed and warm_closed must use the same launch_mode",
        )
    if phases["relaunch_closed"]["launch_mode"] != phases["warm_closed"]["launch_mode"]:
        raise ContractError(
            "manifest.relaunch_mode_mismatch",
            "relaunch_closed must use the same launch_mode as warm_closed",
        )
    if (
        phases["browser_included"]["launch_mode"]
        == phases["warm_closed"]["launch_mode"]
    ):
        raise ContractError(
            "manifest.browser_mode_not_distinct",
            "browser_included must use a launch_mode distinct from browser-closed warm launch",
        )
    scenario_path = resolve_owned_path(
        manifest_path.parent,
        require_string(runtime.get("scenario"), "runtime.scenario"),
        "runtime.scenario",
    )
    if not scenario_path.is_file():
        raise ContractError(
            "manifest.scenario_missing",
            "runtime scenario file does not exist",
            path=str(scenario_path),
        )
    scenario = _load_scenario(scenario_path, verification_profile)
    if scenario.get("run_id") != run_id:
        raise ContractError(
            "manifest.scenario_run_id_mismatch",
            "scenario run_id must match manifest run_id",
            manifest_run_id=run_id,
            scenario_run_id=scenario.get("run_id"),
        )
    if verification_profile in T5_OPTION_META_PROFILES and scenario.get(
        "verification_profile"
    ) != verification_profile:
        raise ContractError(
            "manifest.scenario_profile_mismatch",
            "T5 scenario must declare the exact verification profile",
            expected=verification_profile,
            actual=scenario.get("verification_profile"),
        )
    if (
        verification_profile in T5_OPTION_META_PROFILES
        and scenario.get("scenario_id") != run_id
    ):
        raise ContractError(
            "manifest.scenario_id_mismatch",
            "T5 scenario_id must match the manifest run_id",
            expected=run_id,
            actual=scenario.get("scenario_id"),
        )
    expected_labels = require_list(
        runtime.get("expected_ax_labels"), "runtime.expected_ax_labels"
    )
    if not expected_labels or any(
        not isinstance(item, str) or not item for item in expected_labels
    ):
        raise ContractError(
            "manifest.invalid_ax_labels",
            "runtime.expected_ax_labels must contain strings",
        )
    allowed_external = require_list(
        runtime.get("allowed_external_processes", []),
        "runtime.allowed_external_processes",
    )
    for value in allowed_external:
        if not isinstance(value, str) or not Path(value).is_absolute():
            raise ContractError(
                "manifest.invalid_external_process",
                "allowed external process entries must be absolute paths",
                value=value,
            )
    timeouts = require_mapping(runtime.get("timeouts_ms"), "runtime.timeouts_ms")
    for key in ("phase", "event", "exit"):
        require_number(timeouts.get(key), f"runtime.timeouts_ms.{key}", minimum=1)
    sampling = require_mapping(runtime.get("sampling"), "runtime.sampling")
    require_number(
        sampling.get("interval_ms"), "runtime.sampling.interval_ms", minimum=50
    )
    require_number(sampling.get("count"), "runtime.sampling.count", minimum=3)

    budgets = require_mapping(root.get("budgets"), "budgets")
    required_budgets = {
        "warm_usable_ms": 1,
        "terminal_input_to_present_p95_ms": 1,
        "idle_cpu_percent": 0,
        "browser_closed_rss_mb": 1,
        "browser_included_rss_mb": 1,
    }
    normalized_budgets: dict[str, float] = {}
    for key, minimum in required_budgets.items():
        normalized_budgets[key] = require_number(
            budgets.get(key), f"budgets.{key}", minimum=minimum
        )
        if normalized_budgets[key] > HARD_BUDGET_CEILINGS[key]:
            raise ContractError(
                "manifest.loose_budget",
                "manifest budget cannot be looser than the T1 hard ceiling",
                budget=key,
                hard_ceiling=HARD_BUDGET_CEILINGS[key],
                actual=normalized_budgets[key],
            )
    root["budgets"] = normalized_budgets

    return Manifest(
        path=manifest_path,
        raw=root,
        digest=sha256_bytes(
            canonical_json(
                {
                    "manifest": root,
                    "scenario_sha256": sha256_file(scenario_path),
                }
            )
        ),
        run_id=run_id,
        verification_profile=verification_profile,
        output_dir=output_dir,
        scenario_path=scenario_path,
    )


def _load_scenario(
    path: Path, verification_profile: str = DEFAULT_VERIFICATION_PROFILE
) -> dict[str, Any]:
    try:
        scenario = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(
            "manifest.scenario_unreadable",
            "runtime scenario is not readable JSON",
            path=str(path),
        ) from error
    result = require_mapping(scenario, "scenario")
    if result.get("schema") != "herdr.t1-preflight.scenario.v1":
        raise ContractError(
            "manifest.scenario_schema",
            "runtime scenario schema is not supported",
            path=str(path),
        )
    actions = require_list(result.get("actions"), "scenario.actions")
    base_required_actions = {
        "fixture.assert",
        "terminal.native_input",
        "profile.browser_closed",
        "ax.snapshot",
        "screenshot.capture",
        "zoom.shortcut",
        "focus.click",
        "window.resize",
        "ime.physical_keys",
        "state.persist",
    }
    required_actions = set(base_required_actions)
    if verification_profile in T5_OPTION_META_PROFILES:
        required_actions.add("terminal.option_meta")
    if verification_profile == T5_OPTION_META_V4_PROFILE:
        required_actions.add("terminal.plain_key_control")
    actual_actions = {
        require_string(
            require_mapping(item, "scenario.actions[]").get("action"),
            "scenario.actions[].action",
        )
        for item in actions
    }
    missing = sorted(required_actions - actual_actions)
    if missing:
        raise ContractError(
            "manifest.scenario_incomplete",
            "runtime scenario is missing required actions",
            missing=missing,
        )
    unsupported = sorted(actual_actions - required_actions)
    if unsupported:
        raise ContractError(
            "manifest.scenario_unsupported",
            "runtime scenario contains unsupported actions",
            actions=unsupported,
        )
    if verification_profile in T5_OPTION_META_PROFILES:
        option_actions = [
            require_mapping(item, "scenario.actions[]")
            for item in actions
            if require_mapping(item, "scenario.actions[]").get("action")
            == "terminal.option_meta"
        ]
        if len(option_actions) != 1:
            raise ContractError(
                "manifest.option_meta_count",
                "T5 scenario must contain exactly one terminal.option_meta action",
                actual=len(option_actions),
            )
        option_action = option_actions[0]
        if option_action.get("target_point") != "terminal":
            raise ContractError(
                "manifest.option_meta_target",
                "T5 Option+F action must target the terminal point",
                actual=option_action.get("target_point"),
            )
        if option_action.get("repeat") != 2:
            raise ContractError(
                "manifest.option_meta_repeat",
                "T5 Option+F action must inject exactly twice",
                actual=option_action.get("repeat"),
            )
        if option_action.get("key_code") != 3:
            raise ContractError(
                "manifest.option_meta_key",
                "T5 Option+F action must use the macOS f key code",
                actual=option_action.get("key_code"),
            )
        if option_action.get("modifiers") != ["option"]:
            raise ContractError(
                "manifest.option_meta_modifiers",
                "T5 Option+F action must use only the Option modifier",
                actual=option_action.get("modifiers"),
            )
        if option_action.get("expected_bytes_hex") != "1b 66":
            raise ContractError(
                "manifest.option_meta_bytes",
                "T5 Option+F action must expect ESC followed by lowercase f",
                actual=option_action.get("expected_bytes_hex"),
            )
        latency_budget = option_action.get("latency_budget_ms")
        if (
            isinstance(latency_budget, bool)
            or not isinstance(latency_budget, (int, float))
            or latency_budget <= 0
            or latency_budget > HARD_BUDGET_CEILINGS["terminal_input_to_present_p95_ms"]
        ):
            raise ContractError(
                "manifest.option_meta_latency_budget",
                "T5 Option+F latency_budget_ms must be positive and within the 50ms hard ceiling",
                actual=latency_budget,
            )
        if verification_profile == T5_OPTION_META_V4_PROFILE:
            control_actions = [
                require_mapping(item, "scenario.actions[]")
                for item in actions
                if require_mapping(item, "scenario.actions[]").get("action")
                == "terminal.plain_key_control"
            ]
            if len(control_actions) != 1:
                raise ContractError(
                    "manifest.plain_key_control_count",
                    "T5 v4 scenario must contain exactly one plain-key control action",
                    actual=len(control_actions),
                )
            control_action = control_actions[0]
            control_index = next(
                index
                for index, item in enumerate(actions)
                if item is control_action
            )
            option_index = next(
                index
                for index, item in enumerate(actions)
                if item is option_action
            )
            if control_index + 1 != option_index:
                raise ContractError(
                    "manifest.plain_key_control_order",
                    "T5 v4 plain-key control must immediately precede terminal.option_meta",
                    control_index=control_index,
                    option_index=option_index,
                )
            if control_action.get("target_point") != "terminal":
                raise ContractError(
                    "manifest.plain_key_control_target",
                    "T5 v4 plain-key control must target the terminal point",
                    actual=control_action.get("target_point"),
                )
            if control_action.get("key_code") != 28:
                raise ContractError(
                    "manifest.plain_key_control_key",
                    "T5 v4 plain-key control must use the physical 8 key",
                    actual=control_action.get("key_code"),
                )
            if control_action.get("modifiers") != []:
                raise ContractError(
                    "manifest.plain_key_control_modifiers",
                    "T5 v4 plain-key control must have no modifiers",
                    actual=control_action.get("modifiers"),
                )
            if control_action.get("repeat") != 1:
                raise ContractError(
                    "manifest.plain_key_control_repeat",
                    "T5 v4 plain-key control must inject exactly once",
                    actual=control_action.get("repeat"),
                )
            if control_action.get("expected_bytes_hex") != "38":
                raise ContractError(
                    "manifest.plain_key_control_bytes",
                    "T5 v4 plain-key control must expect ASCII 8",
                    actual=control_action.get("expected_bytes_hex"),
                )
    return result


def percentile_nearest_rank(values: Iterable[float], percentile: float) -> float:
    ordered = sorted(float(value) for value in values)
    if not ordered:
        raise ContractError(
            "evidence.empty_samples", "cannot calculate a percentile without samples"
        )
    index = max(0, math.ceil(percentile * len(ordered)) - 1)
    return ordered[index]


def evaluate_runtime_evidence(
    *,
    budgets: dict[str, float],
    warm_usable_ms: float,
    terminal_latencies_ms: list[float],
    profiles: dict[str, list[dict[str, float]]],
    semantic: dict[str, Any],
) -> dict[str, Any]:
    if len(terminal_latencies_ms) < 20:
        raise ContractError(
            "evidence.insufficient_terminal_samples",
            "terminal latency evidence requires at least 20 samples",
            actual=len(terminal_latencies_ms),
        )
    for name in ("browser_closed", "browser_included"):
        if name not in profiles or len(profiles[name]) < 3:
            raise ContractError(
                "evidence.insufficient_profile_samples",
                "process profile requires at least three samples",
                profile=name,
                actual=len(profiles.get(name, [])),
            )
    terminal_p95 = percentile_nearest_rank(terminal_latencies_ms, 0.95)
    closed_rss = max(sample["rss_mb"] for sample in profiles["browser_closed"])
    included_rss = max(sample["rss_mb"] for sample in profiles["browser_included"])
    idle_cpu = sum(
        sample["cpu_percent"] for sample in profiles["browser_closed"]
    ) / len(profiles["browser_closed"])
    checks = {
        "warm_usable": {
            "actual": warm_usable_ms,
            "budget": budgets["warm_usable_ms"],
            "unit": "ms",
            "pass": warm_usable_ms <= budgets["warm_usable_ms"],
        },
        "terminal_input_to_present_p95": {
            "actual": terminal_p95,
            "budget": budgets["terminal_input_to_present_p95_ms"],
            "unit": "ms",
            "pass": terminal_p95 <= budgets["terminal_input_to_present_p95_ms"],
        },
        "idle_cpu": {
            "actual": idle_cpu,
            "budget": budgets["idle_cpu_percent"],
            "unit": "percent",
            "pass": idle_cpu <= budgets["idle_cpu_percent"],
        },
        "browser_closed_rss": {
            "actual": closed_rss,
            "budget": budgets["browser_closed_rss_mb"],
            "unit": "MiB",
            "pass": closed_rss <= budgets["browser_closed_rss_mb"],
        },
        "browser_included_rss": {
            "actual": included_rss,
            "budget": budgets["browser_included_rss_mb"],
            "unit": "MiB",
            "pass": included_rss <= budgets["browser_included_rss_mb"],
        },
    }
    semantic_checks = {
        "fixture_7_workspaces_11_panes": semantic.get("workspace_count") == 7
        and semantic.get("pane_count") == 11
        and semantic.get("browser_closed") is True,
        "zoom_exact_restore": semantic.get("zoom_before_topology_hash")
        == semantic.get("zoom_after_topology_hash")
        and bool(semantic.get("zoom_before_topology_hash")),
        "focus_changed": semantic.get("focus_before") != semantic.get("focus_after")
        and semantic.get("focus_before") is not None
        and semantic.get("focus_after") is not None,
        "resize_retina": semantic.get("resize_scale_factor", 0) > 1
        and semantic.get("resize_physical_width", 0)
        >= semantic.get("resize_logical_width", 0)
        * semantic.get("resize_scale_factor", 0),
        "relaunch_restored": semantic.get("persisted_state_hash")
        == semantic.get("relaunch_state_hash")
        and bool(semantic.get("persisted_state_hash")),
        "ax_labels_present": semantic.get("ax_labels_present") is True,
        "native_screenshots_present": semantic.get("native_screenshots_present")
        is True,
        "browser_included_cdp": semantic.get("browser_included_cdp") is True,
        "exactly_one_instance": semantic.get("exactly_one_instance") is True,
    }
    if semantic.get("verification_profile") in T5_OPTION_META_PROFILES:
        if semantic.get("verification_profile") == T5_OPTION_META_V4_PROFILE:
            control = semantic.get("plain_key_control")
            control_events = (
                control.get("source_events", [])
                if isinstance(control, dict)
                else []
            )
            control_focus = control.get("focus") if isinstance(control, dict) else None
            native_probe_seq = (
                control.get("native_probe_917_008_seq")
                if isinstance(control, dict)
                else None
            )
            action_armed_seq = (
                control.get("action_armed_seq")
                if isinstance(control, dict)
                else None
            )
            semantic_checks.update(
                {
                    "plain_key_control_exactly_once": isinstance(control, dict)
                    and control.get("count") == 1
                    and control.get("expected_count") == 1,
                    "plain_key_control_bytes_proven": isinstance(control, dict)
                    and control.get("expected_bytes_hex") == "38"
                    and isinstance(control.get("source_events"), list)
                    and bool(control_events)
                    and isinstance(control_events[-1], dict)
                    and control_events[-1].get("bytes_hex") == "38",
                    "plain_key_control_source_timeline": isinstance(control, dict)
                    and control.get("source_order")
                    == ["appkit", "app-routing", "herdr", "pty"]
                    and [
                        item.get("event", "").removeprefix(
                            "input.plain_key_control."
                        )
                        for item in control_events
                    ]
                    == ["appkit", "app-routing", "herdr", "pty"],
                    "plain_key_control_focus_proven": isinstance(control_focus, dict)
                    and all(
                        control_focus.get(field) is True
                        for field in (
                            "app_frontmost",
                            "key_window",
                            "render_view_first_responder",
                        )
                    ),
                    "plain_key_control_after_native_probe": isinstance(
                        native_probe_seq, int
                    )
                    and isinstance(action_armed_seq, int)
                    and native_probe_seq < action_armed_seq,
                }
            )
        option_meta = semantic.get("option_meta")
        option_events = option_meta.get("events", []) if isinstance(option_meta, dict) else []
        option_latencies = (
            option_meta.get("latencies_ms", []) if isinstance(option_meta, dict) else []
        )
        expected_bytes = (
            option_meta.get("expected_bytes_hex") if isinstance(option_meta, dict) else None
        )
        expected_count = (
            option_meta.get("expected_count") if isinstance(option_meta, dict) else None
        )
        source_order = (
            option_meta.get("source_order") if isinstance(option_meta, dict) else None
        )
        option_budget = (
            float(option_meta.get("latency_budget_ms"))
            if isinstance(option_meta, dict)
            and isinstance(option_meta.get("latency_budget_ms"), (int, float))
            else budgets["terminal_input_to_present_p95_ms"]
        )
        option_meta_latencies_valid = bool(option_latencies) and all(
            isinstance(value, (int, float))
            and not isinstance(value, bool)
            and value >= 0
            and value <= option_budget
            for value in option_latencies
        )
        semantic_checks.update(
            {
                "option_meta_exactly_twice": isinstance(option_meta, dict)
                and option_meta.get("count") == 2
                and expected_count == 2
                and len(option_events) == 2,
                "option_meta_bytes_proven": expected_bytes == "1b 66"
                and all(
                    isinstance(event, dict)
                    and event.get("bytes_hex") == expected_bytes
                    for event in option_events
                ),
                "option_meta_source_timeline": source_order
                == ["appkit", "app-routing", "herdr", "pty"]
                and all(
                    isinstance(event, dict)
                    and [
                        item.get("event", "").removeprefix("input.option_meta.")
                        for item in event.get("source_events", [])
                    ]
                    == source_order
                    for event in option_events
                ),
                "option_meta_focus_proven": all(
                    isinstance(focus, dict)
                    and all(
                        focus.get(field) is True
                        for field in (
                            "app_frontmost",
                            "key_window",
                            "render_view_first_responder",
                        )
                    )
                    for focus in (
                        [
                            option_meta.get("focus_before"),
                            *option_meta.get("focus_snapshots", []),
                        ]
                        if isinstance(option_meta, dict)
                        else []
                    )
                ),
                "option_meta_latency_gate": len(option_latencies) == 2
                and option_meta_latencies_valid
                and percentile_nearest_rank(option_latencies, 0.95) <= option_budget,
            }
        )
    passed = all(check["pass"] for check in checks.values()) and all(
        semantic_checks.values()
    )
    return {
        "status": "PASS" if passed else "FAIL",
        "checks": checks,
        "semantic_checks": semantic_checks,
    }


class OutputStore:
    MARKER = ".t1-preflight-owner.json"

    def __init__(self, manifest: Manifest, app_path: Path, mode: str) -> None:
        self.manifest = manifest
        self.app_path = app_path.resolve(strict=False)
        self.mode = mode
        self.marker_path = manifest.output_dir / self.MARKER
        self.result_path = manifest.output_dir / "result.json"

    @property
    def identity(self) -> dict[str, Any]:
        return {
            "schema": "herdr.t1-preflight.output-owner.v1",
            "run_id": self.manifest.run_id,
            "manifest_sha256": self.manifest.digest,
            "app_path": str(self.app_path),
            "app_bundle_sha256": sha256_tree(self.app_path),
            "mode": self.mode,
        }

    def prepare(self) -> dict[str, Any] | None:
        if not self.manifest.output_dir.exists():
            self.manifest.output_dir.mkdir(parents=True)
            self._atomic_write(self.marker_path, self.identity)
            return None
        if (
            self.manifest.output_dir.is_symlink()
            or not self.manifest.output_dir.is_dir()
        ):
            raise ContractError(
                "output.unsafe_existing_path",
                "output path must be a real directory",
                path=str(self.manifest.output_dir),
            )
        if not self.marker_path.is_file():
            raise ContractError(
                "output.unowned",
                "existing output directory has no ownership marker",
                path=str(self.manifest.output_dir),
            )
        marker = json.loads(self.marker_path.read_text(encoding="utf-8"))
        if marker != self.identity:
            raise ContractError(
                "output.identity_mismatch",
                "existing output belongs to different inputs",
                path=str(self.manifest.output_dir),
            )
        if not self.result_path.is_file():
            raise ContractError(
                "output.incomplete",
                "owned output exists without a completed result; manual review is required",
                path=str(self.manifest.output_dir),
            )
        result = json.loads(self.result_path.read_text(encoding="utf-8"))
        if result.get("status") not in {"PASS", "FAIL", "DRY_RUN", "STATIC_PASS"}:
            raise ContractError(
                "output.invalid_result",
                "existing result has an unsupported status",
                result=result,
            )
        return result

    def write_json(self, relative: str, value: Any) -> Path:
        target = resolve_owned_path(
            self.manifest.output_dir, relative, "output artifact"
        )
        target.parent.mkdir(parents=True, exist_ok=True)
        self._atomic_write(target, value)
        return target

    def finish(self, result: dict[str, Any]) -> Path:
        self._atomic_write(self.result_path, result)
        return self.result_path

    @staticmethod
    def _atomic_write(path: Path, value: Any) -> None:
        temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
        temporary.write_text(
            json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        os.replace(temporary, path)
