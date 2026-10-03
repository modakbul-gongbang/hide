import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import fs from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";
import { gzipSync } from "node:zlib";
import { createGitHubClient, prepareDraft } from "../release-draft.mjs";

const tag = "v1.2.3";
const sha = "1".repeat(40);
const marker = `<!-- hide-release-source:${sha}; immutable-policy:owner-enforced -->`;
const names = ["macos-arm64.zip", "windows-x64.zip", "linux-x64.tar.gz"].map((s) => `hide-${tag}-${s}`);
const digest = (data) => `sha256:${createHash("sha256").update(data).digest("hex")}`;

async function fixture(t, options = {}) {
  const runs = path.resolve("agents/runs/release-draft-tests");
  await fs.mkdir(runs, { recursive: true });
  const directory = await fs.mkdtemp(path.join(runs, "fixture-"));
  t.after(() => fs.rm(directory, { recursive: true, force: true }));
  const files = new Map();
  for (const name of names) {
    // Valid empty ZIP and TAR.GZ archives; no bundled program is executed.
    const data = name.endsWith(".zip") ? Buffer.from("504b0506000000000000000000000000000000000000", "hex") : gzipSync(Buffer.alloc(1024));
    files.set(name, data);
    files.set(`${name}.sha256`, Buffer.from(`${digest(data).slice(7)}  ${name}\n`));
  }
  for (const [name, data] of files) await fs.writeFile(path.join(directory, name), data);
  const state = {
    release: options.release ? structuredClone(options.release) : null,
    assets: options.assets ? structuredClone(options.assets) : [],
    requests: [], writes: [], assetGets: 0,
    policy: options.policy ?? { enabled: true, enforced_by_owner: true },
  };
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, "http://fixture.invalid");
    const route = url.pathname.replace("/repos/example/project", "");
    const data = [];
    for await (const part of req) data.push(part);
    const bytes = Buffer.concat(data);
    state.requests.push({ method: req.method, route });
    if (req.method !== "GET") state.writes.push({ method: req.method, route });
    const answer = (status, body) => { res.writeHead(status, { "Content-Type": "application/json" }); res.end(JSON.stringify(body)); };
    if (options.fail === route) return answer(options.status ?? 500, { message: "fixture failure" });
    if (route === "/immutable-releases") return answer(200, state.policy);
    if (route === `/git/ref/tags/${tag}`) return answer(200, { object: { type: options.annotated ? "tag" : "commit", sha: options.tagSha ?? sha } });
    if (route === `/git/tags/${sha}`) return answer(200, { object: { type: "commit", sha } });
    if (route === "/releases" && req.method === "GET") {
      const page = Number(url.searchParams.get("page"));
      if (options.pages) return answer(200, options.pages[page - 1] ?? []);
      return answer(200, state.release ? [{ ...state.release, assets: state.assets }] : []);
    }
    if (route === "/releases" && req.method === "POST") {
      const payload = JSON.parse(bytes);
      state.release = { ...payload, id: 7 };
      return answer(201, state.release);
    }
    if (route === "/releases/7" && req.method === "GET") {
      if (options.publishBeforeRead) state.release.draft = false;
      if (options.wrongId) return answer(200, { ...state.release, id: 8 });
      return answer(200, state.release);
    }
    if (route === "/releases/7/assets" && req.method === "GET") {
      state.assetGets++;
      return answer(200, state.assets);
    }
    if (route === "/releases/7/assets" && req.method === "POST") {
      if (Number(req.headers["content-length"]) !== bytes.length) return answer(411, { message: "asset byte length is required" });
      // A publication after the client's last GET is blocked by server policy.
      if (options.publishDuringUpload) state.release.draft = false;
      if (!state.release.draft) return answer(422, { message: "immutable published release" });
      const name = url.searchParams.get("name");
      if (state.assets.some((a) => a.name === name)) return answer(422, { message: "duplicate asset" });
      const asset = { name, size: bytes.length, state: "uploaded", digest: digest(bytes) };
      state.assets.push(asset);
      return answer(201, asset);
    }
    answer(404, { message: "unhandled fixture route" });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); }));
  const base = `http://127.0.0.1:${server.address().port}`;
  const client = createGitHubClient({ repository: "example/project", writeToken: "fixture-write", policyToken: "fixture-policy", fetchImpl: (url, opts) => fetch(`${base}${new URL(url).pathname}${new URL(url).search}`, opts) });
  return { state, directory, files, run: () => prepareDraft({ tag, sha, directory, client }) };
}

const draft = () => ({ id: 7, tag_name: tag, body: marker, draft: true, prerelease: false });

test("complete fresh draft posts six matching assets without PATCH or DELETE; retry writes nothing", async (t) => {
  const f = await fixture(t);
  assert.deepEqual(await f.run(), { id: 7, tag, assets: 6 });
  assert.equal(f.state.writes.length, 7);
  assert.ok(f.state.writes.every((r) => r.method === "POST"));
  assert.equal(f.state.assets.length, 6);
  for (const a of f.state.assets) assert.equal(a.digest, digest(f.files.get(a.name)));
  f.state.writes.length = 0;
  await f.run();
  assert.deepEqual(f.state.writes, []);
});

test("disabled, unenforced or inaccessible policy stops before any release write", async (t) => {
  for (const options of [{ policy: { enabled: false, enforced_by_owner: false } }, { policy: { enabled: true, enforced_by_owner: false } }, { policy: {} }, { fail: "/immutable-releases", status: 403 }]) {
    const f = await fixture(t, options);
    await assert.rejects(f.run());
    assert.deepEqual(f.state.writes, []);
  }
});

test("published, legacy and ambiguous drafts stop without mutation", async (t) => {
  for (const options of [{ release: { ...draft(), draft: false } }, { release: { ...draft(), body: "old draft" } }, { pages: [[{ ...draft(), assets: [] }, { ...draft(), assets: [] }]] }]) {
    const f = await fixture(t, options);
    await assert.rejects(f.run());
    assert.deepEqual(f.state.writes, []);
  }
});

test("a stale draft beyond two full pages is found, and listing failure is not absence", async (t) => {
  const unrelated = Array.from({ length: 100 }, (_, i) => ({ tag_name: `v0.0.${i}` }));
  const f = await fixture(t, { pages: [unrelated, unrelated, [{ ...draft(), assets: [{ name: "hide-v1.2.2-macos-arm64.zip" }] }]] });
  await assert.rejects(f.run());
  assert.deepEqual(f.state.writes, []);
  assert.equal(f.state.requests.filter((r) => r.route === "/releases").length, 3);
  const failed = await fixture(t, { fail: "/releases", status: 429 });
  await assert.rejects(failed.run());
  assert.deepEqual(failed.state.writes, []);
});

test("matching partial uploaded draft resumes; mismatched, starter or duplicate assets are preserved", async (t) => {
  const f = await fixture(t, { release: draft() });
  const name = names[0], data = f.files.get(name);
  f.state.assets.push({ name, size: data.length, state: "uploaded", digest: digest(data) });
  await f.run();
  assert.equal(f.state.writes.length, 5);
  assert.ok(f.state.writes.every((r) => r.route === "/releases/7/assets"));
  for (const change of [{ digest: "sha256:" + "0".repeat(64) }, { size: 1 }, { state: "starter" }]) {
    const bad = await fixture(t, { release: draft(), assets: [{ name, size: data.length, state: "uploaded", digest: digest(data), ...change }] });
    await assert.rejects(bad.run());
    assert.deepEqual(bad.state.writes, []);
    assert.equal(bad.state.assets.length, 1);
  }
});

test("publication before GET and between GET and upload cannot replace public assets", async (t) => {
  for (const options of [{ publishBeforeRead: true }, { publishDuringUpload: true }]) {
    const f = await fixture(t, { release: draft(), ...options });
    await assert.rejects(f.run());
    assert.equal(f.state.assets.length, 0);
    assert.ok(f.state.writes.every((r) => r.method === "POST" && r.route === "/releases/7/assets"));
    if (options.publishBeforeRead) assert.deepEqual(f.state.writes, []);
  }
});

test("moved tag, wrong release ID and incomplete local package fail; annotated tag works", async (t) => {
  const moved = await fixture(t, { tagSha: "2".repeat(40) });
  await assert.rejects(moved.run());
  assert.deepEqual(moved.state.writes, []);
  const wrong = await fixture(t, { release: draft(), wrongId: true });
  await assert.rejects(wrong.run());
  assert.deepEqual(wrong.state.writes, []);
  const local = await fixture(t);
  await fs.unlink(path.join(local.directory, names[0]));
  await assert.rejects(local.run());
  assert.deepEqual(local.state.requests, []);
  const annotated = await fixture(t, { annotated: true });
  assert.equal((await annotated.run()).assets, 6);
});

test("missing policy credentials are a source-visible blocker", () => {
  assert.throws(() => createGitHubClient({ repository: "example/project", writeToken: "fixture-write" }), /Administration-read/);
});
