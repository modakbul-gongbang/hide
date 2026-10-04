// A fixture process belongs to the Playwright worker that started it. A test
// that fails still runs its own `finally`, but a worker that dies first (an
// unhandled error event, the runner stopping it) skips every one of them, and
// the private Herdr server or hided it started lives on under launchd with its
// temporary directory. Every stop registered here also runs when the worker
// exits, so nothing a worker started outlives it.

const running = new Set<() => void>();
const MAX_OWNED_FIXTURES = 256;

process.once("exit", () => {
  for (const stop of [...running]) {
    try {
      stop();
    } catch (error) {
      console.error("e2e fixture stop failed at worker exit", error);
      process.exitCode = 1;
    }
  }
});

/**
 * `stop`, run at most once, and at worker exit if nothing ran it before.
 * `disown` leaves it to the caller alone, for a process whose directory a
 * successor takes over (a daemon restart reuses it).
 */
export function ownUntilWorkerExit(stop: () => void): { stop: () => void; disown: () => void } {
  if (running.size >= MAX_OWNED_FIXTURES) cleanupAfterFailure(new Error("worker exceeded 256 owned fixtures"), stop);
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

/** Run every release, keeping all failures in the actual serialized cause chain. */
export async function finishFixture(primary: unknown, releases: (() => void | Promise<void>)[]): Promise<void> {
  if (releases.length > 256) throw new Error("fixture release cap exceeded");
  const failures: unknown[] = primary === undefined ? [] : [primary];
  for (const release of releases) {
    try { await release(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throwFixtureFailures(failures);
}

/** Synchronous owners use the same primary identity and serialized chain. */
export function throwFixtureFailures(failures: unknown[]): never {
  if (!failures.length || failures.length > 256) throw new Error("invalid fixture failure inventory");
  let failure = failures[failures.length - 1];
  for (let index = failures.length - 2; index >= 0; index--) {
    try { cleanupAfterFailure(failures[index], () => { throw failure; }); }
    catch (reported) { failure = reported; }
  }
  throw failure;
}

/** Cleanup remains a failure without replacing the original stack/signature. */
export function cleanupAfterFailure(primary: unknown, cleanup: () => void): never {
  try { cleanup(); } catch (secondary) {
    // Playwright serializes message/stack/cause but drops AggregateError.errors.
    // Keep the primary identity intact. Playwright filters stack frames, so
    // extra prose appended to the stack is not a durable diagnostic channel.
    const original = primary instanceof Error ? primary : new Error(String(primary));
    const cleanupError = secondary instanceof Error ? secondary : new Error(String(secondary));
    // Keep a prior cause as well: setup can already carry a native subprocess
    // error, and a second release failure must not erase the first release.
    const causes: Error[] = [];
    const seen = new Set<Error>();
    for (let cause = original.cause; cause !== undefined;) {
      if (causes.length >= 16 || cause instanceof Error && seen.has(cause)) {
        const reported = new Error(original.message, { cause: new Error("fixture error cause chain exceeded its bound", { cause: cleanupError }) });
        reported.name = original.name; reported.stack = original.stack;
        throw reported;
      }
      const value = cause instanceof Error ? cause : new Error(String(cause));
      causes.push(value); seen.add(value); cause = value.cause;
    }
    let chained = cleanupError;
    for (const cause of causes.reverse()) {
      const copy = new Error(cause.message, { cause: chained }); copy.name = cause.name; copy.stack = cause.stack; chained = copy;
    }
    const reported = new Error(original.message, { cause: chained });
    reported.name = original.name;
    reported.stack = original.stack;
    throw reported;
  }
  throw primary;
}
