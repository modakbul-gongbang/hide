// Which side of its trigger a hint opens on while browser pages are shown
// (issue 323). A page is a native view drawn over the shell, so a hint that
// meets one is drawn under it and cut at its edge. Hints never freeze a page,
// since they open and close too often; instead a hint opens on the first
// side, in the order its own side, the opposite one, then the two across,
// that fits the window and meets no shown page, and on its own side when none
// does. With no page shown it keeps its own side.

export type HintSide = "top" | "right" | "bottom" | "left";
export type HintAlign = "start" | "center" | "end";
type Box = { x: number; y: number; width: number; height: number };

const ACROSS: Record<HintSide, [HintSide, HintSide, HintSide]> = {
  top: ["bottom", "right", "left"],
  bottom: ["top", "right", "left"],
  right: ["left", "bottom", "top"],
  left: ["right", "bottom", "top"],
};

export function hintSide(input: {
  side: HintSide;
  align: HintAlign;
  /** The gap between the trigger and the hint. */
  offset: number;
  trigger: Box;
  /** The hint as it renders. */
  size: { width: number; height: number };
  viewport: { width: number; height: number };
  /** The pages shown now. */
  pages: readonly Box[];
}): HintSide {
  if (input.pages.length === 0) return input.side;
  for (const side of [input.side, ...ACROSS[input.side]]) {
    const box = placed(side, input);
    if (box && !input.pages.some((page) => meets(page, box))) return side;
  }
  return input.side;
}

/** Where the hint lands on `side`, held inside the window along its trigger the way Radix shifts it, or null when it does not fit across. */
function placed(side: HintSide, { align, offset, trigger, size, viewport }: Parameters<typeof hintSide>[0]): Box | null {
  const vertical = side === "top" || side === "bottom";
  const along = (start: number, length: number, own: number, room: number) => {
    const at = align === "start" ? start : align === "end" ? start + length - own : start + (length - own) / 2;
    return Math.min(Math.max(at, 0), Math.max(room - own, 0));
  };
  if (vertical) {
    const y = side === "top" ? trigger.y - offset - size.height : trigger.y + trigger.height + offset;
    if (y < 0 || y + size.height > viewport.height) return null;
    return { x: along(trigger.x, trigger.width, size.width, viewport.width), y, ...size };
  }
  const x = side === "left" ? trigger.x - offset - size.width : trigger.x + trigger.width + offset;
  if (x < 0 || x + size.width > viewport.width) return null;
  return { x, y: along(trigger.y, trigger.height, size.height, viewport.height), ...size };
}

function meets(a: Box, b: Box): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}
