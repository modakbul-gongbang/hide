#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
if ! command -v node >/dev/null 2>&1; then
  echo "hcoord build requires Node.js 22 or newer; Node.js was not found" >&2
  exit 1
fi
if ! command -v npm >/dev/null 2>&1; then
  echo "hcoord build requires npm to install its TypeScript build dependency" >&2
  exit 1
fi
cd "$root"
npm install --ignore-scripts --no-audit --no-fund
node scripts/build.mjs
