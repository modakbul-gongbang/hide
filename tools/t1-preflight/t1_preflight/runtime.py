from __future__ import annotations

import json
import os
import socket
import subprocess
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from .macos import (
    capture_accessibility,
    capture_window,
    executable_path_for_pid,
    frontmost_application_identity,
    pids_for_executable,
    run_command,
    sample_process_tree,
)
from .model import (
    ContractError,
    Manifest,
    evaluate_runtime_evidence,
    is_t5_option_meta_profile,
    is_t5_option_meta_v4_profile,
)
from .native_input import NativeInput, layout_invariant_key_codes


TELEMETRY_SCHEMA = "herdr.t1-preflight.telemetry.v1"


class EventReader:
    def __init__(
        self,
        path: Path,
        *,
        run_id: str,
        phase: str,
        pid: int,
        phase_timeout_ms: int | None = None,
        on_event: Callable[[dict[str, Any]], None] | None = None,
        frontmost_identity_probe: Callable[[], dict[str, Any]] | None = None,
    ) -> None:
        self.path = path
        self.run_id = run_id
        self.phase = phase
        self.pid = pid
        self.events: list[dict[str, Any]] = []
        self.seen_lines = 0
        self.last_seq = 0
        self.last_monotonic_ns = 0
        self.on_event = on_event
        self.frontmost_identity_probe = (
            frontmost_identity_probe or frontmost_application_identity
        )
        self.phase_deadline = (
            time.monotonic() + phase_timeout_ms / 1000
            if phase_timeout_ms is not None
            else None
        )

    def poll(self) -> list[dict[str, Any]]:
        if not self.path.exists():
            return []
        raw = self.path.read_text(encoding="utf-8")
        lines = raw.splitlines()
        if raw and not raw.endswith("\n"):
            lines = lines[:-1]
        if len(lines) < self.seen_lines:
            raise ContractError(
                "telemetry.truncated",
                "app telemetry file was truncated",
                path=str(self.path),
            )
        new_events: list[dict[str, Any]] = []
        for line_number, line in enumerate(
            lines[self.seen_lines :], self.seen_lines + 1
        ):
            try:
                event = json.loads(line)
            except json.JSONDecodeError as error:
                raise ContractError(
                    "telemetry.invalid_json",
                    "app telemetry contains invalid JSON",
                    path=str(self.path),
                    line=line_number,
                ) from error
            self._validate(event, line_number)
            self.events.append(event)
            new_events.append(event)
            if self.on_event is not None:
                self.on_event(event)
        self.seen_lines = len(lines)
        return new_events

    def wait_for(
        self,
        event_name: str,
        *,
        timeout_ms: int,
        process: subprocess.Popen[bytes],
        after_seq: int = 0,
        predicate: Callable[[dict[str, Any]], bool] | None = None,
        abort_on_focus_loss_after: int | None = None,
    ) -> dict[str, Any]:
        deadline = time.monotonic() + timeout_ms / 1000
        if self.phase_deadline is not None:
            deadline = min(deadline, self.phase_deadline)
        while time.monotonic() < deadline:
            self.poll()
            for event in self.events:
                if event["seq"] <= after_seq:
                    continue
                if (
                    abort_on_focus_loss_after is not None
                    and event["seq"] > abort_on_focus_loss_after
                    and self._is_focus_loss_event(event)
                ):
                    raise self._focus_interference_error(
                        event_name=event_name,
                        interference=event,
                        injection_boundary_seq=abort_on_focus_loss_after,
                    )
                if (
                    event["event"] == "telemetry.emit_failed"
                    and event.get("target") == event_name
                ):
                    # The app has already said this event was dropped at the
                    # emitter; waiting out the timeout would only hide the cause.
                    raise ContractError(
                        "telemetry.emit_failed",
                        "app dropped required telemetry at the emitter",
                        phase=self.phase,
                        event=event_name,
                        cause=event.get("cause"),
                        seq=event["seq"],
                    )
                if event["event"] == event_name and (
                    predicate is None or predicate(event)
                ):
                    return event
            exit_code = process.poll()
            if exit_code is not None:
                raise ContractError(
                    "runtime.early_exit",
                    "app exited before required telemetry arrived",
                    phase=self.phase,
                    event=event_name,
                    exit_code=exit_code,
                )
            time.sleep(0.01)
        raise ContractError(
            "telemetry.timeout",
            "required app telemetry did not arrive before timeout",
            phase=self.phase,
            event=event_name,
            timeout_ms=timeout_ms,
        )

    @staticmethod
    def _is_focus_loss_event(event: dict[str, Any]) -> bool:
        if event["event"] in ("app.lifecycle.resign", "window.lifecycle.resign"):
            return True
        if event["event"] == "input.focus.state":
            focus = event.get("focus")
            return isinstance(focus, dict) and not all(
                focus.get(field) is True
                for field in (
                    "app_frontmost",
                    "key_window",
                    "render_view_first_responder",
                )
            )
        return False

    def _focus_interference_error(
        self,
        *,
        event_name: str,
        interference: dict[str, Any],
        injection_boundary_seq: int,
    ) -> ContractError:
        try:
            frontmost = self.frontmost_identity_probe()
        except ContractError as error:
            # Keep the causal focus loss as the primary failure while making a
            # failed attribution probe observable and injectable in tests.
            frontmost = {
                "error": {
                    "code": error.code,
                    "message": error.message,
                    "details": error.details,
                }
            }
        except Exception as error:
            frontmost = {
                "error": {
                    "code": "frontmost.identity_query_failed",
                    "message": "in-process frontmost identity probe failed",
                    "error": repr(error),
                }
            }
        return ContractError(
            "environment_focus_interference",
            "app lost foreground focus after injection before the awaited route telemetry",
            phase=self.phase,
            event=event_name,
            interference_event=interference["event"],
            interference_seq=interference["seq"],
            injection_boundary_seq=injection_boundary_seq,
            frontmost_at_detection=frontmost,
        )
        return None

    def latest_focus_state(self) -> dict[str, Any] | None:
        """Return the newest AppKit focus snapshot published by the app."""

        event = self.latest_focus_event()
        return event.get("focus") if event is not None else None

    def latest_focus_event(self) -> dict[str, Any] | None:
        """Return the newest focus event, including its sequence boundary."""

        self.poll()
        for event in reversed(self.events):
            if event.get("event") == "input.focus.state" and isinstance(
                event.get("focus"), dict
            ):
                return event
        return None

    def _validate(self, event: Any, line_number: int) -> None:
        if not isinstance(event, dict):
            raise ContractError(
                "telemetry.invalid_event",
                "telemetry line must be an object",
                line=line_number,
            )
        expected = {
            "schema": TELEMETRY_SCHEMA,
            "run_id": self.run_id,
            "phase": self.phase,
            "pid": self.pid,
        }
        for field, expected_value in expected.items():
            if event.get(field) != expected_value:
                raise ContractError(
                    "telemetry.identity_mismatch",
                    "telemetry identity does not match the launched phase",
                    line=line_number,
                    field=field,
                    expected=expected_value,
                    actual=event.get(field),
                )
        sequence = event.get("seq")
        monotonic_ns = event.get("monotonic_ns")
        if not isinstance(sequence, int) or sequence != self.last_seq + 1:
            raise ContractError(
                "telemetry.sequence_gap",
                "telemetry seq must increase by exactly one",
                line=line_number,
                previous=self.last_seq,
                actual=sequence,
            )
        if not isinstance(monotonic_ns, int) or monotonic_ns <= self.last_monotonic_ns:
            raise ContractError(
                "telemetry.non_monotonic",
                "telemetry monotonic_ns must strictly increase",
                line=line_number,
                previous=self.last_monotonic_ns,
                actual=monotonic_ns,
            )
        if not isinstance(event.get("event"), str) or not event["event"]:
            raise ContractError(
                "telemetry.event_name_missing",
                "telemetry event must have a name",
                line=line_number,
            )
        self.last_seq = sequence
        self.last_monotonic_ns = monotonic_ns


@dataclass
class PhaseResult:
    phase: str
    pid: int
    observed_usable_ms: float
    events: list[dict[str, Any]]
    log_path: Path
    event_path: Path
    profiles: dict[str, list[dict[str, Any]]]
    terminal_latencies_ms: list[float]
    semantic: dict[str, Any]
    screenshots: list[dict[str, Any]]
    accessibility: dict[str, Any] | None
    frontmost_application_timeline: list[dict[str, Any]]

    def as_dict(self) -> dict[str, Any]:
        return {
            "phase": self.phase,
            "pid": self.pid,
            "observed_usable_ms": self.observed_usable_ms,
            "events": self.events,
            "log_path": str(self.log_path),
            "event_path": str(self.event_path),
            "profiles": self.profiles,
            "terminal_latencies_ms": self.terminal_latencies_ms,
            "semantic": self.semantic,
            "screenshots": self.screenshots,
            "accessibility": self.accessibility,
            "frontmost_application_timeline": self.frontmost_application_timeline,
        }


class RuntimeHarness:
    def __init__(self, app_path: Path, manifest: Manifest) -> None:
        self.app_path = app_path.resolve(strict=True)
        self.manifest = manifest
        self.executable = self.app_path / manifest.bundle["main_executable"]
        self.native = NativeInput()
        package_dir = Path(__file__).parent
        self.activate_script = package_dir / "activate_process.applescript"
        self.resize_script = package_dir / "resize_window.applescript"
        self.ax_script = package_dir / "ax_snapshot.applescript"
        self.scenario = json.loads(manifest.scenario_path.read_text(encoding="utf-8"))
        self.event_timeout_ms = int(manifest.runtime["timeouts_ms"]["event"])
        self.exit_timeout_ms = int(manifest.runtime["timeouts_ms"]["exit"])
        self.phase_timeout_ms = int(manifest.runtime["timeouts_ms"]["phase"])
        self._frontmost_application_timeline: list[dict[str, Any]] = []
        self._frontmost_focus_all_true: bool | None = None
        self._frontmost_tracking_enabled = False
        self._current_phase: str | None = None
        self._current_action_socket: Path | None = None
        self._frontmost_identity_probe = frontmost_application_identity

    def run(self) -> dict[str, Any]:
        self._require_exclusive_hands_off()
        existing = pids_for_executable(self.executable)
        if existing:
            raise ContractError(
                "instance.preexisting",
                "exact app executable is already running",
                executable=str(self.executable),
                pids=existing,
            )
        clean = self._run_phase("clean_closed")
        warm = self._run_phase("warm_closed")
        browser_included = self._run_phase("browser_included")
        relaunch = self._run_phase("relaunch_closed")
        semantic = dict(warm.semantic)
        semantic.update(browser_included.semantic)
        semantic.update(relaunch.semantic)
        semantic["exactly_one_instance"] = True
        semantic["verification_profile"] = self.manifest.verification_profile
        profiles = {
            "browser_closed": warm.profiles["browser_closed"],
            "browser_included": browser_included.profiles["browser_included"],
        }
        verdict = evaluate_runtime_evidence(
            budgets=self.manifest.budgets,
            warm_usable_ms=warm.observed_usable_ms,
            terminal_latencies_ms=warm.terminal_latencies_ms,
            profiles=profiles,
            semantic=semantic,
        )
        return {
            "schema": "herdr.t1-preflight.runtime-evidence.v1",
            "status": verdict["status"],
            "verdict": verdict,
            "phases": {
                "clean_closed": clean.as_dict(),
                "warm_closed": warm.as_dict(),
                "browser_included": browser_included.as_dict(),
                "relaunch_closed": relaunch.as_dict(),
            },
        }

    @staticmethod
    def _require_exclusive_hands_off() -> None:
        if os.environ.get("HERDR_T5_EXCLUSIVE_HANDS_OFF") != "1":
            raise ContractError(
                "environment_focus_interference",
                "native runtime verification requires an explicit exclusive hands-off window",
                required_environment={"HERDR_T5_EXCLUSIVE_HANDS_OFF": "1"},
            )

    def _run_phase(self, phase: str) -> PhaseResult:
        self._current_phase = phase
        self._frontmost_application_timeline = []
        self._frontmost_focus_all_true = None
        self._frontmost_tracking_enabled = False
        event_path = self.manifest.output_dir / "runtime" / f"{phase}.events.jsonl"
        log_path = self.manifest.output_dir / "runtime" / f"{phase}.log"
        action_socket = Path("/tmp") / (
            f"herdr-t1-action-{self.manifest.digest[:12]}-{phase}.sock"
        )
        event_path.parent.mkdir(parents=True, exist_ok=True)
        if event_path.exists() or log_path.exists() or action_socket.exists():
            raise ContractError(
                "runtime.artifact_exists",
                "phase artifacts already exist before launch",
                phase=phase,
            )
        arguments = [
            str(self.executable),
            *self.manifest.render_arguments(
                phase, event_path, action_socket=action_socket
            ),
        ]
        log_handle = log_path.open("wb")
        started = time.monotonic()
        process = subprocess.Popen(
            arguments,
            cwd=self.executable.parent,
            stdin=subprocess.DEVNULL,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        reader = EventReader(
            event_path,
            run_id=self.manifest.run_id,
            phase=phase,
            pid=process.pid,
            phase_timeout_ms=self.phase_timeout_ms,
            on_event=self._observe_focus_event,
            frontmost_identity_probe=self._frontmost_identity_probe,
        )
        profiles: dict[str, list[dict[str, Any]]] = {}
        terminal_latencies: list[float] = []
        semantic: dict[str, Any] = {}
        screenshots: list[dict[str, Any]] = []
        accessibility: dict[str, Any] | None = None
        try:
            self._assert_single_instance(process.pid)
            usable = reader.wait_for(
                "app.usable",
                timeout_ms=self.event_timeout_ms,
                process=process,
            )
            observed_usable_ms = (time.monotonic() - started) * 1000
            expected_launch_mode = self.manifest.runtime["phases"][phase]["launch_mode"]
            if usable.get("launch_mode") != expected_launch_mode:
                raise ContractError(
                    "telemetry.launch_mode_mismatch",
                    "app.usable launch_mode does not match the manifest phase",
                    phase=phase,
                    expected=expected_launch_mode,
                    actual=usable.get("launch_mode"),
            )
            self.native.activate(process.pid, self.activate_script)
            self._current_action_socket = action_socket
            self._frontmost_tracking_enabled = phase == "warm_closed"
            self._frontmost_focus_all_true = None
            if phase == "warm_closed":
                (
                    profiles,
                    terminal_latencies,
                    semantic,
                    screenshots,
                    accessibility,
                ) = self._run_warm_actions(process, reader, usable)
            elif phase == "browser_included":
                profiles, semantic, screenshots = self._run_browser_included(
                    process, reader, usable
                )
            elif phase == "relaunch_closed":
                restored = reader.wait_for(
                    "relaunch.restored",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=usable["seq"],
                )
                semantic["relaunch_state_hash"] = self._required_string(
                    restored, "state_hash", "relaunch.restored"
                )
            reader.poll()
            premature_complete = next(
                (
                    event
                    for event in reader.events
                    if event["event"] == "phase.complete"
                    and event["seq"] > usable["seq"]
                ),
                None,
            )
            if premature_complete is not None:
                raise ContractError(
                    "telemetry.premature_phase_complete",
                    "phase.complete arrived before the harness sent native Cmd+Q",
                    phase=phase,
                    seq=premature_complete["seq"],
                )
            pre_quit_seq = reader.last_seq
            self._inject_key(process, reader, 12, ["command"], "phase.quit")
            reader.wait_for(
                "phase.complete",
                timeout_ms=self.event_timeout_ms,
                process=process,
                after_seq=pre_quit_seq,
            )
            try:
                exit_code = process.wait(timeout=self.exit_timeout_ms / 1000)
            except subprocess.TimeoutExpired as error:
                raise ContractError(
                    "runtime.exit_timeout",
                    "app did not exit after native Cmd+Q",
                    phase=phase,
                ) from error
            if exit_code != 0:
                raise ContractError(
                    "runtime.nonzero_exit",
                    "app phase exited non-zero",
                    phase=phase,
                    exit_code=exit_code,
                )
            reader.poll()
            dropped_emits = [
                event
                for event in reader.events
                if event["event"] == "telemetry.emit_failed"
            ]
            if dropped_emits:
                raise ContractError(
                    "telemetry.emit_failed",
                    "app dropped telemetry at the emitter during the phase",
                    phase=phase,
                    dropped=[
                        {
                            "seq": event["seq"],
                            "target": event.get("target"),
                            "cause": event.get("cause"),
                        }
                        for event in dropped_emits
                    ],
                )
            remaining = pids_for_executable(self.executable)
            if remaining:
                raise ContractError(
                    "instance.leaked",
                    "exact app executable remains after phase exit",
                    pids=remaining,
                )
            return PhaseResult(
                phase=phase,
                pid=process.pid,
                observed_usable_ms=observed_usable_ms,
                events=reader.events,
                log_path=log_path,
                event_path=event_path,
                profiles=profiles,
                terminal_latencies_ms=terminal_latencies,
                semantic=semantic,
                screenshots=screenshots,
                accessibility=accessibility,
                frontmost_application_timeline=list(
                    self._frontmost_application_timeline
                ),
            )
        except Exception as error:
            if isinstance(error, ContractError):
                error.details.setdefault(
                    "frontmost_application_timeline",
                    list(self._frontmost_application_timeline),
                )
                if "session_event_taps" not in error.details:
                    try:
                        error.details["session_event_taps"] = (
                            self.native.session_event_taps()
                        )
                    except Exception as tap_error:
                        # Evidence decoration must not mask the causal failure,
                        # but its own failure has to stay visible in the report.
                        error.details["session_event_taps"] = {
                            "error": str(tap_error)
                        }
            self._terminate_owned_process(process)
            if action_socket.exists():
                action_socket.unlink()
            raise
        finally:
            self._current_action_socket = None
            log_handle.close()

    def _run_warm_actions(
        self,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        usable: dict[str, Any],
    ) -> tuple[
        dict[str, list[dict[str, Any]]],
        list[float],
        dict[str, Any],
        list[dict[str, Any]],
        dict[str, Any] | None,
    ]:
        profiles: dict[str, list[dict[str, Any]]] = {}
        terminal_latencies: list[float] = []
        semantic: dict[str, Any] = {}
        screenshots: list[dict[str, Any]] = []
        accessibility: dict[str, Any] | None = None
        cursor = usable["seq"]
        fixture_event: dict[str, Any] | None = None
        zoom_entered: dict[str, Any] | None = None
        option_meta_seen = False
        plain_key_control_seen = False
        for index, action in enumerate(self.scenario["actions"]):
            name = action["action"]
            if name == "fixture.assert":
                fixture_event = reader.wait_for(
                    "fixture.ready",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                )
                cursor = fixture_event["seq"]
                semantic.update(
                    {
                        "workspace_count": fixture_event.get("workspace_count"),
                        "pane_count": fixture_event.get("pane_count"),
                        "browser_closed": fixture_event.get("browser_open") is False,
                    }
                )
                if fixture_event.get("cef_initialized") is not False:
                    raise ContractError(
                        "profile.cef_initialized_in_closed_mode",
                        "browser-closed launch must report cef_initialized=false",
                        actual=fixture_event.get("cef_initialized"),
                    )
            elif name == "terminal.native_input":
                fixture_event = self._require_fixture(fixture_event)
                self._click_named_point(
                    fixture_event,
                    action.get("target_point", "terminal"),
                    process=process,
                    reader=reader,
                    injection=f"{name}.target",
                )
                count = int(action.get("count", 32))
                if count < 20:
                    raise ContractError(
                        "scenario.insufficient_terminal_probes",
                        "terminal.native_input count must be at least 20",
                        count=count,
                    )
                prefix = str(action.get("prefix", "917"))
                for probe_index in range(count):
                    probe = f"{prefix}-{probe_index:03d}"
                    for key_code in layout_invariant_key_codes(probe):
                        self._inject_key(
                            process,
                            reader,
                            key_code,
                            [],
                            f"{name}:{probe}",
                        )
                    self._inject_key(process, reader, 36, [], f"{name}:{probe}.return")
                    presented = reader.wait_for(
                        "terminal.input_presented",
                        timeout_ms=self.event_timeout_ms,
                        process=process,
                        after_seq=cursor,
                        predicate=lambda event, expected=probe: event.get("probe")
                        == expected,
                    )
                    cursor = presented["seq"]
                    input_ns = self._required_int(presented, "input_monotonic_ns", name)
                    present_ns = self._required_int(
                        presented, "present_monotonic_ns", name
                    )
                    if present_ns < input_ns:
                        raise ContractError(
                            "telemetry.negative_latency",
                            "terminal present precedes input",
                            probe=probe,
                        )
                    terminal_latencies.append((present_ns - input_ns) / 1_000_000)
            elif name == "terminal.plain_key_control":
                if not is_t5_option_meta_v4_profile(self.manifest.verification_profile):
                    raise ContractError(
                        "scenario.plain_key_control_unsupported",
                        "plain-key control is only valid for the T5 v4 profile",
                    )
                if plain_key_control_seen:
                    raise ContractError(
                        "scenario.plain_key_control_repeated",
                        "T5 v4 plain-key control may run only once per warm phase",
                    )
                plain_key_control_seen = True
                fixture_event = self._require_fixture(fixture_event)
                click_after_seq = self._click_named_point(
                    fixture_event,
                    action.get("target_point", "terminal"),
                    process=process,
                    reader=reader,
                    injection=f"{name}.target",
                )
                if action.get("repeat") != 1:
                    raise ContractError(
                        "scenario.plain_key_control_repeat",
                        "T5 v4 plain-key control must inject exactly once",
                        repeat=action.get("repeat"),
                    )
                expected_bytes = self._required_string(
                    action, "expected_bytes_hex", name
                )
                native_probe_917_008_seq = self._require_native_probe_before_action(
                    reader.events, cursor
                )
                action_armed = self._arm_scenario_action(
                    process,
                    reader,
                    action=name,
                    key_code=int(action["key_code"]),
                    after_seq=cursor,
                )
                cursor = action_armed["seq"]
                control_injection = self._inject_key(
                    process,
                    reader,
                    int(action["key_code"]),
                    list(action["modifiers"]),
                    name,
                    fresh_focus_after_seq=click_after_seq,
                    frontmost_target_required=True,
                )
                if control_injection is None:
                    raise ContractError(
                        "environment_focus_interference",
                        "plain-key control did not record a fresh focus snapshot",
                        injection=name,
                    )
                pty_event = reader.wait_for(
                    "input.plain_key_control.pty",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                    abort_on_focus_loss_after=control_injection["seq"],
                )
                source_order = ["appkit", "app-routing", "herdr", "pty"]
                source_events = [
                    event
                    for event in reader.events
                    if cursor < event["seq"] <= pty_event["seq"]
                    and event.get("event", "").startswith("input.plain_key_control.")
                ]
                observed_sources = [
                    str(event["event"]).removeprefix("input.plain_key_control.")
                    for event in source_events
                ]
                if observed_sources != source_order:
                    raise ContractError(
                        "telemetry.plain_key_control_timeline",
                        "plain-key control must prove appkit to app-routing to herdr to pty order",
                        expected=source_order,
                        actual=observed_sources,
                        after_seq=cursor,
                        through_seq=pty_event["seq"],
                    )
                for event in source_events:
                    if event.get("key_code") != int(action["key_code"]):
                        raise ContractError(
                            "telemetry.plain_key_control_key",
                            "plain-key control telemetry key code does not match the scenario",
                            expected=int(action["key_code"]),
                            actual=event.get("key_code"),
                            event=event.get("event"),
                        )
                    if event.get("modifiers") != list(action["modifiers"]):
                        raise ContractError(
                            "telemetry.plain_key_control_modifiers",
                            "plain-key control telemetry modifiers do not match the scenario",
                            expected=list(action["modifiers"]),
                            actual=event.get("modifiers"),
                            event=event.get("event"),
                        )
                    NativeInput.assert_injection_focus(
                        event.get("focus"), pid=process.pid, injection=name
                    )
                if pty_event.get("bytes_hex") != expected_bytes:
                    raise ContractError(
                        "telemetry.plain_key_control_bytes",
                        "plain-key control PTY bytes do not match the scenario contract",
                        expected=expected_bytes,
                        actual=pty_event.get("bytes_hex"),
                    )
                if pty_event.get("status") != "accepted":
                    raise ContractError(
                        "telemetry.plain_key_control_rejected",
                        "plain-key control route was not accepted",
                        status=pty_event.get("status"),
                        error=pty_event.get("error"),
                    )
                input_ns = self._required_int(pty_event, "input_monotonic_ns", name)
                pty_ns = self._required_int(pty_event, "pty_monotonic_ns", name)
                if pty_ns < input_ns:
                    raise ContractError(
                        "telemetry.negative_latency",
                        "PTY route precedes plain-key control input",
                        input_monotonic_ns=input_ns,
                        pty_monotonic_ns=pty_ns,
                    )
                semantic["plain_key_control"] = {
                    "count": 1,
                    "expected_count": 1,
                    "expected_bytes_hex": expected_bytes,
                    "source_order": source_order,
                    "source_events": source_events,
                    "pty_seq": pty_event["seq"],
                    "latency_ms": (pty_ns - input_ns) / 1_000_000,
                    "focus": pty_event.get("focus"),
                    "pre_injection_focus_event": control_injection,
                    "action_armed_seq": action_armed["seq"],
                    "action_contract": action_armed.get("input_contract"),
                    "native_probe_917_008_seq": native_probe_917_008_seq,
                }
                cursor = pty_event["seq"]
            elif name == "terminal.option_meta":
                if option_meta_seen:
                    raise ContractError(
                        "scenario.option_meta_repeated",
                        "T5 Option+F action may run only once per warm phase",
                    )
                option_meta_seen = True
                fixture_event = self._require_fixture(fixture_event)
                target_point = action.get("target_point", "terminal")
                click_after_seq = self._click_named_point(
                    fixture_event,
                    target_point,
                    process=process,
                    reader=reader,
                    injection=f"{name}.target",
                )
                repeat = action.get("repeat")
                if repeat != 2:
                    raise ContractError(
                        "scenario.option_meta_repeat",
                        "T5 Option+F action must inject exactly twice",
                        repeat=repeat,
                    )
                expected_bytes = self._required_string(
                    action, "expected_bytes_hex", name
                )
                source_order = ["appkit", "app-routing", "herdr", "pty"]
                option_events: list[dict[str, Any]] = []
                option_latencies: list[float] = []
                focus_before: dict[str, Any] | None = None
                focus_snapshots: list[dict[str, Any]] = []
                pre_injection_focus_events: list[dict[str, Any]] = []
                option_event_start = cursor
                for option_index in range(repeat):
                    pre_injection_after_seq = (
                        click_after_seq if option_index == 0 else cursor
                    )
                    pre_injection_focus_event = self._inject_key(
                        process,
                        reader,
                        int(action["key_code"]),
                        list(action["modifiers"]),
                        f"{name}[{option_index + 1}]",
                        fresh_focus_after_seq=pre_injection_after_seq,
                        frontmost_target_required=is_t5_option_meta_v4_profile(
                            self.manifest.verification_profile
                        ),
                    )
                    if pre_injection_focus_event is None:
                        raise ContractError(
                            "environment_focus_interference",
                            "T5 Option+F injection did not record a fresh focus snapshot",
                            injection=f"{name}[{option_index + 1}]",
                        )
                    pre_injection_focus_events.append(pre_injection_focus_event)
                    focus = pre_injection_focus_event.get("focus")
                    if isinstance(focus, dict):
                        focus_snapshots.append(dict(focus))
                        if focus_before is None:
                            focus_before = dict(focus)
                    modifier_down = reader.wait_for(
                        "input.modifier.flags_changed",
                        timeout_ms=self.event_timeout_ms,
                        process=process,
                        after_seq=pre_injection_focus_event["seq"],
                        abort_on_focus_loss_after=pre_injection_focus_event["seq"],
                        predicate=lambda event: event.get("key_code")
                        == 58
                        and event.get("modifiers") == ["option"],
                    )
                    NativeInput.assert_injection_focus(
                        modifier_down.get("focus"),
                        pid=process.pid,
                        injection=f"{name}[{option_index + 1}].modifier_down",
                    )
                    route_after_seq = pre_injection_focus_event["seq"]
                    pty_event = reader.wait_for(
                        "input.option_meta.pty",
                        timeout_ms=self.event_timeout_ms,
                        process=process,
                        after_seq=route_after_seq,
                        abort_on_focus_loss_after=pre_injection_focus_event["seq"],
                    )
                    source_events = [
                        event
                        for event in reader.events
                        if route_after_seq < event["seq"] <= pty_event["seq"]
                        and event.get("event", "").startswith("input.option_meta.")
                    ]
                    observed_sources = [
                        str(event["event"]).removeprefix("input.option_meta.")
                        for event in source_events
                    ]
                    if observed_sources != source_order:
                        raise ContractError(
                            "telemetry.option_meta_timeline",
                            "Option+F telemetry must prove appkit to app-routing to herdr to pty order",
                            expected=source_order,
                            actual=observed_sources,
                            after_seq=cursor,
                            through_seq=pty_event["seq"],
                        )
                    for event in source_events:
                        if event.get("key_code") != int(action["key_code"]):
                            raise ContractError(
                                "telemetry.option_meta_key",
                                "Option+F telemetry key code does not match the scenario",
                                expected=int(action["key_code"]),
                                actual=event.get("key_code"),
                                event=event.get("event"),
                            )
                        if event.get("modifiers") != list(action["modifiers"]):
                            raise ContractError(
                                "telemetry.option_meta_modifiers",
                                "Option+F telemetry modifiers do not match the scenario",
                                expected=list(action["modifiers"]),
                                actual=event.get("modifiers"),
                                event=event.get("event"),
                            )
                        focus = event.get("focus")
                        NativeInput.assert_injection_focus(
                            focus,
                            pid=process.pid,
                            injection=f"{name}[{option_index + 1}]",
                        )
                        if isinstance(focus, dict):
                            focus_snapshots.append(dict(focus))
                    if pty_event.get("bytes_hex") != expected_bytes:
                        raise ContractError(
                            "telemetry.option_meta_bytes",
                            "PTY Option+F bytes do not match the scenario contract",
                            expected=expected_bytes,
                            actual=pty_event.get("bytes_hex"),
                        )
                    if pty_event.get("status") != "accepted":
                        raise ContractError(
                            "telemetry.option_meta_rejected",
                            "PTY Option+F route was not accepted",
                            status=pty_event.get("status"),
                            error=pty_event.get("error"),
                        )
                    modifier_up = reader.wait_for(
                        "input.modifier.flags_changed",
                        timeout_ms=self.event_timeout_ms,
                        process=process,
                        after_seq=pty_event["seq"],
                        predicate=lambda event: event.get("key_code")
                        == 58
                        and event.get("modifiers") == [],
                        abort_on_focus_loss_after=pre_injection_focus_event["seq"],
                    )
                    NativeInput.assert_injection_focus(
                        modifier_up.get("focus"),
                        pid=process.pid,
                        injection=f"{name}[{option_index + 1}].modifier_up",
                    )
                    input_ns = self._required_int(
                        pty_event, "input_monotonic_ns", name
                    )
                    pty_ns = self._required_int(
                        pty_event, "pty_monotonic_ns", name
                    )
                    if pty_ns < input_ns:
                        raise ContractError(
                            "telemetry.negative_latency",
                            "PTY route precedes Option+F input",
                            input_monotonic_ns=input_ns,
                            pty_monotonic_ns=pty_ns,
                        )
                    option_latencies.append((pty_ns - input_ns) / 1_000_000)
                    option_events.append(
                        {
                            "index": option_index + 1,
                            "pty_seq": pty_event["seq"],
                            "source_events": source_events,
                            "modifier_events": {
                                "down": modifier_down,
                                "up": modifier_up,
                            },
                            "bytes_hex": pty_event.get("bytes_hex"),
                            "latency_ms": option_latencies[-1],
                            "focus": pty_event.get("focus"),
                        }
                    )
                    cursor = modifier_up["seq"]
                semantic["option_meta"] = {
                    "count": len(option_events),
                    "expected_count": repeat,
                    "expected_bytes_hex": expected_bytes,
                    "source_order": source_order,
                    "events": option_events,
                    "latencies_ms": option_latencies,
                    "focus_before": focus_before,
                    "focus_snapshots": focus_snapshots,
                    "pre_injection_focus_events": pre_injection_focus_events,
                    "option_event_start_seq": option_event_start,
                    "latency_budget_ms": float(action["latency_budget_ms"]),
                }
            elif name == "profile.browser_closed":
                fixture_event = self._require_fixture(fixture_event)
                if (
                    fixture_event.get("browser_open") is not False
                    or fixture_event.get("workspace_count") != 7
                    or fixture_event.get("pane_count") != 11
                ):
                    raise ContractError(
                        "profile.closed_fixture_mismatch",
                        "browser-closed profile must have 7 workspaces and 11 panes",
                        event=fixture_event,
                    )
                time.sleep(float(action.get("settle_ms", 0)) / 1000)
                closed_samples = self._sample_profile(process.pid)
                browser_paths = {
                    str((self.app_path / relative).resolve(strict=False))
                    for relative in self.manifest.bundle["browser_executables"]
                }
                observed_paths = {
                    member["executable"]
                    for sample in closed_samples
                    for member in sample["members"]
                }
                forbidden = sorted(browser_paths.intersection(observed_paths))
                if forbidden:
                    raise ContractError(
                        "profile.browser_helper_in_closed_mode",
                        "browser-closed launch spawned a Browser helper",
                        executables=forbidden,
                    )
                profiles["browser_closed"] = closed_samples
            elif name == "ax.snapshot":
                destination = (
                    self.manifest.output_dir
                    / "ax"
                    / f"{index:02d}-{action.get('name', 'main')}.txt"
                )
                accessibility = capture_accessibility(
                    process.pid, self.ax_script, destination
                )
                missing_labels = [
                    label
                    for label in self.manifest.runtime["expected_ax_labels"]
                    if label not in accessibility["raw"]
                ]
                if missing_labels:
                    raise ContractError(
                        "ax.labels_missing",
                        "Accessibility tree is missing required labels",
                        missing=missing_labels,
                    )
                semantic["ax_labels_present"] = True
            elif name == "screenshot.capture":
                fixture_event = self._require_fixture(fixture_event)
                window_id = self._required_int(fixture_event, "window_id", name)
                destination = (
                    self.manifest.output_dir
                    / "screenshots"
                    / f"{index:02d}-{action.get('name', 'window')}.png"
                )
                screenshots.append(capture_window(window_id, destination))
            elif name == "zoom.shortcut":
                self._inject_key(
                    process,
                    reader,
                    int(action.get("key_code", 36)),
                    ["command", "shift"],
                    f"{name}.enter",
                )
                zoom_entered = reader.wait_for(
                    "zoom.entered",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                )
                cursor = zoom_entered["seq"]
                semantic["zoom_before_topology_hash"] = self._required_string(
                    zoom_entered, "before_topology_hash", name
                )
                window_id = self._required_int(zoom_entered, "window_id", name)
                destination = (
                    self.manifest.output_dir / "screenshots" / f"{index:02d}-zoomed.png"
                )
                screenshots.append(capture_window(window_id, destination))
                self._inject_key(
                    process,
                    reader,
                    int(action.get("key_code", 36)),
                    ["command", "shift"],
                    f"{name}.restore",
                )
                restored = reader.wait_for(
                    "zoom.restored",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                )
                cursor = restored["seq"]
                semantic["zoom_after_topology_hash"] = self._required_string(
                    restored, "after_topology_hash", name
                )
            elif name == "focus.click":
                fixture_event = self._require_fixture(fixture_event)
                before = (
                    reader.events[-1].get("focused_pane") if reader.events else None
                )
                self._click_named_point(
                    fixture_event,
                    str(action.get("target_point", "editor")),
                    process=process,
                    reader=reader,
                    injection=name,
                )
                changed = reader.wait_for(
                    "focus.changed",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                )
                cursor = changed["seq"]
                semantic["focus_before"] = changed.get("from", before)
                semantic["focus_after"] = changed.get("to")
            elif name == "window.resize":
                width = int(action["width"])
                height = int(action["height"])
                run_command(
                    [
                        "/usr/bin/osascript",
                        str(self.resize_script),
                        str(process.pid),
                        str(width),
                        str(height),
                    ],
                    timeout=20,
                )
                resized = reader.wait_for(
                    "window.resized",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                )
                cursor = resized["seq"]
                semantic.update(
                    {
                        "resize_logical_width": resized.get("logical_width"),
                        "resize_physical_width": resized.get("physical_width"),
                        "resize_scale_factor": resized.get("scale_factor"),
                    }
                )
            elif name == "ime.physical_keys":
                fixture_event = self._require_fixture(fixture_event)
                self._click_named_point(
                    fixture_event,
                    action.get("target_point", "terminal"),
                    process=process,
                    reader=reader,
                    injection=f"{name}.target",
                )
                expected_source = self._required_string(action, "input_source_id", name)
                actual_source = self.native.current_input_source_id()
                if actual_source != expected_source:
                    raise ContractError(
                        "input.ime_source_mismatch",
                        "Korean physical-key proof requires the manifest input source to already be active",
                        expected=expected_source,
                        actual=actual_source,
                    )
                key_codes = action.get("key_codes")
                if (
                    not isinstance(key_codes, list)
                    or not key_codes
                    or any(not isinstance(item, int) for item in key_codes)
                ):
                    raise ContractError(
                        "scenario.invalid_ime_keys",
                        "ime.physical_keys requires integer key_codes",
                    )
                for key_code in key_codes:
                    self._inject_key(
                        process,
                        reader,
                        key_code,
                        [],
                        f"{name}.key[{key_code}]",
                    )
                    time.sleep(int(action.get("interval_ms", 40)) / 1000)
                self._inject_key(process, reader, 36, [], f"{name}.return")
                expected_text = self._required_string(action, "expected_text", name)
                committed_parts: list[str] = []
                marked_observed = False
                deadline = time.monotonic() + self.event_timeout_ms / 1000
                while time.monotonic() < deadline and len(committed_parts) < 16:
                    remaining_ms = max(1, int((deadline - time.monotonic()) * 1000))
                    committed = reader.wait_for(
                        "ime.committed",
                        timeout_ms=remaining_ms,
                        process=process,
                        after_seq=cursor,
                    )
                    cursor = committed["seq"]
                    committed_parts.append(
                        self._required_string(committed, "text", "ime.committed")
                    )
                    marked_observed |= committed.get("marked_observed") is True
                    actual_text = unicodedata.normalize("NFC", "".join(committed_parts))
                    if actual_text == expected_text and marked_observed:
                        break
                else:
                    actual_text = unicodedata.normalize("NFC", "".join(committed_parts))
                if actual_text != expected_text or not marked_observed:
                    raise ContractError(
                        "ime.composition_not_proven",
                        "IME telemetry must show marked text and the exact expected committed Korean text",
                        expected=expected_text,
                        actual=actual_text,
                        commit_events=len(committed_parts),
                        marked_observed=marked_observed,
                    )
            elif name == "state.persist":
                persisted = reader.wait_for(
                    "state.persisted",
                    timeout_ms=self.event_timeout_ms,
                    process=process,
                    after_seq=cursor,
                )
                cursor = persisted["seq"]
                semantic["persisted_state_hash"] = self._required_string(
                    persisted, "state_hash", name
                )
            else:
                raise ContractError(
                    "scenario.unknown_action",
                    "scenario action is unsupported",
                    action=name,
                )
        if zoom_entered is None:
            raise ContractError(
                "scenario.zoom_not_run", "warm scenario did not execute zoom proof"
            )
        semantic["native_screenshots_present"] = len(screenshots) >= 2 and all(
            Path(item["path"]).is_file() for item in screenshots
        )
        if is_t5_option_meta_profile(self.manifest.verification_profile) and not option_meta_seen:
            raise ContractError(
                "scenario.option_meta_missing",
                "T5 warm scenario did not execute terminal.option_meta",
            )
        if is_t5_option_meta_v4_profile(self.manifest.verification_profile) and not plain_key_control_seen:
            raise ContractError(
                "scenario.plain_key_control_missing",
                "T5 v4 warm scenario did not execute terminal.plain_key_control",
            )
        return profiles, terminal_latencies, semantic, screenshots, accessibility

    def _run_browser_included(
        self,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        usable: dict[str, Any],
    ) -> tuple[dict[str, list[dict[str, Any]]], dict[str, Any], list[dict[str, Any]]]:
        ready = reader.wait_for(
            "browser.profile.ready",
            timeout_ms=self.event_timeout_ms,
            process=process,
            after_seq=usable["seq"],
        )
        if ready.get("browser_open") is not True:
            raise ContractError(
                "profile.browser_not_open",
                "browser-included launch must report an open Browser pane",
            )
        if ready.get("cef_initialized") is not True:
            raise ContractError(
                "profile.cef_not_initialized",
                "browser-included launch must report cef_initialized=true",
                actual=ready.get("cef_initialized"),
            )
        settle_ms = float(
            self.manifest.runtime["phases"]["browser_included"]["profile_settle_ms"]
        )
        time.sleep(settle_ms / 1000)
        samples = self._sample_profile(process.pid)
        browser_paths = {
            str((self.app_path / relative).resolve(strict=False))
            for relative in self.manifest.bundle["browser_executables"]
        }
        observed_paths = {
            member["executable"] for sample in samples for member in sample["members"]
        }
        if not browser_paths.intersection(observed_paths):
            raise ContractError(
                "profile.browser_helper_missing",
                "browser-included launch did not spawn a manifest-declared Browser helper",
                expected=sorted(browser_paths),
                observed=sorted(observed_paths),
            )
        cdp = self._verify_cdp(
            self._required_string(ready, "cdp_http_endpoint", "browser.profile.ready")
        )
        window_id = self._required_int(ready, "window_id", "browser.profile.ready")
        screenshot = capture_window(
            window_id,
            self.manifest.output_dir / "screenshots" / "browser-included.png",
        )
        return (
            {"browser_included": samples},
            {
                "browser_included_cdp": True,
                "cdp": cdp,
            },
            [screenshot],
        )

    def _sample_profile(self, pid: int) -> list[dict[str, Any]]:
        sampling = self.manifest.runtime["sampling"]
        interval = float(sampling["interval_ms"]) / 1000
        count = int(sampling["count"])
        samples: list[dict[str, Any]] = []
        for index in range(count):
            self._assert_single_instance(pid)
            samples.append(
                sample_process_tree(
                    pid,
                    app_path=self.app_path,
                    allowed_external_processes=self.manifest.runtime.get(
                        "allowed_external_processes", []
                    ),
                )
            )
            if index + 1 < count:
                time.sleep(interval)
        return samples

    def _verify_cdp(self, raw_endpoint: str) -> dict[str, Any]:
        parsed = urllib.parse.urlparse(raw_endpoint)
        if (
            parsed.scheme != "http"
            or parsed.hostname not in {"127.0.0.1", "localhost", "::1"}
            or parsed.port is None
        ):
            raise ContractError(
                "cdp.endpoint_not_loopback",
                "CDP HTTP endpoint must be an explicit loopback host and port",
                endpoint=raw_endpoint,
            )
        host = f"[{parsed.hostname}]" if parsed.hostname == "::1" else parsed.hostname
        origin = f"http://{host}:{parsed.port}"
        responses: dict[str, Any] = {}
        for path in ("/json/version", "/json/list"):
            request = urllib.request.Request(
                origin + path, headers={"Accept": "application/json"}
            )
            try:
                with urllib.request.urlopen(
                    request, timeout=self.event_timeout_ms / 1000
                ) as response:
                    if response.status != 200:
                        raise ContractError(
                            "cdp.http_status",
                            "CDP endpoint returned a non-200 response",
                            path=path,
                            status=response.status,
                        )
                    payload = json.loads(response.read().decode("utf-8"))
            except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
                raise ContractError(
                    "cdp.query_failed",
                    "CDP endpoint query failed",
                    path=path,
                    error=repr(error),
                ) from error
            responses[path] = payload
        version = responses["/json/version"]
        targets = responses["/json/list"]
        if not isinstance(version, dict) or not isinstance(
            version.get("webSocketDebuggerUrl"), str
        ):
            raise ContractError(
                "cdp.version_invalid", "CDP version response has no websocket URL"
            )
        if not isinstance(targets, list) or not any(
            isinstance(target, dict) and target.get("type") == "page"
            for target in targets
        ):
            raise ContractError(
                "cdp.page_missing", "CDP target list has no page target"
            )
        return {"endpoint": origin, "version": version, "targets": targets}

    def _click_named_point(
        self,
        fixture: dict[str, Any],
        name: str,
        *,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        injection: str,
    ) -> int:
        points = fixture.get("cg_screen_points")
        if not isinstance(points, dict) or name not in points:
            raise ContractError(
                "telemetry.point_missing",
                "fixture telemetry has no requested CGEvent point",
                point=name,
            )
        point = points[name]
        if (
            not isinstance(point, dict)
            or not isinstance(point.get("x"), (int, float))
            or not isinstance(point.get("y"), (int, float))
        ):
            raise ContractError(
                "telemetry.point_invalid",
                "CGEvent point must contain numeric x and y",
                point=name,
            )
        reader.poll()
        self._assert_injection_precondition(process, reader, injection)
        click_after_seq = reader.last_seq
        self.native.click(float(point["x"]), float(point["y"]))
        return click_after_seq

    def _observe_focus_event(self, event: dict[str, Any]) -> None:
        if not self._frontmost_tracking_enabled:
            return
        if event.get("event") != "input.focus.state":
            return
        focus = event.get("focus")
        if not isinstance(focus, dict):
            return
        all_true = all(
            focus.get(field) is True
            for field in (
                "app_frontmost",
                "key_window",
                "render_view_first_responder",
            )
        )
        if not all_true and self._frontmost_focus_all_true is not False:
            self._capture_frontmost_application("focus_loss", event)
        self._frontmost_focus_all_true = all_true

    def _capture_frontmost_application(
        self, boundary: str, focus_event: dict[str, Any] | None
    ) -> dict[str, Any]:
        try:
            observed = self._frontmost_identity_probe()
            if not isinstance(observed, dict):
                raise TypeError(
                    "frontmost identity probe returned a non-object result"
                )
        except ContractError as error:
            # Attribution is evidence decoration. Preserve the triggering
            # focus/lifecycle event even when the OS identity query itself
            # fails, rather than replacing it with a probe error.
            observed = {
                "error": {
                    "code": error.code,
                    "message": error.message,
                    "details": error.details,
                }
            }
        except Exception as error:
            observed = {
                "error": {
                    "code": "frontmost.identity_query_failed",
                    "message": "in-process frontmost identity probe failed",
                    "error": repr(error),
                }
            }
        entry: dict[str, Any] = {
            "boundary": boundary,
            "probe_boundary": observed.get("boundary"),
            "phase": self._current_phase,
            "event": focus_event.get("event") if focus_event else None,
            "event_seq": focus_event.get("seq") if focus_event else None,
            "event_monotonic_ns": (
                focus_event.get("monotonic_ns") if focus_event else None
            ),
            "focus": focus_event.get("focus") if focus_event else None,
        }
        entry.update({key: value for key, value in observed.items() if key != "boundary"})
        self._frontmost_application_timeline.append(entry)
        return entry

    @staticmethod
    def _require_native_probe_before_action(
        events: list[dict[str, Any]], after_seq: int, probe: str = "917-008"
    ) -> int:
        completed = [
            event
            for event in events
            if event.get("event") == "terminal.input_presented"
            and event.get("probe") == probe
            and event.get("seq", 0) <= after_seq
        ]
        if not completed:
            raise ContractError(
                "telemetry.native_probe_before_control",
                "declared plain-key control requires the native 917-008 probe to complete first",
                probe=probe,
                after_seq=after_seq,
            )
        return int(completed[-1]["seq"])

    def _arm_scenario_action(
        self,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        *,
        action: str,
        key_code: int,
        after_seq: int,
    ) -> dict[str, Any]:
        socket_path = self._current_action_socket
        if socket_path is None:
            raise ContractError(
                "scenario.action_channel_missing",
                "declared scenario action has no native action channel",
                action=action,
            )
        request = {
            "action": action,
            "phase": reader.phase,
            "key_code": key_code,
            "contract": "scenario-action",
        }
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as channel:
                channel.settimeout(2.0)
                channel.connect(str(socket_path))
                channel.sendall((json.dumps(request, sort_keys=True) + "\n").encode())
                response = channel.makefile("rb").readline()
        except OSError as error:
            raise ContractError(
                "scenario.action_channel_unavailable",
                "native scenario action channel could not be reached",
                action=action,
                socket=str(socket_path),
                error=str(error),
            ) from error
        if not response:
            raise ContractError(
                "scenario.action_channel_empty",
                "native scenario action channel returned no acknowledgement",
                action=action,
                socket=str(socket_path),
            )
        try:
            acknowledgement = json.loads(response.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ContractError(
                "scenario.action_channel_invalid_response",
                "native scenario action channel returned invalid JSON",
                action=action,
            ) from error
        if (
            acknowledgement.get("status") != "armed"
            or acknowledgement.get("action") != action
            or acknowledgement.get("phase") != reader.phase
            or acknowledgement.get("key_code") != key_code
            or acknowledgement.get("input_contract") != "scenario-action"
        ):
            raise ContractError(
                "scenario.action_channel_rejected",
                "native scenario action channel rejected the declared action",
                action=action,
                expected={
                    "status": "armed",
                    "action": action,
                    "phase": reader.phase,
                    "key_code": key_code,
                    "input_contract": "scenario-action",
                },
                actual=acknowledgement,
            )
        armed_event = reader.wait_for(
            "input.action.armed",
            timeout_ms=self.event_timeout_ms,
            process=process,
            after_seq=after_seq,
            predicate=lambda event: event.get("action") == action
            and event.get("phase") == reader.phase
            and event.get("key_code") == key_code
            and event.get("input_contract") == "scenario-action",
        )
        return armed_event

    def _inject_key(
        self,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        key_code: int,
        modifiers: list[str],
        injection: str,
        fresh_focus_after_seq: int | None = None,
        frontmost_target_required: bool = False,
    ) -> dict[str, Any] | None:
        fresh_focus_event: dict[str, Any] | None = None
        if fresh_focus_after_seq is not None:
            fresh_focus_event = self._require_fresh_focus(
                process,
                reader,
                after_seq=fresh_focus_after_seq,
                injection=injection,
            )
        self._assert_injection_precondition(process, reader, injection)
        option_meta_boundary = injection.startswith("terminal.option_meta[")
        identity_boundary = option_meta_boundary or frontmost_target_required
        if identity_boundary:
            boundary = (
                "plain_key_control.pre_injection"
                if frontmost_target_required and not option_meta_boundary
                else "option_meta.pre_injection"
            )
            observed = self._capture_frontmost_application(
                boundary, reader.latest_focus_event()
            )
            if frontmost_target_required:
                self._assert_frontmost_target(observed, process, boundary)
        self.native.key(key_code, modifiers)
        if identity_boundary:
            boundary = (
                "plain_key_control.post_injection"
                if frontmost_target_required and not option_meta_boundary
                else "option_meta.post_injection"
            )
            observed = self._capture_frontmost_application(
                boundary, reader.latest_focus_event()
            )
            if frontmost_target_required:
                self._assert_frontmost_target(observed, process, boundary)
        return fresh_focus_event

    def _assert_frontmost_target(
        self,
        observed: dict[str, Any],
        process: subprocess.Popen[bytes],
        boundary: str,
    ) -> None:
        identity = observed.get("identity")
        if not isinstance(identity, dict):
            raise ContractError(
                "environment_focus_interference",
                "frontmost identity is missing at the injection boundary",
                boundary=boundary,
                observed=observed,
            )
        expected_bundle = self.manifest.bundle["identifier"]
        expected_pid = process.pid
        actual_bundle = identity.get("bundle_identifier")
        actual_pid = identity.get("process_identifier")
        if actual_bundle != expected_bundle or actual_pid != expected_pid:
            raise ContractError(
                "environment_focus_interference",
                "frontmost application does not match the declared target before input",
                boundary=boundary,
                expected={
                    "bundle_identifier": expected_bundle,
                    "process_identifier": expected_pid,
                },
                actual={
                    "bundle_identifier": actual_bundle,
                    "process_identifier": actual_pid,
                },
                focus=observed.get("focus"),
            )

    def _require_fresh_focus(
        self,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        *,
        after_seq: int,
        injection: str,
    ) -> dict[str, Any]:
        """Require the first focus event after a structural input boundary."""

        event = reader.wait_for(
            "input.focus.state",
            timeout_ms=self.event_timeout_ms,
            process=process,
            after_seq=after_seq,
        )
        NativeInput.assert_injection_focus(
            event.get("focus"),
            pid=process.pid,
            injection=injection,
        )
        return event

    def _assert_injection_precondition(
        self,
        process: subprocess.Popen[bytes],
        reader: EventReader,
        injection: str,
    ) -> None:
        """Check focus at the last safe point before posting a CGEvent."""

        NativeInput.assert_injection_focus(
            reader.latest_focus_state(),
            pid=process.pid,
            injection=injection,
        )

    def _assert_single_instance(self, pid: int) -> None:
        actual = pids_for_executable(self.executable)
        if actual != [pid]:
            raise ContractError(
                "instance.count_mismatch",
                "exactly one main app instance must be running",
                expected=[pid],
                actual=actual,
            )

    def _terminate_owned_process(self, process: subprocess.Popen[bytes]) -> None:
        if process.poll() is not None:
            return
        actual_path = executable_path_for_pid(process.pid)
        if actual_path != self.executable.resolve(strict=False):
            raise ContractError(
                "cleanup.identity_mismatch",
                "refusing to terminate a PID whose executable identity changed",
                pid=process.pid,
                expected=str(self.executable),
                actual=str(actual_path) if actual_path else None,
            )
        process.terminate()
        try:
            process.wait(timeout=self.exit_timeout_ms / 1000)
        except subprocess.TimeoutExpired as error:
            raise ContractError(
                "cleanup.term_timeout",
                "owned app ignored SIGTERM; fail-closed cleanup will not escalate automatically",
                pid=process.pid,
            ) from error

    @staticmethod
    def _require_fixture(event: dict[str, Any] | None) -> dict[str, Any]:
        if event is None:
            raise ContractError(
                "scenario.fixture_not_ready", "action requires fixture.ready telemetry"
            )
        return event

    @staticmethod
    def _required_string(value: dict[str, Any], field: str, context: str) -> str:
        result = value.get(field)
        if not isinstance(result, str) or not result:
            raise ContractError(
                "telemetry.field_missing",
                "telemetry field must be a non-empty string",
                field=field,
                context=context,
            )
        return result

    @staticmethod
    def _required_int(value: dict[str, Any], field: str, context: str) -> int:
        result = value.get(field)
        if isinstance(result, bool) or not isinstance(result, int):
            raise ContractError(
                "telemetry.field_missing",
                "telemetry field must be an integer",
                field=field,
                context=context,
            )
        return result
