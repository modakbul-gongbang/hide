# Ambient session counts

This document describes the current client-side count boundary, not a server implementation proposal.
The relevant code is [sidebar.rs](../herdr-core/src/sidebar.rs), [pet.rs](../herdr-core/src/pet.rs), and [PetView.swift](../macos/Sources/HerdrMacOS/PetView.swift).

## Accepted data

An agent record may carry an optional `ambient` object with three counts:

```json
{"subagents_active": 2, "background_running": 1, "background_failed": 0}
```

`parse_ambient` accepts absent or null data as no ambient signal.
Missing count keys become zero; present counts must be nonnegative integers that fit `u32`.
Unknown keys are discarded and cannot enter the projected agent through this boundary.
An unreadable ambient record rejects that agent record rather than retaining a partly parsed count or inventing a successful zero result.
Other valid agent records continue to project; the caller records the exclusion diagnostic.

`ambient_totals` sums accepted counts with saturation and returns no counts while disconnected.
`Runtime` puts those totals into the pet snapshot; `PetBadgeRow` renders only positive counts.
The three count fields do not carry task names, prompts, commands, output, or paths.

## Boundaries and regression tests

The projection tests `ambient_counts_parse_and_unknown_keys_never_survive` and `a_malformed_ambient_record_excludes_only_that_agent` cover privacy and malformed-record behavior.
The pet test `ambient_counts_sum_across_panes_and_go_quiet_while_disconnected` covers aggregation and disconnection.

The old standalone-pet documentation described per-target Settings opt-in, retry controls, and a proposed server-side reader.
Those are not contracts implemented by this client and must not be inferred from the optional count fields.
This repository's client does not become the owner of upstream transcript scanning, authorization, or server restart policy.
For the runtime actually shipped, follow the pin and official API contract in [AGENTS.md](../AGENTS.md#herdr-api-contract).
