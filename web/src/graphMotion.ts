// The Agents graph's motion (PRD agents-graph-view D-20, D-24): numbers keyed
// by name move from the picture on screen to the next one over one duration,
// and a picture that equals the last one starts nothing, no timer and no
// animation frame (B39). Framework free: the caller paints each frame
// straight into the DOM, so React never re-renders to animate.

export type Targets = ReadonlyMap<string, number>;

/** Numbers closer than this are one number: a layout recomputed to the same answer is not a move. */
const SAME = 0.01;

export function sameTargets(a: Targets, b: Targets): boolean {
  if (a.size !== b.size) return false;
  for (const [key, value] of a) {
    const other = b.get(key);
    if (other === undefined || Math.abs(other - value) >= SAME) return false;
  }
  return true;
}

function ease(t: number): number {
  return t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
}

export type TweenOptions = {
  /** Called with every picture to draw: the answer at each frame, and the new elements' places at once. */
  paint: (values: Targets) => void;
  durationMs: number;
  /** The OS asked for less motion: every change is one jump (B34). Read per change, so the setting is followed live. */
  reduced: () => boolean;
};

export type Retargeted = {
  /** The picture was not the one already on screen. */
  changed: boolean;
  /** The names new to the picture, which the caller may let enter. */
  added: string[];
};

export class Tween {
  private shown = new Map<string, number>();
  private goal: Targets = new Map();
  private from = new Map<string, number>();
  private started = 0;
  private frame: number | null = null;
  /** How many pictures were applied, for the idle check (`data-graph-revision`). */
  revision = 0;
  /** How many animation frames this tween asked for, for the same check (`data-graph-frames`). */
  frames = 0;

  constructor(private readonly options: TweenOptions) {}

  /** Whether an animation frame is pending: false whenever nothing is moving. */
  get running(): boolean {
    return this.frame !== null;
  }

  retarget(next: Targets): Retargeted {
    if (this.revision > 0 && sameTargets(this.goal, next)) return { changed: false, added: [] };
    this.revision += 1;
    this.goal = next;
    const added: string[] = [];
    for (const [key, value] of next) {
      if (!this.shown.has(key)) {
        // A name new to the picture stands at its place at once; only what was already drawn glides.
        this.shown.set(key, value);
        added.push(key);
      }
    }
    for (const key of [...this.shown.keys()]) if (!next.has(key)) this.shown.delete(key);
    this.cancel();
    let moves = false;
    for (const [key, value] of next) if (Math.abs((this.shown.get(key) ?? value) - value) >= SAME) moves = true;
    if (!moves || this.options.durationMs <= 0 || this.options.reduced()) {
      this.shown = new Map(next);
      this.options.paint(this.shown);
      return { changed: true, added };
    }
    this.from = new Map(this.shown);
    this.started = performance.now();
    this.options.paint(this.shown);
    this.frame = this.schedule();
    return { changed: true, added };
  }

  private step = (now: number) => {
    const t = Math.min(1, (now - this.started) / this.options.durationMs);
    if (t >= 1) {
      this.frame = null;
      this.shown = new Map(this.goal);
    } else {
      const eased = ease(t);
      for (const [key, to] of this.goal) {
        const from = this.from.get(key) ?? to;
        this.shown.set(key, from + (to - from) * eased);
      }
      this.frame = this.schedule();
    }
    this.options.paint(this.shown);
  };

  private schedule(): number {
    this.frames += 1;
    return requestAnimationFrame(this.step);
  }

  /** Paints the picture as it stands, for an element drawn after the numbers were last painted. */
  repaint() {
    this.options.paint(this.shown);
  }

  private cancel() {
    if (this.frame !== null) cancelAnimationFrame(this.frame);
    this.frame = null;
  }

  /** Ends the animation; the owner calls it when the graph leaves the screen. */
  dispose() {
    this.cancel();
  }
}

/** What `Flow` moves: the one attribute of a dash pattern's path. */
export type FlowPath = { setAttribute: (name: string, value: string) => void };

export type FlowOptions = {
  /** The dash pattern's length, one dash and one gap. */
  period: number;
  /** How often the pattern advances. */
  stepMs: number;
  /** How many steps make one period. */
  steps: number;
  reduced: () => boolean;
  /** Told whether the timer runs, after every change to the paths or the setting; the owner shows it where a check can read it. */
  running?: (running: boolean) => void;
};

/**
 * The dashes flowing along working lines (PRD agents-graph-view B9, D-26),
 * stepped a few times a second by a timer. A CSS animation of the same
 * dashes keeps the whole frame pipeline running at the display's rate
 * whatever the dashes' own speed: measured natively with six lines it cost
 * about a tenth of a core, and still three percent when stepped in CSS,
 * where three timer steps a second cost a fortieth of that. No timer runs
 * while no line is working or the system asks for less motion.
 */
export class Flow {
  private paths: readonly FlowPath[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private step = 0;

  constructor(private readonly options: FlowOptions) {}

  /** The paths flowing now; an empty list, or reduced motion, stops the timer. */
  set(paths: readonly FlowPath[]): void {
    this.paths = paths;
    if (paths.length === 0 || this.options.reduced()) {
      this.stop();
      return;
    }
    this.paint();
    if (this.timer === null) this.timer = setInterval(() => this.advance(), this.options.stepMs);
    this.options.running?.(true);
  }

  /** The system's motion setting changed: start again if the dashes may move now. */
  resume(): void {
    this.set(this.paths);
  }

  private advance(): void {
    if (this.options.reduced()) {
      this.stop();
      return;
    }
    this.step = (this.step + 1) % this.options.steps;
    this.paint();
  }

  private paint(): void {
    const offset = -this.step * (this.options.period / this.options.steps);
    for (const path of this.paths) path.setAttribute("stroke-dashoffset", String(offset));
  }

  private stop(): void {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    this.options.running?.(false);
  }

  /** Ends the timer; the owner calls it when the graph leaves the screen. */
  dispose(): void {
    this.stop();
    this.paths = [];
  }
}
