"""A job that calls a workflow grants every permission that workflow asks for.

A called workflow gets no permission its caller does not grant, and GitHub
checks that before any job starts: a missing grant fails the whole calling
workflow as `startup_failure`, hand runs included, with nothing in a log.
On 2026-10-15 `pr.yml`'s `plan` job began to ask for `actions: read`, the
nightly's `verify` job (which calls `pr.yml`) did not grant it, and no
nightly started for a day. actionlint 1.7.7 does not catch this; this does.
"""
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent.parent
WORKFLOWS = ROOT / '.github' / 'workflows'

LEVELS = {'none': 0, 'read': 1, 'write': 2}
# Every scope `read-all` and `write-all` cover.
SCOPES = (
    'actions', 'attestations', 'checks', 'contents', 'deployments', 'discussions', 'id-token',
    'issues', 'models', 'packages', 'pages', 'pull-requests', 'repository-projects',
    'security-events', 'statuses',
)


def grant(value, where):
    """A `permissions:` value as {scope: level}; None when the key is absent."""
    if value is None:
        return None
    if value in ('read-all', 'write-all'):
        level = value.split('-')[0]
        return {scope: level for scope in SCOPES}
    if isinstance(value, dict):
        for scope, level in value.items():
            if level not in LEVELS:
                raise ValueError(f'{where}: permission {scope}: {level!r} is not read, write or none')
        return dict(value)
    raise ValueError(f'{where}: permissions {value!r} is not a mapping, read-all or write-all')


def load(path):
    return yaml.safe_load(path.read_text())


def asked(workflow, name):
    """The highest level of each scope the workflow or any of its jobs asks for."""
    wanted = {}
    blocks = [grant(workflow.get('permissions'), name)]
    for job_id, job in (workflow.get('jobs') or {}).items():
        blocks.append(grant(job.get('permissions'), f'{name} job {job_id}'))
    for block in blocks:
        for scope, level in (block or {}).items():
            if LEVELS[level] > LEVELS[wanted.get(scope, 'none')]:
                wanted[scope] = level
    return wanted


def gaps(root=ROOT):
    """Each calling job under `root` that grants less than its called workflow asks for."""
    found = []
    for path in sorted((root / '.github' / 'workflows').glob('*.yml')):
        workflow = load(path)
        default = grant(workflow.get('permissions'), path.name)
        for job_id, job in (workflow.get('jobs') or {}).items():
            uses = job.get('uses', '')
            if not uses.startswith('./.github/workflows/'):
                continue
            called = root / uses.removeprefix('./')
            where = f'{path.name} job {job_id}'
            given = grant(job.get('permissions'), where)
            if given is None:
                given = default
            if given is None:
                found.append(f'{where} calls {called.name} and grants no explicit permissions')
                continue
            for scope, level in asked(load(called), called.name).items():
                have = given.get(scope, 'none')
                if LEVELS[have] < LEVELS[level]:
                    found.append(f'{where} calls {called.name}, which asks {scope}: {level}; the job grants {scope}: {have}')
    return found


class CalledWorkflowPermissions(unittest.TestCase):
    def test_every_calling_job_grants_what_its_called_workflow_asks_for(self):
        self.assertEqual(gaps(), [])

    def test_the_check_finds_the_gap_that_stopped_the_nightly(self):
        with tempfile.TemporaryDirectory() as tmp:
            workflows = Path(tmp) / '.github' / 'workflows'
            workflows.mkdir(parents=True)
            (workflows / 'nightly.yml').write_text(
                'jobs:\n'
                '  verify:\n'
                '    uses: ./.github/workflows/pr.yml\n'
                '    permissions:\n'
                '      contents: read\n'
                '      issues: write\n'
                '  perf:\n'
                '    uses: ./.github/workflows/pr.yml\n'
            )
            (workflows / 'pr.yml').write_text(
                'permissions:\n'
                '  contents: read\n'
                'jobs:\n'
                '  plan:\n'
                '    permissions:\n'
                '      contents: read\n'
                '      actions: read\n'
            )
            self.assertEqual(gaps(Path(tmp)), [
                'nightly.yml job verify calls pr.yml, which asks actions: read; the job grants actions: none',
                'nightly.yml job perf calls pr.yml and grants no explicit permissions',
            ])

    def test_an_unknown_permission_form_is_refused(self):
        with self.assertRaises(ValueError):
            grant('read', 'x.yml')
        with self.assertRaises(ValueError):
            grant({'contents': 'admin'}, 'x.yml')


if __name__ == '__main__':
    unittest.main()
