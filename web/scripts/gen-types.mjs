#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { compile } from 'json-schema-to-typescript';

const root = path.resolve(process.cwd(), '..');
const schema = JSON.parse(fs.readFileSync(path.join(root, 'contracts/hided-ws.schema.json'), 'utf8'));
const ts = await compile(schema, 'HidedWs', { bannerComment: '/* Generated from contracts/hided-ws.schema.json. */' });
const out = path.join(process.cwd(), 'src/generated');
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'hided-ws.ts'), ts);
console.log('wrote src/generated/hided-ws.ts');
