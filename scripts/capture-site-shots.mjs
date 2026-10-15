#!/usr/bin/env node
// Regenerates the introduction site's product shots (site/assets/shots/) from
// the gallery's `landing` scene: the shell's own components over the invented
// story of the page's one-night timeline, never a capture of anyone's app.
// Each shot is photographed in Dark and Light at 2x and written as a lossy
// WebP, so the images can be remade whenever the UI changes:
//
//   node scripts/capture-site-shots.mjs [attention|agents|core ...]
//
// It starts this checkout's web dev server and a headless Chromium, both
// closed on every exit. Encoding needs `cwebp` (libwebp: `brew install webp`,
// `apt install webp`), because Chromium's screenshots are PNG or JPEG only.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createRequire} from 'node:module';
import {execFileSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';

const PIXEL_RATIO = 2;
const THEMES = ['dark', 'light'];
const WEBP_QUALITY = '90';

/**
 * One shot: the window it is photographed in, in CSS pixels, and what to do once the scene is up.
 * `narrow` is the phone variant (`<name>-narrow-<theme>.webp`): the key part of the shot alone, at a width a phone shows
 * without sideways scrolling. A shot that already fits a phone has none.
 */
const SHOTS = {
  attention: {width: 428, height: 740},
  agents: {width: 960, height: 840, ready: openLeadTree, narrow: {width: 420, height: 780, ready: scrollProjectIntoView}},
  core: {width: 1040, height: 640, narrow: {width: 400, height: 660}},
};

const root = fs.realpathSync(execFileSync('git', ['rev-parse', '--show-toplevel'], {encoding: 'utf8'}).trim());
const out = path.join(root, 'site', 'assets', 'shots');

/** The project list scrolls under the raised groups; the agents shot starts at the project whose tree it shows. */
async function scrollProjectIntoView(page) {
  await page.evaluate(() => {
    const row = document.querySelector('[data-project-row="hide"]');
    if (!row) throw new Error('the scene has no project row to scroll to');
    let scroller = row.parentElement;
    while (scroller && !(scroller.scrollHeight > scroller.clientHeight && getComputedStyle(scroller).overflowY === 'auto')) scroller = scroller.parentElement;
    if (!scroller) throw new Error('the project list does not scroll; the window is tall enough to show the whole tree');
    scroller.scrollTop += row.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 28;
  });
}

/** The agents shot also opens the lead's child row onto the tree of who it started, as a click on it does. */
async function openLeadTree(page) {
  await scrollProjectIntoView(page);
  await page.locator('[data-pane-tree]').first().click();
  await page.locator('[role=dialog], [data-radix-popper-content-wrapper]').first().waitFor();
}

function encode(png, webp) {
  try {
    execFileSync('cwebp', ['-quiet', '-q', WEBP_QUALITY, '-m', '6', '-sharp_yuv', png, '-o', webp]);
  } catch (error) {
    if (error.code === 'ENOENT') throw new Error('cwebp is not installed; install libwebp (brew install webp, apt install webp) and run again');
    throw error;
  }
}

async function main(names) {
  for (const name of names) if (!(name in SHOTS)) throw new Error(`Unknown shot ${name}; the shots are ${Object.keys(SHOTS).join(', ')}`);
  const web = path.join(root, 'web');
  if (!fs.existsSync(path.join(web, 'node_modules'))) throw new Error('web dependencies are not installed; run: bash scripts/verify-web.sh install');
  const require = createRequire(path.join(web, 'package.json'));
  const {createServer} = await import(pathToFileURL(require.resolve('vite')).href);
  const {chromium} = require('@playwright/test');

  const cleanup = [];
  const close = async () => { for (const step of cleanup.reverse()) await step(); };
  for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => close().finally(() => process.exit(130)));
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-site-shots-'));
  cleanup.push(() => fs.rmSync(scratch, {recursive: true, force: true}));
  try {
    const server = await createServer({root: web, configFile: path.join(web, 'vite.config.ts'), logLevel: 'error', server: {host: '127.0.0.1', port: 0, strictPort: false, hmr: false}});
    cleanup.push(() => server.close());
    await server.listen();
    const origin = server.resolvedUrls?.local?.[0]?.replace(/\/$/, '');
    if (!origin) throw new Error('The web dev server did not report an address');
    const browser = await chromium.launch();
    cleanup.push(() => browser.close());

    fs.mkdirSync(out, {recursive: true});
    const variants = names.flatMap((name) => [[name, '', SHOTS[name]], ...(SHOTS[name].narrow ? [[name, 'narrow', SHOTS[name].narrow]] : [])]);
    for (const [name, variant, {width, height, ready}] of variants) {
      const label = variant ? `${name}-${variant}` : name;
      for (const theme of THEMES) {
        const context = await browser.newContext({viewport: {width, height}, deviceScaleFactor: PIXEL_RATIO, reducedMotion: 'reduce'});
        const page = await context.newPage();
        const problems = [];
        page.on('pageerror', (error) => problems.push(error.message));
        await page.goto(`${origin}/gallery?${new URLSearchParams({scene: 'landing', shot: name, theme, ...(variant ? {narrow: '1'} : {})})}`);
        await page.locator(`[data-landing-shot="${name}"]`).waitFor();
        await page.addStyleTag({content: '*, *::before, *::after { transition: none !important; animation: none !important; caret-color: transparent !important; }'});
        await page.evaluate(() => document.fonts.ready);
        await page.waitForFunction(() => [...document.images].every((image) => image.complete && image.naturalWidth > 0));
        if (ready) await ready(page);
        await page.mouse.move(0, 0);
        await page.evaluate(() => document.activeElement?.blur());
        // The page's story is English; a Korean string means a fixture row escaped its rewording.
        const text = await page.evaluate(() => document.body.innerText);
        const korean = text.match(/[ㄱ-힝]+/);
        if (korean) throw new Error(`${label} (${theme}) shows Korean text "${korean[0]}"; reword that row in web/src/gallery/landingSceneData.ts`);
        if (problems.length) throw new Error(`${label} (${theme}) raised a page error: ${problems[0]}`);
        const png = path.join(scratch, `${label}-${theme}.png`);
        await page.screenshot({path: png});
        const webp = path.join(out, `${label}-${theme}.webp`);
        encode(png, webp);
        console.log(`${path.relative(root, webp)}  ${width * PIXEL_RATIO}x${height * PIXEL_RATIO}  ${(fs.statSync(webp).size / 1024).toFixed(0)} KB`);
        await context.close();
      }
    }
  } finally {
    await close();
  }
}

main(process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(SHOTS)).catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
