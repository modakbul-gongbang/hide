"""A required gate must not depend on a tool the runner may not have.

`check-right-panel-sections.sh` and `check-shortcut-contract.sh` were written
as `if rg ...; then fail; fi`. Ripgrep is not installed on the macOS runner, so
`rg` exited 127, the `if` read that as "no match", and both gates reported
success for weeks without ever searching anything. A comment cannot prevent
that from coming back; this can.
"""
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
WORKFLOWS = ROOT / '.github' / 'workflows'

# Tools that are not on the GitHub-hosted runners this repository uses. `git
# grep` and `grep` replace ripgrep; `jq`, `curl` and `python3` are present.
UNPORTABLE = ('rg', 'ag', 'fd', 'sd', 'bat')


def scripts_named_by_workflows():
    """Every repository script a workflow invokes, and what those scripts call."""
    named, pending = set(), []
    for workflow in WORKFLOWS.glob('*.yml'):
        for match in re.finditer(r'(?:scripts|macos/scripts)/[\w./-]+', workflow.read_text()):
            pending.append(match.group(0))
    while pending:
        relative = pending.pop()
        if relative in named:
            continue
        path = ROOT / relative
        if not path.is_file():
            continue
        named.add(relative)
        for match in re.finditer(r'(?:scripts|macos/scripts)/[\w./-]+', path.read_text()):
            pending.append(match.group(0))
    return named


class CIGatePortability(unittest.TestCase):
    def test_every_script_a_workflow_reaches_exists(self):
        for workflow in WORKFLOWS.glob('*.yml'):
            for match in re.finditer(r'(?:bash|zsh|sh|node|python3) ((?:scripts|macos/scripts)/[\w./-]+)',
                                     workflow.read_text()):
                relative = match.group(1)
                self.assertTrue((ROOT / relative).is_file(),
                                f'{workflow.name} runs {relative}, which does not exist')

    def test_no_workflow_script_invokes_an_unportable_tool(self):
        offenders = []
        for relative in sorted(scripts_named_by_workflows()):
            text = (ROOT / relative).read_text()
            for line_number, line in enumerate(text.splitlines(), start=1):
                code = line.split('#', 1)[0]
                for tool in UNPORTABLE:
                    if re.search(rf'(?:^|[|;&(]|\bif |\bthen |\$\()\s*{tool}\b', code):
                        offenders.append(f'{relative}:{line_number}: {line.strip()}')
        self.assertEqual(offenders, [], 'a workflow gate calls a tool the runner does not have:\n'
                                        + '\n'.join(offenders))

    def test_the_gates_a_workflow_names_are_tracked(self):
        tracked = set(subprocess.check_output(['git', 'ls-files'], cwd=ROOT, text=True).split())
        for relative in sorted(scripts_named_by_workflows()):
            self.assertIn(relative, tracked, f'{relative} is run by a workflow but is not tracked')


if __name__ == '__main__':
    unittest.main()
