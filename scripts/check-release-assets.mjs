// A draft must contain one version of every supported package, and nothing
// from another run or target. This validates files, not installation or UI.
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";

const [tag, directory, existingReleaseFile] = process.argv.slice(2);
if (!tag || !directory || process.argv.length > 5) {
  throw new Error("usage: node scripts/check-release-assets.mjs <vX.Y.Z> <directory> [existing-release.json]");
}
if (!/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag)) {
  throw new Error("a stable release requires a vX.Y.Z tag without prerelease or build suffixes");
}
const version = tag.slice(1);
const archives = [
  `hide-v${version}-macos-arm64.zip`,
  `hide-v${version}-windows-x64.zip`,
  `hide-v${version}-linux-x64.tar.gz`,
];
const expected = archives.flatMap((name) => [name, `${name}.sha256`]).sort();
const actual = (await fs.readdir(directory)).sort();
if (JSON.stringify(actual) !== JSON.stringify(expected)) {
  const missing = expected.filter((name) => !actual.includes(name));
  const unexpected = actual.filter((name) => !expected.includes(name));
  throw new Error(`release asset set differs: missing [${missing.join(", ")}]; unexpected [${unexpected.join(", ")}]`);
}
for (const name of expected) {
  const stat = await fs.lstat(path.join(directory, name));
  if (!stat.isFile() || stat.size === 0) throw new Error(`${name} must be a nonempty ordinary file`);
  if (name.endsWith(".sha256") && stat.size > 512) throw new Error(`${name} exceeds the checksum size limit`);
}
for (const name of archives) {
  const checksum = await fs.readFile(path.join(directory, `${name}.sha256`), "utf8");
  const match = /^([a-f0-9]{64})  ([^\r\n]+)\r?\n?$/.exec(checksum);
  if (!match || match[2] !== name) throw new Error(`${name}.sha256 must name only its exact archive`);
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path.join(directory, name))) hash.update(chunk);
  if (hash.digest("hex") !== match[1]) throw new Error(`${name} does not match its SHA-256`);
}
if (existingReleaseFile) {
  const release = JSON.parse(await fs.readFile(existingReleaseFile, "utf8"));
  if (release !== null) {
    if (release.tag_name !== tag || release.draft !== true || release.prerelease !== false) {
      throw new Error("only an existing stable draft for this exact tag may be updated");
    }
    if (!Array.isArray(release.assets)) throw new Error("the existing draft has no asset inventory");
    const names = release.assets.map((asset) => asset.name);
    if (new Set(names).size !== names.length || names.some((name) => !expected.includes(name))) {
      throw new Error("the existing draft contains duplicate or unexpected assets; review it before retrying");
    }
  }
}
console.log(`release assets verified: ${tag}, macos-arm64, windows-x64, linux-x64, 3 archives and 3 checksums`);
