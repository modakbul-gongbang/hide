"""Scene preparation must connect real project tools and owned native history."""

import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import threading
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.contracts import recipes, source_contract
from agent_live_check.conversation import messages
from agent_live_check.protection import ProtectionError
from agent_live_check.scenes import transcript
from agent_live_check.setup import configure
from agent_live_check.history import LABEL, seed
from agent_live_check.scenes import startup_blocker


class ScenePreparation(unittest.TestCase):
    def test_blocked_startup_is_observed_only_for_the_owned_matching_native_agent(self):
        recipe = {"id": "pi", "kind": "pi"}
        actual = {"pane_id": "owned", "agent": "pi", "name": "live-pi-startup", "agent_status": "blocked"}
        error = json.dumps({"error": {"code": "agent_not_ready"}})
        result = (1, "", error)  # Pinned CLI puts structured failures on stderr.
        self.assertTrue(startup_blocker("startup", recipe, "owned", result, actual))
        self.assertFalse(startup_blocker("rest", recipe, "owned", result, actual))
        self.assertFalse(startup_blocker("startup", recipe, "other", result, actual))
        self.assertFalse(startup_blocker("startup", recipe, "owned", result, {**actual, "agent": "codex"}))
        self.assertFalse(startup_blocker("startup", recipe, "owned", (0, error, ""), actual))
        self.assertFalse(startup_blocker("startup", recipe, "owned", (1, error, "not JSON"), actual))
        self.assertFalse(startup_blocker("startup", recipe, "owned", (1, "", '{"error":{"code":"agent_pane_busy"}}'), actual))

    def test_non_jsonl_providers_seed_real_completed_native_prompts_for_resume(self):
        checkout = Path(__file__).resolve().parents[2]
        data = recipes(checkout / "scripts/agent_live_check/recipes", source_contract(checkout)["targets"])
        for key in ("grok", "opencode", "cursor"):
            recipe, index, submitted = data[key], [0], []
            screens = ["❯ ", LABEL + "\nWorking", LABEL + "\n❯ "]
            statuses = ["idle", "working", "done"]
            session = {"kind": "id", "source": "herdr:" + key, "value": "owned-native-session"}
            def agent(pane):
                value = {"agent_status": statuses[index[0]], "agent_session": session}
                index[0] += 1
                return value
            runtime = SimpleNamespace(fixture_bin=None, screen=lambda pane: screens[index[0]], agent=agent,
                                      send=lambda pane, text: submitted.append(text),
                                      owner=SimpleNamespace(cancelled=threading.Event()))
            result = seed(runtime, "owned", recipe, Path("/unused"), {"session_root": None}, 2)
            self.assertEqual(len(submitted), 1)
            self.assertEqual(result["visible_tokens"], [LABEL])
            self.assertFalse(result["assistant_reply_verified"], "screen/status evidence cannot prove assistant mail")

    def test_every_mcp_recipe_connects_only_the_inert_project_server(self):
        checkout = Path(__file__).resolve().parents[2]
        data = recipes(checkout / "scripts/agent_live_check/recipes", source_contract(checkout)["targets"])
        with tempfile.TemporaryDirectory() as name:
            for key, recipe in data.items():
                cwd = Path(name) / key
                cwd.mkdir(mode=0o700)
                calls = []
                runtime = SimpleNamespace(checkout=checkout, short=Path(name), native_env={},
                                          owner=SimpleNamespace(run=lambda args, **kw: calls.append((args, kw))))
                arguments = configure(runtime, Path("/fixture/cli"), recipe, "mcp_approval", cwd)
                if key == "codex":
                    self.assertIn('mcp_servers.live_probe.default_tools_approval_mode="prompt"', arguments)
                    self.assertTrue(any("mcp_fixture.py" in item for item in arguments))
                elif key == "grok":
                    self.assertEqual(calls[0][1]["cwd"], cwd)
                    self.assertIn("--scope", calls[0][0])
                    self.assertIn("project", calls[0][0])
                    self.assertIn("--leader-socket", calls[0][0])
                else:
                    file = cwd / ("live-mcp-mcp_approval.json" if key == "claude-code" else recipe["mcp"]["path"])
                    config = json.loads(file.read_text())
                    servers = config["mcp" if key == "opencode" else "mcpServers"]
                    self.assertEqual(set(servers), {"live_probe"})
                    self.assertIn("mcp_fixture.py", json.dumps(servers))
                    self.assertEqual(file.stat().st_mode & 0o777, 0o600)

    def test_pi_and_omp_use_actual_native_identity_and_separate_tool_text(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            file = root / "2026_fixture-native-id.jsonl"
            file.write_text("\n".join(json.dumps(row) for row in [
                {"type": "session", "id": "fixture-native-id", "cwd": name},
                {"type": "message", "message": {"role": "user", "content": "bell"}},
                {"type": "message", "message": {"role": "toolResult", "content": [{"type": "text", "text": "marker"}]}},
                {"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "reply"}]}}
            ]))
            for kind in ("pi", "omp"):
                session = {"kind": "id", "source": "herdr:" + kind, "value": "fixture-native-id"}
                self.assertEqual(transcript(root, kind, session, root), file)
                self.assertEqual(messages(file, kind), [("user", "bell"), ("tool", "marker"), ("assistant", "reply")])
                session.update(kind="path", value=str(file))
                self.assertEqual(transcript(root, kind, session, root), file)
                session["value"] = str(root.parent / "operator.jsonl")
                with self.assertRaisesRegex(ProtectionError, "outside_owned_history"):
                    transcript(root, kind, session, root)


if __name__ == "__main__":
    unittest.main()
