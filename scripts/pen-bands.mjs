// Place the design canvas's boards by what they are.
//
// A .pen file has no pages: every board is a top-level frame on one canvas, so
// the only structure the file can carry is the board's name and where it sits.
// This module binds the two and draws a label above each band. A board's name prefix says which band it belongs
// to, the band says its y, and the boards in a band are laid out left to right
// in their existing order. gen-pen-layout.mjs writes that placement and
// check-pen-layout.mjs refuses a canvas whose placement or naming disagrees, so
// a board dragged somewhere by hand, or named outside the scheme, is reported
// rather than lost in a 12,000-line JSON file.

export const CANVAS = 'design/hide.pen';

// Bands from top to bottom. A fit-content frame carries no height in the file,
// so the y values cannot be derived; they are spaced for the tallest board each
// band holds today, measured by rendering: a review's findings table is 2320.
export const BANDS = [
  {prefix: 'System /', y: -1600, holds: 'token sheets and primitives generated from or tracking HideTheme'},
  {prefix: 'Component /', y: -900, holds: 'the agreed component set; Screen and Feature boards reference these'},
  {prefix: 'Screen /', y: 0, holds: 'what the app draws today'},
  {prefix: 'Review /', y: 1200, holds: 'an audit, its proposal, and the as-built evidence beside them; deleted once adopted'},
  {prefix: 'Feature /', y: 3800, holds: 'a PRD\'s design, named by its slug; does not outlive the PRD'},
  {prefix: 'Scratch /', y: 5800, holds: 'exploration; deleted or promoted'},
];

export const GAP = 80;

// A fit-content frame declares no width. The layout cannot know how wide it
// renders, so it reserves this much; a small primitive wastes a little space,
// which is harmless, where a guess too small would overlap the next board.
export const FALLBACK_WIDTH = 120;

// A band's label is generator-owned. It is a top-level frame named `Band /
// <name>` sitting above the band's first row, carrying the band's name, what it
// holds, and a hairline the width of the row; it is rebuilt on every layout so
// nothing anyone draws into it survives, and it is what makes the bands
// visible on the canvas rather than only in the file.
export const LABEL_PREFIX = 'Band /';
const LABEL_HEIGHT = 100;
const LABEL_MIN_WIDTH = 1200;

export function bandOf(name) {
  return BANDS.find(band => name.startsWith(band.prefix));
}

function label(band, extent) {
  const name = band.prefix.replace(' /', '');
  const width = Math.max(extent, LABEL_MIN_WIDTH);
  return {
    type: 'frame',
    id: `band-${name.toLowerCase()}`,
    x: 0,
    y: band.y - LABEL_HEIGHT,
    name: `${LABEL_PREFIX} ${name}`,
    width,
    layout: 'vertical',
    gap: '$--spacing-sm',
    children: [
      {type: 'text', id: `band-${name.toLowerCase()}-title`, name: 'Title', fill: '$--color-secondary', content: name, fontFamily: '$--font-ui', fontSize: '$--text-display', fontWeight: '600'},
      {type: 'text', id: `band-${name.toLowerCase()}-holds`, name: 'Holds', fill: '$--color-muted', content: `${band.prefix}  ${band.holds}`, fontFamily: '$--font-ui', fontSize: '$--text-subhead', fontWeight: 'normal'},
      {type: 'rectangle', id: `band-${name.toLowerCase()}-rule`, name: 'Rule', width: 'fill_container', height: 1, fill: '$--color-divider'},
    ],
  };
}

// Returns the laid-out document and the names of boards no band claims. Those
// are left exactly where they are so the caller can decide what to do.
export function layout(document) {
  const unknown = [];
  const rows = new Map(BANDS.map(band => [band, []]));
  for (const node of document.children) {
    const name = node.name ?? '';
    if (name.startsWith(LABEL_PREFIX)) continue;
    const band = bandOf(name);
    if (band) rows.get(band).push(node);
    else unknown.push(name || `<${node.type} ${node.id}>`);
  }
  const children = [];
  for (const [band, nodes] of rows) {
    // Existing order is the author's order: sort by where the boards are now,
    // not by name, so a rename does not shuffle the row.
    nodes.sort((a, b) => (a.y ?? 0) - (b.y ?? 0) || (a.x ?? 0) - (b.x ?? 0));
    let x = 0;
    for (const node of nodes) {
      node.x = x;
      node.y = band.y;
      x += (node.width ?? FALLBACK_WIDTH) + GAP;
    }
    children.push(label(band, x - GAP), ...nodes);
  }
  for (const node of document.children) if (!children.includes(node) && !(node.name ?? '').startsWith(LABEL_PREFIX)) children.push(node);
  return {document: {...document, children}, unknown};
}

export function serialize(document) {
  return JSON.stringify(document, null, 2);
}
