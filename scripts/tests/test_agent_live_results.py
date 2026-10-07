"""Independent safety/letter counterexamples; synthetic fixtures make no native claim."""

import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.contracts import SCENES, recipes, source_contract
from agent_live_check.conversation import bell_turn, messages
from agent_live_check.protection import ProtectionError
from agent_live_check.report import exit_code, save, verdict
from agent_live_check.runtime import Runtime
from agent_live_check.processes import OwnedProcesses
from agent_live_check.authentication import AuthenticationRequired
from agent_live_check.scenes import arrived, matches, observe


def safe_agent():
    return {"id": "codex", "version": "fixture", "model": "fixture", "integration": {},
            "bell_target": True, "delivery": {"outcome": "verified"},
            "scenes": [{"scene": scene, "arrival": "reached", "status": "done",
                        "effect": "new_turn", "reason": "fixture"} for scene in SCENES]}


class MeasurementResults(unittest.TestCase):
    def test_question_requires_a_menu_row_and_rejects_echo_or_prose(self):
        checkout = Path(__file__).resolve().parents[2]
        data = recipes(checkout / "scripts/agent_live_check/recipes", source_contract(checkout)["targets"])
        for recipe in data.values():
            question = recipe["scenes"]["question"]
            self.assertFalse(arrived(question, question["send"], "bell"))
            self.assertFalse(arrived(question, "One option is to continue.", "bell"))
            self.assertFalse(arrived(question, "Here are the choices; no interactive tool is open:\n1. One\n2. Two\n❯ ", "bell"))
            self.assertTrue(arrived(question, "1. One\n2. Two\nEnter to select · Esc to cancel", "bell"))

    def test_login_appearing_immediately_before_bell_receives_no_input(self):
        with tempfile.TemporaryDirectory() as name, OwnedProcesses() as owner:
            runtime = Runtime.__new__(Runtime)
            runtime.owner = owner
            screens = iter(["❯ ", "Sign in to continue"])
            runtime.screen = lambda pane: next(screens)
            runtime.agent = lambda pane: {"agent_status": "idle"}
            submitted = []
            runtime.command = lambda args: submitted.append(args)
            recipe = {"kind": "claude", "scenes": {"rest": {"send": "", "arrived": "❯", "draft": "", "no_match": "", "unsafe": ""}}}
            evidence = Path(name) / "login.json"
            with self.assertRaises(AuthenticationRequired):
                observe(runtime, "owned-pane", recipe, "rest", "bell", Path(name), 1, evidence,
                        Path(name), {"session_root": None, "settings": []})
            self.assertEqual(submitted, [])
            self.assertEqual(json.loads(evidence.read_text())["observation"]["arrival"], "skipped")

    def test_all_current_adapters_have_both_pickers_and_every_required_scene(self):
        checkout = Path(__file__).resolve().parents[2]
        contract = source_contract(checkout)
        data = recipes(checkout / "scripts/agent_live_check/recipes", contract["targets"])
        self.assertEqual(set(data), {"claude-code", "codex", "grok", "opencode", "pi", "omp", "cursor"})
        for recipe in data.values():
            self.assertEqual(set(recipe["scenes"]), {"rest", "working", "shell_approval", "file_approval",
                "question", "plan_approval", "model_picker", "resume_picker", "mcp_approval", "startup"})
            self.assertEqual(recipe["scenes"]["model_picker"]["send"], "/model")
            self.assertEqual(recipe["scenes"]["resume_picker"]["send"], "/resume")

    def test_unsafe_selection_approval_resume_settings_and_draft_dominate_unknown(self):
        for effect in ("selection", "approval", "resumed_session", "settings_write", "unsent_draft"):
            with self.subTest(effect=effect):
                agent = safe_agent()
                agent["scenes"][0].update(arrival="timeout", effect="not_tested")
                agent["scenes"][-1]["effect"] = effect
                self.assertEqual(verdict(agent), "unsafe")
                agent["verdict"] = verdict(agent)
                report = {"agents": [agent], "failures": [], "configuration": {}, "cleanup": {"confirmed": True}}
                self.assertEqual(exit_code(report), 1)
                agent["bell_target"] = False
                self.assertEqual(exit_code(report), 0)

    def test_missing_duplicate_skipped_or_timed_out_scene_never_passes(self):
        base = safe_agent()
        self.assertEqual(verdict(base), "safe_useful")
        for changed in (base["scenes"][:-1], base["scenes"] + [base["scenes"][0]]):
            agent = copy.deepcopy(base)
            agent["scenes"] = changed
            self.assertEqual(verdict(agent), "unknown")
        for arrival in ("timeout", "unreached", "skipped"):
            agent = copy.deepcopy(base)
            agent["scenes"][2]["arrival"] = arrival
            self.assertEqual(verdict(agent), "unknown")
        base["delivery"]["outcome"] = "unknown"
        self.assertEqual(verdict(base), "unknown")

    def test_marker_in_prompt_tool_echo_or_another_turn_is_not_delivery(self):
        bell, marker = "fixture bell", "fresh-fixture-marker"
        self.assertFalse(bell_turn([("user", bell), ("tool", marker)], bell, marker))
        self.assertFalse(bell_turn([("user", marker), ("user", bell), ("assistant", marker)], bell, marker))
        self.assertFalse(bell_turn([("tool", marker), ("user", bell), ("assistant", marker)], bell, marker))
        self.assertFalse(bell_turn([("user", bell), ("user", "another prompt"), ("assistant", marker)], bell, marker))
        self.assertTrue(bell_turn([("user", bell), ("tool", marker), ("assistant", marker)], bell, marker))

    def test_native_readers_keep_tool_output_separate_from_assistant_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            file = Path(temporary) / "session.jsonl"
            file.write_text('\n'.join(json.dumps(value) for value in [
                {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "bell"}]}},
                {"type": "response_item", "payload": {"type": "function_call_output", "output": "marker"}},
                {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "marker"}]}}
            ]))
            self.assertEqual(messages(file, "codex"), [("user", "bell"), ("tool", "marker"), ("assistant", "marker")])
            file.write_text('\n'.join(json.dumps(value) for value in [
                {"type": "user", "message": {"role": "user", "content": "bell"}},
                {"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "marker"}]}},
                {"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "marker"}]}}
            ]))
            self.assertEqual(messages(file, "claude"), [("user", "bell"), ("tool", '"marker"'), ("assistant", "marker")])

    def test_two_reports_preserve_unsafe_scene_and_refuse_overwrite(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            agent = safe_agent()
            agent["scenes"][-1]["effect"] = "unsent_draft"
            report = {"herdr": {}, "agents": [agent], "configuration": {}, "cleanup": {"confirmed": True},
                      "failures": [], "resources": {}}
            self.assertEqual(save(run, report), 1)
            stored = json.loads((run / "report.json").read_text())
            self.assertEqual(stored["agents"][0]["verdict"], "unsafe")
            self.assertIn("unsent_draft", (run / "report.md").read_text())
            with self.assertRaises(FileExistsError):
                save(run, report)

    def test_operator_socket_refused_before_any_executable_or_candidate_check(self):
        with tempfile.TemporaryDirectory() as temporary, OwnedProcesses() as owner:
            root = Path(temporary).resolve()
            run, operator = root / "run", root / "operator"
            run.mkdir(mode=0o700)
            operator.mkdir(mode=0o700)
            with self.assertRaisesRegex(ProtectionError, "operator_socket_refused"):
                Runtime(root, run, operator, owner, herdr_bin=root / "does-not-exist",
                        socket=operator / ".config/herdr/herdr.sock")
            self.assertEqual(owner.children, {})


if __name__ == "__main__":
    unittest.main()
