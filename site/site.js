// Page motion: one-time reveals. They show at once when the visitor asks for reduced motion.

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

mountReveals();
