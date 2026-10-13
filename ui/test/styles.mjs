// Stylesheet contract: dist/app.css is static and CSP-safe, every --bw-* token a rule reads is
// defined, surfaces read tokens rather than the raw host inputs, and the build is reproducible.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = join(here, '..');
const stylesDir = join(uiRoot, 'src', 'styles');
const distCss = join(uiRoot, 'dist', 'app.css');
const css = existsSync(distCss) ? readFileSync(distCss, 'utf8') : '';

test('dist/app.css exists (run npm run build first)', () => {
  assert.ok(css.length > 0);
});

test('dist/app.css loads nothing: no @import, no url() except data:, no legacy expressions', () => {
  assert.doesNotMatch(css, /@import/);
  for (const m of css.matchAll(/url\(\s*(['"]?)([^'")]*)/g)) assert.match(m[2], /^data:/, m[0]);
  const legacy = /expression\(|javascript:|(?<![-\w])behavior:/i.exec(css);
  assert.equal(legacy, null, legacy && `found ${legacy[0]}`);
});

test('cascade layers keep their order: theme, base, components, surfaces, utilities', () => {
  const order = [];
  for (const m of css.matchAll(/@layer ([\w, ]+)[{;]/g)) {
    for (const name of m[1].split(',').map((s) => s.trim())) if (!order.includes(name)) order.push(name);
  }
  assert.deepEqual(order, ['theme', 'base', 'components', 'surfaces', 'utilities']);
});

test('every --bw-* token read in dist/app.css is defined there', () => {
  const defined = new Set([...css.matchAll(/(--bw-[\w-]+)\s*:/g)].map((m) => m[1]));
  // Optional per-element knobs with fallbacks (var(--x, fallback)) are allowed to be unset.
  const optional = new Set(['--bw-icon-size', '--bw-enter-y', '--bw-enter-scale']);
  for (const m of css.matchAll(/var\((--bw-[\w-]+)/g)) {
    if (!optional.has(m[1])) assert.ok(defined.has(m[1]), `${m[1]} is used but never defined`);
  }
});

test('surface and component files read --bw-* tokens, not the raw host inputs', () => {
  const host = /var\(--(bg|fill|border|focus-border|text|placeholder|completion|selection|hover|progress)\)/;
  for (const file of readdirSync(stylesDir)) {
    if (file === 'tokens.css') continue;
    const text = readFileSync(join(stylesDir, file), 'utf8');
    text.split(/\r?\n/).forEach((line, i) => {
      assert.doesNotMatch(line, host, `${file}:${i + 1} reads a host input; use the --bw-* token`);
    });
  }
});

test('every surface file names its owner in a header comment', () => {
  for (const file of ['toolbar', 'tabs', 'rail', 'sidebar', 'panels', 'menu', 'dialog', 'palette', 'siteinfo']) {
    const text = readFileSync(join(stylesDir, `${file}.css`), 'utf8');
    assert.match(text.slice(0, 600), /Surface: [A-Z ]+.*Owner:/s, file);
  }
});

test('hand-tuned chrome geometry is unchanged', () => {
  const tokens = readFileSync(join(stylesDir, 'tokens.css'), 'utf8');
  for (const [name, value] of [['--bw-toolbar-h', '34px'], ['--bw-pill-h', '26px'], ['--bw-r-tab', '9px'], ['--bw-nav-h', '28px']]) {
    assert.match(tokens, new RegExp(`${name}: ${value};`), name);
  }
});

test('toggles: on-state is accent with an on-accent knob; every interactive control has a focus ring', () => {
  const source = readFileSync(join(stylesDir, 'components.css'), 'utf8');
  const on = /\.bw-toggle\[aria-checked="true"\]\s*\{([^}]*)\}/.exec(source);
  assert.ok(on && /background:\s*var\(--bw-accent\)/.test(on[1]), 'toggle on-state track uses --bw-accent');
  const knob = /\.bw-toggle\[aria-checked="true"\]::after\s*\{([^}]*)\}/.exec(source);
  assert.ok(knob && /background:\s*var\(--bw-on-accent\)/.test(knob[1]), 'toggle on-state knob uses --bw-on-accent');
  assert.match(source, /:where\([^)]*\[role="switch"\][^)]*\):focus-visible/s, 'zero-specificity focus-visible fallback for interactive roles');
  assert.match(source, /\.bw-select-button,[\s\S]*?\):focus-visible/, 'select button is in the shared focus ring list');
});

test('panel styles carry no legacy 13px Segoe UI block', () => {
  for (const file of ['panels.css', 'dialog.css', 'rail.css', 'components.css']) {
    const text = readFileSync(join(stylesDir, file), 'utf8');
    assert.doesNotMatch(text, /--bw-font-panel|\.customize-panel|font:\s*13px/, file);
  }
});
