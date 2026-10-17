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
  for (const file of ['toolbar', 'tabs', 'rail', 'sidebar', 'panels', 'menu', 'dialog', 'palette', 'siteinfo', 'split']) {
    const text = readFileSync(join(stylesDir, `${file}.css`), 'utf8');
    assert.match(text.slice(0, 600), /Surface: [A-Z ]+.*Owner:/s, file);
  }
});

test('hand-tuned chrome geometry is unchanged', () => {
  const tokens = readFileSync(join(stylesDir, 'tokens.css'), 'utf8');
  for (const [name, value] of [['--bw-toolbar-h', '30px'], ['--bw-pill-h', '26px'], ['--bw-r-tab', '9px'], ['--bw-nav-h', '28px']]) {
    assert.match(tokens, new RegExp(`${name}: ${value};`), name);
  }
});

// --- two-level stacks (T10b) ---
test('the two rows of two-level stacks are the heights the native layout reserves', () => {
  const tokens = readFileSync(join(stylesDir, 'tokens.css'), 'utf8');
  const layout = readFileSync(join(uiRoot, '..', 'src', 'chrome', 'layout.rs'), 'utf8');
  const rust = (name) => Number(new RegExp(`pub const ${name}: i32 = (\\d+);`).exec(layout)?.[1]);
  assert.equal(rust('STRIP_HEIGHT'), 30, 'the toolbar row is still --bw-toolbar-h');
  assert.match(tokens, new RegExp(`--bw-tab-row-h: ${rust('TAB_ROW_HEIGHT')}px;`), 'the tab row: Metrics::TAB_ROW_HEIGHT');
  assert.match(layout, /pub const STACK_ROW_HEIGHT: i32 = Self::TAB_ROW_HEIGHT;/);
  assert.match(tokens, /--bw-stack-row-h: var\(--bw-tab-row-h\);/, 'the second row: Metrics::STACK_ROW_HEIGHT');
  assert.match(tokens, new RegExp(`--bw-stack-row-compact-h: ${rust('STACK_ROW_COMPACT_HEIGHT')}px;`), 'compact: Metrics::STACK_ROW_COMPACT_HEIGHT');
  const tabs = readFileSync(join(stylesDir, 'tabs.css'), 'utf8');
  assert.match(tabs, /body\[data-stack-row\] #app\.strip \{[^}]*height: var\(--bw-tab-row-h\);/, 'with a second row the tab row keeps its own height');
  assert.match(tabs, /\.stack-row\.strip \{[^}]*height: var\(--bw-stack-row-h\);/);
  assert.match(tabs, /\.stack-row\.strip\[data-size="compact"\] \{[^}]*--bw-tab-h: calc\(var\(--bw-stack-row-compact-h\) - 2px\);[^}]*height: var\(--bw-stack-row-compact-h\);/s);
  assert.match(tabs, /\.stack-row\.strip\[hidden\] \{ display: none; \}/);
  // The row's tabs are the strip's own islands (`.strip .tab`), so no rule of its own styles a tab.
  assert.doesNotMatch(tabs, /\.stack-row[^{]*\.tab/, 'no second-row tab styles');
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

test('tab selection is instant and the settle frame still wins', () => {
  const instant = /\.strip \.tab\.active:where\([^{]*\)\{transition-property:transform\}/.exec(css);
  assert.ok(instant, 'active tab only transitions transform');
  assert.match(css, /\.strip \.tab\.active::?before\{transition:none\}/, 'active fill does not fade in');
  const settle = css.indexOf('.strip.settle .tab{transition:none}');
  assert.ok(settle > instant.index, '.strip.settle .tab must come after the active rule');
});

test('panel styles carry no legacy 13px Segoe UI block', () => {
  for (const file of ['panels.css', 'dialog.css', 'rail.css', 'components.css']) {
    const text = readFileSync(join(stylesDir, file), 'utf8');
    assert.doesNotMatch(text, /--bw-font-panel|\.customize-panel|font:\s*13px/, file);
  }
});
