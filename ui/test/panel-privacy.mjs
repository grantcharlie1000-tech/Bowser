import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'privacy-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'privacy-snapshot.json'), 'utf8'));

const sample = () => ({ profile: 'Default', guest: false, stats: { allTime: 4, session: 2, currentSite: 1, topSites: [{ host: 'example.test', count: 4 }], days: [{ day: '2026-10-12', count: 4 }] }, site: 'example.test', security: 'secure', blocking: true, tracking: 'balanced', trackingSetting: 'strict', efficiencyActive: false, smartScreen: true, blockerSetting: true, sleeping: false, sleepAfter: 30, sites: [{ origin: 'https://example.test', current: true, kinds: ['default', 'block', 'default', 'default', 'allow'] }], denied: [{ origin: 'https://example.test', kind: 'camera', ago: 5, n: 1 }], status: null });

test('privacy parser accepts exact bounded statistics, settings, sites and denied requests', () => {
  const value = sample();
  assert.deepEqual(logic.parsePrivacySnapshot(value), value);
  assert.equal(logic.parsePrivacySnapshot({ ...value, extra: true }), null);
  assert.equal(logic.parsePrivacySnapshot({ ...value, trackingSetting: 'off' }), null);
  assert.equal(logic.parsePrivacySnapshot({ ...value, efficiencyActive: 'yes' }), null);
  assert.equal(logic.parsePrivacySnapshot({ ...value, stats: { ...value.stats, session: -1 } }), null);
  const { sites, ...withoutSites } = value;
  assert.equal(logic.parsePrivacySnapshot(withoutSites), null, 'sites is required');
  assert.equal(logic.parsePrivacySnapshot({ ...value, permissions: [] }), null, 'the old permissions list is gone');
});

test('the Rust privacy snapshot fixture passes the strict contract', () => {
  assert.equal(fixture.t, 'snap');
  assert.equal(fixture.p, 'privacy');
  const parsed = logic.parsePrivacySnapshot(fixture.d);
  assert.ok(parsed, 'fixture must parse');
  // The current site is first and carries its stored popups choice.
  assert.equal(parsed.sites[0].current, true);
  assert.equal(parsed.sites[0].origin, 'https://news.example');
  assert.equal(parsed.sites[0].kinds[logic.KINDS.indexOf('popups')], 'allow');
  assert.equal(parsed.sites.filter((site) => site.current).length, 1);
  assert.equal(parsed.sites.find((site) => site.origin === 'https://meet.example').kinds[logic.KINDS.indexOf('microphone')], 'allow');
  assert.equal(parsed.sites.find((site) => site.origin === 'http://localhost:8080').kinds[logic.KINDS.indexOf('location')], 'block');
  // Recently denied sites are listed too, so they can be changed.
  for (const row of parsed.denied) assert.ok(parsed.sites.some((site) => site.origin === row.origin), row.origin);
  assert.deepEqual(parsed.denied.map((row) => row.kind), ['location', 'notifications', 'popups']);
  assert.equal(parsed.denied[1].n, 2);
  // The dashboard kind order is the one Rust writes (KIND_ORDER).
  assert.deepEqual([...logic.KINDS], ['camera', 'microphone', 'location', 'notifications', 'popups']);
});

test('privacy parser rejects malformed sites and denied rows', () => {
  const value = sample();
  const site = value.sites[0];
  const denied = value.denied[0];
  for (const bad of [
    { ...value, sites: [{ ...site, extra: 1 }] },
    { ...value, sites: [{ ...site, kinds: site.kinds.slice(0, 4) }] },
    { ...value, sites: [{ ...site, kinds: [...site.kinds, 'allow'] }] },
    { ...value, sites: [{ ...site, kinds: ['ask', 'block', 'default', 'default', 'allow'] }] },
    { ...value, sites: [{ ...site, current: 'yes' }] },
    { ...value, sites: [{ ...site, origin: 'https://example.test/path' }] },
    { ...value, sites: [{ ...site, origin: 'javascript:alert(1)' }] },
    { ...value, sites: [{ ...site, origin: 'file:///C:/x' }] },
    { ...value, sites: [{ ...site, origin: `https://${'a'.repeat(300)}.test` }] },
    { ...value, sites: [site, site] },
    { ...value, sites: Array.from({ length: logic.MAX_SITES + 1 }, (_, i) => ({ ...site, origin: `https://s${i}.test` })) },
    { ...value, denied: [{ ...denied, kind: 'bluetooth' }] },
    { ...value, denied: [{ ...denied, kind: 'geolocation' }] },
    { ...value, denied: [{ ...denied, ago: -1 }] },
    { ...value, denied: [{ ...denied, n: 1.5 }] },
    { ...value, denied: [{ ...denied, extra: true }] },
    { ...value, denied: [{ ...denied, origin: 'https://example.test/secret' }] },
    { ...value, denied: Array.from({ length: logic.MAX_DENIED + 1 }, () => denied) },
  ]) assert.equal(logic.parsePrivacySnapshot(bad), null, JSON.stringify(bad.sites?.[0] ?? bad.denied?.[0]).slice(0, 80));
  assert.ok(logic.parsePrivacySnapshot({ ...value, denied: Array.from({ length: logic.MAX_DENIED }, () => denied) }), 'exactly the cap is fine');
});

test('privacy commands are limited to explicit flat panel fields', () => {
  assert.deepEqual(logic.privacyCommand('clear-stats'), { a: 'clear-stats' });
  assert.deepEqual(logic.privacyCommand('clear-data', { n: 2, id: 3 }), { a: 'clear-data', n: 2, id: 3 });
  assert.equal(logic.privacyCommand('clear-data', { n: 4 }), null);
  assert.equal(logic.privacyCommand('anything'), null);
  assert.equal(logic.privacyCommand('permission', { id: 0, n: 2 }), null, 'the index-based command is gone');
  assert.equal(logic.privacyCommand('clear-stats', { id: 1 }), null);
});

test('permission commands carry an origin, a kind index and a choice, and nothing else', () => {
  assert.deepEqual(logic.setPermissionCommand('https://meet.example', 'microphone', 'allow'), { a: 'perm-set', id: 1, n: 0, text: 'https://meet.example' });
  assert.deepEqual(logic.setPermissionCommand('http://localhost:8080', 'popups', 'default'), { a: 'perm-set', id: 4, n: 2, text: 'http://localhost:8080' });
  assert.deepEqual(logic.setPermissionCommand('https://a.test', 'location', 'block'), { a: 'perm-set', id: 2, n: 1, text: 'https://a.test' });
  assert.equal(logic.setPermissionCommand('https://a.test', 'bluetooth', 'allow'), null, 'unknown kind');
  assert.equal(logic.setPermissionCommand('https://a.test', 'camera', 'ask'), null, 'unknown choice');
  assert.equal(logic.setPermissionCommand('https://a.test/path', 'camera', 'allow'), null, 'not an origin');
  assert.equal(logic.setPermissionCommand('file:///C:/x', 'camera', 'allow'), null);
  assert.equal(logic.setPermissionCommand('', 'camera', 'allow'), null);
  assert.equal(logic.privacyCommand('perm-set', { id: 5, n: 0, text: 'https://a.test' }), null, 'kind index out of range');
  assert.equal(logic.privacyCommand('perm-set', { id: 0, n: 3, text: 'https://a.test' }), null, 'choice out of range');
  assert.equal(logic.privacyCommand('perm-set', { id: -1, n: 0, text: 'https://a.test' }), null);
  assert.equal(logic.privacyCommand('perm-set', { id: 0.5, n: 0, text: 'https://a.test' }), null);
  assert.equal(logic.privacyCommand('perm-set', { id: 0, n: 0 }), null, 'origin required');
  assert.equal(logic.privacyCommand('perm-set', { id: 0, n: 0, text: 'https://a.test', on: true }), null, 'no extra fields');
  assert.deepEqual(logic.privacyCommand('perm-reset', { text: 'https://a.test' }), { a: 'perm-reset', text: 'https://a.test' });
  assert.equal(logic.privacyCommand('perm-reset', {}), null);
  assert.equal(logic.privacyCommand('perm-reset', { text: 'https://a.test/x' }), null);
  assert.equal(logic.privacyCommand('perm-reset', { text: 'https://a.test', id: 1 }), null);
  assert.deepEqual(logic.privacyCommand('perm-reset-all'), { a: 'perm-reset-all' });
  assert.deepEqual(logic.privacyCommand('denied-clear'), { a: 'denied-clear' });
  assert.equal(logic.privacyCommand('perm-reset-all', { text: 'https://a.test' }), null);
  assert.equal(logic.privacyCommand('denied-clear', { n: 1 }), null);
});

test('site search matches every word of the origin, case-insensitively', () => {
  const sites = ['https://news.example', 'http://localhost:8080', 'https://meet.example', 'https://Docs.Example'].map((origin) => ({ origin, current: false, kinds: ['default', 'default', 'default', 'default', 'default'] }));
  assert.equal(logic.filterSites(sites, '').length, 4);
  assert.equal(logic.filterSites(sites, '   ').length, 4);
  assert.deepEqual(logic.filterSites(sites, 'EXAMPLE').map((site) => site.origin), ['https://news.example', 'https://meet.example', 'https://Docs.Example']);
  assert.deepEqual(logic.filterSites(sites, 'http: 8080').map((site) => site.origin), ['http://localhost:8080']);
  assert.deepEqual(logic.filterSites(sites, 'docs example').map((site) => site.origin), ['https://Docs.Example']);
  assert.equal(logic.filterSites(sites, 'nothing').length, 0);
  assert.notEqual(logic.filterSites(sites, ''), sites, 'a copy, never the input');
});

test('stored-choice counts drive Reset site and Reset all', () => {
  const none = { origin: 'https://a.test', current: true, kinds: ['default', 'default', 'default', 'default', 'default'] };
  const two = { origin: 'https://b.test', current: false, kinds: ['allow', 'default', 'block', 'default', 'default'] };
  assert.equal(logic.hasStored(none), false);
  assert.equal(logic.hasStored(two), true);
  assert.equal(logic.storedCount([none, two]), 2);
  assert.equal(logic.storedCount([]), 0);
});

test('denied request lines read as plain sentences with a coarse age', () => {
  assert.equal(logic.agoText(0), 'just now');
  assert.equal(logic.agoText(59), 'just now');
  assert.equal(logic.agoText(60), '1 min ago');
  assert.equal(logic.agoText(3599), '59 min ago');
  assert.equal(logic.agoText(3600), '1 h ago');
  assert.equal(logic.agoText(86_399), '23 h ago');
  assert.equal(logic.agoText(86_400), '1 d ago');
  assert.equal(logic.deniedText({ origin: 'https://a.test', kind: 'camera', ago: 120, n: 1 }), 'Camera blocked · 2 min ago');
  assert.equal(logic.deniedText({ origin: 'https://a.test', kind: 'popups', ago: 5, n: 3 }), 'Popup blocked (3 times) · just now');
  assert.equal(logic.deniedText({ origin: 'https://a.test', kind: 'location', ago: 7200, n: 1 }), 'Location blocked · 2 h ago');
});

test('the Private tabs note says, in plain words, what private tabs keep and block', () => {
  assert.equal(logic.PRIVATE_TABS_TITLE, 'Private tabs');
  const text = logic.PRIVATE_TABS_NOTE.map(([label, body]) => `${label}. ${body}`).join(' ');
  for (const phrase of [
    'Ctrl+Shift+P', 'New private tab', 'no history, cookies, site data, permission choices or blocker statistics on disk',
    'memory only', 'last private tab closes', 'not restored after a restart or crash', 'never grouped',
    'Only a bookmark you add yourself is kept', 'can still see your activity', 'Close all private tabs',
    'Strict tracking prevention and the blocker are always on', 'Per-site blocker exceptions are ignored',
    'Every permission request and download in a private tab is blocked',
  ]) assert.ok(text.includes(phrase), phrase);
  // Sentence case, no uppercase eyebrows.
  for (const [label] of logic.PRIVATE_TABS_NOTE) assert.notEqual(label, label.toUpperCase());
  assert.notEqual(logic.PRIVATE_TABS_TITLE, logic.PRIVATE_TABS_TITLE.toUpperCase());
});
