// The nightly failure thread is one intent per run attempt, including reruns.
// Only nightly.yml's main-only report job calls this with issue-write authority.
const TITLE = "Nightly CI is failing";
const LEGACY_TITLE = "Nightly web e2e on macOS is failing";
const PAGE_SIZE = 100;
const MAX_PAGES = 10;

async function boundedList(list, parameters, subject) {
  const items = [];
  for (let page = 1; page <= MAX_PAGES; page++) {
    const response = await list({ ...parameters, per_page: PAGE_SIZE, page });
    items.push(...response.data);
    if (response.data.length < PAGE_SIZE) return items;
  }
  throw new Error(`${subject} exceeded ${MAX_PAGES * PAGE_SIZE} entries; inspect the nightly report job`);
}

module.exports = async function reportNightly({ github, context, results }) {
  if (context.ref !== "refs/heads/main") throw new Error("Nightly reporting requires main");
  const failed = Object.entries(results).filter(([, value]) => value.result !== "success" && value.result !== "skipped");
  if (!failed.length) return { outcome: "no_failures" };

  const { owner, repo } = context.repo;
  const run = (await github.rest.actions.getWorkflowRun({ owner, repo, run_id: context.runId })).data;
  const attempt = run.run_attempt;
  if (!Number.isInteger(attempt) || attempt < 1 || !run.html_url || !run.head_sha) {
    throw new Error("The nightly run identity is incomplete; inspect the Actions API response");
  }
  const marker = `<!-- nightly-run:${context.runId}:${attempt} -->`;
  const lanes = failed.map(([name, value]) => `- ${name}: ${value.result}`).join("\n");
  const body = `${marker}\nNightly CI failed at ${run.head_sha} (attempt ${attempt}): ${run.html_url}\n\nLanes:\n${lanes}\n\nRead the failed jobs and their artifacts in the run.\nA configured matrix is not proof that a suite or physical GUI/IME check executed.`;
  const issues = await boundedList(github.rest.issues.listForRepo, { owner, repo, state: "open", labels: "bug" }, "Open bug issues");
  const issue = issues.find(item => !item.pull_request && [TITLE, LEGACY_TITLE].includes(item.title));
  if (!issue) {
    const created = await github.rest.issues.create({ owner, repo, title: TITLE, labels: ["bug"], body });
    return { outcome: "created", issue: created.data.number };
  }
  const comments = await boundedList(github.rest.issues.listComments, { owner, repo, issue_number: issue.number }, "Nightly issue comments");
  if (issue.body?.includes(marker) || comments.some(comment => comment.body?.includes(marker))) {
    return { outcome: "already_reported", issue: issue.number };
  }
  if (issue.title !== TITLE) await github.rest.issues.update({ owner, repo, issue_number: issue.number, title: TITLE });
  await github.rest.issues.createComment({ owner, repo, issue_number: issue.number, body });
  return { outcome: "commented", issue: issue.number };
};
