#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { compile } from 'json-schema-to-typescript';

// The generator emits only what the root union reaches, and the root is the
// frames that cross the socket. A snapshot section the contract declares is a
// $def no frame references, so each one the web reads is named here and
// emitted beside the root, with the $defs it refers to.
const SNAPSHOT_DEFS = ['providerUsage', 'uiStateUsageHints'];

const root = path.resolve(process.cwd(), '..');
const schema = JSON.parse(fs.readFileSync(path.join(root, 'contracts/hided-ws.schema.json'), 'utf8'));
let ts = await compile(schema, 'HidedWs', { bannerComment: '/* Generated from contracts/hided-ws.schema.json. */' });
for (const name of SNAPSHOT_DEFS) {
  ts += await compile({ ...schema.$defs[name], $defs: schema.$defs }, name, { bannerComment: '' });
}
const out = path.join(process.cwd(), 'src/generated');
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'hided-ws.ts'), ts);
console.log('wrote src/generated/hided-ws.ts');
