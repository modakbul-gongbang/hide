"""Version claims require isolated prepared bytes and positive native reports."""

import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.contracts import recipes, source_contract
from agent_live_check.integration import observe, prepare, project_args
from agent_live_check.overlay import prepare as prepare_overlay
from agent_live_check.protection import ProtectionError, private_directory, write_private


class IntegrationEvidence(unittest.TestCase):
    def test_private_preparation_and_late_binding_never_replace_missing_load_proof(self):
        checkout = Path(__file__).resolve().parents[2]
        data = recipes(checkout / "scripts/agent_live_check/recipes", source_contract(checkout)["targets"])
        with tempfile.TemporaryDirectory() as name:
            base = Path(name)
            operator = base / "operator"
            operator.mkdir()
            for key, recipe in data.items():
                probe = base / key
                probe.mkdir()
                kind = recipe["kind"]
                # The installer double writes fixture bytes. This test does
                # not attest that a real CLI loaded an integration.
                relative = {"claude": ".claude", "codex": ".codex", "pi": ".pi/agent",
                            "omp": ".omp/agent", "opencode": ".config/opencode",
                            "cursor": ".cursor", "grok": ".grok"}[kind]
                def installer(args, *, env, cwd):
                    self.assertEqual(args, ["/fixture/pinned-herdr", "integration", "install", kind])
                    self.assertNotEqual(env["HOME"], str(operator))
                    root = Path(env["HOME"]) / relative
                    write_private(root / "fixture-hook", (
                        "# HERDR_INTEGRATION_ID=" + kind + "\n# HERDR_INTEGRATION_VERSION=17\n").encode())
                    if kind in ("claude", "cursor"):
                        write_private(root / ("settings.json" if kind == "claude" else "hooks.json"), b"{}")
                native = [None]
                runtime = SimpleNamespace(probe=probe, fixture_bin=None, env={}, herdr_bin=Path("/fixture/pinned-herdr"),
                                          owner=SimpleNamespace(run=installer), agent=lambda pane: native[0])
                overlay = prepare_overlay(recipe, probe, operator)
                plan = prepare(runtime, recipe, overlay)
                first = observe(runtime, "owned", recipe, plan)
                self.assertEqual(first["status"], "not_observed")
                self.assertIsNone(first["loaded_version"])
                native[0] = {"agent_session": {"source": "herdr:" + kind, "kind": "id", "value": "native"}}
                later = observe(runtime, "owned", recipe, plan)
                if kind == "cursor":
                    cwd = probe / "project"
                    private_directory(cwd)
                    project_args(plan, recipe, cwd)
                    self.assertEqual(json.loads((cwd / ".cursor/hooks.json").read_text()), {})
                    self.assertIsNone(later["loaded_version"], "project hooks cannot exclude global ambiguity")
                else:
                    self.assertEqual(later["loaded_version"], [17])
                plan["artifacts"][0]["file"].write_bytes(b"changed after preparation")
                with self.assertRaisesRegex(ProtectionError, "integration_changed"):
                    observe(runtime, "owned", recipe, plan)


if __name__ == "__main__":
    unittest.main()
