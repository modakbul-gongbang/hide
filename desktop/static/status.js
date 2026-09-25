// The status page reads its state from its hash (`#connecting`,
// `#failed=<reason>`) and asks for Retry by setting `#retry`, which the
// host sees as an in-page navigation. It has no bridge and no Node API.

const REASONS = {
  cli_missing: "The hide command was not found.",
  start_failed: "hided could not start.",
  no_response: "hided did not respond.",
};

function show() {
  const hash = location.hash.slice(1);
  const failed = hash.startsWith("failed=") ? hash.slice("failed=".length) : null;
  document.getElementById("connecting").hidden = failed !== null;
  document.getElementById("failed").hidden = failed === null;
  if (failed !== null) {
    const reason = document.getElementById("reason");
    reason.textContent = REASONS[failed] ?? REASONS.start_failed;
    reason.dataset.reason = failed;
  }
}

document.getElementById("retry").addEventListener("click", () => {
  location.hash = "retry";
});
window.addEventListener("hashchange", show);
show();
