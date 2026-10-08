"""A device measurement must refuse any target but the isolated account.

hided installs its node into folders it spells `~/` from the SFTP home, which
is the account's own home even where a private sshd gives its commands
another HOME. On 2026-10-09 a probe with the `~/` defaults uploaded its node
into a production account's `~/.hide/host-helper` while every command it ran
saw the private HOME. `scripts/web-shell-measure/device-guard.sh` is what
stops the measurement harness doing the same; this checks that it refuses
each such target before anything dials, and accepts the isolated one.
"""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
GUARD = ROOT / 'scripts' / 'web-shell-measure' / 'device-guard.sh'

CONFIG = """Host private
  HostName 127.0.0.1
  Port 22841
  User me

Host own
  HostName 127.0.0.1
  User me
"""

ISOLATED = {
    'MEASURE_DEVICE_ALIAS': 'private',
    'MEASURE_DEVICE_PORT': '22841',
    'MEASURE_DEVICE_HOME': '/tmp/hcn/home',
    'MEASURE_DEVICE_SOCKET': '/tmp/hcn/h.sock',
    'MEASURE_DEVICE_HELPER_ROOT': '/tmp/hcn/home/.hide/host-helper',
    'MEASURE_DEVICE_CLI_DIR': '/tmp/hcn/home/.local/bin',
}


class MeasureDeviceGuard(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.config = Path(folder.name) / 'config'
        self.config.write_text(CONFIG)

    def guard(self, **changes):
        variables = {**ISOLATED, 'MEASURE_DEVICE_SSH_CONFIG': str(self.config), **changes}
        env = {name: value for name, value in os.environ.items()
               if not name.startswith('MEASURE_DEVICE_')}
        env.update({name: value for name, value in variables.items() if value is not None})
        return subprocess.run(['bash', '-c', 'source "$1"; echo accepted', 'guard', str(GUARD)],
                              env=env, capture_output=True, text=True, timeout=30)

    def test_the_isolated_account_is_accepted(self):
        result = self.guard()
        self.assertEqual((result.returncode, result.stdout.strip()), (0, 'accepted'),
                         result.stderr)

    def test_every_other_target_is_refused_before_dialing(self):
        refusals = [
            ({'MEASURE_DEVICE_HELPER_ROOT': '~/.hide/host-helper'}, 'MEASURE_DEVICE_HELPER_ROOT'),
            ({'MEASURE_DEVICE_CLI_DIR': '~/.local/bin'}, 'MEASURE_DEVICE_CLI_DIR'),
            ({'MEASURE_DEVICE_HELPER_ROOT': '/opt/me/.hide/host-helper'},
             'MEASURE_DEVICE_HELPER_ROOT'),
            ({'MEASURE_DEVICE_HELPER_ROOT': '/tmp/hcn/home/../../opt/me/.hide/host-helper'},
             'MEASURE_DEVICE_HELPER_ROOT'),
            ({'MEASURE_DEVICE_HOME': '/opt/me'}, 'MEASURE_DEVICE_HOME'),
            ({'MEASURE_DEVICE_HOME': None}, 'MEASURE_DEVICE_HOME'),
            ({'MEASURE_DEVICE_SOCKET': '~/.config/herdr/herdr.sock'}, 'MEASURE_DEVICE_SOCKET'),
            ({'MEASURE_DEVICE_ALIAS': 'own', 'MEASURE_DEVICE_PORT': '22'}, 'is 22'),
            ({'MEASURE_DEVICE_ALIAS': 'own'}, 'resolves to port 22,'),
            ({'MEASURE_DEVICE_ALIAS': 'absent'}, 'resolves to port 22,'),
            ({'MEASURE_DEVICE_PORT': 'ssh'}, 'is not a port'),
        ]
        for changes, refusal in refusals:
            with self.subTest(changes=changes):
                result = self.guard(**changes)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertNotIn('accepted', result.stdout)
                self.assertIn(refusal, result.stderr)

    def test_the_harness_runs_the_guard_before_it_dials(self):
        measure = ROOT / 'scripts' / 'web-shell-measure'
        run = (measure / 'run.sh').read_text()
        guarded = run.index('source "$measure_dir/device-guard.sh"')
        for dial in ('"$measure_dir/device-herdr.sh"', '"$measure_dir/device-front.mjs"',
                     'spawn_owned hided'):
            self.assertLess(guarded, run.index(dial), f'run.sh reaches {dial} before the guard')
        herdr = (measure / 'device-herdr.sh').read_text()
        self.assertLess(herdr.index('device-guard.sh'), herdr.index('exec ssh'))


if __name__ == '__main__':
    unittest.main()
