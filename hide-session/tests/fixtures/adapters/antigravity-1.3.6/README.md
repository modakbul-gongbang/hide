# Antigravity CLI 1.3.6 conversation contract

Synthetic conversation with no account history or credentials.
The shapes were read from conversations agy 1.3.6 wrote on 2026-10-16 in a scratch workspace.
A conversation is the folder `~/.gemini/antigravity-cli/brain/<id>/`, named by the conversation's lowercase UUID; its `.system_generated/logs/transcript_full.jsonl` holds one record per step.
A step is `{step_index, source, type, status, created_at}` with `content` (text), `tool_calls` (`[{name, args}]`) or `error`; a model step adds `input_tokens` (without cache reads), `cache_read_tokens` and `output_tokens`.
`USER_INPUT` wraps the typed prompt in `<USER_REQUEST>` and adds an `<ADDITIONAL_METADATA>` block; `PLANNER_RESPONSE` is the model's answer or its tool calls; `GENERIC` is a tool's result and names no tool.
A hook's `injectSteps` message is written with source `SYSTEM_SDK` as `SYSTEM_MESSAGE` (from SessionStart, wrapped in agy's notice) or `EPHEMERAL_MESSAGE`; a subagent's first step is a `SYSTEM` message naming its parent as `sender=<id>`.
The transcript names neither the workspace nor the model: `~/.gemini/antigravity-cli/conversation_summaries.db` (`summaries.sql` is its schema) holds each conversation's `workspace_uris`, `title` and `parent_conversation_id`, with `app_data_dir` `antigravity-cli` for the CLI's rows, and agy keeps the conversation it resumes in `conversations/<id>.db`.
