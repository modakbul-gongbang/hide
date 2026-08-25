---
id: INV-herdr-unseen-token
kind: invariant
status: active
evidence:
  - "session 5e65ea58 (2026-08-17): acknowledged `?` agents were promoted to yellow waiting because the mapper matched on the `status_question` prefix, so the dashboard disagreed with the herdr sidebar"
trigger:
  paths:
    - "crates/herdr-core/src/herdr.rs"
    - "crates/herdr-core/src/model.rs"
    - "crates/herdr-core/src/aggregate.rs"
check:
  type: command
  run: cargo test -p herdr-core
---

herdr distinguishes unseen attention state (`status_question_new`, `status_approval_new`, `status_error_new`, or the legacy boolean form) from state the user has already acknowledged (`status_question` with a string value, whose `agent_status` herdr drops back to `idle`).
Only unseen tokens may be promoted to attention or error; acknowledged ones must fall through to idle and disappear from the dashboard list, matching what the herdr sidebar shows.
Any change to the herdr token mapping or the status aggregation must keep the `crates/herdr-core/src/herdr.rs` status tests green.
