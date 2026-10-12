// Icon family: generated outputs are current, every alias and rail glyph resolves to a drawn
// icon, and the shape data stays inside the family rules.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = join(here, '..');

async function load(entry) {
  const out = esbuild.buildSync({
    entryPoints: [join(uiRoot, 'src', entry)],
    bundle: true,
    format: 'esm',
    platform: 'neutral',
    write: false,
    logLevel: 'silent',
  });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}

const gen = await load('icons.gen.ts');
const rail = await load('rail-icons.ts');
const railLogic = await load('rail-logic.ts');

test('icons.gen.ts and icons/preview.html match the SVG sources', () => {
  const run = spawnSync(process.execPath, [join(uiRoot, 'scripts', 'gen-icons.mjs'), '--check'], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
});

test('every alias points at a drawn icon and no alias shadows one', () => {
  for (const [alias, target] of Object.entries(gen.ICON_ALIASES)) {
    assert.ok(gen.ICON_SHAPES[target], `${alias} -> ${target}`);
    assert.equal(gen.ICON_SHAPES[alias], undefined, alias);
  }
});

test('the names the existing surfaces use all exist', () => {
  const used = ['back', 'forward', 'reload', 'stop', 'menu', 'split', 'plus', 'lock', 'unlock', 'warning', 'info',
    'close', 'person', 'grid', 'key', 'shield', 'gear', 'check', 'mic', 'camera', 'location', 'bell', 'cookie', 'reset'];
  for (const name of used) assert.ok(gen.ICON_NAMES.includes(name), name);
});

test('every rail wire glyph maps to a family icon', () => {
  const wire = [...railLogic.RAIL_ICON_NAMES.filter((n) => n !== 'letter'), ...railLogic.RAIL_GLYPHS.map((g) => `g:${g}`)];
  for (const name of wire) {
    const target = rail.RAIL_ICON[name];
    assert.ok(target, `no family icon for rail glyph ${name}`);
    assert.ok(gen.ICON_NAMES.includes(target), `${name} -> ${target}`);
  }
});

test('shapes carry geometry only: no per-shape stroke weight, style or transform', () => {
  const allowed = new Set(['d', 'cx', 'cy', 'r', 'rx', 'ry', 'x', 'y', 'width', 'height', 'points', 'x1', 'y1', 'x2', 'y2', 'fill', 'stroke']);
  for (const [name, shapes] of Object.entries(gen.ICON_SHAPES)) {
    assert.ok(shapes.length > 0, name);
    for (const [, attrs] of shapes) {
      for (const key of Object.keys(attrs)) assert.ok(allowed.has(key), `${name}: ${key}`);
      if (attrs.stroke !== undefined) assert.equal(attrs.stroke, 'none', name);
    }
  }
});

test('the requested glyphs for upcoming surfaces exist', () => {
  const wanted = ['bookmark', 'star', 'history', 'clock', 'download', 'notes', 'split-view', 'sidebar-toggle', 'workspace',
    'profile', 'more-horizontal', 'chevron-up', 'chevron-down', 'chevron-left', 'chevron-right', 'external', 'copy', 'pin',
    'mute', 'volume', 'refresh', 'stop', 'home', 'settings', 'palette', 'command', 'folder', 'trash', 'edit', 'pencil',
    'eye', 'puzzle', 'tab-group', 'picture-in-picture', 'zoom-in', 'zoom-out', 'print', 'find'];
  for (const name of wanted) assert.ok(gen.ICON_NAMES.includes(name), name);
});
