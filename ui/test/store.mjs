import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildSync } from 'esbuild';
import { fileURLToPath } from 'node:url';
import { FakeDocument } from './fake-dom.mjs';
const bundle = async (file) => {
  const built = buildSync({ entryPoints: [fileURLToPath(new URL(`../src/${file}.ts`, import.meta.url))], bundle: true, format: 'esm', platform: 'neutral', write: false });
  return import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].contents).toString('base64')}`);
};
const logic = await bundle('store-logic');
const protocol = await bundle('protocol');
const ui = await bundle('store-section');
const id = 'a'.repeat(32);
const entry = { id, name: 'Helpful <text>', description: 'Make reading pleasant', publisher: 'Example publisher', category: 'Reading', source: 'Chrome Web Store', homepage: 'https://example.com', provenance: 'Published by Example', checked: '2026-10-18', compatibility: 'untested', compatibilityNote: 'No tested version yet.' };
const snap = (extra = {}) => ({ shown: true, catalogue: [entry], phase: 'idle', requestId: '', extensionId: id, name: '', version: '', error: '', restart: false, permissions: [], hostPermissions: [], contentScripts: [], contentScriptDeclarations: [], optionalPermissions: [], optionalHostPermissions: [], ...extra });
const message = (d) => ({ t: 'snap', p: 'store', d });
const btn = (root, text) => root.querySelectorAll('button').find((node) => node.textContent === text);
function mount() {
  globalThis.document = new FakeDocument();
  const sent = [];
  const mounted = ui.mountStoreSection({ send: (command) => { sent.push(command); return true; }, toast: () => {} });
  mounted.receive(message(snap()));
  return { ...mounted, sent };
}
test('store wire passes protocol and parser, preserves full lists without truncation', () => {
  const d = snap({ phase: 'review', requestId: 'request-1', version: '2.3', name: entry.name, permissions: Array.from({ length: 256 }, (_, i) => `permission-${i}`) });
  assert.equal(protocol.parseInbound(JSON.stringify({ v: 1, ...message(d) })).p, 'store');
  assert.equal(logic.parseStoreSnapshot(d).permissions.length, 256);
  for (const patch of [{ shown: 1 }, { phase: 'approved' }, { permissions: ['bad\nvalue'] }, { requestId: '../x' }, { catalogue: [entry, entry] }, { catalogue: [{ ...entry, id: 'https://evil.test' }] }, { phase: 'review', requestId: '', name: 'X', version: '1' }]) assert.equal(logic.parseStoreSnapshot(snap(patch)), null);
});
test('commands accept only native IDs and opaque bounded request IDs, and name the store', () => {
  assert.deepEqual(logic.storeInstallCommand(id), { a: 'storeinstall', text: id });
  assert.deepEqual(logic.storeInstallCommand(id, 'edge'), { a: 'storeinstall', text: `edge:${id}` });
  assert.deepEqual(logic.storeListingCommand(id, 'edge'), { a: 'storelisting', text: `edge:${id}` });
  assert.equal(logic.storeInstallCommand('../test'), null);
  assert.equal(logic.storeInstallCommand('edge:' + id), null, 'the page never builds its own prefix');
  assert.deepEqual(logic.storeRequestCommand('storeapprove', 'request-1'), { a: 'storeapprove', text: 'request-1' });
  assert.equal(logic.storeRequestCommand('storecancel', 'x'.repeat(65)), null);
  assert.equal(logic.parseStoreSnapshot(snap()).source, 'chrome', 'older senders mean the Chrome Web Store');
  assert.equal(logic.parseStoreSnapshot(snap({ source: 'edge' })).source, 'edge');
  assert.equal(logic.parseStoreSnapshot(snap({ source: 'firefox' })), null);
});
test('install shows what it can do in plain words, every list in full under Details, and waits for consent', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'review', requestId: 'req-2', name: entry.name, version: '9', permissions: [...Array.from({ length: 20 }, (_, i) => `permission-${i}`), 'tabs', 'storage'], hostPermissions: ['<all_urls>'], optionalHostPermissions: ['https://optional.test/*'] })));
  const card = store.root.querySelector('.bs-status');
  assert.equal(card.querySelector('h2').textContent, `Add ${entry.name}?`);
  assert.deepEqual(card.querySelector('.bs-access-list').querySelectorAll('li').map((li) => li.textContent), ['Read and change all your data on all websites', 'Read your browsing history']);
  assert.equal(card.querySelector('.bs-access-note').textContent, 'It may ask for more access later.');
  const details = card.querySelector('.bs-more');
  assert.equal(details.open, false, 'details are folded away');
  for (const value of ['permission-19', 'storage', '<all_urls>', 'https://optional.test/*', id, '9']) assert.ok(details.textContent.includes(value), value);
  assert.equal(store.sent.length, 0, 'receiving review never sends approval');
  btn(card, 'Add extension').click();
  assert.deepEqual(store.sent[0], { a: 'storeapprove', text: 'req-2' });
  assert.equal(btn(store.root, 'Add extension').disabled, true);
});
test('plain-language access: all sites first, named hosts, Chromium’s warnings only', () => {
  const access = (patch) => logic.storeAccess(logic.parseStoreSnapshot(snap(patch))).lines;
  assert.deepEqual(access({}), []);
  assert.deepEqual(access({ permissions: ['storage', 'alarms', 'contextMenus', 'activeTab', 'scripting'] }), [], 'no warning where Chromium shows none');
  assert.deepEqual(access({ contentScripts: ['*://*/*'], permissions: ['declarativeNetRequest'] }), ['Read and change all your data on all websites', 'Block content on any page']);
  assert.deepEqual(access({ hostPermissions: ['https://*.example.com/*'] }), ['Read and change your data on example.com']);
  assert.deepEqual(access({ hostPermissions: ['https://a.test/*', 'https://b.test/*', 'https://c.test/*'] }), ['Read and change your data on a.test, b.test and c.test']);
  assert.deepEqual(access({ hostPermissions: ['https://a.test/*', 'https://b.test/*', 'https://c.test/*', 'https://d.test/*'] }), ['Read and change your data on 4 sites']);
  assert.deepEqual(access({ permissions: ['tabs', 'history', 'webNavigation'] }), ['Read your browsing history', 'Read and change your browsing history']);
  assert.deepEqual(access({ permissions: ['debugger'] }), ['Read and change all your data on all websites']);
  assert.equal(logic.storeAccess(logic.parseStoreSnapshot(snap())).later, false);
});
test('a review with no special access says so in one line', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'review', requestId: 'r-0', name: entry.name, version: '1', permissions: ['storage'] })));
  assert.equal(store.root.querySelector('.bs-access-none').textContent, 'It needs no special access.');
  assert.equal(store.root.querySelector('.bs-access-note'), null);
});
test('cancel sends the matching request; errors and restart-required states say so briefly', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'downloading', requestId: 'req-3' })));
  btn(store.root, 'Cancel').click();
  assert.deepEqual(store.sent, [{ a: 'storecancel', text: 'req-3' }]);
  store.receive(message(snap({ phase: 'error', error: 'Signature verification failed' })));
  assert.ok(store.root.textContent.includes('Signature verification failed'));
  store.receive(message(snap({ phase: 'ready', name: entry.name, restart: true })));
  assert.equal(store.root.querySelector('h2').textContent, `${entry.name} added`);
  assert.ok(store.root.textContent.includes('Restart Bowser to start it.'));
  btn(store.root, 'Manage extensions').click();
  assert.deepEqual(store.sent.at(-1), { a: 'section', text: 'extensions' });
  store.receive(message(snap({ shown: false })));
  assert.equal(store.root.hidden, true);
});
test('idle shows the installed manager, with its own IDs, and no catalogue', () => {
  const store = mount();
  assert.equal(store.root.querySelector('.bs-installed').hidden, false);
  assert.equal(store.root.querySelector('.bs-installed').querySelector('h3').id, 'bs-ext-h');
  assert.equal(store.root.querySelector('.bs-search'), null);
  assert.equal(store.root.querySelector('.bs-grid'), null);
  store.receive(message(snap({ phase: 'review', requestId: 'r-hide', name: entry.name, version: '1' })));
  assert.equal(store.root.querySelector('.bs-installed').hidden, true, 'the card is alone while a package is in flight');
  assert.ok(store.root.classList.contains('is-install'));
  store.receive(message(snap()));
  assert.equal(store.root.classList.contains('is-install'), false);
});

test('malformed replacement invalidates existing approval', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'review', requestId: 'req-4', name: entry.name, version: '1' })));
  store.receive(message(snap({ phase: 'review', permissions: [42] })));
  assert.equal(btn(store.root, 'Add extension'), undefined);
  assert.equal(store.root.querySelector('h2').textContent, 'Something went wrong');
  assert.equal(store.sent.length, 0);
});

test('the store link command accepts only HTTPS Chrome Web Store and Edge Add-ons listing URLs', () => {
  const url = `https://chromewebstore.google.com/detail/reading/${id}`;
  assert.deepEqual(logic.storeUrlCommand(url), { a: 'storeurl', text: url });
  const edge = `https://microsoftedge.microsoft.com/addons/detail/reading/${id}`;
  assert.deepEqual(logic.storeUrlCommand(edge), { a: 'storeurl', text: edge });
  for (const value of ['http://chromewebstore.google.com/detail/' + id, 'https://evil.test/detail/' + id, 'https://chromewebstore.google.com.evil.test/detail/' + id, 'https://x@chromewebstore.google.com/detail/' + id, 'https://chromewebstore.google.com/category/extensions', 'https://microsoftedge.microsoft.com/addons/Microsoft-Edge-Extensions-Home', 'http://microsoftedge.microsoft.com/addons/detail/' + id]) assert.equal(logic.storeUrlCommand(value), null, value);
});

test('leaving the Store discards pending review controls until a fresh native snapshot', () => {
  const store = mount();
  store.receive(message(snap({phase:'review',requestId:'leave-1',name:entry.name,version:'1'})));
  store.hide();
  assert.equal(store.root.hidden,true);
  assert.equal(btn(store.root,'Add extension'),undefined);
  store.receive(message(snap()));
  assert.equal(store.root.hidden,false);
  assert.equal(btn(store.root,'Add extension'),undefined);
});

test('a fresh review focuses its heading, not consent', () => {
  const store = mount();
  store.receive(message(snap({phase:'review',requestId:'focus-1',name:entry.name,version:'1'})));
  assert.equal(document.activeElement.tagName,'h2');
  assert.equal(document.activeElement.textContent,`Add ${entry.name}?`);
  assert.notEqual(document.activeElement,btn(store.root,'Add extension'));
});

test('permission bounds count Unicode scalars and reject more than the native cap', () => {
  const value = '😀'.repeat(256);
  assert.equal(logic.parseStoreSnapshot(snap({ permissions:[value] })).permissions[0],value);
  assert.equal(logic.parseStoreSnapshot(snap({ permissions:[value+'😀'] })),null);
});

test('Details name the store, the version and ID, and open the listing for that store', () => {
  const store = mount();
  const noncatalog='c'.repeat(32);
  store.receive(message(snap({phase:'review',requestId:'source-2',extensionId:noncatalog,name:'Resolved package',version:'4',source:'edge'})));
  const facts=store.root.querySelector('.bs-facts');
  assert.ok(facts.textContent.includes(noncatalog));
  assert.ok(facts.textContent.includes('Edge Add-ons'));
  btn(store.root,'Edge Add-ons').click();
  assert.deepEqual(store.sent,[{a:'storelisting',text:`edge:${noncatalog}`}]);
  assert.equal(logic.parseStoreSnapshot(snap({phase:'review',requestId:'bad-source',name:'X',version:'1',extensionId:'https://evil.test'})),null);
});

test('complete static declarations stay in Details without truncation and reject unsafe wire text', () => {
  const store = mount();
  const declaration = JSON.stringify({ declared: { matches: ['https://example.test/*'], exclude_matches: ['https://example.test/private/*'], all_frames: true, match_about_blank: true, match_origin_as_fallback: true, world: 'MAIN', run_at: 'document_start', future: 'preserved' }, documented_defaults_for_omitted_fields: {}, unrecognized_fields_effect_not_assumed: ['future'] });
  const review = snap({ phase: 'review', requestId: 'full-scripts', name: entry.name, version: '1', contentScriptDeclarations: [declaration] });
  assert.ok(logic.parseStoreSnapshot(review));
  store.receive(message(review));
  assert.ok(store.root.querySelector('.bs-more').textContent.includes(declaration));
  for (const contentScriptDeclarations of [['x'.repeat(180 * 1024 + 1)], ['hidden‮text'], ['bad\nvalue'], Array(257).fill('{}')]) assert.equal(logic.parseStoreSnapshot(snap({ contentScriptDeclarations })), null);
  const missing = { ...review }; delete missing.contentScriptDeclarations; assert.equal(logic.parseStoreSnapshot(missing), null);
});

// ---- Installation card: progress, failures, recovery ----
const VERIFY_CAP = 'Package verification failed: unpack failed: an archive entry is larger than the per-file cap. This is not a runtime compatibility verdict.';

test('optional progress fields parse strictly and never appear unless both byte counts are sane', () => {
  assert.ok(logic.parseStoreSnapshot(snap({ phase: 'downloading', requestId: 'p-1', stage: 'download', received: 10, total: 20 })));
  assert.ok(logic.parseStoreSnapshot(snap({ phase: 'downloading', requestId: 'p-1', stage: 'verify' })));
  for (const patch of [{ stage: 'unpack' }, { received: 5 }, { total: 5 }, { received: 6, total: 5 }, { received: -1, total: 5 }, { received: 1, total: 0 }, { received: 1.5, total: 5 }, { received: '1', total: 5 }]) assert.equal(logic.parseStoreSnapshot(snap({ phase: 'downloading', requestId: 'p-1', ...patch })), null, JSON.stringify(patch));
});

test('progress is determinate only with real byte counts and only while work runs', () => {
  const progress = (patch) => logic.storeProgress(logic.parseStoreSnapshot(snap(patch)));
  assert.equal(progress({ phase: 'downloading', requestId: 'p' }).kind, 'indeterminate');
  const known = progress({ phase: 'downloading', requestId: 'p', stage: 'download', received: 1468006, total: 3355443 });
  assert.equal(known.kind, 'determinate');
  assert.equal(known.text, '1.4 MB of 3.2 MB');
  assert.ok(Math.abs(known.value - 0.4375) < 0.001);
  assert.equal(progress({ phase: 'downloading', requestId: 'p', stage: 'verify', received: 5, total: 5 }).kind, 'indeterminate', 'verification has no byte progress');
  assert.equal(progress({ phase: 'installing' }).kind, 'indeterminate');
  for (const phase of ['idle', 'ready', 'error', 'cancelled']) assert.equal(progress({ phase }), null, phase);
});

test('failures read in a few plain words, keep the native text verbatim and offer only honest retries', () => {
  const cap = logic.storeFailure(VERIFY_CAP);
  assert.equal(cap.detail, VERIFY_CAP, 'the native message is kept unchanged under Details');
  assert.equal(cap.retry, 'offer', 'a size-cap failure repeats, so retry is not pushed');
  assert.equal(cap.body, 'A file in it is larger than Bowser allows.');
  assert.equal(logic.storeFailure('Package download failed: timed out. This is not a runtime compatibility verdict.').retry, 'suggest');
  // Raised before a request starts: Rust keeps the previous request's ID, so neither show nor retry it.
  for (const error of ['Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.', 'Use a persistent normal profile to install extensions.', 'That extension ID is not valid.', 'This WebView2 runtime cannot run supported extensions. Update the WebView2 runtime before installing; restarting alone will not fix this. Nothing was installed.', 'Enter a Chrome Web Store or Edge Add-ons extension link.']) {
    const failure = logic.storeFailure(error);
    assert.equal(failure.identity, false, error);
    assert.equal(failure.retry, 'none', error);
  }
  const duplicate = logic.storeFailure('This extension identity is already installed in this profile. Verified extension updates are not enabled yet.');
  assert.equal(duplicate.title, 'Already added');
  assert.equal(duplicate.manage, true);
  assert.equal(duplicate.retry, 'none');
  const unknown = logic.storeFailure('A brand new native message.');
  assert.equal(unknown.body, 'A brand new native message.');
  assert.equal(unknown.detail, '', 'an unrecognised message is shown once, as the explanation');
  // No failure explains itself in more than one short sentence.
  for (const error of [VERIFY_CAP, 'Package download failed: x', 'already installed', 'too large to display safely', 'No extension was installed.', 'WebView2 runtime cannot run supported extensions', 'Other extension downloads are finishing', 'persistent normal profile']) {
    const { title, body } = logic.storeFailure(error);
    assert.ok(title.length <= 40, title);
    assert.ok(body.split('. ').length === 1 && body.length <= 80, body);
  }
});

test('try again reuses storeinstall with the same store only after a cancel or a retryable error', () => {
  const retry = (patch) => logic.storeRetryCommand(logic.parseStoreSnapshot(snap(patch)));
  assert.deepEqual(retry({ phase: 'error', error: VERIFY_CAP }), { a: 'storeinstall', text: id });
  assert.deepEqual(retry({ phase: 'cancelled' }), { a: 'storeinstall', text: id });
  assert.deepEqual(retry({ phase: 'cancelled', source: 'edge' }), { a: 'storeinstall', text: `edge:${id}` });
  assert.equal(retry({ phase: 'error', error: 'Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.' }), null);
  assert.equal(retry({ phase: 'error', error: VERIFY_CAP, extensionId: '' }), null);
  for (const phase of ['idle', 'ready']) assert.equal(retry({ phase }), null, phase);
});

test('error card: title, one line, folded native detail, and a working retry', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'error', requestId: 'e-1', name: entry.name, error: VERIFY_CAP })));
  const card = store.root.querySelector('.bs-status');
  assert.equal(card.dataset.phase, 'error');
  assert.equal(card.querySelector('h2').textContent, 'The package didn’t pass Bowser’s checks');
  assert.equal(card.querySelector('.bs-tech').querySelector('.bs-tech-text').textContent, VERIFY_CAP);
  assert.equal(card.querySelector('.bs-tech').open, false);
  assert.ok(card.querySelector('.bs-ident-icon'), 'the package tile stays');
  // The bar breaks: two red halves around an exclamation mark, Try again on the same row.
  const broken = card.querySelector('.bs-bar.is-failed');
  assert.equal(broken.querySelectorAll('.bs-bar-half').length, 2);
  assert.equal(broken.querySelector('.bs-bar-break').textContent, '!');
  assert.equal(broken.getAttribute('aria-label'), 'Download failed');
  assert.equal(btn(card, 'Try again').parent, broken.parent, 'Try again sits beside the bar');
  assert.equal(store.root.querySelector('[role="alert"]').textContent.startsWith('The package didn’t pass'), true, 'errors are announced assertively');
  btn(card, 'Try again').click();
  assert.deepEqual(store.sent.at(-1), { a: 'storeinstall', text: id });
  assert.equal(store.root.querySelector('.bs-status').querySelector('h2').textContent, entry.name, 'the pending card still names the package');
  assert.equal(store.root.querySelector('[role="alert"]').textContent, '', 'the error announcement is withdrawn');
});

test('only a failed download breaks the bar; other failures keep their mark and action row', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'error', requestId: 'e-2', name: entry.name, error: 'Package download failed: offline. This is not a runtime compatibility verdict.' })));
  assert.equal(store.root.querySelector('h2').textContent, 'Download failed');
  assert.ok(store.root.querySelector('.bs-bar.is-failed'));
  store.receive(message(snap({ phase: 'error', requestId: 'e-3', name: entry.name, error: 'No extension was installed.' })));
  assert.equal(store.root.querySelector('.bs-bar.is-failed'), null);
  assert.ok(btn(store.root.querySelector('.bs-actions'), 'Try again'));
});

test('an error raised before a request started shows neither the stale package nor a retry', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'error', requestId: 'old', name: 'Previous extension', error: 'Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.' })));
  const card = store.root.querySelector('.bs-status');
  assert.equal(card.querySelector('.bs-ident-icon'), null);
  assert.ok(card.querySelector('.bs-head-mark'));
  assert.equal(btn(card, 'Try again'), undefined);
  assert.equal(card.querySelector('.bs-tech'), null);
  assert.ok(!card.textContent.includes('Previous extension'));
});

test('in-progress card: neutral bar, secondary cancel, status announced once', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'downloading', requestId: 'd-1', name: entry.name })));
  const live = store.root.querySelector('[role="status"]');
  assert.equal(live.textContent, `${entry.name}: Downloading…`);
  let bar = store.root.querySelector('[role="progressbar"]');
  assert.equal(bar.getAttribute('aria-valuenow'), null, 'unknown progress is indeterminate');
  const cancel = btn(store.root, 'Cancel');
  assert.equal(cancel.classList.contains('is-primary'), false);
  assert.equal(cancel.parent, bar.parent, 'Cancel sits on the same row as the bar');
  assert.equal(store.root.querySelector('.bs-actions'), null, 'no separate action row while downloading');
  cancel.focus();
  store.receive(message(snap({ phase: 'downloading', requestId: 'd-1', name: entry.name, stage: 'download', received: 512 * 1024, total: 1024 * 1024 })));
  bar = store.root.querySelector('[role="progressbar"]');
  assert.equal(bar.getAttribute('aria-valuenow'), '50');
  assert.equal(bar.style.getPropertyValue('--bs-f'), '0.5');
  assert.equal(document.activeElement.textContent, 'Cancel', 'a progress tick keeps focus on Cancel');
  assert.equal(store.root.querySelector('[role="status"]'), live, 'the live region persists across renders');
  store.receive(message(snap({ phase: 'cancelled', requestId: 'd-1', name: entry.name })));
  assert.equal(document.activeElement.tagName, 'h2', 'focus moves to the new state when its control goes away');
  assert.equal(document.activeElement.textContent, entry.name);
  assert.ok(store.root.textContent.includes('Cancelled'));
  assert.ok(btn(store.root, 'Try again'));
});

test('review card: consent first in the action row, cancel after it, both bound to the request', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'review', requestId: 'r-1', name: entry.name, version: '3', permissions: ['storage'] })));
  const actions = store.root.querySelector('.bs-actions');
  assert.ok(actions.classList.contains('is-sticky'));
  const [approve, cancel] = actions.querySelectorAll('button');
  assert.equal(approve.textContent, 'Add extension');
  assert.equal(approve.getAttribute('aria-label'), `Add ${entry.name} 3`);
  assert.ok(approve.classList.contains('is-primary'));
  assert.equal(cancel.textContent, 'Cancel');
  cancel.click();
  assert.deepEqual(store.sent.at(-1), { a: 'storecancel', text: 'r-1' });
});
test('package icon: only bounded base64 raster data URLs survive parsing, anything else becomes empty', async () => {
  const { storeIcons } = await import('../shots/store-icons.mjs');
  const real = storeIcons().brand;
  assert.match(real, /^data:image\/png;base64,/);
  const review = (extra) => snap({ phase: 'review', requestId: 'req-icon', name: entry.name, version: '1', ...extra });
  const pixel = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';
  for (const good of [real, `data:image/png;base64,${pixel}`, 'data:image/jpeg;base64,/9j/4AAQ', 'data:image/gif;base64,R0lGODlh', 'data:image/webp;base64,UklGRg==']) {
    assert.equal(logic.parseStoreSnapshot(review({ icon: good })).icon, good);
  }
  for (const bad of [
    'data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=', 'data:image/svg+xml,<svg onload="alert(1)"/>', 'javascript:alert(1)', 'http://example.com/icon.png',
    'https://example.com/icon.png', 'data:image/png,rawbytes', 'data:image/png;base64,abc def', 'data:image/png;base64,abc===', 'data:image/png;base64,',
    ' data:image/png;base64,AAAA', 'data:image/png;base64,AAAA\n', 'DATA:IMAGE/PNG;BASE64,AAAA', 'data:text/html;base64,PGI+', 'garbage',
    `data:image/png;base64,${'A'.repeat(131072)}`, 42, null, {}, ['data:image/png;base64,AAAA'],
  ]) {
    const parsed = logic.parseStoreSnapshot(review({ icon: bad }));
    assert.ok(parsed, `a bad icon must not reject the snapshot: ${String(bad).slice(0, 40)}`);
    assert.equal(parsed.icon, '');
  }
  assert.equal(logic.parseStoreSnapshot(review({ icon: `data:image/png;base64,${'A'.repeat(131072 - 22)}` })).icon.length, 131072);
  assert.equal(logic.parseStoreSnapshot(snap()).icon, '', 'absent field defaults to empty for older senders');
});
test('identity tile shows the package icon from review onwards, the puzzle otherwise, and falls back when it fails', async () => {
  const { storeIcons } = await import('../shots/store-icons.mjs');
  const real = storeIcons().tile;
  const store = mount();
  const tile = () => store.root.querySelector('.bs-ident-icon');
  const img = () => tile()?.querySelector('img') ?? null;
  const run = { requestId: 'req-i', name: entry.name, version: '3', icon: real };
  store.receive(message(snap({ ...run, phase: 'downloading' })));
  assert.equal(img(), null, 'no icon while downloading, even if one is sent');
  assert.ok(tile().querySelector('svg'));
  for (const phase of ['review', 'installing', 'ready']) {
    store.receive(message(snap({ ...run, phase })));
    assert.equal(img()?.src, real, phase);
    assert.equal(img().alt, '');
    assert.ok(tile().classList.contains('has-image'));
    assert.equal(tile().querySelector('svg'), null);
  }
  store.receive(message(snap({ ...run, phase: 'review', icon: 'data:image/svg+xml;base64,PHN2Zz4=' })));
  assert.equal(img(), null, 'an invalid icon renders the puzzle');
  assert.ok(tile().querySelector('svg'));
  store.receive(message(snap({ ...run, phase: 'review', icon: '' })));
  assert.equal(img(), null);
  // A well-formed URL that does not decode: the error swaps in the puzzle and later snapshots skip it.
  const broken = 'data:image/png;base64,AAAA';
  store.receive(message(snap({ ...run, phase: 'review', icon: broken })));
  img().dispatch('error');
  assert.equal(img(), null);
  assert.ok(tile().querySelector('svg'));
  assert.ok(!tile().classList.contains('has-image'));
  store.receive(message(snap({ ...run, phase: 'installing', icon: broken })));
  assert.equal(img(), null, 'a failed icon is not retried on the next snapshot');
  store.receive(message(snap({ ...run, phase: 'ready', icon: real })));
  assert.equal(img()?.src, real, 'a different icon is tried');
  // Idle shows the installed list, which never gets package icons.
  store.receive(message(snap()));
  assert.equal(store.root.querySelectorAll('img').length, 0);
});
test('the page heading is "Extensions" in every mode', () => {
  const store = mount();
  assert.equal(store.root.querySelector('h1').textContent, 'Extensions');
  store.receive(message(snap({ phase: 'review', requestId: 'req-h', name: entry.name, version: '1' })));
  assert.equal(store.root.querySelector('h1').textContent, 'Extensions');
});
