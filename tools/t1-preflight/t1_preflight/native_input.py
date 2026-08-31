from __future__ import annotations

import ctypes
import time
from pathlib import Path
from typing import Any, Mapping

from .macos import run_command
from .model import ContractError


K_CG_HID_EVENT_TAP = 0
K_CG_EVENT_FLAG_MASK_SHIFT = 1 << 17
K_CG_EVENT_FLAG_MASK_CONTROL = 1 << 18
K_CG_EVENT_FLAG_MASK_ALTERNATE = 1 << 19
K_CG_EVENT_FLAG_MASK_COMMAND = 1 << 20
MODIFIER_FLAGS = {
    "shift": K_CG_EVENT_FLAG_MASK_SHIFT,
    "control": K_CG_EVENT_FLAG_MASK_CONTROL,
    "option": K_CG_EVENT_FLAG_MASK_ALTERNATE,
    "command": K_CG_EVENT_FLAG_MASK_COMMAND,
}
MODIFIER_KEY_CODES = {
    "shift": 56,
    "control": 59,
    "option": 58,
    "command": 55,
}


def chord_steps(key_code: int, modifiers: list[str]) -> list[tuple[int, bool, int]]:
    """(virtual_key, key_down, flags) steps reproducing a physical chord.

    A physical keyboard never delivers a modified key event without first
    delivering the modifier key's own flags-changed transition. Session
    event-tap consumers (window managers, Screen Sharing) track modifier
    state from those transitions, so a synthetic chord that skips them is
    an inconsistent stream those filters may consume or reroute; an
    unmodified key must keep the exact two-step shape that has always been
    injected.
    """

    if isinstance(key_code, bool) or not 0 <= key_code <= 127:
        raise ContractError(
            "input.invalid_key_code",
            "key code is outside the macOS range",
            key_code=key_code,
        )
    seen: set[str] = set()
    for modifier in modifiers:
        if modifier not in MODIFIER_FLAGS:
            raise ContractError(
                "input.invalid_modifier", "unsupported modifier", modifier=modifier
            )
        if modifier in seen:
            raise ContractError(
                "input.duplicate_modifier",
                "a physical chord cannot press the same modifier twice",
                modifier=modifier,
            )
        seen.add(modifier)

    engaged = 0
    steps: list[tuple[int, bool, int]] = []
    for modifier in modifiers:
        engaged |= MODIFIER_FLAGS[modifier]
        steps.append((MODIFIER_KEY_CODES[modifier], True, engaged))
    steps.append((key_code, True, engaged))
    steps.append((key_code, False, engaged))
    for modifier in reversed(modifiers):
        engaged &= ~MODIFIER_FLAGS[modifier]
        steps.append((MODIFIER_KEY_CODES[modifier], False, engaged))
    return steps


LAYOUT_INVARIANT_KEY_CODES = {
    "0": 29,
    "1": 18,
    "2": 19,
    "3": 20,
    "4": 21,
    "5": 23,
    "6": 22,
    "7": 26,
    "8": 28,
    "9": 25,
    "-": 27,
}


def layout_invariant_key_codes(value: str) -> list[int]:
    try:
        return [LAYOUT_INVARIANT_KEY_CODES[character] for character in value]
    except KeyError as error:
        raise ContractError(
            "input.layout_dependent_text",
            "physical probe text must use only layout-invariant digits and hyphen",
            value=value,
            unsupported_character=error.args[0],
        ) from error


class CGPoint(ctypes.Structure):
    _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]


class NativeInput:
    def __init__(self) -> None:
        self.core_graphics = ctypes.CDLL(
            "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics"
        )
        self.core_foundation = ctypes.CDLL(
            "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation"
        )
        self.carbon = ctypes.CDLL("/System/Library/Frameworks/Carbon.framework/Carbon")
        self._configure_signatures()

    def _configure_signatures(self) -> None:
        cg = self.core_graphics
        cg.CGEventCreateKeyboardEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ushort,
            ctypes.c_bool,
        ]
        cg.CGEventCreateKeyboardEvent.restype = ctypes.c_void_p
        cg.CGEventSetFlags.argtypes = [ctypes.c_void_p, ctypes.c_uint64]
        cg.CGEventKeyboardSetUnicodeString.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.POINTER(ctypes.c_uint16),
        ]
        cg.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
        cg.CGEventCreateMouseEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint32,
            CGPoint,
            ctypes.c_uint32,
        ]
        cg.CGEventCreateMouseEvent.restype = ctypes.c_void_p
        self.core_foundation.CFRelease.argtypes = [ctypes.c_void_p]
        self.core_foundation.CFStringGetCString.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_long,
            ctypes.c_uint32,
        ]
        self.core_foundation.CFStringGetCString.restype = ctypes.c_bool
        self.carbon.TISCopyCurrentKeyboardInputSource.argtypes = []
        self.carbon.TISCopyCurrentKeyboardInputSource.restype = ctypes.c_void_p
        self.carbon.TISGetInputSourceProperty.argtypes = [
            ctypes.c_void_p,
            ctypes.c_void_p,
        ]
        self.carbon.TISGetInputSourceProperty.restype = ctypes.c_void_p

    def activate(self, pid: int, script_path: Path) -> dict[str, Any]:
        evidence = run_command(
            ["/usr/bin/osascript", str(script_path), str(pid)], timeout=15
        )
        return evidence.as_dict()

    @staticmethod
    def assert_injection_focus(
        focus_state: Mapping[str, Any] | None,
        *,
        pid: int,
        injection: str,
    ) -> None:
        """Fail closed when native input cannot be attributed to our app.

        The runtime app publishes this snapshot from the AppKit main thread.
        Keeping the check in the input boundary means every CGEvent path uses
        the same precondition, and a user-owned foreground app is reported as
        environment interference instead of a product input failure.
        """

        required = (
            "app_frontmost",
            "key_window",
            "render_view_first_responder",
        )
        if focus_state is None:
            raise ContractError(
                "environment_focus_interference",
                "native injection aborted because the app has not published a complete focus snapshot",
                pid=pid,
                injection=injection,
                missing=list(required),
            )
        invalid = [
            field
            for field in required
            if not isinstance(focus_state.get(field), bool)
        ]
        if invalid:
            raise ContractError(
                "environment_focus_interference",
                "native injection aborted because the app focus snapshot is incomplete",
                pid=pid,
                injection=injection,
                invalid_fields=invalid,
                focus_state=dict(focus_state),
            )
        failed = {
            field: focus_state[field]
            for field in required
            if focus_state[field] is not True
        }
        if failed:
            raise ContractError(
                "environment_focus_interference",
                "native injection aborted because another app or window owns focus",
                pid=pid,
                injection=injection,
                failed_checks=failed,
                focus_state=dict(focus_state),
            )

    def key(self, key_code: int, modifiers: list[str] | None = None) -> None:
        for step_key_code, key_down, flags in chord_steps(key_code, modifiers or []):
            self._post_key(step_key_code, key_down, flags)

    def physical_keys(self, key_codes: list[int], *, interval_ms: int = 35) -> None:
        for key_code in key_codes:
            self.key(key_code)
            time.sleep(interval_ms / 1000)

    def unicode_text(self, value: str) -> None:
        if not value:
            raise ContractError(
                "input.empty_text", "native Unicode input must not be empty"
            )
        encoded = value.encode("utf-16-le")
        units = (ctypes.c_uint16 * (len(encoded) // 2)).from_buffer_copy(encoded)
        event_down = self._create_key_event(0, True)
        event_up = self._create_key_event(0, False)
        try:
            self.core_graphics.CGEventKeyboardSetUnicodeString(
                event_down, len(units), units
            )
            self.core_graphics.CGEventKeyboardSetUnicodeString(
                event_up, len(units), units
            )
            self.core_graphics.CGEventPost(K_CG_HID_EVENT_TAP, event_down)
            self.core_graphics.CGEventPost(K_CG_HID_EVENT_TAP, event_up)
        finally:
            self.core_foundation.CFRelease(event_down)
            self.core_foundation.CFRelease(event_up)

    def layout_invariant_text(self, value: str) -> None:
        if not value:
            raise ContractError(
                "input.empty_text", "native physical input must not be empty"
            )
        for key_code in layout_invariant_key_codes(value):
            self.key(key_code)

    def click(self, x: float, y: float) -> None:
        if x < 0 or y < 0:
            raise ContractError(
                "input.invalid_point", "click point must be on-screen", x=x, y=y
            )
        point = CGPoint(float(x), float(y))
        down = self.core_graphics.CGEventCreateMouseEvent(None, 1, point, 0)
        up = self.core_graphics.CGEventCreateMouseEvent(None, 2, point, 0)
        if not down or not up:
            if down:
                self.core_foundation.CFRelease(down)
            if up:
                self.core_foundation.CFRelease(up)
            raise ContractError(
                "input.mouse_event_failed", "CGEventCreateMouseEvent returned null"
            )
        try:
            self.core_graphics.CGEventPost(K_CG_HID_EVENT_TAP, down)
            self.core_graphics.CGEventPost(K_CG_HID_EVENT_TAP, up)
        finally:
            self.core_foundation.CFRelease(down)
            self.core_foundation.CFRelease(up)

    def current_input_source_id(self) -> str:
        source = self.carbon.TISCopyCurrentKeyboardInputSource()
        if not source:
            raise ContractError(
                "input.source_unavailable",
                "current keyboard input source is unavailable",
            )
        try:
            property_key = ctypes.c_void_p.in_dll(
                self.carbon, "kTISPropertyInputSourceID"
            ).value
            value = self.carbon.TISGetInputSourceProperty(source, property_key)
            if not value:
                raise ContractError(
                    "input.source_id_unavailable",
                    "current keyboard input source has no identifier",
                )
            buffer = ctypes.create_string_buffer(512)
            utf8_encoding = 0x08000100
            if not self.core_foundation.CFStringGetCString(
                value, buffer, ctypes.sizeof(buffer), utf8_encoding
            ):
                raise ContractError(
                    "input.source_id_decode", "input source identifier is not UTF-8"
                )
            return buffer.value.decode("utf-8")
        finally:
            self.core_foundation.CFRelease(source)

    def session_event_taps(self) -> list[dict[str, Any]]:
        """Enumerate installed CGEvent taps for injection-failure evidence.

        An enabled filter tap (options=0) on keyboard events can consume or
        stall an injected chord before any app sees it, which is invisible
        from inside the target process; recording the tap table is the only
        way a failure report can name the candidate consumers.
        """

        class CGEventTapInformation(ctypes.Structure):
            _fields_ = [
                ("eventTapID", ctypes.c_uint32),
                ("tapPoint", ctypes.c_uint32),
                ("options", ctypes.c_uint32),
                ("eventsOfInterest", ctypes.c_uint64),
                ("tappingProcess", ctypes.c_int32),
                ("processBeingTapped", ctypes.c_int32),
                ("enabled", ctypes.c_bool),
                ("minUsecLatency", ctypes.c_float),
                ("avgUsecLatency", ctypes.c_float),
                ("maxUsecLatency", ctypes.c_float),
            ]

        cg = self.core_graphics
        cg.CGGetEventTapList.argtypes = [
            ctypes.c_uint32,
            ctypes.POINTER(CGEventTapInformation),
            ctypes.POINTER(ctypes.c_uint32),
        ]
        cg.CGGetEventTapList.restype = ctypes.c_int32
        count = ctypes.c_uint32(0)
        error = cg.CGGetEventTapList(0, None, ctypes.byref(count))
        if error != 0:
            raise ContractError(
                "input.event_tap_list_unavailable",
                "CGGetEventTapList could not report the installed event taps",
                cg_error=error,
            )
        buffer = (CGEventTapInformation * max(count.value, 1))()
        error = cg.CGGetEventTapList(count.value, buffer, ctypes.byref(count))
        if error != 0:
            raise ContractError(
                "input.event_tap_list_unavailable",
                "CGGetEventTapList could not report the installed event taps",
                cg_error=error,
            )
        keyboard_mask = (1 << 10) | (1 << 11) | (1 << 12)
        taps = []
        for tap in buffer[: count.value]:
            taps.append(
                {
                    "tap_id": tap.eventTapID,
                    "tap_point": tap.tapPoint,
                    "listen_only": tap.options == 1,
                    "events_of_interest": hex(tap.eventsOfInterest),
                    "taps_keyboard": bool(tap.eventsOfInterest & keyboard_mask),
                    "tapping_pid": tap.tappingProcess,
                    "enabled": bool(tap.enabled),
                    "avg_usec_latency": float(tap.avgUsecLatency),
                    "max_usec_latency": float(tap.maxUsecLatency),
                }
            )
        return taps

    def _post_key(self, key_code: int, down: bool, flags: int) -> None:
        event = self._create_key_event(key_code, down)
        try:
            # A null CGEventSource can inherit the preceding synthetic event's
            # modifier state. Explicitly write zero too so one phase's Cmd+Q
            # cannot turn the next phase's plain input into Command chords.
            self.core_graphics.CGEventSetFlags(event, flags)
            self.core_graphics.CGEventPost(K_CG_HID_EVENT_TAP, event)
        finally:
            self.core_foundation.CFRelease(event)

    def _create_key_event(self, key_code: int, down: bool) -> int:
        event = self.core_graphics.CGEventCreateKeyboardEvent(None, key_code, down)
        if not event:
            raise ContractError(
                "input.event_creation_failed",
                "CGEventCreateKeyboardEvent returned null",
                key_code=key_code,
                key_down=down,
            )
        return event
