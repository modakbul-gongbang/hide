"""Bounded role-aware evidence, excluding tool output and typed screen echoes."""

import json
from pathlib import Path

from .protection import ProtectionError

MAX_SESSION_BYTES = 8 * 1024 * 1024


def messages(file: Path, kind: str) -> list[tuple[str, str]]:
    if not file.is_file() or file.is_symlink():
        return []
    if file.stat().st_size > MAX_SESSION_BYTES:
        raise ProtectionError("session_evidence_over_budget")
    result = []
    for line in file.read_text(errors="replace").splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue  # An incomplete append is retried on the next observation.
        if kind == "codex":
            if value.get("type") != "response_item":
                continue
            value = value.get("payload", {})
            if value.get("type") == "function_call_output":
                output = value.get("output")
                if isinstance(output, str):
                    result.append(("tool", output))
                continue
            if value.get("type") != "message":
                continue
            role = value.get("role")
            content = value.get("content", [])
            texts = [part["text"] for part in content
                     if part.get("type") in ("input_text", "output_text") and isinstance(part.get("text"), str)]
        elif kind == "claude":
            if value.get("type") not in ("user", "assistant"):
                continue
            value = value.get("message", {})
            role = value.get("role")
            content = value.get("content", [])
            if isinstance(content, list):
                for part in content:
                    if part.get("type") == "tool_result":
                        result.append(("tool", json.dumps(part.get("content", ""))))
            texts = [content] if isinstance(content, str) else [part["text"] for part in content
                     if part.get("type") == "text" and isinstance(part.get("text"), str)]
        else:
            return []  # Unsupported conversation formats cannot prove delivery.
        if role in ("user", "assistant") and texts:
            result.append((role, "\n".join(texts)))
    return result


def bell_turn(messages_: list[tuple[str, str]], bell: str, marker: str | None = None) -> bool:
    """The marker must be absent before this exact bell, and in its assistant reply.

    The bell's hook attachment may contain the letter, but the controller must
    not place it in an earlier user prompt or assistant message.
    """
    start = None
    for index, (role, text) in enumerate(messages_):
        if role == "user" and (text == bell or text.startswith(bell + "\n")):
            start = index
            break
    if start is None or (marker and any(marker in text for _, text in messages_[:start])):
        return False
    for role, text in messages_[start + 1:]:
        if role == "user":
            break
        if role == "assistant" and (marker is None or marker in text):
            return True
    return False
