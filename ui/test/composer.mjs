// Tests for the anchored composers (New workspace, Add web app, Edit web app): the page payload and
// local checks (src/composer-logic.ts), the senders in protocol.ts, and the markup, style and
// wiring contracts of composer.ts, popup.ts, menu.css and the Customize panel.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const read = (...parts) => readFileSync(join(here, '..', ...parts), 'utf8');
const { code } = esbuild.transformSync(read('src', 'composer-logic.ts'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const bundled = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', 'protocol.ts')], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
const protocol = await import(`data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].contents).toString('base64')}`);

// The strings Rust writes (src/win32/composer.rs `the_page_carries_values_the_error_under_its_field_and_done`).
const workspace = { v: 1, t: 'composer', tok: 4, kind: 'composer:workspace-new', mode: 'workspace-new', max: 40, name: 'Workspace 3' };
const addApp = { v: 1, t: 'composer', tok: 5, kind: 'composer:app-add', mode: 'app-add', max: 40, name: '', url: '' };
const editApp = { v: 1, t: 'composer', tok: 6, kind: 'composer:app-edit:9', mode: 'app-edit', max: 40, name: 'Mail', url: 'https://mail.example/' };

test('composer payloads: the three modes, an error under its field, and done', () => {
  for (const msg of [workspace, addApp, editApp]) assert.equal(logic.validComposerPayload(msg), true, msg.kind);
  assert.equal(logic.validComposerPayload({ ...addApp, error: { field: 'url', text: 'Enter a web address.' } }), true);
  assert.equal(logic.validComposerPayload({ ...editApp, done: true }), true);
  assert.equal(logic.validComposerPayload({ ...workspace, done: true }), true);
});

test('composer payloads reject unknown keys, modes, fields and shapes', () => {
  assert.equal(logic.validComposerPayload({ ...workspace, extra: 1 }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, mode: 'delete' }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, t: 'menu' }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, kind: 'menu' }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, tok: -1 }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, max: 41 }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, url: 'https://x.test/' }), false, 'a workspace has no address');
  assert.equal(logic.validComposerPayload({ ...addApp, url: undefined }), false, 'a web app has one');
  assert.equal(logic.validComposerPayload({ ...workspace, done: false }), false);
  assert.equal(logic.validComposerPayload({ ...workspace, error: { field: 'url', text: 'x' } }), false, 'no address field to flag');
  assert.equal(logic.validComposerPayload({ ...addApp, error: { field: 'path', text: 'x' } }), false);
  assert.equal(logic.validComposerPayload({ ...addApp, error: { field: 'name', text: '' } }), false);
  assert.equal(logic.validComposerPayload({ ...addApp, error: { field: 'name', text: 'x'.repeat(129) } }), false);
  assert.equal(logic.validComposerPayload({ ...addApp, error: { field: 'name', text: 'x', more: 1 } }), false);
  assert.equal(logic.validComposerPayload(null), false);
  assert.equal(logic.validComposerPayload([]), false);
});

test('local name and address checks mirror Rust (Enter submits only valid values)', () => {
  assert.equal(logic.localNameError('Work'), '');
  assert.equal(logic.localNameError('   '), 'Enter a name.');
  assert.equal(logic.localNameError('a'.repeat(40)), '');
  assert.equal(logic.localNameError('a'.repeat(41)), 'Use at most 40 characters.');
  assert.equal(logic.localNameError('é'.repeat(40)), '');
  assert.match(logic.localNameError('a\u0007b'), /control/);
  assert.equal(logic.localUrlError('https://example.com/'), '');
  assert.equal(logic.localUrlError(' http://example.com/a?b=1 '), '');
  for (const bad of ['', 'example.com', 'ftp://example.com/', 'javascript:alert(1)', 'file:///c:/x', 'https://user:pw@example.com/']) {
    assert.notEqual(logic.localUrlError(bad), '', bad);
  }
});

test('the first failing field is reported in reading order; a workspace has no address', () => {
  assert.deepEqual(logic.firstComposerError('app-add', '', ''), { field: 'name', text: 'Enter a name.' });
  assert.deepEqual(logic.firstComposerError('app-add', 'Mail', ''), { field: 'url', text: 'Enter a web address.' });
  assert.equal(logic.firstComposerError('app-add', 'Mail', 'https://mail.example/'), null);
  assert.equal(logic.firstComposerError('workspace-new', 'Work', ''), null);
  assert.deepEqual(logic.firstComposerError('workspace-new', '', 'ignored'), { field: 'name', text: 'Enter a name.' });
});

test('Tab and Shift+Tab cycle fields and the confirm row without leaving the composer', () => {
  assert.equal(logic.trapStep(0, 3, false), 1);
  assert.equal(logic.trapStep(2, 3, false), 0, 'forward wraps to the first field');
  assert.equal(logic.trapStep(0, 3, true), 2, 'backward wraps to the confirm row');
  assert.equal(logic.trapStep(-1, 3, false), 0, 'focus outside the stops enters at the first');
  assert.equal(logic.trapStep(-1, 3, true), 2);
  assert.equal(logic.trapStep(0, 0, false), -1);
});

test('each composer has its heading, verb label and existing glyphs (no new icon)', () => {
  const copy = logic.COMPOSER_COPY;
  assert.equal(copy['workspace-new'].heading, 'New workspace');
  assert.equal(copy['workspace-new'].confirm, 'Create workspace');
  assert.equal(copy['app-add'].heading, 'Add web app');
  assert.equal(copy['app-add'].confirm, 'Add web app');
  assert.equal(copy['app-edit'].heading, 'Edit web app');
  assert.equal(copy['app-edit'].confirm, 'Save web app');
  assert.deepEqual([copy['workspace-new'].nameGlyph, copy['app-add'].nameGlyph, copy['app-edit'].nameGlyph], ['workspace-add', 'app-tile', 'app-tile']);
  assert.deepEqual([copy['workspace-new'].confirmGlyph, copy['app-add'].confirmGlyph, copy['app-edit'].confirmGlyph], ['plus', 'plus', 'edit']);
  assert.equal(logic.ADDRESS_PLACEHOLDER, 'https://example.com/');
  const icons = read('src', 'icons.gen.ts');
  for (const name of ['workspace-add', 'app-tile', 'link', 'plus', 'edit']) assert.ok(icons.includes(`'${name}'`) || icons.includes(`"${name}"`) || icons.includes(`${name}:`), `${name} exists`);
});

test('senders: compose carries text, composeopen a rectangle, and both are validated', () => {
  const wires = [];
  globalThis.window = { chrome: { webview: { postMessage: wire => wires.push(JSON.parse(wire)) } } };
  assert.equal(protocol.sendPopupCompose(3, 'Work'), true);
  assert.equal(protocol.sendPopupCompose(3, 'Mail', 'https://mail.example/'), true);
  assert.equal(protocol.sendPopupCompose(-1, 'Work'), false);
  assert.equal(protocol.sendPopupCompose(3, 'x'.repeat(129)), false);
  const box = { left: 10, top: 20, right: 110, bottom: 48 };
  assert.equal(protocol.sendComposerOpen('workspace-new', box), true);
  assert.equal(protocol.sendComposerOpen('app-add', box, { name: 'Spotify', url: 'https://open.spotify.com/' }), true);
  assert.equal(protocol.sendComposerOpen('app-edit', box, { id: 9 }), true);
  assert.equal(protocol.sendComposerOpen('app-edit', box), false, 'an edit names its app');
  assert.equal(protocol.sendComposerOpen('app-add', box, { id: 9 }), false);
  assert.equal(protocol.sendComposerOpen('workspace-new', box, { url: 'https://x.test/' }), false);
  assert.equal(protocol.sendComposerOpen('app-add', { left: NaN, top: 0, right: 1, bottom: 1 }), false);
  assert.deepEqual(wires, [
    { v: 1, t: 'compose', tok: 3, name: 'Work' },
    { v: 1, t: 'compose', tok: 3, name: 'Mail', url: 'https://mail.example/' },
    { v: 1, t: 'composeopen', a: 'workspace-new', r: '10,20,100,28' },
    { v: 1, t: 'composeopen', a: 'app-add', name: 'Spotify', url: 'https://open.spotify.com/', r: '10,20,100,28' },
    { v: 1, t: 'composeopen', a: 'app-edit', id: 9, r: '10,20,100,28' },
  ]);
});

test('the composer view is on the menu surface and follows the markup and keyboard contract', () => {
  const view = read('src', 'composer.ts');
  assert.match(view, /root\.className = 'html-menu bw-menu-surface'/);
  assert.match(view, /root\.dataset\.kind = 'composer'/);
  assert.match(view, /setAttribute\('role', 'dialog'\)/);
  assert.match(view, /setAttribute\('aria-modal', 'true'\)/);
  assert.match(view, /setAttribute\('aria-labelledby'/);
  assert.match(view, /setAttribute\('aria-label', label\)/, 'inputs carry their accessible names');
  assert.match(view, /setAttribute\('aria-describedby'/);
  assert.match(view, /setAttribute\('aria-live', 'polite'\)/);
  assert.match(view, /event\.isComposing/, 'Enter does not submit while an IME composes');
  assert.match(view, /event\.key === 'Escape'/);
  assert.match(view, /event\.key === 'Tab'/);
  assert.match(view, /sendPopupCompose/);
  assert.match(view, /sendPopupDismiss/);
  assert.doesNotMatch(view, /disabled/, 'the confirm row is never disabled');
  assert.doesNotMatch(view, /Cancel/i, 'no Cancel row: Escape and click-away');
  assert.match(view, /classList\.add\('is-entering'\)/, 'the menu enter motion starts when the size is reported');
  assert.match(view, /classList\.add\('is-leaving'\)/);
  assert.match(view, /reduced\(\)/, 'reduced motion exits at once');
});

test('the popup host routes composer pages and the Customize panel opens the composer, not inline forms', () => {
  assert.match(read('src', 'popup.ts'), /case 'composer': composer\.show\(value\)/);
  const panel = read('src', 'panels', 'customize.ts');
  assert.match(panel, /sendComposerOpen/);
  for (const mode of ['workspace-new', 'app-add', 'app-edit']) assert.ok(panel.includes(`openComposer('${mode}'`), mode);
  assert.doesNotMatch(panel, /h\('form', 'cust-form/, 'no inline create or add form');
  assert.doesNotMatch(panel, /(?:sendAction|customizeCommand)\('(?:workspace-create|app-add|app-update)'/, 'no create or update command from the panel');
  assert.doesNotMatch(panel, /editingApp/, 'no per-app editor');
  assert.match(panel, /Edit \$\{app\.name\}/, 'the per-app edit button keeps its accessible name');
});

test('menu.css: fixed 280px composer, 28px field rows, error line, and the 100ms ease-in exit', () => {
  const css = read('src', 'styles', 'menu.css');
  const tokens = read('src', 'styles', 'tokens.css');
  assert.equal(logic.COMPOSER_WIDTH, 280);
  assert.match(css, /#app\.html-menu\[data-kind="composer"\]\s*\{\s*width: 280px;/);
  assert.match(css, /\.html-composer-input\s*\{[^}]*height: var\(--menu-item-h\)/);
  assert.match(css, /\.html-composer-field\s*\{[^}]*min-height: var\(--menu-item-h\)/);
  assert.match(css, /\.html-composer-error\.is-shown\s*\{\s*animation: html-menu-fade var\(--bw-dur-fast\) linear/);
  assert.match(css, /#app\.html-menu\.is-leaving\s*\{\s*animation: html-composer-exit 100ms var\(--bw-ease-in\)/);
  assert.match(tokens, /--bw-ease-in: cubic-bezier\(0\.4, 0, 1, 1\)/);
  assert.equal(logic.COMPOSER_EXIT_MS, 100);
  // Only opacity animates; no layout property is transitioned or animated for the composer.
  const composer = css.slice(css.indexOf('Composer (New workspace'));
  assert.doesNotMatch(composer, /transition:\s*[^;]*\b(width|height|left|top)\b/);
});
