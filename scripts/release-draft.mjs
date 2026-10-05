// Never publish, PATCH release metadata, DELETE or overwrite an asset.
// The repository's immutable releases setting protects publication during an upload.
import { createReadStream } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { verifyAssets, selectDraft } from "./check-release-assets.mjs";

const MAX_PAGES = 100;
const MAX_JSON_BYTES = 8 * 1024 * 1024;
const MAX_INVENTORY_BYTES = 16 * 1024 * 1024;

export function createGitHubClient({ repository, writeToken, fetchImpl = fetch }) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error("a GitHub owner/repository is required");
  if (!writeToken) throw new Error("a release write credential is required");
  const prefix = `/repos/${repository}`;
  async function request(method, suffix, { body, upload = false, uploadSize } = {}) {
    const base = upload ? "https://uploads.github.com" : "https://api.github.com";
    const response = await fetchImpl(`${base}${prefix}${suffix}`, {
      method, redirect: "error", signal: AbortSignal.timeout(30_000),
      headers: {
        Authorization: `Bearer ${writeToken}`,
        Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28",
        ...(body ? { "Content-Type": upload ? "application/octet-stream" : "application/json" } : {}),
        ...(upload ? { "Content-Length": String(uploadSize) } : {}),
      },
      ...(body ? { body: upload ? body : JSON.stringify(body), ...(upload ? { duplex: "half" } : {}) } : {}),
    });
    if (!response.ok) {
      await response.body?.cancel();
      throw new Error(`GitHub ${method} ${upload ? "asset upload" : "release request"} failed with HTTP ${response.status}`);
    }
    let size = 0;
    const chunks = [];
    for await (const chunk of response.body) {
      size += chunk.length;
      if (size > MAX_JSON_BYTES) throw new Error("GitHub response exceeds 8 MiB");
      chunks.push(chunk);
    }
    try {
      return JSON.parse(Buffer.concat(chunks).toString("utf8"));
    } catch {
      throw new Error("GitHub returned invalid JSON");
    }
  }
  async function list(suffix) {
    const pages = [];
    let size = 0;
    for (let page = 1; page <= MAX_PAGES; page++) {
      const rows = await request("GET", `${suffix}?per_page=100&page=${page}`);
      if (!Array.isArray(rows) || rows.length > 100) throw new Error("GitHub list response is invalid");
      size += Buffer.byteLength(JSON.stringify(rows));
      if (size > MAX_INVENTORY_BYTES) throw new Error("GitHub inventory exceeds 16 MiB");
      pages.push(rows);
      if (rows.length < 100) return pages;
    }
    throw new Error("GitHub inventory exceeds 100 pages; preparation stopped");
  }
  return {
    releases: () => list("/releases"),
    release: (id) => request("GET", `/releases/${id}`),
    assets: async (id) => (await list(`/releases/${id}/assets`)).flat(),
    tag: async (tag) => {
      let object = (await request("GET", `/git/ref/tags/${encodeURIComponent(tag)}`)).object;
      for (let depth = 0; depth < 5; depth++) {
        if (!object || !/^[a-f0-9]{40}$/.test(object.sha)) throw new Error("tag object is invalid");
        if (object.type === "commit") return object.sha;
        if (object.type !== "tag") throw new Error("tag does not refer to a commit");
        object = (await request("GET", `/git/tags/${object.sha}`)).object;
      }
      throw new Error("annotated tag nesting exceeds five objects");
    },
    create: (record) => request("POST", "/releases", { body: record }),
    upload: async (id, asset) => {
      const stream = createReadStream(asset.file);
      try {
        return await request("POST", `/releases/${id}/assets?name=${encodeURIComponent(asset.name)}`, { body: stream, upload: true, uploadSize: asset.size });
      } finally {
        stream.destroy();
      }
    },
  };
}

function requireRelease(release, tag, marker, id = release?.id) {
  if (!Number.isSafeInteger(release?.id) || release.id <= 0 || release.tag_name !== tag
    || release.id !== id || release.draft !== true || release.prerelease !== false || release.body !== marker) {
    throw new Error("release must be the exact unpublished draft created under this source; review legacy drafts separately");
  }
}

function compareAssets(remote, manifest, complete = false) {
  const names = new Set();
  for (const asset of remote) {
    const expected = manifest.find(({ name }) => name === asset?.name);
    if (!expected || names.has(asset.name) || asset.state !== "uploaded"
      || asset.size !== expected.size || asset.digest !== expected.digest) {
      throw new Error("remote assets differ in name, upload state, size or digest; no asset was deleted or overwritten");
    }
    names.add(asset.name);
  }
  if (complete && names.size !== manifest.length) throw new Error("final draft asset inventory is incomplete");
  return names;
}

export async function prepareDraft({ tag, sha, directory, client }) {
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error("the release event must identify an exact commit SHA");
  const manifest = await verifyAssets(tag, directory);
  const marker = `<!-- hide-release-source:${sha} -->`;
  async function preflight() {
    if (await client.tag(tag) !== sha) throw new Error("the current tag no longer identifies the release event commit");
  }
  await preflight();
  let release = selectDraft(tag, await client.releases());
  if (release) requireRelease(release, tag, marker);
  else {
    await preflight();
    // The tag already exists; under immutable releases GitHub refuses a target commit for it with HTTP 403.
    release = await client.create({ tag_name: tag, name: tag, body: marker, draft: true, prerelease: false });
    requireRelease(release, tag, marker);
  }
  const id = release.id;
  let uploaded = compareAssets(await client.assets(id), manifest);
  for (const asset of manifest) {
    if (uploaded.has(asset.name)) continue;
    await preflight();
    requireRelease(await client.release(id), tag, marker, id);
    uploaded = compareAssets(await client.assets(id), manifest);
    if (uploaded.has(asset.name)) continue;
    const result = await client.upload(id, asset);
    if (result?.name !== asset.name) throw new Error("GitHub upload returned another asset name");
    compareAssets([result], manifest);
  }
  await preflight();
  requireRelease(await client.release(id), tag, marker, id);
  compareAssets(await client.assets(id), manifest, true);
  return { id, tag, assets: manifest.length };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [tag, sha, directory] = process.argv.slice(2);
  try {
    if (!tag || !sha || !directory || process.argv.length !== 5) throw new Error("usage: node scripts/release-draft.mjs <vX.Y.Z> <commit-sha> <directory>");
    const client = createGitHubClient({ repository: process.env.GITHUB_REPOSITORY, writeToken: process.env.GH_TOKEN });
    const result = await prepareDraft({ tag, sha, directory, client });
    console.log(`complete unpublished draft verified: ${result.tag}, release ${result.id}, ${result.assets} assets`);
  } catch (error) {
    console.error(`release preparation blocked: ${error.message}`);
    process.exitCode = 1;
  }
}
