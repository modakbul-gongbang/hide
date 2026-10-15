// The one-night scene: agents on machines that stay on work through a night while your room stays
// dark, and the six moments of that night as a list beside it. It replaces the static lane
// timeline inside #night-stage, plays once from 23:00 when the section comes into view, pauses off
// screen and in a hidden tab, and rests on the morning. Under reduced motion it shows only the
// morning, with every moment listed.

const NARROW = window.matchMedia("(max-width: 720px)");
const REDUCE = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function mountNight(stage) {
  const scene = stage.querySelector(".night-scene");
  const replay = stage.querySelector(".night-replay");
  let layout = null;
  let started = false;
  let inView = false;
  let finished = false;

  // Every animation of the night: the picture's and the list's, one shared timeline.
  const animations = () => stage.getAnimations({ subtree: true });

  const sync = () => {
    const run = started && inView && !document.hidden && !finished;
    for (const a of animations()) run ? a.play() : a.pause();
  };

  const finish = () => {
    finished = true;
    stage.classList.add("is-morning");
    if (replay) replay.hidden = REDUCE;
  };

  async function load() {
    const want = NARROW.matches ? "tall" : "wide";
    if (want === layout) return;
    const at = layout ? animations()[0]?.currentTime ?? 0 : 0;
    const response = await fetch(`./assets/night/night-${want}.svg`);
    if (!response.ok) throw new Error(`night-${want}.svg answered ${response.status}`);
    scene.innerHTML = await response.text();
    layout = want;
    stage.classList.add("is-live");
    // The picture fits the column, so the timeline's sideways-scrolling region no longer scrolls.
    const region = stage.closest(".night-scroll");
    if (region) {
      region.setAttribute("aria-label", "An example night, from 23:00 to 07:00.");
      region.removeAttribute("tabindex");
    }
    if (REDUCE) {
      stage.classList.add("is-still");
      scene.querySelector("svg").classList.add("still");
      finish();
      return;
    }
    for (const a of animations()) a.currentTime = at;
    sync();
    Promise.all(animations().map(a => a.finished)).then(() => { finish(); sync(); }, () => {});
  }

  if (replay) {
    replay.addEventListener("click", () => {
      finished = false;
      stage.classList.remove("is-morning");
      replay.hidden = true;
      for (const a of animations()) a.currentTime = 0;
      sync();
      Promise.all(animations().map(a => a.finished)).then(() => { finish(); sync(); }, () => {});
    });
  }

  new IntersectionObserver(entries => {
    inView = entries[entries.length - 1].isIntersecting;
    if (inView) started = true;
    sync();
  }, { threshold: 0.35 }).observe(stage);
  document.addEventListener("visibilitychange", sync);
  NARROW.addEventListener("change", () => load().catch(error => console.error("night scene:", error)));

  // Without the picture the static timeline stays, so a failed load only goes to the console.
  load().catch(error => console.error("night scene:", error));
}

const stage = document.getElementById("night-stage");
if (stage && "IntersectionObserver" in window && stage.getAnimations) mountNight(stage);
