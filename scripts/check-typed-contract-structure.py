#!/usr/bin/env python3
"""Fail when wire deserialization leaks back into the replica or domain."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent

def absent(pattern, text, description):
    result = subprocess.run(['rg', '-n', pattern, '-'], input=text, text=True, capture_output=True)
    if result.returncode != 1:
        raise SystemExit(f'{description}:\n{result.stdout}{result.stderr}')

replica = (ROOT / 'herdr-core/src/session_sync.rs').read_text().split('#[cfg(test)]\nmod tests', 1)[0]
absent(r'WorkspaceWire|WorkspaceWorktreeWire|TabWire|PaneWire|WireAgent|AgentListResult|SequencedEventEnvelope|Deserialize|serde_json::from_|Value::|\.get\("|\.as_array\(', replica, 'replica owns wire parsing')
for name in ['runtime.rs', 'domain.rs', 'sidebar.rs', 'session_sync.rs']:
    text = (ROOT / 'herdr-core/src' / name).read_text()
    absent(r'herdr_contract::wire|OUT_DIR|res::SessionSnapshot|ev::EventData', text, f'{name} references generated types')
for path in (ROOT / 'herdr-core/src').glob('*.rs'):
    if path.name in {'wire.rs', 'herdr_contract.rs'}:
        continue
    absent(r'herdr_contract::wire', path.read_text(), f'{path.name} bypasses the boundary')
print('PASS: replica has no handwritten wire parser; generated types stay in the boundary')
