// Settings uses the same dialog fit, row overlap and text-cutoff measurements
// as the first-run sheet; its baseline supplies the expected layout rules.
import {measure as measureDialog} from './onboarding-geometry.mjs';

export const measure = page => measureDialog(page, '[data-project-settings]');
