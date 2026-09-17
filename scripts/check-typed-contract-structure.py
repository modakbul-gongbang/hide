#!/usr/bin/env python3
"""Fail when wire deserialization leaks back into the replica or domain."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent


def runtime_sources():
    """Return the complete runtime surface, including extracted submodules."""
    return [ROOT / 'herdr-core/src/runtime.rs',
            *sorted((ROOT / 'herdr-core/src/runtime').glob('*.rs'))]


def session_sync_sources():
    return [ROOT / 'herdr-core/src/session_sync.rs',
            *sorted((ROOT / 'herdr-core/src/session_sync').glob('*.rs'))]

def absent(pattern, text, description):
    matches = [f'{line_number}:{line}'
               for line_number, line in enumerate(text.splitlines(), 1)
               if re.search(pattern, line)]
    if matches:
        details = '\n'.join(matches)
        raise SystemExit(f'{description}:\n{details}')

replica = (ROOT / 'herdr-core/src/session_sync/replica.rs').read_text()
absent(r'WorkspaceWire|WorkspaceWorktreeWire|TabWire|PaneWire|WireAgent|AgentListResult|SequencedEventEnvelope|Deserialize|serde_json::from_|Value::|\.get\("|\.as_array\(', replica, 'replica owns wire parsing')
for path in [*runtime_sources(),
             ROOT / 'herdr-core/src/domain.rs',
             ROOT / 'herdr-core/src/sidebar.rs',
             *session_sync_sources()]:
    text = path.read_text()
    absent(r'herdr_contract::wire|OUT_DIR|res::SessionSnapshot|ev::EventData',
           text, f'{path.relative_to(ROOT)} references generated types')
for path in (ROOT / 'herdr-core/src').glob('*.rs'):
    if path.name in {'wire.rs', 'herdr_contract.rs'}:
        continue
    absent(r'herdr_contract::wire', path.read_text(), f'{path.name} bypasses the boundary')
for name in ['live.rs', 'remote.rs']:
    production = (ROOT / 'herdr-core/src' / name).read_text().split('#[cfg(test)]\nmod tests', 1)[0]
    absent(r'\.pointer\(|Value::as_|\.get\("|\["[a-z_]+"\]', production, f'{name} navigates untyped JSON')
    # JSON telemetry is not a Herdr request. Every retained macro must declare
    # its component, so a new request body cannot hide among those diagnostics.
    for macro in re.finditer(r'(?:serde_json::)?json!\s*\(\s*\{', production):
        if not re.match(r'\s*"component"\s*:', production[macro.end():]):
            raise SystemExit(f'{name} builds a handwritten JSON request at offset {macro.start()}')
print('PASS: live, remote and replica use one generated wire boundary')
