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
                    self.assertEqual(env["XDG_CONFIG_HOME"], str(Path(env["HOME"]) / ".config"))
                    self.assertTrue(all(str(value).startswith(env["HOME"] + "/") for key, value in env.items()
                                        if key.startswith("XDG_")))
                    root = Path(env["HOME"]) / relative
                    write_private(root / "fixture-hook", (
                        "# HERDR_INTEGRATION_ID=" + kind + "\n# HERDR_INTEGRATION_VERSION=17\n").encode())
                    if kind == "opencode":
                        write_private(root / "fixture-tui", (
                            "// HERDR_INTEGRATION_ID=opencode-tui\n// HERDR_INTEGRATION_VERSION=18\n").encode())
                    if kind in ("claude", "cursor"):
                        write_private(root / ("settings.json" if kind == "claude" else "hooks.json"), b"{}")
                native = [None]
                runtime = SimpleNamespace(probe=probe, fixture_bin=None,
                                          env={"XDG_CONFIG_HOME": "/other/config", "XDG_DATA_HOME": "/other/data",
                                               "XDG_STATE_HOME": "/other/state", "XDG_CACHE_HOME": "/other/cache"},
                                          herdr_bin=Path("/fixture/pinned-herdr"),
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
                elif kind == "opencode":
                    self.assertIsNone(later["loaded_version"], "native source does not identify alternative frontend versions")
                    self.assertEqual({row["version"] for row in later["prepared_artifacts"]}, {17, 18})
                else:
                    self.assertEqual(later["loaded_version"], [17])
                plan["artifacts"][0]["file"].write_bytes(b"changed after preparation")
                with self.assertRaisesRegex(ProtectionError, "integration_changed"):
                    observe(runtime, "owned", recipe, plan)

    def test_copied_opencode_configuration_cannot_prove_exclusive_emitter(self):
        checkout = Path(__file__).resolve().parents[2]
        recipe = recipes(checkout / "scripts/agent_live_check/recipes", source_contract(checkout)["targets"])["opencode"]
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            operator, probe = root / "operator", root / "probe"
            private_directory(probe)
            config = operator / ".config/opencode/opencode.json"
            private_directory(config.parent)
            write_private(config, b'{"plugin":["/external/integration.js"]}')
            def installer(args, *, env, cwd):
                write_private(Path(env["HOME"]) / ".config/opencode/fixture-hook",
                              b"// HERDR_INTEGRATION_ID=opencode\n// HERDR_INTEGRATION_VERSION=17\n")
            runtime = SimpleNamespace(probe=probe, fixture_bin=None, env={}, herdr_bin=Path("/fixture/herdr"),
                                      owner=SimpleNamespace(run=installer),
                                      agent=lambda pane: {"agent_session": {"source": "herdr:opencode"}})
            overlay = prepare_overlay(recipe, probe, operator)
            plan = prepare(runtime, recipe, overlay)
            result = observe(runtime, "owned", recipe, plan)
            self.assertIsNone(result["loaded_version"], "copied configuration can load a competing integration")
            self.assertEqual(result["status"], "native_session_observed")
            self.assertEqual(config.read_bytes(), b'{"plugin":["/external/integration.js"]}')


if __name__ == "__main__":
    unittest.main()
