#!/usr/bin/env python3
"""Delta size and inter-arrival stats from a capture JSONL file."""
import json
import sys
from pathlib import Path


def main():
    path = Path(sys.argv[1])
    sizes = []
    gaps = []
    last_t = None
    chunks = 0
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        encoded = line.encode("utf-8")
        sizes.append(len(encoded))
        t = row.get("t_ms")
        if last_t is not None and t is not None:
            gaps.append(t - last_t)
        last_t = t
        payload = row.get("payload") or {}
        chunk_list = payload.get("chunks") or (payload.get("terminal") or {}).get("chunks") or []
        chunks += len(chunk_list)
    sizes_sorted = sorted(sizes)
    gaps_sorted = sorted(gaps)
    mid = lambda values: values[len(values) // 2] if values else None
    print(
        json.dumps(
            {
                "messages": len(sizes),
                "chunks": chunks,
                "bytes_total": sum(sizes),
                "bytes_median": mid(sizes_sorted),
                "bytes_max": sizes_sorted[-1] if sizes_sorted else None,
                "gap_ms_median": mid(gaps_sorted),
                "gap_ms_max": gaps_sorted[-1] if gaps_sorted else None,
                "duration_ms": (last_t if last_t is not None else 0),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
