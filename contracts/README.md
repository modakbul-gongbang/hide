# Herdr API contract

`herdr-api.schema.json` is what the pinned Herdr binary answers to `herdr api schema --json`, written by `scripts/bump-herdr.sh` in the same run that moves the pin in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`.
Herdr owns the schema and the protocol revision; `herdr-core/build.rs` derives `HERDR_PROTOCOL_REVISION` from this file, so no protocol number is maintained by hand anywhere in this repository.

<!-- herdr-provenance:start -->
hide distributes the [upstream Herdr release v0.9.1](https://github.com/herdrdev/herdr/releases/tag/v0.9.1).
The bundled binary is not modified by hide.
The weekly `herdr-update.yml` workflow proposes upstream stable releases with `--repo herdrdev/herdr`; updates must pass contract and runtime checks.
<!-- herdr-provenance:end -->

Never edit this file directly.
Move the pin and the contract together:

```sh
./scripts/bump-herdr.sh <release-tag>
```

Re-running the script for the tag already pinned rewrites the contract only when it drifted from what the binary reports, and is otherwise a no-op.

Before launching the IDE against a local runtime, verify that the contract, the CLI, and the running server agree:

```sh
./scripts/check-herdr-contract.sh --herdr-bin <path to the pinned binary>
```

# Snapshot wire enums

`snapshot-wire-enums.json` lists, for every string enum the core serializes into the shell snapshot, the exact values it emits.
The core is the writer of those strings and the shell decodes them strictly, so a value one side does not know fails the whole snapshot decode and freezes the shell on its last good frame.
`herdr-core/src/model.rs` tests that every variant of each enum serializes to the listed value and nothing else; `SnapshotWireEnumTests` in the shell tests that each listed value decodes and that the Swift enum has no extra case.
Add the variant to this file in the same change as the Rust variant and the Swift case; either test fails until all three agree.

`workspace_view.mode` (`agents`, `together`, `views`) is not listed: the core writes `workspace_view` only for a shell with separate View areas, which today is the web shell, and omits it from the Swift snapshot, so no strict decoder reads it.
The View area tree inside it (PRD S7) takes the same exception for the same reason: a split's `axis` (`row`, `column`), a display's `kind` (`file`, `diff`) and a display's `state` (`open`, `opening`, `waiting`, `unavailable`).
List them here, with Swift cases, in the change that lets the Swift shell draw separate areas.
