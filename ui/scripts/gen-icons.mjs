// Generates the single Bowser icon family from the SVG sources:
//   icons/menu/*.svg   native menu set (also drawn by src/win32/menu.rs)
//   icons/rail/*.svg   rail set (also ported to src/chrome/rail_icons.rs)
//   icons/extra/*.svg  everything else the HTML chrome needs
// Outputs ui/src/icons.gen.ts (shape data, consumed by ui/src/icons.ts) and icons/preview.html
// (gallery at 16/20/24px, 1x and 2x). Run: npm run icons (also part of npm run build).
//
// Every source must be on the 24-unit grid with the family stroke (1.5, round caps and joins,
// fill none); the script fails on anything else, and on two names drawing identical shapes
// unless the pair is declared in ALIASES or DROPPED below.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const SOURCES = ['menu', 'rail', 'extra']; // later sources override earlier ones only via DROPPED

/** Source files whose name is not the family name. */
const RENAMED = {
  'menu/main-menu': 'menu',
  'rail/dots': 'more-vertical',
};

/** Source files left out of the family, with the reason (shown in the audit and STYLEGUIDE.md). */
const DROPPED = {
  'menu/new-tab': 'identical to menu/plus',
  'menu/tab-close': 'identical to menu/close',
  'menu/caption-close': 'identical to menu/close',
  'menu/reset': 'identical to menu/reload; extra/reset is the counter-clockwise variant',
  'menu/tab-menu': 'a 14x7 chevron; the family chevron-down (extra) is the one chevron shape',
  'rail/plus': 'a 15-unit plus; menu/plus (16 units) is the family plus',
};

/** Extra names for existing glyphs: semantic names other agents may reach for. */
const ALIASES = {
  stop: 'close',
  'tab-close': 'close',
  'caption-close': 'close',
  'new-tab': 'plus',
  refresh: 'reload',
  settings: 'gear',
  profile: 'person',
  history: 'clock',
  find: 'search',
  star: 'workspace-star',
  'split-view': 'split',
  'sidebar-toggle': 'sidebar',
  more: 'more-horizontal',
  dots: 'more-vertical',
  palette: 'command',
  extensions: 'puzzle',
  'picture-in-picture': 'pip',
  pencil: 'edit',
  'arrow-left': 'back',
  'arrow-right': 'forward',
};

const ROOT_ATTRS = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  'stroke-width': '1.5',
  'stroke-linecap': 'round',
  'stroke-linejoin': 'round',
};
const SHAPES = new Set(['path', 'circle', 'rect', 'ellipse', 'polygon', 'polyline', 'line']);
// Only these child attributes are kept; anything else (style, class, transform) is an error.
const CHILD_ATTRS = new Set(['d', 'cx', 'cy', 'r', 'rx', 'ry', 'x', 'y', 'width', 'height', 'points', 'x1', 'y1', 'x2', 'y2', 'fill', 'stroke']);

const errors = [];
const attrsOf = (text) => Object.fromEntries([...text.matchAll(/([\w:-]+)="([^"]*)"/g)].map((m) => [m[1], m[2]]));
const normaliseNumbers = (value) => value.replace(/\s+/g, ' ').replace(/\s*([A-Za-z])\s*/g, '$1').trim();

function parse(file, key) {
  const text = readFileSync(file, 'utf8');
  const root = /<svg\b([^>]*)>/.exec(text);
  if (!root) return errors.push(`${key}: no <svg> root`), [];
  const rootAttrs = attrsOf(root[1]);
  for (const [name, value] of Object.entries(ROOT_ATTRS)) {
    if (rootAttrs[name] !== value) errors.push(`${key}: root ${name}="${rootAttrs[name]}" (family is "${value}")`);
  }
  const shapes = [];
  for (const m of text.matchAll(/<(\w+)\b([^>]*?)\/?>/g)) {
    const tag = m[1];
    if (tag === 'svg') continue;
    if (!SHAPES.has(tag)) {
      errors.push(`${key}: unsupported element <${tag}>`);
      continue;
    }
    const attrs = attrsOf(m[2]);
    for (const name of Object.keys(attrs)) {
      if (!CHILD_ATTRS.has(name)) errors.push(`${key}: <${tag}> attribute ${name} is not allowed (stroke weight and caps come from the family)`);
    }
    if (attrs.fill && attrs.fill !== 'currentColor' && attrs.fill !== 'none') errors.push(`${key}: fill must be currentColor or none`);
    if (attrs.stroke && attrs.stroke !== 'none') errors.push(`${key}: child stroke may only be "none" (filled dots)`);
    if (attrs.d) attrs.d = normaliseNumbers(attrs.d);
    if (attrs.points) attrs.points = attrs.points.replace(/\s+/g, ' ').trim();
    shapes.push([tag, Object.fromEntries(Object.entries(attrs).sort(([a], [b]) => (a < b ? -1 : 1)))]);
  }
  if (shapes.length === 0) errors.push(`${key}: no shapes`);
  return shapes;
}

const family = new Map(); // name -> { source, shapes }
const audit = [];
for (const dir of SOURCES) {
  const files = readdirSync(join(repo, 'icons', dir)).filter((f) => f.endsWith('.svg')).sort();
  for (const file of files) {
    const key = `${dir}/${basename(file, '.svg')}`;
    const shapes = parse(join(repo, 'icons', dir, file), key);
    if (DROPPED[key]) {
      audit.push(`dropped ${key}: ${DROPPED[key]}`);
      continue;
    }
    const name = RENAMED[key] ?? basename(file, '.svg');
    if (family.has(name)) {
      errors.push(`${key}: name "${name}" already comes from ${family.get(name).source}; drop or rename one`);
      continue;
    }
    if (RENAMED[key]) audit.push(`renamed ${key} -> ${name}`);
    family.set(name, { source: key, shapes });
  }
}

// Identical drawings under two names must be declared as an alias instead.
const seen = new Map();
for (const [name, { shapes }] of family) {
  const sig = JSON.stringify(shapes);
  if (seen.has(sig)) errors.push(`"${name}" draws exactly the same shapes as "${seen.get(sig)}"; make one an alias`);
  else seen.set(sig, name);
}
for (const [alias, target] of Object.entries(ALIASES)) {
  if (!family.has(target)) errors.push(`alias ${alias} -> missing icon ${target}`);
  if (family.has(alias)) errors.push(`alias ${alias} shadows a real icon`);
}

if (errors.length) {
  for (const e of errors) console.error(`gen-icons: ${e}`);
  process.exit(1);
}

const names = [...family.keys()].sort();
const aliasNames = Object.keys(ALIASES).sort();

// ---- ui/src/icons.gen.ts ----
const ts = [
  '// GENERATED by ui/scripts/gen-icons.mjs from icons/{menu,rail,extra}/*.svg. Do not edit; run',
  '// `npm run icons`. Shapes are on the 24-unit grid; ui/src/icons.ts turns them into SVG nodes.',
  '',
  "export type IconShape = readonly [tag: 'path' | 'circle' | 'rect' | 'ellipse' | 'polygon' | 'polyline' | 'line', attrs: Readonly<Record<string, string>>];",
  '',
  'const SHAPES = {',
  ...names.map((name) => `  ${JSON.stringify(name)}: ${JSON.stringify(family.get(name).shapes)},`),
  '} as const satisfies Record<string, readonly IconShape[]>;',
  '',
  '/** Semantic aliases: each maps to a drawn icon above. */',
  'const ALIASES = {',
  ...aliasNames.map((alias) => `  ${JSON.stringify(alias)}: ${JSON.stringify(ALIASES[alias])},`),
  '} as const satisfies Record<string, keyof typeof SHAPES>;',
  '',
  'export type DrawnIconName = keyof typeof SHAPES;',
  'export type IconName = DrawnIconName | keyof typeof ALIASES;',
  '',
  'export const ICON_SHAPES: Readonly<Record<DrawnIconName, readonly IconShape[]>> = SHAPES;',
  'export const ICON_ALIASES: Readonly<Record<keyof typeof ALIASES, DrawnIconName>> = ALIASES;',
  `export const ICON_NAMES: readonly IconName[] = ${JSON.stringify([...names, ...aliasNames].sort())};`,
  '',
].join('\n');

// ---- icons/preview.html ----
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');
const svg = (name, size) => {
  const body = family
    .get(name)
    .shapes.map(([tag, attrs]) => `<${tag}${Object.entries(attrs).map(([k, v]) => ` ${k}="${esc(v)}"`).join('')}/>`)
    .join('');
  return `<svg class="i" viewBox="0 0 24 24" width="${size}" height="${size}" aria-hidden="true">${body}</svg>`;
};
const aliasesOf = (name) => aliasNames.filter((a) => ALIASES[a] === name);
const cells = names
  .map((name) => {
    const sources = family.get(name).source;
    const also = aliasesOf(name);
    return `<figure title="${esc(sources)}">${[16, 20, 24].map((s) => svg(name, s)).join('')}<figcaption>${esc(name)}${
      also.length ? `<small>${esc(also.join(', '))}</small>` : ''
    }</figcaption></figure>`;
  })
  .join('\n');
const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Bowser icon family</title>
<!-- GENERATED by ui/scripts/gen-icons.mjs. Open from disk; no scripts. -->
<style>
  body { margin: 24px; font: 12px/16px "Segoe UI Variable", "Segoe UI", sans-serif; background: #f4f6f8; color: #14181d; }
  body.dark { background: #20242c; color: #fff; }
  h1 { font-size: 20px; line-height: 28px; font-weight: 600; margin: 0 0 4px; }
  h2 { font-size: 14px; font-weight: 600; margin: 24px 0 8px; }
  p { margin: 0 0 8px; max-width: 72ch; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(132px, 1fr)); gap: 4px; }
  figure { margin: 0; padding: 8px; border-radius: 8px; display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  figure:hover { background: rgba(127, 127, 127, .14); }
  figcaption { flex-basis: 100%; }
  small { display: block; opacity: .62; }
  .i { fill: none; stroke: currentColor; stroke-width: 1.5px; stroke-linecap: round; stroke-linejoin: round; }
  .i > * { vector-effect: non-scaling-stroke; }
  .x2 { zoom: 2; }
  .row { display: flex; gap: 0; }
  .row > section { flex: 1; padding: 16px; }
</style>
</head>
<body>
<h1>Bowser icon family (${names.length} glyphs, ${aliasNames.length} aliases)</h1>
<p>24-unit grid, 1.5px rendered stroke (vector-effect: non-scaling-stroke), round caps and joins, currentColor.
Each cell shows the glyph at 16, 20 and 24px. Hover a cell for its source file. Aliases are listed under the name.</p>
<div class="row">
<section><h2>Light, 1x</h2><div class="grid">
${cells}
</div></section>
<section class="dark-wrap" style="background:#20242c;color:#fff;border-radius:8px"><h2>Dark, 1x</h2><div class="grid">
${cells}
</div></section>
</div>
<h2>2x (zoom: 2, equivalent to 200% display scaling)</h2>
<div class="x2"><div class="grid">
${cells}
</div></div>
<h2>Audit</h2>
<ul>
${audit.map((line) => `<li>${esc(line)}</li>`).join('\n')}
</ul>
</body>
</html>
`;
const outputs = [
  [join(repo, 'ui', 'src', 'icons.gen.ts'), ts],
  [join(repo, 'icons', 'preview.html'), html],
];
if (process.argv.includes('--check')) {
  // Used by test/icons.mjs: the committed outputs must match the sources.
  const stale = outputs.filter(([file, text]) => {
    try {
      return readFileSync(file, 'utf8').replace(/\r\n/g, '\n') !== text;
    } catch {
      return true;
    }
  });
  for (const [file] of stale) console.error(`gen-icons: ${file} is stale; run npm run icons`);
  process.exit(stale.length ? 1 : 0);
}
for (const [file, text] of outputs) writeFileSync(file, text);

console.log(`gen-icons: ${names.length} glyphs, ${aliasNames.length} aliases`);
for (const line of audit) console.log(`  ${line}`);
