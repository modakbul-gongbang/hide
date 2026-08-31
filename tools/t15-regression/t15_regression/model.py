from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SCHEMA = "herdr.ide.t15-regression.manifest.v1"
CHECK_MODES = frozenset(
    {
        "build/static",
        "automated behavior",
        "protocol/integration",
        "native runtime",
        "browser/CDP runtime",
        "agent runtime",
        "remote runtime",
        "performance/bundle",
    }
)
CHECK_STATUSES = frozenset({"RUN", "PASS", "BLOCKED", "NOT_RUN"})
ID_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
GLOB_CHARS = frozenset("*?[]{}")
FORBIDDEN_SHELL_CHARS = frozenset(";&|$`()<>\n")


class ContractError(RuntimeError):
    def __init__(self, code: str, message: str, **details: Any) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details

    def as_dict(self) -> dict[str, Any]:
        return {
            "schema": "herdr.ide.t15-regression.failure.v1",
            "event": "t15_regression.failed",
            "code": self.code,
            "message": self.message,
            "details": self.details,
        }


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractError("manifest.invalid_type", f"{field} must be a non-empty string", field=field)
    return value


def _exact_id(value: Any, field: str) -> str:
    result = _string(value, field)
    if not ID_PATTERN.fullmatch(result):
        raise ContractError("manifest.id_invalid", f"{field} must be a stable exact id", field=field, value=result)
    return result


def _relative_path(root: Path, value: Any, field: str) -> Path:
    raw = _string(value, field)
    path = Path(raw)
    if path.is_absolute() or path == Path(".") or ".." in path.parts or any(character in raw for character in GLOB_CHARS):
        raise ContractError("manifest.path_invalid", f"{field} must be one exact root-relative path", field=field, value=raw)
    # Keep the contract lexical. The approved worktree uses an explicit sibling
    # symlink for generated Rust targets; resolving that ancestor here would
    # reject a safe root-relative output before the ownership marker can bind it.
    # Parent traversal and absolute paths are already rejected above.
    return root.resolve(strict=True) / path


@dataclass(frozen=True)
class CheckSpec:
    check_id: str
    mode: str
    required: bool
    can_block: bool
    initial_status: str
    reason: str | None
    command: tuple[str, ...] | None
    environment: tuple[tuple[str, str], ...]
    timeout_seconds: float
    evidence_paths: tuple[str, ...]

    @classmethod
    def from_raw(cls, raw: Any, index: int) -> "CheckSpec":
        if not isinstance(raw, dict):
            raise ContractError("manifest.check_invalid", "checks entries must be objects", index=index)
        check_id = _exact_id(raw.get("id"), f"checks[{index}].id")
        mode = _string(raw.get("mode"), f"checks[{index}].mode")
        if mode not in CHECK_MODES:
            raise ContractError("manifest.mode_invalid", "check mode is not in the T15 verification contract", mode=mode)
        required = raw.get("required", True)
        can_block = raw.get("can_block", False)
        if not isinstance(required, bool) or not isinstance(can_block, bool):
            raise ContractError("manifest.boolean_invalid", "required and can_block must be booleans", check_id=check_id)
        initial_status = raw.get("status", "RUN")
        if initial_status not in CHECK_STATUSES:
            raise ContractError("manifest.status_invalid", "check status is not supported", check_id=check_id, status=initial_status)
        reason = raw.get("reason")
        if reason is not None and not isinstance(reason, str):
            raise ContractError("manifest.reason_invalid", "check reason must be text", check_id=check_id)
        command_raw = raw.get("command")
        command: tuple[str, ...] | None = None
        if command_raw is not None:
            if not isinstance(command_raw, list) or not command_raw or any(not isinstance(item, str) or not item for item in command_raw):
                raise ContractError("manifest.command_invalid", "check command must be an argv array", check_id=check_id)
            if any(any(character in item for character in FORBIDDEN_SHELL_CHARS) for item in command_raw):
                raise ContractError("manifest.shell_command", "T15 commands must not contain shell syntax", check_id=check_id)
            command = tuple(command_raw)
        if initial_status == "RUN" and command is None:
            raise ContractError("manifest.command_missing", "RUN checks require an explicit command", check_id=check_id)
        env_raw = raw.get("environment", {})
        if not isinstance(env_raw, dict) or any(not isinstance(key, str) or not isinstance(value, str) for key, value in env_raw.items()):
            raise ContractError("manifest.environment_invalid", "check environment must be a string map", check_id=check_id)
        timeout = raw.get("timeout_seconds", 120)
        if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or timeout <= 0 or timeout > 900:
            raise ContractError("manifest.timeout_invalid", "check timeout must be between 0 and 900 seconds", check_id=check_id)
        evidence_raw = raw.get("evidence_paths", [])
        if not isinstance(evidence_raw, list) or any(not isinstance(item, str) for item in evidence_raw):
            raise ContractError("manifest.evidence_invalid", "evidence_paths must be a string array", check_id=check_id)
        return cls(
            check_id=check_id,
            mode=mode,
            required=required,
            can_block=can_block,
            initial_status=initial_status,
            reason=reason,
            command=command,
            environment=tuple(sorted(env_raw.items())),
            timeout_seconds=float(timeout),
            evidence_paths=tuple(evidence_raw),
        )

    def as_dict(self) -> dict[str, Any]:
        value: dict[str, Any] = {
            "id": self.check_id,
            "mode": self.mode,
            "required": self.required,
            "can_block": self.can_block,
            "status": self.initial_status,
            "timeout_seconds": self.timeout_seconds,
            "evidence_paths": list(self.evidence_paths),
        }
        if self.reason is not None:
            value["reason"] = self.reason
        if self.command is not None:
            value["command"] = list(self.command)
        if self.environment:
            value["environment"] = dict(self.environment)
        return value


@dataclass(frozen=True)
class RegressionManifest:
    root: Path
    manifest_path: Path
    raw: dict[str, Any]
    run_id: str
    verification_profile: str
    output_dir: Path
    checks: tuple[CheckSpec, ...]

    @property
    def digest(self) -> str:
        return hashlib.sha256(canonical_json(self.raw)).hexdigest()

    def ownership_marker(self) -> dict[str, Any]:
        return {
            "schema": "herdr.ide.t15-regression.output-owner.v1",
            "run_id": self.run_id,
            "verification_profile": self.verification_profile,
            "manifest_sha256": self.digest,
            "output_dir": str(self.output_dir.relative_to(self.root)),
        }


def load_manifest(path: Path, *, root: Path | None = None) -> RegressionManifest:
    manifest_path = path.resolve(strict=True)
    root_path = (root or manifest_path.parent).resolve(strict=True)
    try:
        manifest_path.relative_to(root_path)
    except ValueError as error:
        raise ContractError("manifest.outside_root", "manifest must live inside root", manifest=str(manifest_path), root=str(root_path)) from error
    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError("manifest.unreadable", "manifest is not readable JSON", path=str(manifest_path), error=repr(error)) from error
    if not isinstance(raw, dict) or raw.get("schema") != SCHEMA:
        raise ContractError("manifest.schema", "T15 manifest schema is not supported", expected=SCHEMA)
    run_id = _exact_id(raw.get("run_id"), "run_id")
    profile = _exact_id(raw.get("verification_profile"), "verification_profile")
    output_dir = _relative_path(root_path, raw.get("output_dir"), "output_dir")
    if output_dir == root_path:
        raise ContractError("manifest.output_invalid", "output_dir must be a dedicated child")
    checks_raw = raw.get("checks")
    if not isinstance(checks_raw, list) or not checks_raw:
        raise ContractError("manifest.checks_missing", "T15 manifest must define checks")
    checks = tuple(CheckSpec.from_raw(value, index) for index, value in enumerate(checks_raw))
    check_ids = [check.check_id for check in checks]
    if len(set(check_ids)) != len(check_ids):
        raise ContractError("manifest.check_duplicate", "check IDs must be unique")
    return RegressionManifest(
        root=root_path,
        manifest_path=manifest_path,
        raw=raw,
        run_id=run_id,
        verification_profile=profile,
        output_dir=output_dir,
        checks=checks,
    )
