# T11 public draft release

Date: 2026-08-31.

## Merge and test baseline

The feature branch was merged into the local `main` branch with `--no-ff`.

The merged local `main` commit is `ee1353f69be4725ef5d6c31e83e7f73460fd40ae`.

The local development commits remain intact, and no local history was rewritten.

The final full test pass used an external Rust target directory and the required external Swift scratch directory so build artifacts were not created and removed in the repository between verification rounds.

The release source included the current Swift shell, Rust core, packaging scripts, bundled resources, and the official Herdr v0.8.2 macOS arm64 asset pin.

## Public repository and tag

Repository: `https://github.com/modakbul-gongbang/hide`.

Default branch: `main`.

Final release candidate commit: `7c0ca7e0dad68f2d4204d36d9e82cd3f3265e615`.

Final tag: `v0.1.9`.

The tag resolves to the final candidate commit without a force push.

## Actions and draft release

Actions run: `https://github.com/modakbul-gongbang/hide/actions/runs/33319301333`.

The run completed the release build, archive verification, and draft-release upload.

Draft release: `https://github.com/modakbul-gongbang/hide/releases/tag/untagged-970925d650f6deb06a33`.

The release is still a draft and has not been published.

| Asset | Size | SHA-256 |
| --- | ---: | --- |
| `hide-v0.1.9-macos-arm64.zip` | 48,336,092 bytes | `e315951d5402ce40a26c5390055329fc3926cf81bf187df952b163abf09a0b06` |
| `hide-v0.1.9-macos-arm64.zip.sha256` | 94 bytes | `3156e13ce88d68bb366657cab756791db30bcbf79e463bee720c8e6f4318c483` |

The downloaded sidecar contains the portable record `e315951d5402ce40a26c5390055329fc3926cf81bf187df952b163abf09a0b06  hide-v0.1.9-macos-arm64.zip`.

Running `shasum -a 256 -c ./*.sha256` from the external archive directory returned `hide-v0.1.9-macos-arm64.zip: OK`.

The GitHub release API reports the same digest for the uploaded zip asset.

## Bundled Herdr provenance

The official `herdr-macos-aarch64` asset for Herdr v0.8.2 was checked through the Herdr release API.

Its recorded SHA-256 is `a5d4f4d504d8b309c91f811050559300faba31258425f53c50852fc96f6ae574`.

The bundle records that digest and includes the Herdr Apache-2.0 notice.

Earlier pre-release evidence records `bba6c79874689d5c8ec45811518ecf5cef9b521e61b081a9f56ddd406a482328` for a superseded candidate.
That digest is not the shipped v0.1.9 binary; the release candidate and installed bundle use the official v0.8.2 asset recorded above.

Earlier release attempts were retained as ordinary non-force history while the release workflow was corrected for the Swift runner, compiler workaround, official Herdr digest, and portable checksum sidecar.

The final `v0.1.9` run is the successful draft-release candidate.
