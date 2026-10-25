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
test('commands accept only native catalogue IDs and opaque bounded request IDs', () => {
  assert.deepEqual(logic.storeInstallCommand(id), { a: 'storeinstall', text: id });
  assert.equal(logic.storeInstallCommand('../test'), null);
  assert.deepEqual(logic.storeRequestCommand('storeapprove', 'request-1'), { a: 'storeapprove', text: 'request-1' });
  assert.equal(logic.storeRequestCommand('storecancel', 'x'.repeat(65)), null);
});
test('search matches all terms across name publisher category and description', () => {
  assert.equal(logic.filterCatalogue([entry], 'pleasant example').length, 1);
  assert.equal(logic.filterCatalogue([entry], '', 'Reading').length, 1);
  assert.equal(logic.filterCatalogue([entry], 'missing').length, 0);
});
test('catalogue detail and source actions do not navigate the trusted host', () => {
  const store = mount();
  assert.equal(store.root.hidden, false);
  assert.ok(store.root.textContent.includes('Untested'));
  btn(store.root, entry.name).click();
  assert.ok(store.root.textContent.includes(entry.provenance));
  btn(store.root, 'View original listing').click();
  btn(store.root, 'Publisher website').click();
  assert.deepEqual(store.sent, [{ a: 'storelisting', text: id }, { a: 'storesource', text: id }]);
  assert.equal(store.root.querySelectorAll('iframe').length, 0);
  assert.equal(store.root.querySelectorAll('a').length, 0);
});
test('install requests package first, then shows every resolved permission before explicit consent', () => {
  const store = mount();
  btn(store.root, 'Get extension').click();
  assert.deepEqual(store.sent, [{ a: 'storeinstall', text: id }]);
  assert.equal(btn(store.root, 'Get extension').disabled, true);
  store.receive(message(snap({ phase: 'review', requestId: 'req-2', name: entry.name, version: '9', permissions: Array.from({ length: 20 }, (_, i) => `permission-${i}`), hostPermissions: ['<all_urls>'], optionalHostPermissions: ['https://optional.test/*'] })));
  assert.ok(store.root.textContent.includes('permission-19'));
  assert.ok(store.root.textContent.includes('<all_urls>'));
  assert.ok(store.root.textContent.includes('https://optional.test/*'));
  assert.equal(store.sent.length, 1, 'receiving review never sends approval');
  btn(store.root, `Install ${entry.name} 9`).click();
  assert.deepEqual(store.sent[1], { a: 'storeapprove', text: 'req-2' });
  assert.equal(btn(store.root, `Install ${entry.name} 9`).disabled, true);
});
test('cancel sends matching request, errors and restart-required states are honest', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'downloading', requestId: 'req-3' })));
  btn(store.root, 'Cancel installation').click();
  assert.deepEqual(store.sent, [{ a: 'storecancel', text: 'req-3' }]);
  store.receive(message(snap({ phase: 'error', error: 'Signature verification failed' })));
  assert.ok(store.root.textContent.includes('Signature verification failed'));
  store.receive(message(snap({ phase: 'ready', restart: true })));
  assert.ok(store.root.textContent.includes('Restart Bowser'));
  store.receive(message(snap({ shown: false })));
  assert.equal(store.root.hidden, true);
});
test('installed section reuses native extension manager and separate IDs', () => {
  const store = mount();
  btn(store.root, 'Installed').click();
  assert.equal(store.root.querySelector('.bs-installed').hidden, false);
  assert.equal(store.root.querySelector('.bs-search').hidden, true);
  assert.equal(store.root.querySelector('.bs-installed').querySelector('h3').id, 'bs-ext-h');
});

test('malformed replacement invalidates existing approval, even after navigation', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'review', requestId: 'req-4', name: entry.name, version: '1' })));
  store.receive(message(snap({ phase: 'review', permissions: [42] })));
  btn(store.root, 'Discover').click();
  assert.equal(btn(store.root, `Install ${entry.name} 1`), undefined);
  assert.equal(store.sent.length, 0);
});


test('optional store link accepts only verified-format HTTPS CWS listing URLs', () => {
  const url = `https://chromewebstore.google.com/detail/reading/${id}`;
  assert.deepEqual(logic.storeUrlCommand(url), { a: 'storeurl', text: url });
  for (const value of ['http://chromewebstore.google.com/detail/' + id, 'https://evil.test/detail/' + id, 'https://chromewebstore.google.com.evil.test/detail/' + id, 'https://x@chromewebstore.google.com/detail/' + id, 'https://chromewebstore.google.com/category/extensions']) assert.equal(logic.storeUrlCommand(value), null);
});


test('leaving the Store discards pending review controls until a fresh native snapshot', () => {
  const store = mount();
  store.receive(message(snap({phase:'review',requestId:'leave-1',name:entry.name,version:'1'})));
  store.hide();
  assert.equal(store.root.hidden,true);
  assert.equal(btn(store.root,`Install ${entry.name} 1`),undefined);
  store.receive(message(snap()));
  assert.equal(store.root.hidden,false);
  assert.equal(btn(store.root,`Install ${entry.name} 1`),undefined);
});


test('detail back restores card focus and a fresh review focuses its heading, not consent', () => {
  const store = mount();
  btn(store.root,entry.name).click();
  assert.equal(document.activeElement.tagName,'h2');
  btn(store.root,'Back to catalogue').click();
  assert.equal(document.activeElement.textContent,entry.name);
  store.receive(message(snap({phase:'review',requestId:'focus-1',name:entry.name,version:'1'})));
  assert.equal(document.activeElement.tagName,'h2');
  assert.ok(document.activeElement.textContent.startsWith('Review'));
  assert.notEqual(document.activeElement,btn(store.root,`Install ${entry.name} 1`));
});


test('permission bounds count Unicode scalars and reject more than the native cap', () => {
  const value = '😀'.repeat(256);
  assert.equal(logic.parseStoreSnapshot(snap({ permissions:[value] })).permissions[0],value);
  assert.equal(logic.parseStoreSnapshot(snap({ permissions:[value+'😀'] })),null);
});

test('every resolved review identifies source and unverified publisher without assuming catalogue membership', () => {
  const store = mount();
  store.receive(message(snap({phase:'review',requestId:'source-1',name:entry.name,version:'1'})));
  assert.ok(store.root.querySelector('.bs-review-facts').textContent.includes(entry.publisher));
  assert.ok(store.root.querySelector('.bs-review-facts').textContent.includes('not independently verified'));
  const noncatalog='c'.repeat(32);
  store.receive(message(snap({phase:'review',requestId:'source-2',extensionId:noncatalog,name:'Resolved package',version:'4'})));
  const facts=store.root.querySelector('.bs-review-facts').textContent;
  assert.ok(facts.includes(noncatalog));
  assert.ok(facts.includes('Unknown. Not independently verified.'));
  assert.ok(facts.includes('Untested for this resolved package version'));
  assert.ok(facts.includes('Not part of the curated Bowser catalogue'));
  btn(store.root,'View package source').click();
  assert.deepEqual(store.sent,[{a:'storelisting',text:noncatalog}]);
  assert.equal(logic.parseStoreSnapshot(snap({phase:'review',requestId:'bad-source',name:'X',version:'1',extensionId:'https://evil.test'})),null);
});

test('complete static declarations remain visible without truncation and reject unsafe wire text', () => {
  const store = mount();
  const declaration = JSON.stringify({ declared: { matches: ['https://example.test/*'], exclude_matches: ['https://example.test/private/*'], all_frames: true, match_about_blank: true, match_origin_as_fallback: true, world: 'MAIN', run_at: 'document_start', future: 'preserved' }, documented_defaults_for_omitted_fields: {}, unrecognized_fields_effect_not_assumed: ['future'] });
  const review = snap({ phase: 'review', requestId: 'full-scripts', name: entry.name, version: '1', contentScriptDeclarations: [declaration] });
  assert.ok(logic.parseStoreSnapshot(review));
  store.receive(message(review));
  assert.ok(store.root.textContent.includes(declaration));
  assert.ok(store.root.textContent.includes('not a guarantee of WebView2 behavior'));
  for (const contentScriptDeclarations of [['x'.repeat(180 * 1024 + 1)], ['hidden\u202etext'], ['bad\nvalue'], Array(257).fill('{}')]) assert.equal(logic.parseStoreSnapshot(snap({ contentScriptDeclarations })), null);
  const missing = { ...review }; delete missing.contentScriptDeclarations; assert.equal(logic.parseStoreSnapshot(missing), null);
});


test('native installation review does not present a replacement discovery catalogue', () => {
  const store = mount();
  store.receive(message(snap({phase:'review',requestId:'native-cws',name:entry.name,version:'1'})));
  assert.equal(store.root.querySelector('.bs-layout').classList.contains('is-review'),true);
  assert.equal(store.root.querySelector('.bs-side').hidden,true);
  assert.equal(store.root.querySelector('.bs-search').hidden,true);
  assert.equal(store.root.querySelector('.bs-body').hidden,true);
  store.root.dispatch('keydown', {key:'f',ctrlKey:true,altKey:false,shiftKey:false});
  assert.equal(store.root.querySelector('.bs-search').hidden,true);
  assert.equal(store.root.querySelector('.bs-body').hidden,true);
  assert.ok(btn(store.root,`Install ${entry.name} 1`));
});

test('review-only single-column layout resets with idle catalogue state', () => {
  const store=mount();
  store.receive(message(snap({phase:'review',requestId:'wide-review',name:entry.name,version:'1'})));
  assert.equal(store.root.querySelector('.bs-layout').classList.contains('is-review'),true);
  store.receive(message(snap()));
  assert.equal(store.root.querySelector('.bs-layout').classList.contains('is-review'),false);
});

// ---- Installation card: steps, progress, failures, recovery ----
const VERIFY_CAP = 'Package verification failed: unpack failed: an archive entry is larger than the per-file cap. This is not a runtime compatibility verdict.';
const stateOf = (steps) => steps.map((step) => `${step.key}:${step.state}`).join(' ');

test('optional progress fields parse strictly and never appear unless both byte counts are sane', () => {
  assert.ok(logic.parseStoreSnapshot(snap({ phase: 'downloading', requestId: 'p-1', stage: 'download', received: 10, total: 20 })));
  assert.ok(logic.parseStoreSnapshot(snap({ phase: 'downloading', requestId: 'p-1', stage: 'verify' })));
  for (const patch of [{ stage: 'unpack' }, { received: 5 }, { total: 5 }, { received: 6, total: 5 }, { received: -1, total: 5 }, { received: 1, total: 0 }, { received: 1.5, total: 5 }, { received: '1', total: 5 }]) assert.equal(logic.parseStoreSnapshot(snap({ phase: 'downloading', requestId: 'p-1', ...patch })), null, JSON.stringify(patch));
});

test('steps follow the phase; without a stage download and verification are both in progress', () => {
  const steps = (patch) => stateOf(logic.storeSteps(logic.parseStoreSnapshot(snap(patch))));
  assert.equal(steps({ phase: 'downloading', requestId: 's' }), 'download:current verify:current review:todo install:todo');
  assert.equal(steps({ phase: 'downloading', requestId: 's', stage: 'download' }), 'download:current verify:todo review:todo install:todo');
  assert.equal(steps({ phase: 'downloading', requestId: 's', stage: 'verify' }), 'download:done verify:current review:todo install:todo');
  assert.equal(steps({ phase: 'review', requestId: 's', name: 'X', version: '1' }), 'download:done verify:done review:current install:todo');
  assert.equal(steps({ phase: 'installing' }), 'download:done verify:done review:done install:current');
  assert.equal(steps({ phase: 'ready' }), 'download:done verify:done review:done install:done');
  assert.equal(steps({ phase: 'error', error: VERIFY_CAP }), 'download:done verify:failed review:todo install:todo');
  assert.equal(steps({ phase: 'error', error: 'Package download failed: offline. This is not a runtime compatibility verdict.' }), 'download:failed verify:todo review:todo install:todo');
  for (const patch of [{ phase: 'idle' }, { phase: 'cancelled' }, { phase: 'error', error: 'Something new went wrong.' }]) assert.equal(steps(patch), '', JSON.stringify(patch));
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

test('failures read in plain language, keep the native text verbatim and offer only honest retries', () => {
  const cap = logic.storeFailure(VERIFY_CAP);
  assert.equal(cap.step, 'verify');
  assert.equal(cap.detail, VERIFY_CAP, 'technical detail is the native message unchanged');
  assert.equal(cap.retry, 'offer', 'a size-cap failure repeats, so retry is not pushed');
  assert.ok(cap.body.includes('not a verdict on whether the extension works'));
  assert.equal(logic.storeFailure('Package download failed: timed out. This is not a runtime compatibility verdict.').retry, 'suggest');
  // Raised before a request starts: Rust keeps the previous request's ID, so neither show nor retry it.
  for (const error of ['Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.', 'Use a persistent normal profile to install extensions.', 'That extension ID is not valid.', 'This WebView2 runtime cannot run supported extensions. Update the WebView2 runtime before installing; restarting alone will not fix this. Nothing was installed.']) {
    const failure = logic.storeFailure(error);
    assert.equal(failure.identity, false, error);
    assert.equal(failure.retry, 'none', error);
  }
  const duplicate = logic.storeFailure('This extension identity is already installed in this profile. Verified extension updates are not enabled yet.');
  assert.equal(duplicate.manage, true);
  assert.equal(duplicate.retry, 'none');
  const unknown = logic.storeFailure('A brand new native message.');
  assert.equal(unknown.body, 'A brand new native message.');
  assert.equal(unknown.detail, '', 'an unrecognised message is shown once, as the explanation');
  assert.ok(logic.storeFailure('').body.includes('Nothing was approved automatically'));
});

test('try again reuses storeinstall with the request ID only after a cancel or a retryable error', () => {
  const retry = (patch) => logic.storeRetryCommand(logic.parseStoreSnapshot(snap(patch)));
  assert.deepEqual(retry({ phase: 'error', error: VERIFY_CAP }), { a: 'storeinstall', text: id });
  assert.deepEqual(retry({ phase: 'cancelled' }), { a: 'storeinstall', text: id });
  assert.equal(retry({ phase: 'error', error: 'Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.' }), null);
  assert.equal(retry({ phase: 'error', error: VERIFY_CAP, extensionId: '' }), null);
  for (const phase of ['idle', 'ready']) assert.equal(retry({ phase }), null, phase);
});

test('error card: title, explanation, folded native detail, and working recovery actions', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'error', requestId: 'e-1', name: entry.name, error: VERIFY_CAP })));
  const card = store.root.querySelector('.bs-status');
  assert.equal(card.dataset.phase, 'error');
  assert.equal(card.querySelector('h2').textContent, 'This package did not pass Bowser’s checks');
  assert.equal(card.querySelector('.bs-tech').querySelector('.bs-tech-text').textContent, VERIFY_CAP);
  assert.ok(card.querySelector('.bs-ident').textContent.includes(entry.name));
  assert.equal(card.querySelector('.bs-step[data-state="failed"]').textContent.startsWith('Verify'), true);
  assert.equal(store.root.querySelector('[role="alert"]').textContent.startsWith('This package did not pass'), true, 'errors are announced assertively');
  btn(card, 'Web Store').click();
  assert.deepEqual(store.sent.at(-1), { a: 'storebrowse' });
  btn(card, 'Try again').click();
  assert.deepEqual(store.sent.at(-1), { a: 'storeinstall', text: id });
  assert.equal(store.root.querySelector('.bs-status').querySelector('h2').textContent, 'Requesting the extension package…');
  assert.ok(store.root.querySelector('.bs-status').querySelector('.bs-ident').textContent.includes(entry.name), 'the pending card still names the package');
  assert.equal(store.root.querySelector('[role="alert"]').textContent, '', 'the error announcement is withdrawn');
});

test('an error raised before a request started shows neither the stale package nor a retry', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'error', requestId: 'old', name: 'Previous extension', error: 'Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.' })));
  const card = store.root.querySelector('.bs-status');
  assert.equal(card.querySelector('.bs-ident'), null);
  assert.equal(btn(card, 'Try again'), undefined);
  assert.ok(btn(card, 'Web Store'));
  assert.equal(card.querySelector('.bs-tech'), null);
});

test('in-progress card: neutral bar, secondary cancel, status announced once', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'downloading', requestId: 'd-1', name: entry.name })));
  const live = store.root.querySelector('[role="status"]');
  assert.equal(live.textContent, 'Downloading and verifying the package…');
  let bar = store.root.querySelector('[role="progressbar"]');
  assert.equal(bar.getAttribute('aria-valuenow'), null, 'unknown progress is indeterminate');
  const cancel = btn(store.root, 'Cancel installation');
  assert.equal(cancel.classList.contains('is-primary'), false);
  cancel.focus();
  store.receive(message(snap({ phase: 'downloading', requestId: 'd-1', name: entry.name, stage: 'download', received: 512 * 1024, total: 1024 * 1024 })));
  bar = store.root.querySelector('[role="progressbar"]');
  assert.equal(bar.getAttribute('aria-valuenow'), '50');
  assert.equal(bar.style.getPropertyValue('--bs-f'), '0.5');
  assert.equal(document.activeElement.textContent, 'Cancel installation', 'a progress tick keeps focus on Cancel');
  assert.equal(store.root.querySelector('[role="status"]'), live, 'the live region persists across renders');
  store.receive(message(snap({ phase: 'cancelled', requestId: 'd-1', name: entry.name })));
  assert.equal(document.activeElement.tagName, 'h2', 'focus moves to the new state when its control goes away');
  assert.equal(document.activeElement.textContent, 'Installation cancelled');
  assert.ok(btn(store.root, 'Try again'));
});

test('review card: consent first in the action row, cancel after it, both bound to the request', () => {
  const store = mount();
  store.receive(message(snap({ phase: 'review', requestId: 'r-1', name: entry.name, version: '3', permissions: ['storage'] })));
  const actions = store.root.querySelector('.bs-actions');
  assert.ok(actions.classList.contains('is-sticky'));
  const [approve, cancel] = actions.querySelectorAll('button');
  assert.equal(approve.textContent, `Install ${entry.name} 3`);
  assert.ok(approve.classList.contains('is-primary'));
  assert.equal(cancel.textContent, 'Cancel installation');
  assert.ok(store.root.textContent.includes('None declared: Required site access'));
  assert.equal(store.root.querySelector('.bs-step[aria-current="step"]').dataset.state, 'current');
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
  // Catalogue cards never get package icons.
  store.receive(message(snap()));
  assert.equal(store.root.querySelectorAll('img').length, 0);
});
test('the page heading is "Extension installation" in every mode', () => {
  const store = mount();
  assert.equal(store.root.querySelector('h1').textContent, 'Extension installation');
  store.receive(message(snap({ phase: 'review', requestId: 'req-h', name: entry.name, version: '1' })));
  assert.equal(store.root.querySelector('h1').textContent, 'Extension installation');
  store.receive(message(snap()));
  assert.equal(store.root.querySelector('h1').textContent, 'Extension installation');
});
