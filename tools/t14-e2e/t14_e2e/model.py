from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


SCHEMA = "herdr.ide.t14-e2e.manifest.v1"
SCENARIO_SCHEMA = "herdr.ide.t14-e2e.scenario.v1"
RUN_ID_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
IDENTITY_PATTERN = re.compile(r"^[^\s*?\[\]{}]+$")
GLOB_CHARS = frozenset("*?[]{}")


class ContractError(RuntimeError):
    """A fail-closed T14 contract error with machine-readable details."""

    def __init__(self, code: str, message: str, **details: Any) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details

    def as_dict(self) -> dict[str, Any]:
        return {
            "schema": "herdr.ide.t14-e2e.failure.v1",
            "event": "t14_e2e.failed",
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
    """Hash a bundle without following symlinks or recording user content."""

    if not path.is_dir():
        return "missing"
    digest = hashlib.sha256()
    for candidate in sorted(
        path.rglob("*"), key=lambda item: str(item.relative_to(path))
    ):
        relative = str(candidate.relative_to(path))
        if candidate.is_symlink():
            payload = {"path": relative, "type": "symlink", "target": os.readlink(candidate)}
        elif candidate.is_file():
            payload = {"path": relative, "type": "file", "sha256": sha256_file(candidate)}
        elif candidate.is_dir():
            continue
        else:
            payload = {"path": relative, "type": "other"}
        digest.update(canonical_json(payload))
        digest.update(b"\n")
    return digest.hexdigest()


def _required_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractError("manifest.invalid_type", f"{field} must be a non-empty string", field=field)
    return value


def _relative_path(value: Any, field: str) -> Path:
    raw = _required_string(value, field)
    path = Path(raw)
    if path.is_absolute() or path == Path(".") or ".." in path.parts:
        raise ContractError(
            "manifest.unsafe_path",
            f"{field} must be a non-empty root-relative path without parent traversal",
            field=field,
            value=raw,
        )
    if any(character in raw for character in GLOB_CHARS):
        raise ContractError(
            "manifest.path_glob",
            f"{field} must identify one exact path, not a glob",
            field=field,
            value=raw,
        )
    return path


def _resolve(root: Path, value: Any, field: str) -> Path:
    relative = _relative_path(value, field)
    root = root.resolve(strict=True)
    # Keep the manifest identity lexical. This worktree intentionally exposes
    # generated targets through an approved sibling symlink; resolving that
    # ancestor would reject an otherwise exact root-relative app/output path.
    # Absolute paths and parent traversal are rejected by _relative_path.
    return root / relative


def _exact_identity(value: Any, field: str) -> str:
    identity = _required_string(value, field)
    if not IDENTITY_PATTERN.fullmatch(identity) or any(
        character in identity for character in GLOB_CHARS
    ):
        raise ContractError(
            "fixture.identity_not_exact",
            f"{field} must be one exact identifier; wildcard/prefix cleanup is forbidden",
            field=field,
            value=identity,
        )
    return identity


@dataclass(frozen=True)
class FixtureResource:
    kind: str
    identity: str
    cleanup: str

    @classmethod
    def from_raw(cls, value: Any, index: int) -> "FixtureResource":
        if not isinstance(value, dict):
            raise ContractError(
                "fixture.invalid_resource",
                "owned_resources entries must be objects",
                index=index,
            )
        kind = _exact_identity(value.get("kind"), f"owned_resources[{index}].kind")
        identity = _exact_identity(value.get("id"), f"owned_resources[{index}].id")
        cleanup = _required_string(
            value.get("cleanup", "explicit-confirmation"),
            f"owned_resources[{index}].cleanup",
        )
        if cleanup != "explicit-confirmation":
            raise ContractError(
                "fixture.cleanup_policy_invalid",
                "T14 never performs implicit broad cleanup; resources require explicit confirmation",
                index=index,
                cleanup=cleanup,
            )
        return cls(kind=kind, identity=identity, cleanup=cleanup)

    def as_dict(self) -> dict[str, str]:
        return {"kind": self.kind, "id": self.identity, "cleanup": self.cleanup}


@dataclass(frozen=True)
class E2EManifest:
    root: Path
    manifest_path: Path
    raw: dict[str, Any]
    run_id: str
    verification_profile: str
    working_directory: str
    app_path: Path
    executable: Path
    bundle_kind: str
    bundle_identifier: str
    expected_bundle_sha256: str | None
    scenario_path: Path
    scenario_sha256: str
    scenario: dict[str, Any]
    output_dir: Path
    ports: tuple[int, ...]
    owned_resources: tuple[FixtureResource, ...]
    expected_protocol: int
    harness_relative_path: str
    telemetry_path: Path | None
    herdr_snapshot_command: tuple[str, ...] | None
    launch_arguments: tuple[str, ...]

    @property
    def digest(self) -> str:
        return sha256_bytes(canonical_json(self.raw))

    @property
    def scenario_id(self) -> str:
        return str(self.scenario["scenario_id"])

    @property
    def owned_resource_ids(self) -> frozenset[str]:
        return frozenset(resource.identity for resource in self.owned_resources)

    @property
    def exact_command(self) -> list[str]:
        """Return the canonical, root-relative command used by the harness."""

        return [
            self.harness_relative_path,
            "--root",
            ".",
            "--manifest",
            str(self.manifest_path.relative_to(self.root)),
            "--mode",
            "run",
        ]

    def ownership_marker(self) -> dict[str, Any]:
        return {
            "schema": "herdr.ide.t14-e2e.output-owner.v1",
            "run_id": self.run_id,
            "verification_profile": self.verification_profile,
            "manifest_sha256": self.digest,
            "scenario_sha256": self.scenario_sha256,
            "app_path": str(self.app_path.relative_to(self.root)),
            "bundle_kind": self.bundle_kind,
            "output_dir": str(self.output_dir.relative_to(self.root)),
        }


def _validate_scenario(root: Path, raw: dict[str, Any], run_id: str, profile: str) -> tuple[Path, str, dict[str, Any]]:
    scenario_path = _resolve(root, raw.get("scenario", {}).get("path"), "scenario.path")
    if not scenario_path.is_file():
        raise ContractError(
            "scenario.missing",
            "the owned scenario manifest does not exist",
            path=str(scenario_path),
        )
    try:
        scenario = json.loads(scenario_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(
            "scenario.unreadable",
            "the owned scenario is not readable JSON",
            path=str(scenario_path),
            error=repr(error),
        ) from error
    if not isinstance(scenario, dict) or scenario.get("schema") != SCENARIO_SCHEMA:
        raise ContractError(
            "scenario.schema",
            "scenario schema is not supported",
            expected=SCENARIO_SCHEMA,
        )
    for field, expected in (("run_id", run_id), ("verification_profile", profile)):
        if scenario.get(field) != expected:
            raise ContractError(
                "scenario.identity_mismatch",
                f"scenario {field} does not match manifest",
                field=field,
                expected=expected,
                actual=scenario.get(field),
            )
    scenario_id = _exact_identity(scenario.get("scenario_id"), "scenario.scenario_id")
    if scenario_id != run_id:
        raise ContractError(
            "scenario.identity_mismatch",
            "scenario_id must equal run_id for exact rerun ownership",
            expected=run_id,
            actual=scenario_id,
        )
    actions = scenario.get("actions")
    if not isinstance(actions, list) or not actions:
        raise ContractError("scenario.actions_missing", "scenario must contain at least one action")
    allowed = {"ax.snapshot", "screenshot.capture", "native_input", "herdr.snapshot"}
    labels: set[str] = set()
    for index, action in enumerate(actions):
        if not isinstance(action, dict) or action.get("action") not in allowed:
            raise ContractError(
                "scenario.action_unsupported",
                "T14 scenario contains an unsupported action",
                index=index,
                action=action.get("action") if isinstance(action, dict) else None,
            )
        label = _exact_identity(action.get("id", f"action-{index}"), f"actions[{index}].id")
        if label in labels:
            raise ContractError(
                "scenario.action_duplicate",
                "scenario action ids must be unique",
                action_id=label,
            )
        labels.add(label)
        if action["action"] == "screenshot.capture":
            window_id = action.get("window_id")
            if window_id != "owned-main" and (
                isinstance(window_id, bool)
                or not isinstance(window_id, int)
                or window_id <= 0
            ):
                raise ContractError(
                    "scenario.window_id_invalid",
                    "screenshot.capture window_id must be a positive integer or owned-main",
                    index=index,
                )
        if action["action"] == "native_input":
            key_code = action.get("key_code")
            if isinstance(key_code, bool) or not isinstance(key_code, int) or not 0 <= key_code <= 127:
                raise ContractError(
                    "scenario.key_code_invalid",
                    "native_input key_code must be an integer in the HID range",
                    index=index,
                )
            modifiers = action.get("modifiers", [])
            if not isinstance(modifiers, list) or any(not isinstance(item, str) for item in modifiers):
                raise ContractError(
                    "scenario.modifiers_invalid",
                    "native_input modifiers must be a string array",
                    index=index,
                )
    return scenario_path, sha256_file(scenario_path), scenario


def load_manifest(path: Path, *, root: Path | None = None) -> E2EManifest:
    manifest_path = path.resolve(strict=True)
    root_path = (root or manifest_path.parent).resolve(strict=True)
    try:
        manifest_path.relative_to(root_path)
    except ValueError as error:
        raise ContractError(
            "manifest.outside_root",
            "manifest must live inside the declared worktree root",
            manifest=str(manifest_path),
            root=str(root_path),
        ) from error
    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(
            "manifest.unreadable",
            "manifest is not readable JSON",
            path=str(manifest_path),
            error=repr(error),
        ) from error
    if not isinstance(raw, dict) or raw.get("schema") != SCHEMA:
        raise ContractError("manifest.schema", "T14 manifest schema is not supported", expected=SCHEMA)
    run_id = _required_string(raw.get("run_id"), "run_id")
    if not RUN_ID_PATTERN.fullmatch(run_id):
        raise ContractError("manifest.run_id_invalid", "run_id is not a stable rerun identity", run_id=run_id)
    profile = _required_string(raw.get("verification_profile"), "verification_profile")
    working_directory = _required_string(raw.get("working_directory", "."), "working_directory")
    if working_directory != ".":
        raise ContractError(
            "manifest.cwd_invalid",
            "T14 commands must run from the worktree root",
            expected=".",
            actual=working_directory,
        )
    app = raw.get("app")
    if not isinstance(app, dict):
        raise ContractError("manifest.app_missing", "manifest.app must be an object")
    app_path = _resolve(root_path, app.get("path"), "app.path")
    executable = _resolve(root_path, app.get("executable"), "app.executable")
    try:
        executable.relative_to(app_path)
    except ValueError as error:
        raise ContractError(
            "manifest.executable_outside_bundle",
            "app.executable must be inside app.path",
        ) from error
    executable_relative = executable.relative_to(app_path)
    if executable_relative.parts[:2] != ("Contents", "MacOS"):
        raise ContractError(
            "manifest.executable_location_invalid",
            "app.executable must be the bundle's Contents/MacOS executable",
            value=str(executable_relative),
        )
    bundle_kind = _required_string(app.get("bundle_kind"), "app.bundle_kind")
    if bundle_kind not in {"installed", "dev"}:
        raise ContractError("manifest.bundle_kind_invalid", "app.bundle_kind must be installed or dev")
    bundle_identifier = _required_string(app.get("bundle_identifier"), "app.bundle_identifier")
    expected_bundle_sha256 = app.get("sha256")
    if expected_bundle_sha256 is not None and (
        not isinstance(expected_bundle_sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", expected_bundle_sha256)
    ):
        raise ContractError("manifest.bundle_hash_invalid", "app.sha256 must be a lowercase SHA-256 value")
    scenario_spec = raw.get("scenario")
    if not isinstance(scenario_spec, dict):
        raise ContractError("manifest.scenario_missing", "manifest.scenario must be an object")
    scenario_path, scenario_sha256, scenario = _validate_scenario(root_path, raw, run_id, profile)
    expected_scenario_sha256 = scenario_spec.get("sha256")
    if expected_scenario_sha256 is not None and expected_scenario_sha256 != scenario_sha256:
        raise ContractError(
            "manifest.scenario_hash_mismatch",
            "scenario SHA-256 does not match the manifest pin",
            expected=expected_scenario_sha256,
            actual=scenario_sha256,
        )
    output_dir = _resolve(root_path, raw.get("output_dir"), "output_dir")
    if output_dir == root_path:
        raise ContractError("manifest.output_invalid", "output_dir must be a dedicated child path")
    ports_raw = raw.get("ports", [])
    if not isinstance(ports_raw, list) or any(isinstance(port, bool) or not isinstance(port, int) for port in ports_raw):
        raise ContractError("manifest.ports_invalid", "ports must be an integer array")
    ports = tuple(ports_raw)
    if len(set(ports)) != len(ports) or any(port < 1024 or port > 65535 for port in ports):
        raise ContractError("manifest.ports_invalid", "ports must be unique values in 1024..65535")
    resources_raw = raw.get("owned_resources")
    if not isinstance(resources_raw, list) or not resources_raw:
        raise ContractError("fixture.resources_missing", "owned_resources must contain exact fixture identities")
    resources = tuple(FixtureResource.from_raw(value, index) for index, value in enumerate(resources_raw))
    identities = [resource.identity for resource in resources]
    if len(set(identities)) != len(identities):
        raise ContractError("fixture.identity_duplicate", "owned fixture identities must be unique")
    expected_protocol = raw.get("herdr_protocol", 21)
    if isinstance(expected_protocol, bool) or not isinstance(expected_protocol, int) or expected_protocol <= 0:
        raise ContractError("manifest.protocol_invalid", "herdr_protocol must be a positive integer")
    harness_relative_path = _relative_path(raw.get("harness", "tools/t14-e2e/t14-e2e"), "harness").as_posix()
    telemetry_raw = raw.get("telemetry_path")
    telemetry_path = _resolve(root_path, telemetry_raw, "telemetry_path") if telemetry_raw else None
    snapshot_command_raw = raw.get("herdr_snapshot_command")
    snapshot_command: tuple[str, ...] | None = None
    if snapshot_command_raw is not None:
        if not isinstance(snapshot_command_raw, list) or not snapshot_command_raw or any(
            not isinstance(item, str) or not item for item in snapshot_command_raw
        ):
            raise ContractError("manifest.snapshot_command_invalid", "herdr_snapshot_command must be a non-empty argv array")
        snapshot_command = tuple(snapshot_command_raw)
    launch_arguments_raw = raw.get("launch_arguments", [])
    if not isinstance(launch_arguments_raw, list) or any(
        not isinstance(item, str) or not item for item in launch_arguments_raw
    ):
        raise ContractError(
            "manifest.launch_arguments_invalid",
            "launch_arguments must be a string argv array",
        )
    launch_arguments = tuple(launch_arguments_raw)
    return E2EManifest(
        root=root_path,
        manifest_path=manifest_path,
        raw=raw,
        run_id=run_id,
        verification_profile=profile,
        working_directory=working_directory,
        app_path=app_path,
        executable=executable,
        bundle_kind=bundle_kind,
        bundle_identifier=bundle_identifier,
        expected_bundle_sha256=expected_bundle_sha256,
        scenario_path=scenario_path,
        scenario_sha256=scenario_sha256,
        scenario=scenario,
        output_dir=output_dir,
        ports=ports,
        owned_resources=resources,
        expected_protocol=expected_protocol,
        harness_relative_path=harness_relative_path,
        telemetry_path=telemetry_path,
        herdr_snapshot_command=snapshot_command,
        launch_arguments=launch_arguments,
    )


def extract_ids(snapshot: Any, field: str) -> set[str]:
    """Extract stable IDs from common Herdr snapshot shapes without content capture."""

    if not isinstance(snapshot, dict):
        return set()
    value = snapshot.get(field, [])
    if not isinstance(value, list):
        return set()
    result: set[str] = set()
    for item in value:
        if isinstance(item, str):
            result.add(item)
        elif isinstance(item, dict):
            for key in ("id", field.removesuffix("s") + "_id", "workspace_id", "tab_id", "pane_id", "agent_instance_id"):
                candidate = item.get(key)
                if isinstance(candidate, str):
                    result.add(candidate)
                    break
    return result


def compare_snapshots(before: dict[str, Any], after: dict[str, Any], *, expected_protocol: int) -> dict[str, Any]:
    if not isinstance(before, dict) or not isinstance(after, dict):
        raise ContractError("snapshot.invalid", "Herdr before/after snapshots must be objects")
    before_protocol = before.get("protocol", before.get("protocol_revision"))
    after_protocol = after.get("protocol", after.get("protocol_revision"))
    if before_protocol != expected_protocol or after_protocol != expected_protocol:
        raise ContractError(
            "snapshot.protocol_mismatch",
            "Herdr snapshot protocol does not match the native contract",
            expected=expected_protocol,
            before=before_protocol,
            after=after_protocol,
        )
    fields = ("workspaces", "tabs", "panes", "agents", "lineage")
    stable_ids = {
        field: {
            "before": sorted(extract_ids(before, field)),
            "after": sorted(extract_ids(after, field)),
        }
        for field in fields
    }
    before_host = before.get("host")
    after_host = after.get("host")
    return {
        "status": "PASS",
        "protocol": expected_protocol,
        "host": {
            "before": before_host,
            "after": after_host,
            "changed": before_host != after_host,
        },
        "stable_ids": stable_ids,
        "changed_id_sets": {
            field: values["before"] != values["after"]
            for field, values in stable_ids.items()
        },
    }


def validate_exact_cleanup(resources: Iterable[FixtureResource]) -> dict[str, Any]:
    resources = tuple(resources)
    return {
        "mode": "exact-identities-only",
        "automatic_cleanup": False,
        "requires_explicit_confirmation": True,
        "resources": [resource.as_dict() for resource in resources],
        "forbidden": ["glob", "prefix", "current-workspace", "all-processes"],
    }
