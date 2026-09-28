// Each independent column publishes its own last committed frame. A menu or
// palette reads the geometry the operator sees, never the other column's tree.
import type { AgentFrame } from "./agentLayout";
import type { Geometry, LayoutSizes, ViewFrame } from "./viewLayout";
export type DrawnViews = ViewFrame & { geometry: Geometry; sizes: LayoutSizes };
type Frames = { view: DrawnViews | null; agent: AgentFrame | null };
const frames: Frames = { view: null, agent: null };
export function noteAreaFrame<K extends keyof Frames>(column: K, frame: Frames[K]) { frames[column] = frame; }
export function areaFrame<K extends keyof Frames>(column: K): Frames[K] { return frames[column]; }
