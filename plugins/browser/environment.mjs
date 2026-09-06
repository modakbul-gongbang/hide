import os from 'node:os';
import path from 'node:path';

// Runtime-injected routing values and optional user configuration have one
// enumerable contract. No other module reads process.env.
export const environmentRegistry = Object.freeze({
  HERDR_ENV: { required: false, shape: '1 inside Herdr', note: 'CLI requires an explicit target outside a managed pane.' },
  HERDR_PANE_ID: { required: false, shape: 'opaque pane ID', note: 'Used only when --target-pane is absent.' },
  HERDR_SOCKET_PATH: { required: false, shape: 'absolute path', note: 'Herdr resolves its default socket when absent.' },
  HERDR_PLUGIN_STATE_DIR: { required: false, shape: 'absolute path', note: 'Herdr injects this directory for browser host diagnostics; required in host mode.' },
  HIDE_BROWSER_REQUEST: { required: false, shape: 'JSON', note: 'Required in plugin host mode; absent in CLI mode.' },
  CHROMUX_HOME: { required: false, shape: 'absolute path', fallback: path.join(os.homedir(), '.chromux'), note: 'Uses the same profile root as chromux.' },
});

export function readEnvironment(inherited = process.env) {
  const result = {};
  const errors = [];
  for (const [key, rule] of Object.entries(environmentRegistry)) {
    const value = inherited[key] || rule.fallback;
    if (value && rule.shape === '1 inside Herdr' && value !== '1') errors.push(`${key}: expected 1`);
    if (value && rule.shape === 'opaque pane ID' && !/^w[^:\s]+:p[^:\s]+$/.test(value)) errors.push(`${key}: invalid pane ID`);
    if (value && rule.shape === 'absolute path' && !path.isAbsolute(value)) errors.push(`${key}: expected an absolute path`);
    if (value && rule.shape === 'JSON') {
      try {
        const parsed = JSON.parse(value);
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) errors.push(`${key}: expected a JSON object`);
      } catch { errors.push(`${key}: invalid JSON`); }
    }
    result[key] = value;
  }
  if (errors.length) throw new Error(errors.join('; '));
  return Object.freeze(result);
}
