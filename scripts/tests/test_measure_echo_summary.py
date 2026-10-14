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


def run_summary(kind, docs, *args):
    with tempfile.TemporaryDirectory() as folder:
        paths = []
        for index, doc in enumerate(docs):
            path = Path(folder) / f'trial-{index}.json'
            path.write_text(json.dumps(doc))
            paths.append(str(path))
        result = subprocess.run(['python3', str(SUMMARIZE), kind, *args, *paths],
                                capture_output=True, text=True)
    return result.returncode, json.loads(result.stdout)


def record(samples):
    return {'samples': samples, 'expected_samples': len(samples), 'complete': True,
            'boundary': 'page_keydown_to_xterm_write'}


def driven_frames():
    """Two panes with independently observed progress in all three window thirds."""
    panes = ['pane-a', 'pane-b']
    return {
        'frames': [{'dt': 16}] * 7500, 'done': True,
        'window': {'start': 100, 'end': 120100, 'duration_ms': 120000, 'requested_ms': 120000},
        'driver_launch_complete': True, 'driver_cleanup_complete': True,
        'driven_panes': panes,
        'workload_evidence': {
            'method': 'pane_sequence_progress_v1', 'sample_interval_ms': 500,
            'panes': [
                {'pane_id': pane, 'marker': f'f0123456789abcdef{index}:', 'phases': [
                    {'first': {'at_ms': phase * 40000 + 1000, 'sequence': phase * 100 + 1},
                     'last': {'at_ms': phase * 40000 + 39000, 'sequence': phase * 100 + 90},
                     'observations': 2} for phase in range(3)]}
                for index, pane in enumerate(panes)],
        },
    }


def summarize(doc):
    complete = {**record(doc['samples']), **doc}
    status, summary = run_summary('echo', [complete])
    if status != 0:
        raise AssertionError(summary)
    return summary['trials'][0]


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


class MeasurementValidity(unittest.TestCase):
    def test_impossible_samples_never_pass_or_contribute_percentiles(self):
        for samples in ([-5, -4, -3, -2, -1], [1, float('nan')],
                        [float('inf')], [True], ['1'], [None], []):
            with self.subTest(samples=samples):
                status, result = run_summary('echo', [record(samples)])
                self.assertNotEqual(status, 0)
                self.assertFalse(result['pass'])
                self.assertFalse(result['valid'])
                self.assertTrue(result['trials'][0]['errors'])
                self.assertIsNone(result['median_trial_p95_ms'])

    def test_unproven_clock_or_incomplete_trial_never_passes(self):
        cases = [dict(record([1, 2]), boundary='driver_cli_return_to_page_write'),
                 dict(record([1, 2]), expected_samples=3),
                 dict(record([1, 2]), complete=False),
                 dict(record([1, 2]), error='setup failed')]
        for field in ('boundary', 'expected_samples', 'complete', 'samples'):
            missing = record([1, 2])
            del missing[field]
            cases.append(missing)
        for doc in cases:
            with self.subTest(doc=doc):
                status, result = run_summary('echo', [doc])
                self.assertNotEqual(status, 0)
                self.assertFalse(result['valid'])
                self.assertFalse(result['pass'])

    def test_one_invalid_trial_cannot_be_hidden_by_good_trials(self):
        docs = [record([1, 2]), record([1, 2]), record([-1, 2])]
        status, result = run_summary('echo', docs)
        self.assertNotEqual(status, 0)
        self.assertFalse(result['pass'])
        self.assertIsNone(result['median_trial_p95_ms'])

    def test_valid_observations_have_no_implicit_historical_threshold(self):
        status, result = run_summary('echo', [record([1, 2, 3])])
        self.assertEqual(status, 0)
        self.assertTrue(result['valid'])
        self.assertEqual(result['median_trial_p95_ms'], 3)
        self.assertIsNone(result['threshold_ms'])
        self.assertIsNone(result['pass'])

    def test_an_explicit_threshold_judges_only_complete_valid_trials(self):
        for maximum, expected in ((10, True), (10.1, False)):
            with self.subTest(maximum=maximum):
                status, result = run_summary('echo', [record([1, 2, maximum])], '--max-p95-ms', '10')
                self.assertEqual(status, 0 if expected else 1)
                self.assertEqual(result['pass'], expected)
                self.assertTrue(result['valid'])

    def test_no_trials_is_an_invalid_measurement(self):
        status, result = run_summary('echo', [])
        self.assertNotEqual(status, 0)
        self.assertFalse(result['valid'])
        self.assertFalse(result['pass'])

    def test_invalid_frames_are_not_silently_dropped(self):
        for bad in ({}, {'dt': -1}, {'dt': float('nan')}, {'dt': float('inf')}, {'dt': True}):
            with self.subTest(bad=bad):
                doc = driven_frames()
                doc['frames'].append(bad)
                status, result = run_summary('frames', [doc])
                self.assertNotEqual(status, 0)
                self.assertFalse(result['pass'])
                self.assertFalse(result['complete'])

    def test_an_unfinished_frame_window_does_not_pass(self):
        for doc, expected in ((driven_frames(), True),
                              (dict(driven_frames(), done=False), False),
                              (dict(driven_frames(), frames=[{'dt': 16}]), False)):
            status, result = run_summary('frames', [doc])
            self.assertEqual(result['pass'], expected)
            self.assertEqual(status, 0 if expected else 1)


class DrivenWorkloadValidity(unittest.TestCase):
    def test_failed_collector_preserves_observations_without_a_valid_verdict(self):
        doc = driven_frames()
        doc.update(error='frame writer retirement unconfirmed', driver_cleanup_complete=False,
                   pane_diagnostics={'after_stop': [{'pane_id': 'pane-a', 'retired': False,
                                                     'tail': 'writer output retained'}]})
        status, result = run_summary('frames', [doc])
        self.assertNotEqual(status, 0)
        self.assertFalse(result['valid'])
        self.assertFalse(result['pass'])
        self.assertEqual(result['count'], len(doc['frames']))
        self.assertEqual(result['workload_evidence'], doc['workload_evidence'])
        self.assertEqual(result['pane_diagnostics'], doc['pane_diagnostics'])
        self.assertIn(doc['error'], result['errors'])

    def test_failed_unobserved_driver_cannot_pass_on_idle_frames(self):
        doc = {'frames': [{'dt': 16}] * 7500, 'done': True,
               'window': {'start': 0, 'end': 120000, 'duration_ms': 120000},
               'driver_exit': 1, 'ws_frames_during_run': 0, 'driven_panes': []}
        status, result = run_summary('frames', [doc])
        self.assertNotEqual(status, 0)
        self.assertFalse(result['pass'])
        self.assertFalse(result['valid'])

    def test_actual_progress_in_every_pane_and_phase_can_pass_without_ws_counts(self):
        status, result = run_summary('frames', [driven_frames()])
        self.assertEqual(status, 0)
        self.assertTrue(result['pass'])
        self.assertTrue(result['workload_valid'])

    def test_launch_success_and_ws_traffic_cannot_replace_pane_evidence(self):
        for field in ('workload_evidence', 'driver_launch_complete', 'driver_cleanup_complete'):
            doc = driven_frames()
            del doc[field]
            doc['ws_frames_during_run'] = 100000
            with self.subTest(field=field):
                status, result = run_summary('frames', [doc])
                self.assertNotEqual(status, 0)
                self.assertFalse(result['pass'])

    def test_one_missing_or_stalled_pane_invalidates_the_whole_window(self):
        for kind in ('missing', 'duplicate', 'unobserved', 'stale', 'outside', 'failed', 'cleanup'):
            doc = driven_frames()
            panes = doc['workload_evidence']['panes']
            phase = panes[1]['phases'][1]
            if kind == 'missing':
                panes.pop()
            elif kind == 'duplicate':
                panes[1] = panes[0]
            elif kind == 'unobserved':
                phase.update(first=None, last=None, observations=0)
            elif kind == 'stale':
                phase['last']['sequence'] = phase['first']['sequence']
            elif kind == 'outside':
                phase['first']['at_ms'] = -1
            elif kind == 'failed':
                doc['driver_launch_complete'] = False
            else:
                doc['driver_cleanup_complete'] = False
            with self.subTest(kind=kind):
                status, result = run_summary('frames', [doc])
                self.assertNotEqual(status, 0)
                self.assertFalse(result['workload_valid'])
                self.assertFalse(result['pass'])

    def test_workload_evidence_must_belong_to_the_recorded_window(self):
        for kind in ('wrong_phase', 'earlier_sequence', 'wrong_window', 'nonfinite', 'marker'):
            doc = driven_frames()
            pane = doc['workload_evidence']['panes'][0]
            if kind == 'wrong_phase':
                pane['phases'][2]['first']['at_ms'] = 1000
            elif kind == 'earlier_sequence':
                pane['phases'][2]['first']['sequence'] = 1
            elif kind == 'wrong_window':
                doc['window']['duration_ms'] = 600000
            elif kind == 'nonfinite':
                pane['phases'][0]['last']['at_ms'] = float('nan')
            else:
                pane['marker'] = doc['workload_evidence']['panes'][1]['marker']
            with self.subTest(kind=kind):
                status, result = run_summary('frames', [doc])
                self.assertNotEqual(status, 0)
                self.assertFalse(result['pass'])


if __name__ == '__main__':
    unittest.main()
