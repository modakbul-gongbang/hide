// Page motion: one-time reveals and the hero owl's peeking.
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

// The hero owl plays its loop only while the stage is on screen and the tab is visible; otherwise,
// and under reduced motion, the image holds one still frame.
function mountPeek() {
  const owl = document.querySelector("[data-peek]");
  if (!owl || reduceMotion) return;
  const still = owl.getAttribute("src");
  const moving = owl.dataset.peek;
  let inView = !("IntersectionObserver" in window);
  const sync = () => {
    const want = inView && !document.hidden ? moving : still;
    if (owl.getAttribute("src") !== want) owl.setAttribute("src", want);
  };
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(entries => { inView = entries[entries.length - 1].isIntersecting; sync(); }, { threshold: 0.1 }).observe(owl.closest(".stage"));
  }
  document.addEventListener("visibilitychange", sync);
  sync();
}

mountReveals();
mountPeek();
