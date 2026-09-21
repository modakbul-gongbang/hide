#!/usr/bin/env python3
"""Replace terminal bytes with same-length filler. Keep length only."""
import base64
import json
import sys


FILL = 0x78  # ASCII 'x'


def redact_obj(obj):
    if isinstance(obj, dict):
        chunks = obj.get("chunks")
        if isinstance(chunks, list):
            obj["chunks"] = [redact_chunk(chunk) for chunk in chunks]
        terminal = obj.get("terminal")
        if isinstance(terminal, dict):
            obj["terminal"] = redact_obj(terminal)
        payload = obj.get("payload")
        if isinstance(payload, dict):
            obj["payload"] = redact_obj(payload)
        for key, value in list(obj.items()):
            if key not in ("chunks", "terminal", "payload"):
                obj[key] = redact_obj(value)
        return obj
    if isinstance(obj, list):
        return [redact_obj(item) for item in obj]
    return obj


def redact_chunk(chunk):
    if not isinstance(chunk, dict):
        return chunk
    encoded = chunk.get("bytes_base64")
    if not isinstance(encoded, str) or encoded == "":
        return chunk
    raw = base64.b64decode(encoded)
    chunk["bytes_len"] = len(raw)
    chunk["bytes_base64"] = base64.b64encode(bytes([FILL]) * len(raw)).decode("ascii")
    chunk["redacted"] = True
    return chunk


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        obj = json.loads(line)
        json.dump(redact_obj(obj), sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")


if __name__ == "__main__":
    main()
