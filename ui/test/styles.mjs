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

test('maximised with the spacing removed, grouped tabs reach the top edge as other tabs do (U6)', () => {
  const tabs = readFileSync(join(stylesDir, 'tabs.css'), 'utf8');
  const rule = (selector) => new RegExp(`(?:^|\\n)${selector.replace(/[.*[\]()]/g, '\\$&')} \\{([^}]*)\\}`).exec(tabs)?.[1] ?? '';
  assert.match(rule('.strip.flush .tab'), /height: auto;/);
  assert.match(rule('.strip .group'), /height: var\(--bw-tab-h\);/, 'a group is otherwise a box of the tab height');
  const flushGroup = rule('.strip.flush .group');
  assert.match(flushGroup, /height: auto;/, 'flush, the group grows with the row');
  assert.match(flushGroup, /align-items: stretch;/, 'and its tabs fill it');
  const built = /\.strip\.flush \.group\{([^}]*)\}/.exec(css)?.[1] ?? '';
  assert.match(built, /height:auto/, 'the built sheet has it');
  assert.match(built, /align-items:stretch/, 'the built sheet has it');
});

// ---- U11: what wins the cascade on a picked or stack-target tab ----

/** CSS specificity [ids, classes, types] of one complex selector (:is/:not/:has count their heaviest argument, :where nothing). */
function specificity(selector) {
  let [a, b, c] = [0, 0, 0];
  const nested = /:(is|not|has|where)\(((?:[^()]|\([^()]*\))*)\)/g;
  const rest = selector.replace(nested, (_, name, inner) => {
    if (name === 'where') return '';
    const best = inner.split(',').map((one) => specificity(one.trim())).sort(compare).at(-1);
    a += best[0]; b += best[1]; c += best[2];
    return '';
  });
  a += (rest.match(/#[\w-]+/g) ?? []).length;
  b += (rest.match(/\.[\w-]+|\[[^\]]*\]|(?<!:):[\w-]+/g) ?? []).length;
  c += (rest.match(/::[\w-]+/g) ?? []).length + (rest.match(/(?:^|[\s>+~])[a-z][\w-]*/gi) ?? []).length;
  return [a, b, c];
}
const compare = (x, y) => x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
/** Every selector in `file` (comments dropped) that sets one of `props`. */
function selectorsSetting(file, props) {
  const text = readFileSync(join(stylesDir, file), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  const found = [];
  for (const m of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (!props.some((p) => new RegExp(`(?:^|;|\\s)${p}\\s*:`).test(m[2]))) continue;
    for (const one of m[1].split(/,(?![^(]*\))/)) found.push(one.trim());
  }
  return found;
}

test('specificity helper agrees with the CSS rules it is used for', () => {
  assert.deepEqual(specificity(':root[data-scheme="light"] .strip .tab::before'), [0, 4, 1]);
  assert.deepEqual(specificity('.strip .tab[data-pair="shown"]:not(.active)::before'), [0, 4, 1]);
  assert.deepEqual(specificity('.side-tab[data-private="true"]:not(:focus-visible, .dragging)'), [0, 3, 0]);
  assert.deepEqual(specificity('.strip:not(#x) .tab.picked::before'), [1, 3, 1]);
  assert.deepEqual(specificity('.strip .tab.active:where(:not(.landing))'), [0, 3, 0]);
});

test('a picked or stack-target tab shows its tint and ring over every wash, in both schemes and on private tabs (U11)', () => {
  // Strip: the islands are painted by ::before. Every other rule that paints a tab's ::before.
  const strip = [...selectorsSetting('tabs.css', ['background', 'box-shadow']), ...selectorsSetting('split.css', ['background', 'box-shadow'])]
    .filter((s) => /\.tab\b[^,]*::before$/.test(s));
  const marks = (cls) => strip.filter((s) => s.includes(`.${cls}`));
  const others = strip.filter((s) => !s.includes('.picked') && !s.includes('.stack-target'));
  assert.ok(others.includes(':root[data-scheme="light"] .strip .tab::before'), 'the light wash is among the rivals');
  assert.ok(others.some((s) => s.includes('[data-pair="shown"]')), 'and a shown pair half');
  for (const cls of ['picked', 'stack-target']) {
    const own = marks(cls).filter((s) => !s.includes('[data-private'));
    assert.ok(own.length > 0, cls);
    const best = own.map(specificity).sort(compare).at(-1); // a list applies its heaviest matching selector
    for (const rival of others) assert.ok(compare(best, specificity(rival)) > 0, `.${cls} loses to ${rival}`);
  }
  // Sidebar: the rows paint themselves.
  // Rules on the row itself (its last compound is .side-tab), leaving out hover, the focus ring (the
  // picked ring yields to it on purpose) and a held row.
  const lastCompound = (s) => s.replace(/\([^()]*\)/g, '()').split(/\s+/).at(-1);
  const side = selectorsSetting('sidebar.css', ['background', 'background-color', 'box-shadow'])
    .filter((s) => lastCompound(s).startsWith('.side-tab') && !/:hover|:focus-visible$|\.dragging$/.test(lastCompound(s)) && !/\.side-tab:focus-visible|\.dragging$/.test(s));
  const pickedFill = selectorsSetting('sidebar.css', ['background']).filter((s) => /\.side-tab\.picked$/.test(s));
  const pickedRing = selectorsSetting('sidebar.css', ['box-shadow']).filter((s) => s.includes('.side-tab.picked') && !s.includes('[data-private'));
  assert.ok(pickedFill.length > 0 && pickedRing.length > 0);
  const rivals = side.filter((s) => !s.includes('.picked'));
  assert.ok(rivals.includes('[data-scheme="light"] .side-tab:not(.is-active)'));
  assert.ok(rivals.includes('.side-tab[data-private="true"]:not(:focus-visible, .dragging)'));
  for (const rival of rivals) {
    for (const own of [...pickedFill, ...pickedRing]) assert.ok(compare(specificity(own), specificity(rival)) > 0, `${own} loses to ${rival}`);
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

// ---- font_size x ui_zoom and ui_font reach the HTML chrome (the type tokens scale) ----

test('every type token scales with --bw-font-scale and is its design px size at scale 1', () => {
  const tokens = readFileSync(join(stylesDir, 'tokens.css'), 'utf8');
  assert.match(tokens, /--bw-font-scale: 1;/);
  const sizes = [...tokens.matchAll(/(--bw-(?:text|leading)-(?:chrome|caption|body|omni|subtitle|display)):\s*([^;]+);/g)];
  assert.equal(sizes.length, 10, 'chrome, caption, body, omni, subtitle and display sizes with their leadings');
  for (const [, name, value] of sizes) assert.match(value, /^calc\(\d+px \* var\(--bw-font-scale\)\)$/, name);
  assert.match(tokens, /--bw-font: var\(--bw-font-stock\);/, 'ui_font overrides --bw-font; the stock stack stays reachable');
});

test('a font-size is a type token or scales with --bw-font-scale, except glyphs and badges in fixed boxes', () => {
  // These size a monogram, emoji or count inside a box of fixed px size, so scaling the text would
  // overflow the box. Everything else must follow font_size.
  const FIXED = new Map([
    ['address.css', ['.omni-search-engine', '.omni-search-engine .tb-tile']],
    ['hover.css', ['.bw-card-badge']],
    ['menu.css', ['.html-menu-badge']],
    ['panels.css', ['.pn-favicon']],
    ['rail.css', ['.rb-tile', '.rb-emoji', '.rb-count']],
    ['settings.css', ['.sp-badge', '.sp-avatar']],
    ['sidebar.css', ['.side-workspace-emoji']],
    ['start.css', ['.sp-letter']],
  ]);
  const seen = new Set();
  for (const name of readdirSync(stylesDir).filter((n) => n.endsWith('.css'))) {
    const text = readFileSync(join(stylesDir, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
    for (const m of text.matchAll(/([^{}]*)\{([^{}]*)\}/g)) {
      const selector = m[1].trim();
      for (const d of m[2].matchAll(/(?:^|;)\s*font-size:\s*([^;]+)/g)) {
        const value = d[1].trim();
        if (/var\(--bw-text-|var\(--bw-font-scale\)|em$|^0$|^inherit$|^clamp\(/.test(value)) continue;
        const allowed = (FIXED.get(name) ?? []).find((s) => selector.endsWith(s));
        assert.ok(allowed, `${name}: ${selector} { font-size: ${value} } does not follow font_size`);
        seen.add(`${name} ${allowed}`);
      }
    }
  }
  const listed = [...FIXED].flatMap(([file, sels]) => sels.map((s) => `${file} ${s}`));
  for (const entry of listed) assert.ok(seen.has(entry), `${entry} is listed but has no fixed size any more: drop it`);
});

test('applyTheme turns fontScale into --bw-font-scale and ui_font into a quoted --bw-font stack', async () => {
  const esbuild = await import('esbuild');
  const out = esbuild.buildSync({ entryPoints: [join(uiRoot, 'src', 'font-logic.ts')], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  const logic = await import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
  assert.equal(logic.fontScaleValue(100), '1');
  assert.equal(logic.fontScaleValue(125), '1.25');
  assert.equal(logic.uiFontStack(''), null, 'no face: the stock stack');
  assert.equal(logic.uiFontStack('  , '), null);
  assert.equal(logic.uiFontStack('Comic Sans MS'), '"Comic Sans MS", var(--bw-font-stock)');
  assert.equal(logic.uiFontStack('"Fira Code", Consolas, monospace'), '"Fira Code", "Consolas", monospace, var(--bw-font-stock)');
  assert.equal(logic.uiFontStack(String.raw`a"b\c`), String.raw`"a\"b\\c", var(--bw-font-stock)`, 'quotes and backslashes cannot end the string');
  const view = readFileSync(join(uiRoot, 'src', 'view.ts'), 'utf8');
  assert.doesNotMatch(view, /root\.style\.fontSize|root\.style\.fontFamily/, 'the old html font-size/family had no effect on the px tokens');
});
