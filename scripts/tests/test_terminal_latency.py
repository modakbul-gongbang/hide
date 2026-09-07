import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("latency", Path(__file__).parents[1] / "summarize-terminal-latency.py")
latency = importlib.util.module_from_spec(spec)
spec.loader.exec_module(latency)


class TerminalLatencySummaryTests(unittest.TestCase):
    def test_four_intervals_keep_counts_quantiles_and_exclusions(self):
        records = []
        for name in latency.INTERVALS:
            for duration in range(1, 21):
                records.append(json.dumps({"eventMessage": f"hide_latency interval={name} pane=fixture milliseconds={duration}.0 hz=120.0 outcome=completed"}))
            records.append(json.dumps({"eventMessage": f"hide_latency interval={name} pane=fixture milliseconds=999.0 hz=120.0 outcome=released"}))
        summary = latency.summarize(records)
        self.assertEqual(summary["refresh_rates_hz"], [120.0])
        for interval in summary["intervals"].values():
            self.assertEqual(interval, {"count": 20, "p50_ms": 10.0, "p95_ms": 19.0, "max_ms": 20.0, "excluded": {"released": 1}})

    def test_no_measurement_is_unknown_instead_of_zero_latency(self):
        for interval in latency.summarize([])["intervals"].values():
            self.assertEqual(interval["count"], 0)
            self.assertIsNone(interval["p95_ms"])

    def test_window_uses_interval_start_and_requires_a_timestamp(self):
        message = "hide_latency interval=wheel_to_draw pane=fixture milliseconds=2000.0 hz=120.0 outcome=completed"
        record = json.dumps({"eventMessage": message, "timestamp": "1970-01-01T00:00:11+00:00"})
        interval = latency.summarize([record], started_after=10)["intervals"]["wheel_to_draw"]
        self.assertEqual(interval["count"], 0)
        self.assertEqual(interval["excluded"], {"began_before_window": 1})
        with self.assertRaisesRegex(ValueError, "timestamped JSON"):
            latency.summarize([message], started_after=10)

    def test_native_log_timestamp_without_offset_colon(self):
        record = json.dumps({
            "eventMessage": "hide_latency interval=key_to_send pane=fixture milliseconds=3.0 hz=120.0 outcome=completed",
            "timestamp": "1970-01-01 09:00:11.000000+0900",
        })
        interval = latency.summarize([record], started_after=10)["intervals"]["key_to_send"]
        self.assertEqual(interval["count"], 1)
        self.assertEqual(interval["p95_ms"], 3.0)

    def test_exclusions_and_unknown_refresh_rate_cannot_become_fast_samples(self):
        records = [
            "hide_latency interval=wheel_to_draw pane=visible milliseconds=8.0 hz=0.0 outcome=completed",
            "hide_latency interval=wheel_to_draw pane=hidden milliseconds=0.0 hz=120.0 outcome=hidden",
            "hide_latency interval=wheel_to_draw pane=released milliseconds=0.0 hz=120.0 outcome=released",
        ]
        summary = latency.summarize(records)
        self.assertEqual(summary["refresh_rates_hz"], [])
        self.assertEqual(summary["panes"], ["visible"])
        self.assertEqual(summary["intervals"]["wheel_to_draw"], {
            "count": 1, "p50_ms": 8.0, "p95_ms": 8.0, "max_ms": 8.0,
            "excluded": {"hidden": 1, "released": 1},
        })

    def test_long_stall_is_retained_instead_of_trimmed_as_an_outlier(self):
        records = [f"hide_latency interval=wheel_to_draw pane=fixture milliseconds={ms}.0 hz=120.0 outcome=completed"
                   for ms in [1] * 19 + [8000]]
        interval = latency.summarize(records)["intervals"]["wheel_to_draw"]
        self.assertEqual(interval["count"], 20)
        self.assertEqual(interval["p95_ms"], 1.0)
        self.assertEqual(interval["max_ms"], 8000.0)
        self.assertEqual(interval["excluded"], {})


if __name__ == "__main__":
    unittest.main()
