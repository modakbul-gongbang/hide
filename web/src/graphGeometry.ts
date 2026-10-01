import type { GraphGeometry } from "./agentGraph";

// The graph's sizes are tokens (`--graph-*` in design/tokens.json, the column
// gap being the Dependencies board's `--home-dependency-gap`). The layout is
// numbers, so they are read once from the root's computed style rather than
// restated here; a token that does not resolve is a broken build, not a zero.

const TOKENS: Record<keyof GraphGeometry, string> = {
  boxWidth: "--graph-box-width",
  boxBorder: "--size-hairline",
  headHeight: "--graph-head-height",
  rowHeight: "--graph-row-height",
  askingRowHeight: "--graph-row-asking-height",
  boxPadBottom: "--graph-box-pad-bottom",
  columnGap: "--home-dependency-gap",
  boxGap: "--graph-box-gap",
  pad: "--graph-pad",
  portOffset: "--graph-port-offset",
  trunkInset: "--graph-trunk-inset",
  trunkStep: "--graph-trunk-step",
  corridorClear: "--graph-corridor-clear",
  trayInsetX: "--graph-tray-inset-x",
  trayInsetY: "--graph-tray-inset-y",
};

function read(style: CSSStyleDeclaration, name: string): number {
  const value = Number.parseFloat(style.getPropertyValue(name));
  if (!Number.isFinite(value)) throw new Error(`The graph needs the ${name} token, and it does not resolve to a number.`);
  return value;
}

export function readGraphGeometry(root: Element = document.documentElement): GraphGeometry {
  const style = getComputedStyle(root);
  const entries = (Object.keys(TOKENS) as (keyof GraphGeometry)[]).map((key) => [key, read(style, TOKENS[key])] as const);
  return Object.fromEntries(entries) as GraphGeometry;
}

/** The glide's duration, a unitless token in milliseconds. */
export function readMotionMs(root: Element = document.documentElement): number {
  return read(getComputedStyle(root), "--graph-motion-ms");
}

/** How the dashes of a working line step: the dash pattern's period and how often, and in how many steps, it advances. */
export type FlowTiming = { period: number; stepMs: number; steps: number };

export function readFlowTiming(root: Element = document.documentElement): FlowTiming {
  const style = getComputedStyle(root);
  return { period: read(style, "--graph-flow-dash") + read(style, "--graph-flow-gap"), stepMs: read(style, "--graph-flow-step-ms"), steps: read(style, "--graph-flow-steps") };
}
