# Herdr API contract

`herdr-api.schema.json` is what the pinned Herdr binary answers to `herdr api schema --json`, written by `scripts/bump-herdr.sh` in the same run that moves the pin in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`.
Herdr owns the schema and the protocol revision; `herdr-core/build.rs` derives `HERDR_PROTOCOL_REVISION` from this file, so no protocol number is maintained by hand anywhere in this repository.

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
