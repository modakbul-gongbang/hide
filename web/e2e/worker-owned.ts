// A fixture process belongs to the Playwright worker that started it. A test
// that fails still runs its own `finally`, but a worker that dies first (an
// unhandled error event, the runner stopping it) skips every one of them, and
// the private Herdr server or hided it started lives on under launchd with its
// temporary directory. Every stop registered here also runs when the worker
// exits, so nothing a worker started outlives it.

const running = new Set<() => void>();

process.once("exit", () => {
  for (const stop of [...running]) {
    try {
      stop();
    } catch (error) {
      console.error("e2e fixture stop failed at worker exit", error);
    }
  }
});

/**
 * `stop`, run at most once, and at worker exit if nothing ran it before.
 * `disown` leaves it to the caller alone, for a process whose directory a
 * successor takes over (a daemon restart reuses it).
 */
export function ownUntilWorkerExit(stop: () => void): { stop: () => void; disown: () => void } {
  let done = false;
  const owned = () => {
    running.delete(owned);
    if (done) return;
    done = true;
    stop();
  };
  running.add(owned);
  return { stop: owned, disown: () => void running.delete(owned) };
}

/**
 * Runs `cleanup` after `primary` failed and returns `primary` for the caller
 * to throw. A cleanup that also fails is appended to the stack Playwright
 * prints, so the original message and location stay first and the cleanup
 * failure is still reported.
 */
export function afterCleanup(primary: unknown, cleanup: () => void): unknown {
  try {
    cleanup();
  } catch (secondary) {
    const detail = secondary instanceof Error ? (secondary.stack ?? secondary.message) : String(secondary);
    if (!(primary instanceof Error)) return new Error(`${String(primary)}\n\ncleanup after it also failed: ${detail}`);
    primary.stack = `${primary.stack ?? primary.message}\n\ncleanup after it also failed: ${detail}`;
  }
  return primary;
}
