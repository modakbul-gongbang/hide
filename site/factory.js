// The Factory line demo: a scripted loop of Tasks moving through Intake, Work, Verify and Merge.
// Who answers each question follows the mode table in docs/factory.md: Code runs the rules, Factory AI
// answers what the chosen mode hands it, and a permission always waits for a person.
// The loop runs only while the demo is on screen and the tab is visible; reduced motion shows one still frame.

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// Which question kinds Factory AI answers in each mode. A permission is in none of them.
const MODES = {
  manual: { line: "Every decision is mine", ai: [] },
  assist: { line: "Technical to AI, product to me", ai: ["tech"] },
  autonomous: { line: "Only permissions to me", ai: ["tech", "product", "risk"] },
};
const STAGES = ["intake", "work", "verify", "merge"];
const TASKS = {
  417: { title: "Redraw the cleanup table", col: "verify", worker: "claude", note: "checks queued" },
  420: { title: "Task detail API", col: "work", worker: "codex", note: "working" },
  431: { title: "Export invoices as CSV", col: "work", worker: "claude", note: "working" },
  432: { title: "Sort the export by date", col: "intake", note: "after #431", waiting: true },
};
const ADDED = { id: 434, title: "Retry rate-limited calls" };
const QUESTIONS = {
  tech: { kind: "tech", label: "Open decision", short: "asks: REST or WS?", text: "Serve Task details from a new REST endpoint, or fold them into the WS snapshot?", options: ["WS snapshot", "REST endpoint"], ai: "chose the WS snapshot: no new route", you: "WS snapshot" },
  product: { kind: "product", label: "Open decision", short: "asks: column order", text: "Should the CSV put the invoice date or the customer first?", options: ["Date first", "Customer first"], ai: "chose date first, as the page shows it", you: "date first" },
  perm: { kind: "perm", label: "Permission", short: "asks: paid tier?", text: "Higher rate limits need the vendor's paid tier at $29 a month. Upgrade?", options: ["Stay on free tier", "Upgrade"], you: "stay on the free tier" },
  risk: { kind: "risk", label: "Waiting to merge", short: "risky path: hided/", text: "PR #563 changes the risky path hided/. Merge it?", options: ["Merge", "Request changes"], ai: "approved the risky-path merge", you: "merge PR #563" },
};

function mountFactory(root) {
  const stage = root.querySelector(".fd-stage");
  const lists = Object.fromEntries([...root.querySelectorAll("[data-col]")].map(c => [c.dataset.col, c.querySelector(".fd-list")]));
  const deskBox = root.querySelector(".fd-desk");
  const desk = root.querySelector("[data-desk]");
  const deskEmpty = desk.querySelector(".fd-desk-empty");
  const deskCount = root.querySelector("[data-desk-count]");
  const mergedEl = root.querySelector("[data-merged]");
  const cmd = root.querySelector("[data-cmd]");
  const log = root.querySelector("[data-log]");
  const modeLine = root.querySelector("[data-mode-line]");
  let mode = "assist";
  let merged = 12;

  for (const button of root.querySelectorAll("[data-mode]")) {
    button.addEventListener("click", () => {
      mode = button.dataset.mode;
      for (const b of root.querySelectorAll("[data-mode]")) b.setAttribute("aria-pressed", String(b === button));
      modeLine.textContent = MODES[mode].line;
    });
  }

  const animate = (el, frames, options) => (reduceMotion ? null : el.animate(frames, options));

  // Waiting counts only while the demo is visible, so a hidden loop resumes where it stopped.
  let visible = false;
  const wakers = new Set();
  const setVisible = v => {
    visible = v;
    if (!v) return;
    for (const wake of wakers) wake();
    wakers.clear();
  };
  const wait = ms => new Promise(resolve => {
    let left = ms;
    let last = performance.now();
    const tick = () => {
      if (!visible) { wakers.add(() => { last = performance.now(); requestAnimationFrame(tick); }); return; }
      const now = performance.now();
      left -= now - last;
      last = now;
      if (left <= 0) resolve(); else requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  // Move cards with FLIP so a card glides from its old place to its new one, whatever the layout.
  const flip = mutate => {
    const moving = () => [...stage.querySelectorAll(".fd-card, .fd-q")];
    const before = new Map(moving().map(c => [c, c.getBoundingClientRect()]));
    mutate();
    for (const c of moving()) {
      const a = before.get(c);
      if (!a) continue;
      const b = c.getBoundingClientRect();
      const dx = a.left - b.left;
      const dy = a.top - b.top;
      if (dx || dy) animate(c, [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: 650, easing: "cubic-bezier(.2,.75,.2,1)" });
    }
  };

  const el = (tag, cls, html) => {
    const e = document.createElement(tag);
    e.className = cls;
    e.innerHTML = html;
    return e;
  };
  const card = id => stage.querySelector(`.fd-card[data-id="${id}"]`);

  const makeCard = (id, title) => {
    const c = el("div", "fd-card",
      `<span class="fd-flag"></span><p class="fd-card-t"><b>${title}</b><span>#${id}</span></p>`
      + `<p class="fd-bar">${STAGES.map(() => "<i></i>").join("")}</p>`
      + '<p class="fd-card-m"><span class="fd-worker"></span><span class="fd-note"></span></p>');
    c.dataset.id = id;
    return c;
  };
  // A card's bar: finished stages green, the current one blue while it runs and yellow while it waits on someone.
  const paint = (id, { state, worker, note } = {}) => {
    const c = card(id);
    if (state) c.dataset.state = state;
    const col = c.closest("[data-col]").dataset.col;
    const at = STAGES.indexOf(col);
    c.querySelectorAll(".fd-bar i").forEach((seg, i) => {
      seg.className = col === "merged" || i < at ? "done" : i === at ? c.dataset.state : "";
    });
    if (worker !== undefined) c.querySelector(".fd-worker").textContent = worker;
    if (note !== undefined) c.querySelector(".fd-note").textContent = note;
  };
  const move = (id, col) => {
    flip(() => lists[col].appendChild(card(id)));
    paint(id);
  };

  const say = (who, text) => {
    const li = el("li", "", `<span class="fd-who" data-who="${who}">${who}</span><span class="fd-said">${text}</span>`);
    log.prepend(li);
    animate(li, [{ opacity: 0, transform: "translateY(-6px)" }, { opacity: 1, transform: "none" }], { duration: 400, easing: "ease-out" });
    while (log.children.length > 4) log.lastElementChild.remove();
  };
  const land = (id, pr) => {
    move(id, "merged");
    merged += 1;
    mergedEl.textContent = merged;
    animate(mergedEl, [{ transform: "scale(1.3)" }, { transform: "none" }], { duration: 600, easing: "ease-out" });
    say("Code", `#${id} merged · PR #${pr}`);
  };
  const setDesk = n => {
    deskCount.textContent = n;
    deskBox.dataset.open = n > 0 ? "yes" : "no";
    deskEmpty.hidden = n > 0;
  };
  const question = (id, q) => el("div", `fd-q kind-${q.kind}`,
    `<p class="fd-q-k">${q.label} · #${id}</p><p class="fd-q-t">${q.text}</p>`
    + `<p class="fd-q-o">${q.options.map((o, i) => (i === 0 ? `<span class="rec">${o}<em>Recommended</em></span>` : `<span>${o}</span>`)).join("")}</p>`);

  const check = async id => {
    const c = card(id);
    c.classList.add("checking");
    paint(id, { note: "checks running" });
    await wait(1200);
    c.classList.remove("checking");
    paint(id, { note: "checks passed" });
    say("Code", `#${id} checks passed`);
  };

  // A question goes to Factory AI when the mode hands it that kind; anything else waits on the desk.
  const decide = async (id, q, after) => {
    const c = card(id);
    paint(id, { state: "ask", note: q.short });
    await wait(800);
    if (MODES[mode].ai.includes(q.kind)) {
      c.classList.add("ai");
      await wait(900);
      say("Factory AI", `#${id} ${q.ai}`);
      await wait(700);
      c.classList.remove("ai");
      paint(id, { state: "run", note: after });
      return;
    }
    const chip = question(id, q);
    flip(() => desk.appendChild(chip));
    animate(chip, [{ opacity: 0, transform: "translateY(10px) scale(.98)" }, { opacity: 1, transform: "none" }], { duration: 450, easing: "ease-out" });
    setDesk(1);
    await wait(1900);
    chip.querySelector(".rec").classList.add("picked");
    await wait(700);
    say("You", `#${id} ${q.you}`);
    await animate(chip, [{ opacity: 1 }, { opacity: 0, transform: "translateX(16px)" }], { duration: 350, easing: "ease-in" }).finished;
    chip.remove();
    setDesk(0);
    paint(id, { state: "run", note: after });
  };

  const prompt = () => { cmd.innerHTML = '<span class="p">$</span> <span class="t"></span><span class="caret"></span>'; };
  const typeAdd = async () => {
    const t = cmd.querySelector(".t");
    const text = `hide factory add "${ADDED.title}"`;
    for (let i = 1; i <= text.length; i++) {
      t.textContent = text.slice(0, i);
      await wait(32);
    }
    await wait(450);
    const c = makeCard(ADDED.id, ADDED.title);
    flip(() => lists.intake.appendChild(c));
    animate(c, [{ opacity: 0, transform: "translateY(12px)" }, { opacity: 1, transform: "none" }], { duration: 450, easing: "ease-out" });
    paint(ADDED.id, { state: "run", worker: "", note: "intake review" });
    prompt();
    say("Code", `#${ADDED.id} added to Intake`);
  };

  const reset = () => {
    for (const list of Object.values(lists)) list.replaceChildren();
    desk.querySelectorAll(".fd-q").forEach(q => q.remove());
    setDesk(0);
    log.replaceChildren();
    prompt();
    merged = 12;
    mergedEl.textContent = merged;
    for (const [id, t] of Object.entries(TASKS)) {
      lists[t.col].appendChild(makeCard(id, t.title));
      paint(id, { state: t.waiting ? "wait" : "run", worker: t.worker || "", note: t.note });
    }
  };

  const cycle = async () => {
    reset();
    await wait(1000);
    await typeAdd();
    await wait(600);
    await check(417);
    await wait(400);
    move(417, "merge");
    paint(417, { note: "merging" });
    await wait(1000);
    land(417, 560);
    await wait(700);
    await decide(420, QUESTIONS.tech, "building");
    await wait(600);
    move(420, "verify");
    paint(420, { note: "checks queued" });
    await wait(500);
    move(ADDED.id, "work");
    paint(ADDED.id, { worker: "codex", note: "working" });
    say("Code", `#${ADDED.id} started · codex`);
    await wait(900);
    await decide(431, QUESTIONS.product, "building");
    await wait(500);
    move(431, "verify");
    paint(431, { note: "checks queued" });
    await check(420);
    await wait(400);
    move(420, "merge");
    paint(420, { note: "merging" });
    await wait(900);
    land(420, 561);
    await wait(500);
    await decide(ADDED.id, QUESTIONS.perm, "working");
    await check(431);
    await wait(400);
    move(431, "merge");
    await decide(431, QUESTIONS.risk, "merging");
    await wait(700);
    land(431, 563);
    await wait(700);
    move(432, "work");
    paint(432, { state: "run", worker: "claude", note: "working" });
    say("Code", "#432 started: #431 merged");
    await wait(3600);
    const out = stage.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 500, fill: "forwards" });
    await out.finished;
    cycle();
    out.cancel();
    stage.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 500 });
  };

  if (reduceMotion) {
    // One still frame: a risky-path merge waits on the desk while the rest of the line moves on.
    reset();
    lists.merged.append(card(417), card(420));
    lists.merge.appendChild(card(431));
    lists.work.appendChild(makeCard(ADDED.id, ADDED.title));
    for (const id of [417, 420]) paint(id, { note: "merged" });
    paint(ADDED.id, { state: "run", worker: "codex", note: "working" });
    paint(431, { state: "ask", note: QUESTIONS.risk.short });
    merged = 14;
    mergedEl.textContent = merged;
    desk.appendChild(question(431, QUESTIONS.risk));
    setDesk(1);
    say("Code", "#420 merged · PR #561");
    say("Code", "#431 checks passed");
    return;
  }

  // Draw the opening board at once, so the line is never empty before the loop reaches the screen.
  reset();
  let started = false;
  let inView = false;
  const sync = () => {
    setVisible(inView && !document.hidden);
    if (visible && !started) {
      started = true;
      cycle();
    }
  };
  // One callback can carry several entries for the demo; the last one is its current state.
  new IntersectionObserver(entries => { inView = entries[entries.length - 1].isIntersecting; sync(); }, { threshold: 0.2 }).observe(root);
  document.addEventListener("visibilitychange", sync);
}

const factoryRoot = document.querySelector("[data-factory]");
if (factoryRoot) mountFactory(factoryRoot);
