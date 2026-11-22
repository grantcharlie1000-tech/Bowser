// Passwords and autofill: the settings section's pure rules and its wire contract with
// src/chrome_host/passwords_page.rs (the shared fixture), plus the page-side detector and fill script
// (src/autofill/*.js) checked as plain JavaScript.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const bundle = (entry) => {
  const out = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', entry)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};
globalThis.window = { chrome: { webview: { postMessage() {}, addEventListener() {} } } };
const logic = await bundle('passwords-logic.ts');
const page = await bundle('settings-page-logic.ts');
const protocol = await bundle('protocol.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'passwords-snapshot.json'), 'utf8'));

test('the snapshot Rust writes parses into lists, preferences and one-shot fields', () => {
  const snap = logic.parsePasswordsSnapshot(fixture);
  assert.ok(snap);
  assert.equal(snap.available, true);
  assert.deepEqual(snap.prefs, { save: true, addresses: true, cards: true });
  assert.deepEqual(snap.never, ['https://never.example']);
  assert.equal(snap.addresses[0].name, 'Jane Doe');
  assert.equal(snap.addresses[0].city, 'Leeds');
  assert.deepEqual(snap.cards[0], { id: 6, name: 'Jane Doe', brand: 'Visa', last4: '4242', month: 4, year: 2028, nick: 'Work' });
  assert.deepEqual(snap.importState, { stage: 'preview', provider: 'Bitwarden', fresh: 3, dup: 1, conflict: 0, bad: 2 });
  assert.equal(snap.ack, 'saved');
  assert.equal(snap.generated, 'Gen#1');
  assert.equal(snap.reveal, null);
  assert.equal(logic.parsePasswordsSnapshot({ ...fixture, p: 'pagetools' }), null);
  assert.equal(logic.parsePasswordsSnapshot({ ...fixture, d: { ...fixture.d, l: 'nope' } }), null);
});

test('the snapshot passes the bridge validator and the section is a known panel token and settings section', () => {
  const parsed = protocol.parseInbound(JSON.stringify(fixture));
  assert.equal(parsed?.t, 'snap');
  assert.ok(protocol.SETTINGS_SECTIONS.includes('passwords'));
  assert.ok(page.SECTIONS.some((s) => s.key === 'passwords'));
  assert.equal(page.areaOf('passwords').key, 'passwords');
  assert.ok(page.ROWS.some((r) => r.id === 'passwords' && r.section === 'passwords'));
});

test('commands carry exactly the fields Rust accepts', () => {
  assert.deepEqual(logic.cmd.copy(7, true), { a: 'pw-copy', id: 7, n: 1 });
  assert.deepEqual(logic.cmd.pref('cards', false), { a: 'pw-pref', text: 'cards', on: false });
  assert.deepEqual(logic.saveLogin(null, ' example.com ', 'me', 'pa\nss'), { a: 'pw-save', text: 'example.com\nme\npa\nss' });
  assert.deepEqual(logic.saveLogin(4, 'https://a.example', 'me', 'x'), { a: 'pw-save', id: 4, text: 'https://a.example\nme\nx' });
  assert.equal(logic.saveLogin(null, '', 'me', 'x'), null);
  assert.equal(logic.saveLogin(null, 'a.example', 'm\ne', 'x'), null);
  assert.deepEqual(logic.saveAddress(null, { name: 'Jane = Doe', postal: 'LS1\n1AA' }), { a: 'af-address-save', text: 'name=Jane Doe\npostal=LS1 1AA' });
  assert.equal(logic.saveAddress(null, {}), null);
  assert.deepEqual(logic.saveCard(null, { name: 'J', number: '4242 4242 4242 4242', month: '4', year: '2028', nick: '' }),
    { a: 'af-card-save', text: 'name=J\nnumber=4242424242424242\nmonth=4\nyear=2028\nnick=' });
  assert.equal(logic.saveCard(null, { name: 'J', number: '4242 4242 4242 4241', month: '', year: '', nick: '' }), null);
  assert.ok(logic.saveCard(3, { name: 'J', number: '', month: '5', year: '2030', nick: '' }), 'an edit may keep the saved number');
  for (const fields of [logic.cmd.refresh(), logic.cmd.copyGenerated(), logic.cmd.importConfirm(), logic.cmd.forgetNever('https://a.example'), logic.cmd.generate(logic.DEFAULT_GENERATOR)]) {
    assert.match(fields.a, /^(?=.{1,24}$)[a-z]+(?:-[a-z]+)*$/, fields.a);
  }
});

test('the generator options use the compact form Rust parses', () => {
  assert.equal(logic.generatorText(logic.DEFAULT_GENERATOR), '20:luds:a');
  assert.equal(logic.generatorText({ ...logic.DEFAULT_GENERATOR, length: 4, symbols: false, avoidAmbiguous: false }), '8:lud');
  assert.equal(logic.generatorText({ length: 200, lower: false, upper: false, digits: false, symbols: false, avoidAmbiguous: false }), '64:l');
  assert.deepEqual(logic.charKinds('aB3$'), ['lower', 'upper', 'digit', 'symbol']);
});

test('strength agrees with the Rust estimate on the same examples', () => {
  for (const weak of ['password', 'Password1!', 'P@ssw0rd', '123456789', 'qwertyuiop', 'aaaaaaaaaaaa', 'abc', 'letmein2024']) {
    assert.ok(logic.strength(weak) < 2, `${weak} scored ${logic.strength(weak)}`);
  }
  for (const strong of ['vT7#qL9!mZ2@xR4$', 'correct-horse-battery-staple-91', 'Xk3$9pQz!vL2mN8&wR5t']) {
    assert.ok(logic.strength(strong) >= 3, `${strong} scored ${logic.strength(strong)}`);
  }
});

test('cards: Luhn, brand, grouping and expiry', () => {
  assert.ok(logic.luhnValid('4242424242424242') && !logic.luhnValid('4242424242424241'));
  assert.equal(logic.cardBrand('378282246310005'), 'American Express');
  assert.equal(logic.cardBrand('5555555555554444'), 'Mastercard');
  assert.equal(logic.formatCardNumber('4242424242424242'), '4242 4242 4242 4242');
  assert.equal(logic.formatCardNumber('378282246310005'), '3782 822463 10005');
  const card = { id: 1, name: '', brand: 'Visa', last4: '4242', month: 4, year: 2028, nick: '' };
  assert.equal(logic.cardExpiry(card), '04/28');
  assert.equal(logic.cardExpired(card, new Date(2028, 4, 1)), true);
  assert.equal(logic.cardExpired(card, new Date(2028, 3, 30)), false);
});

test('lists filter, sort and summarise health', () => {
  const logins = [
    { id: 1, host: 'b.example', origin: 'https://b.example', user: 'z', score: 4, reused: false },
    { id: 2, host: 'a.example', origin: 'https://a.example', user: 'y', score: 1, reused: true },
  ];
  assert.deepEqual(logic.filterLogins(logins, '', 'all').map((l) => l.id), [2, 1]);
  assert.deepEqual(logic.filterLogins(logins, '', 'weak').map((l) => l.id), [2]);
  assert.deepEqual(logic.filterLogins(logins, 'Z', 'all').map((l) => l.id), [1]);
  assert.equal(logic.healthText(logins), '2 saved · 1 weak · 1 reused');
  assert.equal(logic.healthText([]), 'No passwords saved yet');
  assert.equal(logic.monogram('www.github.com'), 'G');
  assert.equal(logic.importSummary({ stage: 'preview', provider: 'X', fresh: 1, dup: 0, conflict: 0, bad: 0 }), '1 new password');
  assert.equal(logic.importSummary({ stage: 'done', added: 3, skipped: 1, failed: 0 }), '3 passwords imported (1 skipped)');
  assert.ok(logic.PROVIDERS.length >= 10 && logic.PROVIDERS.every((p) => p.steps.length === 3));
});

const read = (name) => readFileSync(join(root, 'src', 'autofill', name), 'utf8');
test('the page detector and fill script are valid scripts that never use innerHTML or eval', () => {
  const detector = `(()=>{if(window.top!==window||!window.chrome||!window.chrome.webview)return;${read('fields.js')}${read('page.js')}})();`;
  const fill = `(()=>{const O="https://a.example";const M="form";const V=new Map();${read('fields.js')}${read('fill.js')}})()`;
  assert.doesNotThrow(() => new vm.Script(detector));
  assert.doesNotThrow(() => new vm.Script(fill));
  for (const source of [read('fields.js'), read('page.js'), read('fill.js')]) {
    assert.doesNotMatch(source, /innerHTML|outerHTML|insertAdjacentHTML|\beval\(|new Function/);
  }
  assert.match(read('page.js'), /mode: 'closed'/, 'the fill button lives in a closed shadow root');
});

// ---- the field classifier (src/autofill/fields.js) on a sign-up like Trade Me's, in a small fake DOM ----

function classifier() {
  class El {
    constructor(tag, attrs = {}, label = '') {
      this.tag = tag; this.attrs = attrs; this.type = tag === 'input' ? (attrs.type || 'text') : tag; this.id = attrs.id || '';
      this.labels = label ? [{ textContent: label }] : []; this.disabled = false; this.hidden = false; this.form = null; this.order = 0;
    }
    getAttribute(k) { return k in this.attrs ? this.attrs[k] : null; }
    getBoundingClientRect() { return { width: 100, height: 20 }; }
    closest() { return null; }
    compareDocumentPosition(other) { return other.order > this.order ? 4 : 2; }
  }
  class HTMLInputElement extends El {}
  class HTMLSelectElement extends El {}
  class HTMLTextAreaElement extends El {}
  const form = { fields: [], getAttribute: () => null, id: '', className: '', querySelectorAll() { return this.fields.filter((f) => f.type === 'password'); } };
  const make = (Kind, tag, attrs, label) => { const e = new Kind(tag, attrs, label); e.form = form; e.order = form.fields.length; form.fields.push(e); return e; };
  const ctx = vm.createContext({
    HTMLInputElement, HTMLSelectElement, HTMLTextAreaElement, Node: { DOCUMENT_POSITION_FOLLOWING: 4 }, Map, Set,
    getComputedStyle: () => ({ display: 'block', visibility: 'visible', opacity: '1' }), document: { getElementById: () => null },
  });
  vm.runInContext(`${readFileSync(join(root, 'src', 'autofill', 'fields.js'), 'utf8')}\nthis.api = { bwKind, bwGroup, bwWire };`, ctx);
  const input = (attrs, label) => make(HTMLInputElement, 'input', attrs, label);
  const select = (attrs, label) => make(HTMLSelectElement, 'select', attrs, label);
  return { api: ctx.api, input, select };
}

test('a sign-up form: the email is yours, the username is new, and a date of birth is never a phone or an address', () => {
  const { api, input } = classifier();
  const email = input({ type: 'email', name: 'email', id: 'tg-1' }, 'Email');
  input({ type: 'password', name: 'password', id: 'tg-2', placeholder: '8 or more characters' }, 'Password');
  input({ type: 'password', name: 'confirmPassword', id: 'tg-3' }, 'Confirm password');
  const user = input({ type: 'text', name: 'userName', id: 'tg-4' }, 'Username');
  const kindGroup = (e) => `${api.bwKind(e)}/${api.bwGroup(e, api.bwKind(e))}`;
  assert.equal(kindGroup(email), 'email/identity', 'the email box offers your saved details, not a saved sign-in');
  assert.equal(kindGroup(user), 'username/', 'nothing is offered for a new username');

  const two = classifier();
  const day = two.input({ type: 'tel', name: 'day', id: 'd', placeholder: 'Day' }, 'Day');
  const month = two.select({ name: 'month', id: 'm' }, 'Month');
  const year = two.input({ type: 'tel', name: 'year', id: 'y', placeholder: 'Year (YYYY)' }, 'Year');
  const dob = two.input({ name: 'dob', id: 'b', placeholder: 'DD/MM/YYYY' }, 'Date of birth');
  for (const e of [day, month, year, dob]) assert.equal(two.api.bwKind(e), '', `${e.attrs.name} is left alone`);
  const prefix = two.select({ name: 'primaryPhonePrefix', id: 'pp' }, 'Phone number');
  const number = two.input({ type: 'tel', name: 'primaryPhoneNumber', id: 'pn', placeholder: '1234567' });
  const area = two.input({ name: 'areacode', id: 'ac' }, 'Area code');
  assert.equal(two.api.bwKind(prefix), 'tel-area');
  assert.equal(two.api.bwKind(area), 'tel-area');
  assert.equal(two.api.bwKind(number), 'tel');
  assert.equal(two.api.bwWire('tel-area'), 'tel', 'Bowser only ever hears of a phone');
  assert.equal(two.api.bwKind(two.input({ type: 'tel', name: 'dayphone', id: 'dp' }, 'Daytime phone')), 'tel');

  // A plain sign-in still offers the saved sign-in from its email box.
  const three = classifier();
  const signIn = three.input({ type: 'email', name: 'email', id: 'e' }, 'Email');
  three.input({ type: 'password', name: 'password', id: 'p' }, 'Password');
  assert.equal(three.api.bwGroup(signIn, three.api.bwKind(signIn)), 'login');
});
