# Grok 1.0.46 session contract

Synthetic conversation with no account history or credentials.
Native semantics come from xai-org/grok-build at the 2026-09-29 sync (the 1.0.46 release line): `xai-grok-shell/src/session/storage/` (the `{timestamp, method, params}` envelope, append-only `updates.jsonl`, chunk merging), `xai-grok-shell/src/session/persistence.rs` (`Summary`), `xai-grok-shell/src/session/plan_mode.rs` and `xai-grok-config/src/paths.rs` (the URL-encoded cwd group), and the bundled user guide `17-sessions.md` and `19-plan-mode.md`.
A session is the folder `<GROK_HOME>/sessions/<url-encoded cwd>/<id>/`; `summary.json` owns the id, the cwd and the current title (`generated_title`, `title_is_manual`), and is replaced whole.
`updates.jsonl` carries one record per prompt block (`promptIndex`) and one per contiguous answer text run (`promptId`); Hide joins the parts of one prompt and of one turn's answer.
`agentTimestampMs` is the record's own time, which a fork keeps while it rewrites the envelope's whole seconds.
A `hideFromScrollback` prompt is Grok's own wake, not a person; `turn_completed` ends a turn; a terminal `tool_call_update` carries the tool's printed output.
Plan approval is `plan_mode.json` (`Active`, `awaiting_plan_approval`) with its text in `plan.md`; questions are `ask_user_question` tool calls (`x.ai/tool` kind `ask_user`) until their terminal update.
