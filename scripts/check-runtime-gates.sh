#!/bin/zsh
set -euo pipefail
export LC_ALL=en_US.UTF-8
cd "$(dirname "$0")/.."
bash scripts/verify-cargo.sh test
bash scripts/verify-swift.sh test
zsh scripts/check-herdr-pin-single-source.sh
runtime=$(zsh scripts/fetch-herdr-runtime.sh)
zsh scripts/check-herdr-contract.sh --herdr-bin "$runtime" --schema-only
