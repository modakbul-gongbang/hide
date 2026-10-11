// Page motion: one-time reveals, and the agent sidebar mock, which is the page's only running animation.
// Both stop when the visitor asks for reduced motion.

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function mountReveals() {
  const items = document.querySelectorAll(".reveal");
  if (reduceMotion || !("IntersectionObserver" in window)) {
    items.forEach(item => item.classList.add("in"));
    return;
  }
  const observer = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add("in");
      observer.unobserve(entry.target);
    }
  }, { rootMargin: "0px 0px -8% 0px" });
  items.forEach(item => observer.observe(item));
}

// The sidebar mock: six agents from different CLIs move through the same four groups.
const AGENTS = {
  a: { title: "Billing schema migration", where: "claude code · api › feat/billing-v2" },
  b: { title: "Fix stale PR badge after restart", where: "codex · web › fix/sidebar-pr-state" },
  c: { title: "Port the session reader", where: "pi · core › feat/pi-sessions" },
  d: { title: "Profile terminal scroll", where: "opencode · web › perf/scroll" },
  e: { title: "Draft release notes", where: "grok · docs › main" },
  f: { title: "Tidy settings copy", where: "cursor · web › chore/settings" },
};
const GROUPS = [
  ["needs", "Needs you"],
  ["done", "Done"],
  ["working", "Working"],
  ["seen", "Seen"],
];
// Each frame: agent -> [group, mark, line]. The line replaces the title's checkout when the agent asks something.
const FRAMES = [
  { a: ["needs", "?", "asks: approve the migration plan?"], b: ["done", "✓"], c: ["working"], d: ["working"], e: ["seen"], f: ["seen"] },
  { a: ["working"], b: ["seen"], c: ["working"], d: ["done", "✓"], e: ["seen"], f: ["seen"] },
  { a: ["working"], b: ["seen"], c: ["needs", "!", "approval: run the migration on staging"], d: ["done", "✓"], e: ["working"], f: ["seen"] },
  { a: ["done", "✓"], b: ["seen"], c: ["working"], d: ["seen"], e: ["working"], f: ["needs", "?", "asks: which settings page first?"] },
];

function renderFrame(body, frame, changed) {
  const html = [];
  for (const [group, name] of GROUPS) {
    const ids = Object.keys(frame).filter(id => frame[id][0] === group);
    if (!ids.length) continue;
    html.push(`<p class="grp"><b>${name}</b> · ${ids.length}</p>`);
    for (const id of ids) {
      const [, mark = "", ask] = frame[id];
      const agent = AGENTS[id];
      const cls = ["row", group === "needs" ? "hl" : "", changed.has(id) ? "fresh" : ""].join(" ").trim();
      html.push(
        `<div class="${cls}"><span class="mk ${group}">${mark}</span>`
        + `<span><span class="t">${agent.title}</span><span class="sub">${ask || agent.where}</span></span>`
        + `<span class="meta">${group === "working" ? "now" : ""}</span></div>`,
      );
    }
  }
  body.innerHTML = html.join("");
}

function mountLiveSidebar() {
  const root = document.querySelector("[data-live]");
  const body = root?.querySelector("[data-live-body]");
  if (!body) return;
  let index = 0;
  renderFrame(body, FRAMES[0], new Set());
  if (reduceMotion) return;

  let timer = null;
  const step = () => {
    const prev = FRAMES[index];
    index = (index + 1) % FRAMES.length;
    const next = FRAMES[index];
    const changed = new Set(Object.keys(next).filter(id => next[id][0] !== prev[id][0]));
    body.classList.add("moving");
    window.setTimeout(() => {
      renderFrame(body, next, changed);
      body.classList.remove("moving");
    }, 300);
  };
  // Run only while the mock is on screen and the tab is visible.
  let inView = !("IntersectionObserver" in window);
  const sync = () => {
    const run = inView && !document.hidden;
    if (run && !timer) timer = window.setInterval(step, 3200);
    if (!run && timer) { window.clearInterval(timer); timer = null; }
  };
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(([entry]) => { inView = entry.isIntersecting; sync(); }, { threshold: 0.3 }).observe(root);
  }
  document.addEventListener("visibilitychange", sync);
  sync();
}

mountReveals();
mountLiveSidebar();
