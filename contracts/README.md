# Herdr API contract

`herdr-api.schema.json` is what the pinned Herdr binary answers to `herdr api schema --json`, written by `scripts/bump-herdr.sh` in the same run that moves the pin recorded in `contracts/herdr-bundle.json`.
Herdr owns the schema and the protocol revision; `herdr-core/build.rs` derives `HERDR_PROTOCOL_REVISION` from this file, so no protocol number is maintained by hand anywhere in this repository.

<!-- herdr-provenance:start -->
hide distributes the [upstream Herdr release v0.9.3](https://github.com/herdrdev/herdr/releases/tag/v0.9.3).
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
The core is the writer of those strings.
`herdr-core/src/model.rs`'s `wire_enum_tests` pins the writing side: it fails to compile until a new variant is matched there, and then fails the test until that variant is added to this file.

Nothing in this repository checks an incoming wire value against this file from the reading side.
The web shell (`web/src/snapshot.ts`) types these fields with TypeScript, which is a compile-time check only and does not reject an unrecognized value at runtime; a field the core stops emitting, or emits a new value for, is not caught by a test today.

`workspace_view.tool` (`explorer`, `changes`) is not listed here.
Neither is the View area layout inside `workspace_view`: a split's `axis` (`row`, `column`), a display's `kind` (`file`, `diff`, `browser`), and a display's `state` (`open`, `opening`, `waiting`, `unavailable`).
Neither is `ui_state.theme` (`system`, `light`, `dark`); the web shell reads a value it does not know as Dark rather than failing.
List an enum here, with a Rust-side pin and a reading-side check for it, in the change that adds one.

# hide CLI contract

`hide-cli.json` is what `hide contract --json` exports: the commands other tools call (`hide agent`, `request`, `inbox`, `watch`), their options, and the JSON Schema of each answer ([docs/delivery.md](../docs/delivery.md#the-contract-a-calling-tool-checks)).
The build is the writer; `hided/src/cli_contract.rs`'s `the_committed_contract_is_the_exported_one` fails until this file matches what it exports.
Regenerate it after a change to those commands, and review the diff as the change callers see:

```sh
target/debug/hide contract --json | python3 -m json.tool > contracts/hide-cli.json
```
