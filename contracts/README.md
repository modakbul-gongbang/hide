# Herdr API contract

`herdr-api.schema.json` is what the pinned Herdr binary answers to `herdr api schema --json`, written by `scripts/bump-herdr.sh` in the same run that moves the pin in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`.
Herdr owns the schema and the protocol revision; `herdr-core/build.rs` derives `HERDR_PROTOCOL_REVISION` from this file, so no protocol number is maintained by hand anywhere in this repository.

hide distributes a modified Herdr preview from the [modakbul-gongbang/herdr fork](https://github.com/modakbul-gongbang/herdr/releases/tag/preview-2026-09-06-13d8d0b99033), built from commit `13d8d0b99033e6855ce66bc0f96654615c8a17a6`.
This fork supplies host-scoped snapshots, ordered event sequences, and agent lineage that the upstream stable release does not yet expose.
The weekly `herdr-update.yml` workflow continues to propose upstream stable releases with `--repo herdrdev/herdr`; return to upstream when the contract field tests and runtime checks pass.

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
