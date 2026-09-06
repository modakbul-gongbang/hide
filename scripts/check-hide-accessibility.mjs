import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn, execFileSync, spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';

const app = path.resolve(process.argv[2]);
const executable = path.join(app, 'Contents/MacOS/HerdrMacOS');
const existing = spawnSync('pgrep', ['-f', executable], {encoding: 'utf8'});
assert.equal(existing.status, 1, 'Close only your own previous dev instance before this check');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-accessibility-'));
const env = {...process.env, HERDR_SOCKET_PATH: path.join(root, 'fixture.sock'), XDG_CONFIG_HOME: path.join(root, 'config'), XDG_STATE_HOME: path.join(root, 'state')};
for (const key of ['SSH_AUTH_SOCK', 'HERDR_SESSION', 'HERDR_WORKSPACE_ID', 'HERDR_TAB_ID', 'HERDR_PANE_ID']) delete env[key];
const child = spawn(executable, ['--verification-background', '--verification-ui-fixture', '--workspace-root', root, '--state-path', path.join(root, 'ui.json')], {env, stdio: ['ignore', 'inherit', 'inherit']});
try {
    // The built-in UI fixture bypasses live transport. No user server is read.
    const expected = ['New agent (⌘N)', 'New project (⇧⌘N)', 'Search (⌘K)', 'Hide left sidebar (⌘B)', 'Settings'];
    const deadline = Date.now() + 45_000;
    let observation = {controls: []};
    let missing = expected;
    do {
        await new Promise(resolve => setTimeout(resolve, 500));
        assert.equal(execFileSync('pgrep', ['-f', executable], {encoding: 'utf8'}).trim(), String(child.pid));
        try {
            observation = JSON.parse(execFileSync('/tmp/herdr-ide-verify/hide-accessibility-probe', [String(child.pid)], {encoding: 'utf8'}));
        } catch (error) {
            observation = {
                controls: [],
                probeError: error instanceof Error ? error.message : String(error),
                stdout: error?.stdout?.toString() ?? '',
                stderr: error?.stderr?.toString() ?? '',
            };
        }
        const controls = Array.isArray(observation.controls) ? observation.controls : [];
        missing = expected.filter(help => !controls.some(control => control.help === help));
    } while (missing.length > 0 && Date.now() < deadline);

    if (missing.length > 0) {
        console.error('Last AX observation before the 45 second deadline:');
        console.error(JSON.stringify(observation, null, 2));
        assert.fail(`Missing AXHelp after polling: ${missing.join(', ')}`);
    }
    console.log(JSON.stringify(observation, null, 2));
    console.log('PASS: native AXHelp exposes registry shortcuts and chordless labels; exhaustive registry equality is covered by HideTooltipTests.');
} finally {
    if (child.exitCode === null) {
        child.kill('SIGTERM');
        await new Promise(resolve => child.once('exit', resolve));
    }
    fs.rmSync(root, {recursive: true});
}
