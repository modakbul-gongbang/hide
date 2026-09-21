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

HERE = Path(__file__).resolve().parent

class MeasurementTests(unittest.TestCase):
    def score(self, frames, done=True):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'frames.json'
            path.write_text(json.dumps({'frames': [{'dt': v} for v in frames], 'done': done, 'window': None}))
            return json.loads(subprocess.check_output([sys.executable, str(HERE/'frames.py'), str(path)]))

    def test_short_series_cannot_pass_full_window(self):
        self.assertFalse(self.score([10] * 1400)['complete'])
        self.assertTrue(self.score([10] * 12000)['complete'])
        self.assertFalse(self.score([10] * 12000, False)['complete'])

    def test_long_stalls_remain_milliseconds(self):
        result = self.score([120001])
        self.assertEqual(result['covered_ms'], 120001)
        self.assertEqual(result['over_16_7ms'], 1)
        self.assertEqual(result['percent'], 100)

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
