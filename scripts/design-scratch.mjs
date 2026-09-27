#!/usr/bin/env node
// Create a local, library-linked scratch. Never edit or copy the shared library.
import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {CANVAS} from './pen-tokens.mjs';
import {PEN_VERSION, pen, requirePen} from './pen-cli.mjs';

const usage = 'Usage: node scripts/design-scratch.mjs <task-slug>';

function ordinaryFile(file) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.size === 0) throw new Error(`Expected a non-empty ordinary file: ${file}`);
}

function localDirectory(root, parts) {
  let directory = root;
  for (const part of parts) {
    directory = path.join(directory, part);
    try { fs.mkdirSync(directory); }
    catch (error) { if (error.code !== 'EEXIST') throw error; }
    if (!fs.lstatSync(directory).isDirectory()) throw new Error(`Refusing non-directory or symlink: ${directory}`);
  }
  return directory;
}

let temporary;
try {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--help') {
    console.log(`${usage}\nCreates ignored agents/runs/<task-slug>/design/scratch.pen in the current checkout.\nRequires pen ${PEN_VERSION} and an existing Pen login. Never overwrites an existing scratch.`);
  } else {
    const [slug] = args;
    if (args.length !== 1 || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug) || slug.length > 80) throw new Error(usage + '\nUse a lowercase, hyphen-separated task name (at most 80 characters).');
    const root = fs.realpathSync(execFileSync('git', ['rev-parse', '--show-toplevel'], {encoding: 'utf8'}).trim());
    const library = path.join(root, CANVAS);
    ordinaryFile(library);
    if (fs.realpathSync(library) !== library) throw new Error(`Refusing a library linked outside its checkout path: ${library}`);
    const relative = `agents/runs/${slug}/design/scratch.pen`;
    // check-ignore refuses tracked files too. Do not create deliverables here.
    execFileSync('git', ['check-ignore', '--quiet', '--', relative], {cwd: root});
    const target = path.join(root, relative);
    if (fs.existsSync(target)) throw new Error(`Scratch already exists; not overwritten: ${target}`);
    await requirePen(root);
    const directory = localDirectory(root, ['agents', 'runs', slug, 'design']);
    // Same directory preserves the import's relative path on publication.
    temporary = path.join(directory, `.scratch-${randomUUID()}.pen`);
    const output = await pen(['--repo', root, '--out', temporary, '--library', library], {cwd: root, action: 'scratch creation'});
    ordinaryFile(temporary);
    // Atomic publication fails if another creator (or a symlink) won the name.
    fs.linkSync(temporary, target);
    console.log(output.trim());
    console.log(`\nScratch: ${target}\nLibrary: ${library}\nNo product screens were added to the shared library.`);
    const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
    console.log(`\nEdit (one writer, close the desktop document first):\npen interactive --in ${quote(target)} --out ${quote(target)}`);
    console.log(`\nInside the CLI: list_libraries(), get_app_state(), read_skill().\nSave with save(), then exit() before human review.\nReview in Pen: open -a Pen ${quote(target)}`);
  }
} catch (error) {
  console.error(`Scratch creation failed: ${error.message}`);
  process.exitCode = 1;
} finally {
  if (temporary) {
    try { fs.unlinkSync(temporary); }
    catch (error) { if (error.code !== 'ENOENT') { console.error(`Could not remove temporary scratch: ${error.message}`); process.exitCode = 1; } }
  }
}
