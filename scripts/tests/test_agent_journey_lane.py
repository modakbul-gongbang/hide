"""The CI lane's verdict from a journey report; no agent runs."""

import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "agent-journey-lane.py"
spec = importlib.util.spec_from_file_location("agent_journey_lane", SCRIPT)
lane = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lane)


def report(table, agents, **extra):
    return {"fixture": False, "cleanup": {"confirmed": True}, "failures": [], "table": table, "agents": agents, **extra}


class Login(unittest.TestCase):
    def test_a_spent_day_passes_as_not_measured_for_the_nightly_report(self):
        quota_reason = "provider quota exhausted (RequestsPerDay)"
        passed, notes = lane.judge(report(
            {"codex": {"start": {"status": "pass", "reason": ""},
                       "reply": {"status": "not measured", "reason": quota_reason},
                       "resume": {"status": "not measured", "reason": quota_reason}}},
            [{"id": "codex", "quota": ["RequestsPerDay"]}]), "login")
        self.assertTrue(passed)
        # The title and first line scripts/ci-flaky-report.py reads to keep the lane from being green.
        self.assertEqual(notes, ["::warning title=Not measured::agent journey codex%0Aprovider quota exhausted "
                                 "(RequestsPerDay): reply, resume not reached"])

    def test_a_refusal_the_contract_does_not_name_fails(self):
        passed, notes = lane.judge(report({"claude-code": {"reply": {"status": "fail", "reason": "rate_limit"}}},
                                          [{"id": "claude-code"}]), "login")
        self.assertFalse(passed)
        self.assertIn("::error title=agent journey claude-code::reply: rate_limit", notes)


if __name__ == "__main__":
    unittest.main()
