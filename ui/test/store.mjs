import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildSync } from 'esbuild';
import { FakeDocument } from './fake-dom.mjs';
const bundle = async (file) => {
  const built = buildSync({ entryPoints: [new URL(`../src/${file}.ts`, import.meta.url).pathname], bundle: true, format: 'esm', platform: 'neutral', write: false });
  return import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].contents).toString('base64')}`);
};
const logic = await bundle('store-logic');
const protocol = await bundle('protocol');
const ui = await bundle('store-section');
const id = 'a'.repeat(32);
const entry = { id, name: 'Helpful <text>', description: 'Make reading pleasant', publisher: 'Example publisher', category: 'Reading', source: 'Chrome Web Store', homepage: 'https://example.com', provenance: 'Published by Example', checked: '2026-10-18', compatibility: 'untested', compatibilityNote: 'No tested version yet.' };
const snap = (extra = {}) => ({ shown: true, catalogue: [entry], phase: 'idle', requestId: '', extensionId: id, name: '', version: '', error: '', restart: false, permissions: [], hostPermissions: [], contentScripts: [], optionalPermissions: [], optionalHostPermissions: [], ...extra });
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
