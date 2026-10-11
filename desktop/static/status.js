// The status page reads its state from its hash (`#connecting`,
// `#failed=<reason>`, with `&file=<path>` when a stored file stopped the
// daemon) and asks for Retry by setting `#retry`, and for Copy diagnostics
// by adding `&copy=1`, which the host sees as an in-page navigation; the
// host answers a copy with `&copied=1`, or with `&diagnostics=<text>` when
// the clipboard refused it. It has no bridge and no Node API.
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
document.getElementById("failed-title").textContent = words("failed");
document.getElementById("retry").textContent = words("retry");
const copy = document.getElementById("copy");
const REASONS = ["cli_missing", "start_failed", "no_response", "other_build", "state_refused"];

function show() {
  const hash = new URLSearchParams(location.hash.slice(1));
  const failed = hash.get("failed");
  const file = document.getElementById("file");
  file.textContent = hash.get("file") ?? "";
  file.hidden = !file.textContent;
  document.getElementById("connecting").hidden = failed !== null;
  document.getElementById("failed").hidden = failed === null;
  copy.textContent = words(hash.has("copied") ? "copied" : "copy");
  const diagnostics = document.getElementById("diagnostics");
  diagnostics.textContent = hash.get("diagnostics") ?? "";
  diagnostics.hidden = !diagnostics.textContent;
  if (failed !== null) {
    const reason = document.getElementById("reason");
    reason.textContent = words(REASONS.includes(failed) ? failed : "start_failed");
    reason.dataset.reason = failed;
  }
}

document.getElementById("retry").addEventListener("click", () => {
  location.hash = "retry";
});
copy.addEventListener("click", () => {
  const hash = new URLSearchParams(location.hash.slice(1));
  hash.delete("copied");
  hash.delete("diagnostics");
  hash.set("copy", "1");
  location.hash = hash.toString();
});
window.addEventListener("hashchange", show);
show();
