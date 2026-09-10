#!/bin/zsh
set -euo pipefail
export LC_ALL=en_US.UTF-8
cd "$(dirname "$0")/.."
. scripts/build-scratch.sh
cargo test --manifest-path herdr-core/Cargo.toml --target-dir "$HIDE_CARGO_SCRATCH"
cargo build --release --manifest-path herdr-core/Cargo.toml
# Swift links the library built for the dev bundle by build_dev_app.sh.
swift build --package-path macos --scratch-path "$HIDE_SWIFT_SCRATCH" --disable-keychain --disable-sandbox
swift test --package-path macos --scratch-path "$HIDE_SWIFT_SCRATCH" --disable-keychain --disable-sandbox
zsh scripts/check-herdr-pin-single-source.sh
runtime=$(zsh scripts/fetch-herdr-runtime.sh)
zsh scripts/check-herdr-contract.sh --herdr-bin "$runtime" --schema-only
