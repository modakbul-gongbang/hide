#!/bin/zsh
set -euo pipefail
export LC_ALL=en_US.UTF-8
cargo test --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo
cargo build --release --manifest-path herdr-core/Cargo.toml --target-dir /tmp/herdr-ide-verify/cargo
# Swift links the library built for the dev bundle by build_dev_app.sh.
swift build --package-path macos --scratch-path /tmp/herdr-ide-verify/swift --disable-keychain --disable-sandbox
swift test --package-path macos --scratch-path /tmp/herdr-ide-verify/swift --disable-keychain --disable-sandbox
zsh scripts/check-herdr-pin-single-source.sh
runtime=$(zsh scripts/fetch-herdr-runtime.sh)
zsh scripts/check-herdr-contract.sh --herdr-bin "$runtime" --schema-only
