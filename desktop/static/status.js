// The status page reads its state from its hash (`#connecting`,
// `#failed=<reason>`, with `&file=<path>` when a stored file stopped the
// daemon, and `&said=<sentence>&builds=<line>` when the core is newer than
// this app or its update failed) and asks for Retry by setting `#retry`,
// which the
// host sees as an in-page navigation. It has no bridge and no Node API.
// Its sentences arrive in its address, written by the host from the
// interface catalogs in the language in effect; a missing one is a host bug
// and fails loudly rather than showing a guess.

const params = new URLSearchParams(location.search);

function words(key) {
  const value = params.get(key);
  if (value === null) throw new Error("status page was opened without its " + key + " text");
  return value;
}

document.documentElement.lang = words("lang");
document.getElementById("connecting").textContent = words("connecting");
document.getElementById("retry").textContent = words("retry");
const REASONS = ["cli_missing", "start_failed", "no_response", "other_build", "state_refused", "core_newer", "update_failed"];
// The reasons a build names its own title for: the app or the core is the one to update, not a failure to connect.
const TITLES = { core_newer: "core_newer_title", update_failed: "update_failed_title" };

function shownLine(id, text) {
  const line = document.getElementById(id);
  line.textContent = text ?? "";
  line.hidden = !line.textContent;
}

function show() {
  const hash = new URLSearchParams(location.hash.slice(1));
  const failed = hash.get("failed");
  shownLine("file", hash.get("file"));
  shownLine("builds", hash.get("builds"));
  document.getElementById("connecting").hidden = failed !== null;
  document.getElementById("failed").hidden = failed === null;
  if (failed !== null) {
    const known = REASONS.includes(failed) ? failed : "start_failed";
    document.getElementById("failed-title").textContent = words(TITLES[known] ?? "failed");
    const reason = document.getElementById("reason");
    reason.textContent = hash.get("said") ?? words(TITLES[known] ? "start_failed" : known);
    reason.dataset.reason = failed;
  }
}

document.getElementById("retry").addEventListener("click", () => {
  location.hash = "retry";
});
window.addEventListener("hashchange", show);
show();
