// File integrity and release inventory, not installation or native UI evidence.
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

/**
 * The tag is the one source of a release's kind: `vX.Y.Z` is a stable
 * release, published as the latest; `vX.Y.Z-rc.N` is a pre-release, never the
 * latest. Any other tag is refused.
 */
export function releaseKind(tag) {
  const match = typeof tag === "string" && /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-rc\.[1-9]\d*)?$/.exec(tag);
  if (!match) throw new Error("a release requires a vX.Y.Z or vX.Y.Z-rc.N tag without other suffixes");
  return { prerelease: match[4] !== undefined };
}

export function expectedAssetNames(tag) {
  releaseKind(tag);
  const archives = ["macos-arm64.zip", "windows-x64.zip", "linux-x64.tar.gz"].map((suffix) => `hide-${tag}-${suffix}`);
  return archives.flatMap((name) => [name, `${name}.sha256`]).sort();
}

export async function verifyAssets(tag, directory) {
  const expected = expectedAssetNames(tag);
  const actual = (await fs.readdir(directory)).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    const missing = expected.filter((name) => !actual.includes(name));
    const unexpected = actual.filter((name) => !expected.includes(name));
    throw new Error(`release asset set differs: missing [${missing.join(", ")}]; unexpected [${unexpected.join(", ")}]`);
  }
  const manifest = [];
  for (const name of expected) {
    const file = path.join(directory, name);
    const stat = await fs.lstat(file);
    if (!stat.isFile() || stat.size === 0) throw new Error(`${name} must be a nonempty ordinary file`);
    if (name.endsWith(".sha256") && stat.size > 512) throw new Error(`${name} exceeds the checksum size limit`);
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(file)) hash.update(chunk);
    manifest.push({ name, file, size: stat.size, digest: `sha256:${hash.digest("hex")}` });
  }
  for (const asset of manifest.filter(({ name }) => !name.endsWith(".sha256"))) {
    const checksum = await fs.readFile(`${asset.file}.sha256`, "utf8");
    const match = /^([a-f0-9]{64})  ([^\r\n]+)\r?\n?$/.exec(checksum);
    if (!match || match[2] !== asset.name) throw new Error(`${asset.name}.sha256 must name only its exact archive`);
    if (`sha256:${match[1]}` !== asset.digest) throw new Error(`${asset.name} does not match its SHA-256`);
  }
  return manifest;
}

export function selectDraft(tag, pages) {
  const expected = expectedAssetNames(tag);
  if (!Array.isArray(pages) || pages.some((page) => !Array.isArray(page))) {
    throw new Error("release inventory must contain every page from the authenticated release list");
  }
  const releases = pages.flat();
  if (releases.some((release) => release === null || typeof release !== "object" || typeof release.tag_name !== "string")) {
    throw new Error("release inventory contains an invalid release record");
  }
  const matching = releases.filter((release) => release.tag_name === tag);
  if (matching.length > 1) throw new Error("multiple releases use this tag; review them before retrying");
  if (!matching.length) return null;
  const release = matching[0];
  if (release.draft !== true || release.prerelease !== releaseKind(tag).prerelease) {
    throw new Error("only an existing draft of this exact tag and its release kind may be used");
  }
  if (!Array.isArray(release.assets)) throw new Error("the existing draft has no asset inventory");
  const names = release.assets.map((asset) => asset?.name);
  if (new Set(names).size !== names.length || names.some((name) => !expected.includes(name))) {
    throw new Error("the existing draft contains duplicate or unexpected assets; review it before retrying");
  }
  return release;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [tag, directory, releasePagesFile] = process.argv.slice(2);
  if (!tag || !directory || process.argv.length > 5) throw new Error("usage: node scripts/check-release-assets.mjs <vX.Y.Z[-rc.N]> <directory> [release-pages.json]");
  await verifyAssets(tag, directory);
  if (releasePagesFile) {
    if ((await fs.stat(releasePagesFile)).size > 16 * 1024 * 1024) throw new Error("release inventory exceeds 16 MiB");
    selectDraft(tag, JSON.parse(await fs.readFile(releasePagesFile, "utf8")));
  }
  console.log(`release assets verified: ${tag}, macos-arm64, windows-x64, linux-x64, 3 archives and 3 checksums`);
}
