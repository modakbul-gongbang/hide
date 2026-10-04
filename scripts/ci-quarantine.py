#!/usr/bin/env python3
"""Validate exact exclusions and choose a registry-backed scenario filter."""
import argparse
from datetime import date
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def validate(root=ROOT, today=None):
    value = json.loads((root / 'contracts/ci-quarantine.json').read_text())
    if value.get('version') != 1 or not value.get('entries'):
        raise ValueError('unknown or empty quarantine registry')
    entries = value['entries']
    seen = set()
    for entry in entries:
        for field in ('id', 'suite', 'file', 'title', 'oses', 'owner', 'issue', 'evidence', 'registered', 'expires', 'signature', 'fix_paths', 'alternative', 'return_criteria'):
            if not entry.get(field):
                raise ValueError(f'missing {field}: {entry.get("id")}')
        if entry['id'] in seen:
            raise ValueError('duplicate quarantine id')
        seen.add(entry['id'])
        if date.fromisoformat(entry['expires']) <= (today or date.today()):
            raise ValueError(f'expired quarantine: {entry["id"]}')
        if not 0 < (date.fromisoformat(entry['expires']) - date.fromisoformat(entry['registered'])).days <= 7:
            raise ValueError('quarantine duration requires a separately reviewed registration')
        if entry['suite'] not in ('web', 'desktop') or not set(entry['oses']) <= {'Linux','macOS','Windows'}:
            raise ValueError('unknown quarantine surface')
        patterns = entry['signature'].get('any_of')
        if entry['signature'].get('category') != 'assertion' or not patterns or any(not isinstance(p, list) or len(p) < 2 or any(not isinstance(token, str) or not token for token in p) for p in patterns):
            raise ValueError('quarantine must name an assertion signature')
        source = (root / entry['file']).read_text()
        declaration = next((line for line in source.splitlines() if 'test(' + json.dumps(entry['title'], ensure_ascii=False) in line), '')
        if '@flaky' not in declaration or entry['issue'] not in declaration:
            raise ValueError(f'registry/source mismatch: {entry["id"]}')
        for alternative in entry['alternative']:
            if not (root / alternative).is_file():
                raise ValueError(f'missing alternative coverage: {alternative}')
    declarations = []
    for package in ('web','desktop'):
        for file in (root / package / 'e2e').glob('*.spec.ts'):
            # A tag can be placed on its own line. Every literal tag, excluding
            # comment-only documentation, must belong to a registered test.
            declarations.extend((file, line) for line in file.read_text().splitlines() if '@flaky' in line and not line.lstrip().startswith(('//','*')))
    if len(declarations) != len(entries):
        raise ValueError('unregistered quarantine declaration')
    return entries


def selection(entries, suite, os, mode, plan=None):
    if not os:
        raise ValueError('exact scenario selection requires OS')
    selected = None
    if mode != 'exclude':
        if plan is None:
            raise ValueError('scenario execution needs the recorded plan')
        selected = plan['quarantine_required' if mode == 'required' else 'quarantine_observe']
        if not isinstance(selected, list) or set(selected) - {e['id'] for e in entries}:
            raise ValueError('unknown scenario selection')
    return [e for e in entries if e['suite'] == suite and os in e['oses'] and (selected is None or e['id'] in selected)]


def test_list(entries):
    # Playwright's list format compares file and title path tokens, not a
    # substring of its combined grep title. Config rootDir is PACKAGE/e2e.
    lines = []
    for entry in entries:
        file = Path(entry['file']).relative_to(f"{entry['suite']}/e2e").as_posix()
        if any(c in entry['title'] for c in ('\n', '\r', '>', '›')):
            raise ValueError('unsupported test-list title delimiter')
        lines.append(f"{file} > {entry['title']}")
    return '\n'.join(lines) + ('\n' if lines else '')


def results(entries, ledger, required, os):
    records = ledger.get('records')
    if not isinstance(records, list):
        raise ValueError('missing scenario result records')
    for entry in entries:
        observed = [r for r in records if r.get('suite') == entry['file'] and r.get('test') == entry['title'] and r.get('os') == os and r.get('retry') == 0]
        if len(observed) != 1 or observed[0].get('status') not in ('passed', 'failed', 'timedOut'):
            raise ValueError(f'missing or ambiguous scenario result: {entry["id"]}')
        if required and observed[0]['status'] != 'passed':
            raise ValueError(f'required scenario did not pass: {entry["id"]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('check','filter','results'))
    parser.add_argument('--suite', choices=('web','desktop'))
    parser.add_argument('--mode', choices=('exclude','advisory','required'), default='advisory')
    parser.add_argument('--os', choices=('Linux','macOS','Windows'))
    parser.add_argument('--plan')
    parser.add_argument('--output')
    parser.add_argument('--ledger')
    args = parser.parse_args()
    entries = validate()
    if args.command != 'check':
        if not args.suite:
            raise ValueError('filter requires suite')
        selected = selection(entries, args.suite, args.os, args.mode, json.loads(Path(args.plan).read_text()) if args.plan else None)
        if args.command == 'results':
            if not args.ledger:
                raise ValueError('scenario result validation requires ledger')
            results(selected, json.loads(Path(args.ledger).read_text()), args.mode == 'required', args.os)
        elif args.output:
            Path(args.output).write_text(test_list(selected))
            print(len(selected))
        else:
            print(test_list(selected), end='')


if __name__ == '__main__':
    main()
