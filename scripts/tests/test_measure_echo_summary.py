"""A key echo summary reports the clock offset its cross-check rests on.

Until 2026-10-10 `key-echo.mjs` took t0 on the driver's clock and t1 on the
page's, and the two differed by a constant that changed per run (+75, -21
and -22 ms in three runs), so a 5 ms comparison measured the offset. The
sample is now taken on the page's clock alone, and the driver's offset from
the page is kept per hop; this checks that `summarize.py echo` reports that
offset and the driver-clock samples per trial, and adds nothing for an echo
file that carries neither.
"""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
SUMMARIZE = ROOT / 'scripts' / 'web-shell-measure' / 'summarize.py'


def summarize(doc):
    with tempfile.TemporaryDirectory() as folder:
        path = Path(folder) / 'key-echo-idle.json'
        path.write_text(json.dumps(doc))
        out = subprocess.run(['python3', str(SUMMARIZE), 'echo', str(path)],
                             check=True, capture_output=True, text=True).stdout
    return json.loads(out)['trials'][0]


class EchoSummaryClocks(unittest.TestCase):
    def test_a_key_echo_reports_its_offset_and_driver_clock_samples(self):
        hops = [{'clock_offset_ms': offset, 'clock_round_trip_ms': trip}
                for offset, trip in ((-21.5, 0.4), (-21.0, 0.9), (-22.0, 0.3), (-21.2, 0.5))]
        trial = summarize({'samples': [18.0, 19.0, 20.0, 21.0],
                           'node_t0_samples': [-2.0, -1.0, 0.0, 1.0], 'hops': hops})
        self.assertEqual((trial['p50_ms'], trial['p95_ms'], trial['negative']), (19.0, 21.0, 0))
        self.assertEqual(trial['clock_offset_ms'], {'p50': -21.5, 'min': -22.0, 'max': -21.0})
        self.assertEqual(trial['clock_round_trip_max_ms'], 0.9)
        self.assertEqual((trial['node_t0_p50_ms'], trial['node_t0_p95_ms']), (-1.0, 1.0))

    def test_an_echo_without_a_cross_check_reports_none(self):
        trial = summarize({'samples': [5.0, 6.0], 'hops': [{'sample': 0}, {'sample': 1}]})
        self.assertNotIn('clock_offset_ms', trial)
        self.assertNotIn('node_t0_p50_ms', trial)


if __name__ == '__main__':
    unittest.main()
