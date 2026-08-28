# Herdr API contract

`herdr-api.schema.json` is a generated consumer artifact copied from Herdr's canonical `docs/next/api/herdr-api.schema.json`.
Herdr owns the schema and protocol revision.
Herdr IDE must never maintain a second handwritten protocol number.

Refresh the artifact after changing Herdr's API contract:

```sh
./scripts/sync-herdr-contract.sh --herdr-root /path/to/herdr
```

Before launching the IDE against a local runtime, verify that the bundled contract, installed CLI, and running server agree:

```sh
./scripts/check-herdr-contract.sh
```
