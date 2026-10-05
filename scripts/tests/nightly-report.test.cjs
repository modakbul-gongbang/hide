const { test } = require("node:test");
const assert = require("node:assert/strict");
const report = require("../nightly-report.cjs");

// The only double is GitHub's external API; the report entrypoint runs unchanged.
function fixture() {
  const issues = [];
  const comments = [];
  const github = { rest: {
    actions: { getWorkflowRun: async () => ({ data: { run_attempt: 1, head_sha: "abc123", html_url: "https://github.com/example/hide/actions/runs/7" } }) },
    issues: {
      listForRepo: async ({ page, per_page }) => ({ data: issues.slice((page - 1) * per_page, page * per_page) }),
      listComments: async ({ page, per_page }) => ({ data: comments.slice((page - 1) * per_page, page * per_page) }),
      create: async (value) => { issues.push({ ...value, number: 5 }); return { data: issues.at(-1) }; },
      update: async ({ title }) => { issues[0].title = title; },
      createComment: async ({ body }) => { comments.push({ body }); },
    },
  } };
  const context = { ref: "refs/heads/main", repo: { owner: "example", repo: "hide" }, runId: 7 };
  const results = { web: { result: "success" }, desktop: { result: "failure" }, package: { result: "cancelled" } };
  return { github, context, results, issues, comments };
}

test("all nightly lane failures reach one issue with run identity", async () => {
  const f = fixture();
  assert.equal((await report(f)).outcome, "created");
  assert.equal(f.issues.length, 1);
  assert.match(f.issues[0].body, /desktop: failure/);
  assert.match(f.issues[0].body, /package: cancelled/);
  assert.match(f.issues[0].body, /abc123.*attempt 1/);
  assert.match(f.issues[0].body, /actions\/runs\/7/);
  assert.equal((await report(f)).outcome, "already_reported");
  assert.equal(f.comments.length, 0);
});

test("a new attempt uses the existing thread and repeated reporting converges", async () => {
  const f = fixture();
  f.issues.push({ number: 5, title: "Nightly web e2e on macOS is failing", body: "Earlier failure" });
  await report(f);
  await report(f);
  assert.equal(f.issues.length, 1);
  assert.equal(f.issues[0].title, "Nightly CI is failing");
  assert.equal(f.comments.length, 1);
  f.github.rest.actions.getWorkflowRun = async () => ({ data: { run_attempt: 2, head_sha: "abc123", html_url: "https://github.com/example/hide/actions/runs/7" } });
  await report(f);
  assert.equal(f.comments.length, 2);
  assert.match(f.comments[1].body, /nightly-run:7:2/);
});

test("a green run has a distinct no-work outcome", async () => {
  const f = fixture();
  f.results = { web: { result: "success" } };
  assert.equal((await report(f)).outcome, "no_failures");
  assert.equal(f.issues.length, 0);
});

test("a lane a hand run left out is skipped and is not a failure", async () => {
  const f = fixture();
  f.results = { web: { result: "success" }, desktop: { result: "skipped" }, package: { result: "skipped" } };
  assert.equal((await report(f)).outcome, "no_failures");
  assert.equal(f.issues.length, 0);
});

test("wrong branch, incomplete run identity and API failures fail the caller", async () => {
  const f = fixture();
  await assert.rejects(report({ ...f, context: { ...f.context, ref: "refs/heads/topic" } }), /requires main/);
  f.github.rest.actions.getWorkflowRun = async () => ({ data: {} });
  await assert.rejects(report(f), /identity is incomplete/);
  f.github.rest.actions.getWorkflowRun = async () => { throw new Error("API unavailable"); };
  await assert.rejects(report(f), /API unavailable/);
  assert.equal(f.issues.length, 0);
});

test("a saturated search reports overflow instead of creating a duplicate", async () => {
  const f = fixture();
  for (let i = 0; i < 1000; i++) f.issues.push({ number: i, title: "Other bug" });
  await assert.rejects(report(f), /exceeded 1000/);
  assert.equal(f.issues.length, 1000);
  assert.equal(f.comments.length, 0);
});
