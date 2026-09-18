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
