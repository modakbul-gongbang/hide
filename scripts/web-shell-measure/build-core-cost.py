#!/usr/bin/env python3
"""Link the same probe against a worktree's already-built release libraries.

Run with the repository's pinned rustc on PATH (scripts/toolchain-env.sh).
No Cargo manifest, build flags or source of either measured product is changed.
"""
import json
import hashlib
import pathlib
import subprocess
import sys

if len(sys.argv) != 3 or sys.argv[2] not in ("legacy", "typed"):
    raise SystemExit("usage: build-core-cost.py <worktree> legacy|typed")
root = pathlib.Path(sys.argv[1]).resolve()
deps = root / "target/release/deps"
core = list(deps.glob("libherdr_core-*.rlib"))
if len(core) != 1:
    raise SystemExit("expected exactly one release core library")
core_hash = core[0].stem.removeprefix("libherdr_core-")
fingerprints = root / "target/release/.fingerprint"
record = json.loads((fingerprints / f"herdr-core-{core_hash}/lib-herdr_core.json").read_text())
expected = {name: digest for _, name, _, digest in record["deps"]}
arguments = ["rustc", "--edition=2024", "-C", "opt-level=3", "-C", "lto=thin", "-C", "codegen-units=1", "-L", f"dependency={deps}"]
linked = []
for name in ("herdr_core", "hide_node", "hide_node_link", "serde", "serde_json"):
    libraries = list(deps.glob(f"lib{name}-*.rlib"))
    if name in expected:
        digest = expected[name].to_bytes(8, "little").hex()
        package = name if name == "serde_json" else name.replace("_", "-")
        libraries = [lib for lib in libraries if
                     (fingerprints / f"{package}-{lib.stem.rsplit('-', 1)[1]}" / f"lib-{name}").is_file()
                     and (fingerprints / f"{package}-{lib.stem.rsplit('-', 1)[1]}" / f"lib-{name}").read_text() == digest]
    if len(libraries) != 1:
        raise SystemExit(f"ambiguous release dependency {name}")
    arguments.extend(["--extern", f"{name}={libraries[0]}"])
    linked.append({"name": name, "path": str(libraries[0]),
                   "sha256": hashlib.file_digest(libraries[0].open("rb"), "sha256").hexdigest()})
if sys.argv[2] == "typed":
    arguments.extend(["--cfg", "typed_snapshot"])
arguments.extend([str(pathlib.Path(__file__).with_name("core-cost.rs")), "-o", str(root / "target/release/responsive-core-cost")])
subprocess.run(arguments, check=True)
source = pathlib.Path(__file__).with_name("core-cost.rs")
identity = {
    "source_head": subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip(),
    "mode": sys.argv[2],
    "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
    "driver_sha256": hashlib.file_digest(source.open("rb"), "sha256").hexdigest(),
    "linked": linked,
}
(root / "target/release/responsive-core-cost.identity.json").write_text(json.dumps(identity, indent=2) + "\n")
