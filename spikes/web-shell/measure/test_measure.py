#!/usr/bin/env python3
"""Regression boundaries: incomplete frame windows and orphan process ownership."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from importlib.util import spec_from_file_location, module_from_spec

HERE = Path(__file__).resolve().parent

class MeasurementTests(unittest.TestCase):
    def score(self, frames, done=True):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'frames.json'
            path.write_text(json.dumps({'frames': [{'dt': v} for v in frames], 'done': done, 'window': None}))
            return json.loads(subprocess.check_output([sys.executable, str(HERE/'frames.py'), str(path)]))

    def test_same_return_origin_for_both_shells(self):
        spec = spec_from_file_location('report', HERE/'write-report.py')
        report = module_from_spec(spec); spec.loader.exec_module(report)
        sample = {'hops': [{'t0_ms': 0, 'cli_return_ms': 110, 'write_ms': 112, 'draw_ms': 113}]}
        self.assertEqual(report.echo_samples(sample, 'web'), [2])
        self.assertEqual(report.echo_samples(sample, 'swift'), [3])
        sample['hops'][0]['write_ms'] = 109
        self.assertEqual(report.echo_samples(sample, 'web'), [-1])

    def test_short_series_cannot_pass_full_window(self):
        self.assertFalse(self.score([10] * 1400)['complete'])
        self.assertTrue(self.score([10] * 12000)['complete'])
        self.assertFalse(self.score([10] * 12000, False)['complete'])

    def test_long_stalls_remain_milliseconds(self):
        result = self.score([120001])
        self.assertEqual(result['covered_ms'], 120001)
        self.assertEqual(result['over_16_7ms'], 1)
        self.assertEqual(result['percent'], 100)

    def test_pid_publication_failure_leaves_no_child_group(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = 's0-child-' + uuid.uuid4().hex
            result = subprocess.run([sys.executable, str(HERE/'owned.py'), str(os.getpid()),
                str(Path(tmp)/'missing'/'child.pid'), sys.executable, '-c',
                'import time; time.sleep(60)', marker], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
            rows = subprocess.check_output(['ps', '-axo', 'pid=,command='], text=True).splitlines()
            children = [int(row.split(None,1)[0]) for row in rows if marker in row]
            try:
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(children, [], 'PID publication failure orphaned its child')
            finally:
                for pid in children:
                    try: os.killpg(pid, signal.SIGKILL)
                    except ProcessLookupError: pass

    def test_killed_owner_leaves_no_child_group(self):
        with tempfile.TemporaryDirectory() as tmp:
            pidfile = Path(tmp) / 'child.pid'
            # The script execs the wrapper, preserving the declared owner PID.
            owner = subprocess.Popen([sys.executable, '-c',
                'import subprocess,sys,time,os; subprocess.Popen([sys.executable,sys.argv[1],str(os.getpid()),sys.argv[2],sys.executable,"-c","import time; time.sleep(60)"]); time.sleep(60)',
                str(HERE/'owned.py'), str(pidfile)], start_new_session=True)
            child = None
            try:
                deadline = time.monotonic()+5
                while not pidfile.exists() and time.monotonic()<deadline: time.sleep(.05)
                child = int(pidfile.read_text())
                owner.kill(); owner.wait()
                while time.monotonic()<deadline:
                    try: os.kill(child,0)
                    except ProcessLookupError: break
                    time.sleep(.05)
                with self.assertRaises(ProcessLookupError): os.kill(child,0)
            finally:
                if owner.poll() is None: owner.kill(); owner.wait()
                if child:
                    try: os.killpg(child,signal.SIGKILL)
                    except ProcessLookupError: pass

if __name__ == '__main__': unittest.main()
