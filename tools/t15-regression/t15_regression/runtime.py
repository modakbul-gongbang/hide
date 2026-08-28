from __future__ import annotations

import json
import os
import re
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Protocol

from .model import ContractError, RegressionManifest


class CommandRunner(Protocol):
    def run(self, command: tuple[str, ...], *, cwd: Path, environment: dict[str, str], timeout_seconds: float) -> "CommandResult": ...


@dataclass(frozen=True)
class CommandResult:
    argv: tuple[str, ...]
    exit_code: int
    stdout: str
    stderr: str
    duration_ms: float

    def as_dict(self) -> dict[str, Any]:
        return {
            "argv": list(self.argv),
            "exit_code": self.exit_code,
            "stdout": redact(self.stdout),
            "stderr": redact(self.stderr),
            "duration_ms": self.duration_ms,
        }


class SubprocessCommandRunner:
    def run(self, command: tuple[str, ...], *, cwd: Path, environment: dict[str, str], timeout_seconds: float) -> CommandResult:
        started = time.monotonic()
        env = os.environ.copy()
        env.update(environment)
        try:
            completed = subprocess.run(
                list(command),
                cwd=str(cwd),
                env=env,
                check=False,
                capture_output=True,
                text=True,
                timeout=timeout_seconds,
            )
        except subprocess.TimeoutExpired as error:
            raise ContractError("check.timeout", "T15 check exceeded its bounded timeout", argv=list(command), timeout_seconds=timeout_seconds, error=repr(error)) from error
        except OSError as error:
            raise ContractError("check.unavailable", "T15 check command could not start", argv=list(command), error=repr(error)) from error
        return CommandResult(
            argv=command,
            exit_code=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
            duration_ms=(time.monotonic() - started) * 1000,
        )


def redact(value: str, *, limit: int = 8000) -> str:
    text = value[-limit:]
    home = os.path.expanduser("~")
    text = text.replace(home, "<HOME>")
    text = re.sub(r"(?i)(api[_-]?key|token|authorization|cookie|password|secret)=([^\s&]+)", r"\1=<REDACTED>", text)
    return text


class OutputStore:
    MARKER = ".t15-regression-owner.json"

    def __init__(self, manifest: RegressionManifest) -> None:
        self.manifest = manifest
        self.marker_path = manifest.output_dir / self.MARKER
        self.result_path = manifest.output_dir / "result.json"

    def prepare(self) -> dict[str, Any] | None:
        if not self.manifest.output_dir.exists():
            self.manifest.output_dir.mkdir(parents=True)
            self.write(self.MARKER, self.manifest.ownership_marker())
            return None
        if self.manifest.output_dir.is_symlink() or not self.manifest.output_dir.is_dir():
            raise ContractError("output.unsafe", "T15 output must be a real directory", path=str(self.manifest.output_dir))
        if not self.marker_path.is_file():
            raise ContractError("output.unowned", "existing T15 output lacks its ownership marker", path=str(self.manifest.output_dir))
        try:
            marker = json.loads(self.marker_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise ContractError("output.marker_invalid", "T15 output marker is unreadable", error=repr(error)) from error
        if marker != self.manifest.ownership_marker():
            raise ContractError("output.identity_mismatch", "T15 output belongs to different inputs", path=str(self.manifest.output_dir))
        if not self.result_path.is_file():
            raise ContractError("output.incomplete", "T15 output is incomplete; create a new run identity", path=str(self.manifest.output_dir))
        return json.loads(self.result_path.read_text(encoding="utf-8"))

    def write(self, relative: str, value: Any) -> Path:
        target = (self.manifest.output_dir / Path(relative)).resolve(strict=False)
        try:
            target.relative_to(self.manifest.output_dir.resolve())
        except ValueError as error:
            raise ContractError("output.path_escape", "T15 evidence path escapes its output directory", path=relative) from error
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_name(f".{target.name}.tmp-{os.getpid()}")
        temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        os.replace(temporary, target)
        return target

    def finish(self, value: dict[str, Any]) -> Path:
        return self.write("result.json", value)


@dataclass
class RegressionRunner:
    manifest: RegressionManifest
    command_runner: CommandRunner
    clock_ns: Callable[[], int] = time.monotonic_ns

    def __post_init__(self) -> None:
        self.output = OutputStore(self.manifest)

    def run(self, *, execute: bool = True) -> dict[str, Any]:
        reused = self.output.prepare()
        if reused is not None:
            return reused
        started_ns = self.clock_ns()
        checks: list[dict[str, Any]] = []
        for spec in self.manifest.checks:
            check_started = self.clock_ns()
            if spec.initial_status in {"BLOCKED", "NOT_RUN"}:
                checks.append({
                    "id": spec.check_id,
                    "mode": spec.mode,
                    "required": spec.required,
                    "can_block": spec.can_block,
                    "status": spec.initial_status,
                    "reason": spec.reason or "not scheduled by manifest",
                    "duration_ms": (self.clock_ns() - check_started) / 1_000_000,
                })
                continue
            if spec.command is None:
                raise ContractError("check.command_missing", "RUN check has no command", check_id=spec.check_id)
            if not execute:
                checks.append({
                    "id": spec.check_id,
                    "mode": spec.mode,
                    "required": spec.required,
                    "can_block": spec.can_block,
                    "status": "NOT_RUN",
                    "reason": "dry-run mode",
                    "command": list(spec.command),
                    "duration_ms": (self.clock_ns() - check_started) / 1_000_000,
                })
                continue
            environment = dict(spec.environment)
            result: CommandResult | None = None
            try:
                result = self.command_runner.run(
                    spec.command,
                    cwd=self.manifest.root,
                    environment=environment,
                    timeout_seconds=spec.timeout_seconds,
                )
            except ContractError as error:
                if spec.can_block and error.code == "check.unavailable":
                    checks.append({
                        "id": spec.check_id,
                        "mode": spec.mode,
                        "required": spec.required,
                        "can_block": spec.can_block,
                        "status": "BLOCKED",
                        "reason": error.message,
                        "error": error.as_dict(),
                        "duration_ms": (self.clock_ns() - check_started) / 1_000_000,
                    })
                    continue
                raise
            assert result is not None
            status = "PASS" if result.exit_code == 0 else "FAIL"
            checks.append({
                "id": spec.check_id,
                "mode": spec.mode,
                "required": spec.required,
                "can_block": spec.can_block,
                "status": status,
                "command": result.as_dict(),
                "evidence_paths": list(spec.evidence_paths),
                "duration_ms": (self.clock_ns() - check_started) / 1_000_000,
            })
        required = [check for check in checks if check["required"]]
        if any(check["status"] == "FAIL" for check in required):
            status = "FAIL"
        elif any(check["status"] in {"BLOCKED", "NOT_RUN"} for check in required):
            status = "PARTIAL"
        else:
            status = "PASS"
        result = {
            "schema": "herdr.ide.t15-regression.result.v1",
            "status": status,
            "run_id": self.manifest.run_id,
            "verification_profile": self.manifest.verification_profile,
            "manifest_sha256": self.manifest.digest,
            "task_status": self.manifest.raw.get("task_status", {}),
            "checks": checks,
            "required_summary": {
                "total": len(required),
                "pass": sum(check["status"] == "PASS" for check in required),
                "blocked": sum(check["status"] == "BLOCKED" for check in required),
                "not_run": sum(check["status"] == "NOT_RUN" for check in required),
                "fail": sum(check["status"] == "FAIL" for check in required),
            },
            "side_effect_policy": "commands are argv-only; no shell, physical input, user-app cleanup, remote mutation, or provider API calls",
            "duration_ms": (self.clock_ns() - started_ns) / 1_000_000,
        }
        self.output.write("matrix.json", result)
        self.output.finish(result)
        return result
