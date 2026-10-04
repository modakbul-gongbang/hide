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
    if (done) return;
    stop();
    done = true;
    running.delete(owned);
  };
  running.add(owned);
  return { stop: owned, disown: () => void running.delete(owned) };
}

/** Cleanup remains a failure without replacing the original stack/signature. */
export function cleanupAfterFailure(primary: unknown, cleanup: () => void): never {
  try { cleanup(); } catch (secondary) {
    throw new AggregateError([primary, secondary], "fixture primary failure and secondary cleanup failure", { cause: primary });
  }
  throw primary;
}
