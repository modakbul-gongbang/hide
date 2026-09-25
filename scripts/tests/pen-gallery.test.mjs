import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { compare, readLibrary, readManifest } from '../check-pen-gallery.mjs';

function fixture(manifest, sheets) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pen-gallery-'));
  fs.mkdirSync(path.join(root, 'web/src/gallery'), { recursive: true });
  fs.mkdirSync(path.join(root, 'design'));
  fs.writeFileSync(path.join(root, 'web/src/gallery/manifest.ts'), `// x\nexport const GALLERY = ${JSON.stringify(manifest, null, 2)} as const;\n`);
  const children = [{ type: 'frame', name: 'System / Foundations', children: [] }, ...Object.entries(sheets).map(([name, states]) => ({
    type: 'frame',
    name: `System / ${name}`,
    children: ['Light', 'Dark'].map(theme => ({ type: 'frame', name: theme, children: states.map(state => ({ type: 'ref', name: state })) })),
  }))];
  fs.writeFileSync(path.join(root, 'design/hide-ui.lib.pen'), JSON.stringify({ version: '2', children, variables: {} }));
  return root;
}

test('a library and a gallery naming the same parts and states agree', () => {
  const root = fixture({ Button: ['Default', 'Disabled'] }, { Button: ['Default', 'Disabled'] });
  assert.deepEqual(compare(readManifest(root), readLibrary(root)), []);
});

test('a part on one side only is refused, naming the side', () => {
  const root = fixture({ Button: ['Default'], Switch: ['On'] }, { Button: ['Default'], Tooltip: ['Open'] });
  const failures = compare(readManifest(root), readLibrary(root));
  assert.ok(failures.some(line => line.includes('`Switch` has no `System / Switch` sheet')));
  assert.ok(failures.some(line => line.includes('`System / Tooltip` has no gallery section')));
});

test('a state drawn in Pen but not rendered, or the reverse, is refused', () => {
  const root = fixture({ Button: ['Default', 'Pending'] }, { Button: ['Default', 'Hover'] });
  const failures = compare(readManifest(root), readLibrary(root));
  assert.ok(failures.some(line => line.includes('does not draw: Pending')));
  assert.ok(failures.some(line => line.includes('states the gallery does not list: Hover')));
});
