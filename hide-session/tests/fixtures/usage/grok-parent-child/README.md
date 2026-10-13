This fixture preserves only native usage fields from a local parent session and its child, paired through the parent's subagent metadata.
The parent completion follows the child completion.
All identities, paths and timestamps are invented; no conversation, tool arguments, output or title is retained.
The expected result is the parent's PromptUsage: 105296 input including 90752 cache reads, 819 output including 658 reasoning, and 793780000 cost ticks.
The child is excluded because its summary carries `session_kind: subagent`.

Source contract: xai-org/grok-build commit `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`, `crates/codegen/xai-grok-shell/src/session/persistence.rs` (`Summary.session_kind` and `is_hidden`) and `crates/codegen/xai-grok-shell/src/extensions/notification.rs` (`PromptUsage`).
Grok includes child model usage in the parent's prompt usage; one tick is 10^-10 USD.
