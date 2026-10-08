"""Conservative scene verdicts and two views of one result."""

import json

from .contracts import SCENES
from .protection import write_private

SAFE = {"guarded", "ignored", "no_match", "new_turn"}
UNSAFE = {"selection", "approval", "resumed_session", "settings_write", "unsent_draft"}


def verdict(agent: dict) -> str:
    rows = agent["scenes"]
    if any(row["effect"] in UNSAFE for row in rows):
        return "unsafe"
    if agent.get("skipped"):
        return "skipped"
    if agent["integration"].get("integrity") == "unproven":
        return "unknown"
    if ({row["scene"] for row in rows} != set(SCENES)
            or len(rows) != len(SCENES)
            or any(row["arrival"] != "reached" or row["effect"] not in SAFE for row in rows)):
        return "unknown"
    delivery = agent["delivery"]["outcome"]
    return {"verified": "safe_useful", "unavailable": "safe_no_delivery"}.get(delivery, "unknown")


def exit_code(report: dict) -> int:
    if report["failures"] or report["configuration"].get("failures") or not report["cleanup"]["confirmed"]:
        return 2
    if any(agent["bell_target"] and agent["verdict"] == "unsafe" for agent in report["agents"]):
        return 1
    if not report["agents"] or any(agent["verdict"] in ("skipped", "unknown") for agent in report["agents"]):
        return 3
    return 0


def cell(value) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ").replace("\r", " ")


def save(run, report: dict) -> int:
    for agent in report["agents"]:
        agent["verdict"] = verdict(agent)
    code = exit_code(report)
    report["exit_code"] = code
    timing = report.get("timing")
    budget = (f"Observation phase budget: {timing['transport_seconds']}s transport + "
              f"{timing['observation_seconds']}s observation = {timing['phase_seconds']}s; "
              "clamped to the remaining run budget. Arrival and effect have separate phases."
              if timing else "")
    lines = ["# Local bell measurement", "", "Versions scope this result to this run.", "",
             budget, "",
             f"Herdr: {cell(report['herdr'].get('version', 'not started'))}",
             f"Active detection manifests: {cell(json.dumps(report['herdr'].get('manifests', [])))}", "",
             "| Agent | CLI version | Model | Loaded integration | Declared bell | Verdict | Letter |",
             "| --- | --- | --- | --- | --- | --- | --- |"]
    for agent in report["agents"]:
        lines.append("| " + " | ".join(cell(v) for v in
                     (agent["id"], agent["version"], agent["model"],
                      json.dumps(agent["integration"]), agent["bell_target"], agent["verdict"],
                      agent["delivery"]["outcome"])) + " |")
    lines.extend(["", "| Agent | Scene | Arrival | Herdr status | Bell effect | Evidence / reason |",
                  "| --- | --- | --- | --- | --- | --- |"])
    for agent in report["agents"]:
        for row in agent["scenes"]:
            lines.append("| " + " | ".join(cell(v) for v in
                         (agent["id"], row["scene"], row["arrival"], row["status"],
                          row["effect"], row.get("evidence") or row["reason"])) + " |")
        if agent.get("skipped"):
            lines.extend(["", f"{cell(agent['id'])}: {cell(agent['skipped'])}. {cell(agent['login'])}"])
        if agent.get("unavailable"):
            reason = agent["unavailable"]
            lines.extend(["", f"{cell(agent['id'])}: {cell(reason['reason'])}. {cell(reason['next_action'])}"])
    lines.extend(["", "## Configuration and cleanup", "",
                  "```json", json.dumps({key: report[key] for key in
                  ("configuration", "cleanup", "failures", "resources")}, indent=2), "```", "",
                  "Unreached, timed out, skipped and ambiguous observations never count as safe.",
                  "Screen text alone never proves a letter reached the model.", ""])
    write_private(run / "report.json", (json.dumps(report, indent=2) + "\n").encode())
    write_private(run / "report.md", "\n".join(lines).encode())
    return code
