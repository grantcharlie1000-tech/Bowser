// Pure logic for the Privacy section's blocking, cookie-notice, history and website-appearance
// controls (docs/html-chrome-protocol.md "protection"): the strict snapshot contract, the exact
// commands the page may send, and the words. No DOM; test/protection.mjs covers all of it, and the
// Rust fixture (test/fixtures/protection-snapshot.json) keeps the two sides in step.
import type { PanelFields } from '../protocol';

// ---- Closed value sets (Rust: blocker::Level, settings::HistoryRetention, ...) ----

export const LEVELS = ['none', 'trackers', 'ads'] as const;
export type Level = typeof LEVELS[number];
export const LEVEL_LABELS: Readonly<Record<Level, string>> = { none: 'None', trackers: 'Trackers', ads: 'Ads' };
export const LEVEL_ARIA: Readonly<Record<Level, string>> = { none: 'None', trackers: 'Trackers', ads: 'Trackers and ads' };
export const LEVEL_HINTS: Readonly<Record<Level, string>> = {
  none: 'Nothing is blocked. Cookie notices can still be hidden.',
  trackers: 'Stops tracking and measurement requests. Ads are left alone.',
  ads: 'Stops trackers, ad requests and hides ad slots on the page.',
};

/** A site's own level: one of the three, or `default` (follow the global level). */
export const SITE_LEVELS = ['none', 'trackers', 'ads', 'default'] as const;
export type SiteLevel = typeof SITE_LEVELS[number];
export const SITE_LEVEL_LABELS: Readonly<Record<SiteLevel, string>> = { none: 'None', trackers: 'Trackers', ads: 'Ads', default: 'Default' };
/** The `n` the `site-level` command carries (Rust: 0 none, 1 trackers, 2 trackers and ads, 3 follow the global level). */
export const SITE_LEVEL_N: Readonly<Record<SiteLevel, number>> = { none: 0, trackers: 1, ads: 2, default: 3 };

/** A site's cookie-notice choice. */
export const SITE_COOKIES = ['show', 'hide', 'default'] as const;
export type SiteCookies = typeof SITE_COOKIES[number];
export const SITE_COOKIE_LABELS: Readonly<Record<SiteCookies, string>> = { show: 'Show', hide: 'Hide', default: 'Default' };
/** The `n` the `site-cookies` command carries (Rust: 0 show, 1 hide, 2 follow the global switch). */
export const SITE_COOKIE_N: Readonly<Record<SiteCookies, number>> = { show: 0, hide: 1, default: 2 };

export const RETENTIONS = ['session', '1d', '1w', '1m', '3m', '6m', '1y', 'forever'] as const;
export type Retention = typeof RETENTIONS[number];
export const RETENTION_LABELS: Readonly<Record<Retention, string>> = {
  session: 'Session',
  '1d': '1 day',
  '1w': '1 week',
  '1m': '1 month',
  '3m': '3 months',
  '6m': '6 months',
  '1y': '1 year',
  forever: 'Forever',
};

export const APPEARANCES = ['auto', 'light', 'dark'] as const;
export type Appearance = typeof APPEARANCES[number];
export const APPEARANCE_LABELS: Readonly<Record<Appearance, string>> = { auto: 'Auto', light: 'Light', dark: 'Dark' };

export const PRIVATE_THEMES = ['follow', 'light', 'dark'] as const;
export type PrivateThemeKey = typeof PRIVATE_THEMES[number];
export const PRIVATE_THEME_LABELS: Readonly<Record<PrivateThemeKey, string>> = { follow: 'Match', light: 'Light', dark: 'Dark' };

export const SOURCE_KINDS = ['trackers', 'ads', 'cookies'] as const;
export type SourceKind = typeof SOURCE_KINDS[number];
export const SOURCE_KIND_LABELS: Readonly<Record<SourceKind, string>> = { trackers: 'Trackers', ads: 'Ads', cookies: 'Cookie notices' };
export const SOURCE_ORIGINS = ['builtin', 'filters', 'file', 'url'] as const;
export type SourceOrigin = typeof SOURCE_ORIGINS[number];

// ---- Delete Browsing Data ----

/** The kinds, in the order Rust numbers their bits (`browsing_data::DataKind::ALL`): bit n is `DELETE_KINDS[n]`. */
export const DELETE_KINDS = [
  { key: 'history', label: 'History', hint: '' },
  { key: 'cookies', label: 'Cookies', hint: '' },
  { key: 'storage', label: 'Storage', hint: '' },
  { key: 'cache', label: 'Cache', hint: '' },
  { key: 'logins', label: 'Sign-ins', hint: '' },
  { key: 'autofill', label: 'Autofill', hint: '' },
  { key: 'sitesettings', label: 'Site settings', hint: '' },
] as const;
export type DeleteKind = typeof DELETE_KINDS[number]['key'];
export const LOGINS_BIT = 1 << DELETE_KINDS.findIndex((kind) => kind.key === 'logins');
/** The periods, in the order Rust numbers them (`browsing_data::Range::ALL`). */
export const DELETE_RANGES = [
  ['0', 'Past hour'],
  ['1', 'Past day'],
  ['2', 'Past week'],
  ['3', 'Past 4 weeks'],
  ['4', 'Everything'],
] as const;
export type DeleteRangeKey = typeof DELETE_RANGES[number][0];
export const DEFAULT_DELETE_KINDS: readonly DeleteKind[] = ['history', 'cookies', 'storage', 'cache'];

/** The kinds bitmask for `kinds`. */
export function kindsBits(kinds: Iterable<DeleteKind>): number {
  let bits = 0;
  for (const kind of kinds) bits |= 1 << DELETE_KINDS.findIndex((entry) => entry.key === kind);
  return bits;
}

/** What the Delete button says it will do, in the user's words: "Delete browsing history and cookies from the past day". */
export function deleteSummary(kinds: readonly DeleteKind[], range: DeleteRangeKey): string {
  const labels = DELETE_KINDS.filter((kind) => kinds.includes(kind.key)).map((kind) => kind.label.toLowerCase());
  if (labels.length === 0) return 'Choose what to delete';
  const list = labels.length === 1 ? labels[0]! : `${labels.slice(0, -1).join(', ')} and ${labels[labels.length - 1]}`;
  const when = DELETE_RANGES.find(([key]) => key === range)?.[1] ?? '';
  return range === '4' ? `Delete ${list}` : `Delete ${list} from the ${when.toLowerCase()}`;
}

// ---- Limits (Rust: chrome_host::panels::protection) ----

export const MAX_SITES_SHOWN = 200;
export const MAX_ALLOW_SHOWN = 100;
export const MAX_SOURCES = 24;
export const MAX_FIELD = 253;
export const MAX_URL = 2048;
export const MAX_RESULT = 400;
export const MAX_SOURCE_NAME = 60;

// ---- The snapshot ----

export interface CurrentSite { readonly host: string; readonly level: Level; readonly cookies: boolean; readonly private: boolean }
export interface SiteRow { readonly site: string; readonly level: SiteLevel; readonly cookies: SiteCookies }
export interface SourceRow {
  readonly id: string; readonly name: string; readonly kind: SourceKind; readonly enabled: boolean;
  readonly origin: SourceOrigin; readonly url: string; readonly rules: number; readonly removable: boolean;
}
export interface ProtectionResult { readonly seq: number; readonly ok: boolean; readonly text: string }
export interface ProtectionSnapshot {
  readonly level: Level; readonly cookieNotices: boolean; readonly retention: Retention;
  readonly websiteAppearance: Appearance; readonly forceDark: boolean; readonly privateTheme: PrivateThemeKey;
  readonly readable: boolean; readonly guest: boolean; readonly current: CurrentSite | null;
  readonly sites: readonly SiteRow[]; readonly allow: readonly string[]; readonly sources: readonly SourceRow[];
  readonly blocked: { readonly page: number; readonly session: number };
  readonly result: ProtectionResult | null; readonly busy: boolean;
}

const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const exact = (v: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(v).length === keys.length && keys.every((k) => k in v);
const count = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const str = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max;
const oneOf = <T extends string>(v: unknown, list: readonly T[]): v is T => typeof v === 'string' && (list as readonly string[]).includes(v);
const CONTROL = /[\u0000-\u001f\u007f]/;
const SITE = /^[^\s\u0000-\u001f\u007f,/\\:@?#]{1,253}$/u;
const ID = /^[a-z0-9-]{1,24}$/;
const KEYS = ['level', 'cookieNotices', 'retention', 'websiteAppearance', 'forceDark', 'privateTheme', 'readable', 'guest', 'current', 'sites', 'allow', 'sources', 'blocked', 'result', 'busy'] as const;

/** Exact keys and bounded values are required before any snapshot reaches the DOM; `null` for anything else. */
export function parseProtectionSnapshot(value: unknown): ProtectionSnapshot | null {
  if (!record(value) || !exact(value, KEYS)) return null;
  const v = value;
  if (!oneOf(v.level, LEVELS) || typeof v.cookieNotices !== 'boolean' || !oneOf(v.retention, RETENTIONS)) return null;
  if (!oneOf(v.websiteAppearance, APPEARANCES) || typeof v.forceDark !== 'boolean' || !oneOf(v.privateTheme, PRIVATE_THEMES)) return null;
  if (typeof v.readable !== 'boolean' || typeof v.guest !== 'boolean' || typeof v.busy !== 'boolean') return null;
  let current: CurrentSite | null = null;
  if (v.current !== null) {
    const c = v.current;
    if (!record(c) || !exact(c, ['host', 'level', 'cookies', 'private']) || !str(c.host, MAX_FIELD) || !SITE.test(c.host as string)) return null;
    if (!oneOf(c.level, LEVELS) || typeof c.cookies !== 'boolean' || typeof c.private !== 'boolean') return null;
    current = { host: c.host as string, level: c.level, cookies: c.cookies, private: c.private };
  }
  if (!Array.isArray(v.sites) || v.sites.length > MAX_SITES_SHOWN || !Array.isArray(v.allow) || v.allow.length > MAX_ALLOW_SHOWN) return null;
  const sites: SiteRow[] = [];
  const seen = new Set<string>();
  for (const raw of v.sites) {
    if (!record(raw) || !exact(raw, ['site', 'level', 'cookies']) || !str(raw.site, MAX_FIELD) || !SITE.test(raw.site as string) || seen.has(raw.site as string)) return null;
    if (!oneOf(raw.level, SITE_LEVELS) || !oneOf(raw.cookies, SITE_COOKIES)) return null;
    seen.add(raw.site as string);
    sites.push({ site: raw.site as string, level: raw.level, cookies: raw.cookies });
  }
  const allow: string[] = [];
  for (const raw of v.allow) {
    if (!str(raw, MAX_FIELD) || !SITE.test(raw as string)) return null;
    allow.push(raw as string);
  }
  if (!Array.isArray(v.sources) || v.sources.length > MAX_SOURCES) return null;
  const sources: SourceRow[] = [];
  const ids = new Set<string>();
  for (const raw of v.sources) {
    if (!record(raw) || !exact(raw, ['id', 'name', 'kind', 'enabled', 'origin', 'url', 'rules', 'removable'])) return null;
    if (!str(raw.id, 24) || !ID.test(raw.id as string) || ids.has(raw.id as string) || !str(raw.name, MAX_SOURCE_NAME * 2) || CONTROL.test(raw.name as string)) return null;
    if (!oneOf(raw.kind, SOURCE_KINDS) || typeof raw.enabled !== 'boolean' || !oneOf(raw.origin, SOURCE_ORIGINS) || !count(raw.rules) || typeof raw.removable !== 'boolean') return null;
    if (!str(raw.url, MAX_URL) || CONTROL.test(raw.url as string) || (raw.origin === 'url') !== (raw.url !== '')) return null;
    if (raw.url !== '' && !/^https:\/\//.test(raw.url as string)) return null;
    ids.add(raw.id as string);
    sources.push({ id: raw.id as string, name: raw.name as string, kind: raw.kind, enabled: raw.enabled, origin: raw.origin, url: raw.url as string, rules: raw.rules, removable: raw.removable });
  }
  const b = v.blocked;
  if (!record(b) || !exact(b, ['page', 'session']) || !count(b.page) || !count(b.session)) return null;
  let result: ProtectionResult | null = null;
  if (v.result !== null) {
    const r = v.result;
    if (!record(r) || !exact(r, ['seq', 'ok', 'text']) || !count(r.seq) || typeof r.ok !== 'boolean' || !str(r.text, MAX_RESULT * 2)) return null;
    result = { seq: r.seq, ok: r.ok, text: r.text };
  }
  return {
    level: v.level, cookieNotices: v.cookieNotices, retention: v.retention, websiteAppearance: v.websiteAppearance, forceDark: v.forceDark,
    privateTheme: v.privateTheme, readable: v.readable, guest: v.guest, current, sites, allow, sources, blocked: { page: b.page, session: b.session }, result, busy: v.busy,
  };
}

// ---- Commands ----

type Fields = Omit<PanelFields, 'a'>;
const isSite = (v: unknown): v is string => typeof v === 'string' && v.length <= MAX_FIELD && v.includes('.') && SITE.test(v);
const isId = (v: unknown): v is string => typeof v === 'string' && ID.test(v);

/**
 * The flat panel fields for one command, or `null` when they are not exactly what Rust accepts
 * (chrome_host::panels::protection::map_command): each command carries its own fields and no others.
 */
export function protectionCommand(a: string, f: Fields = {}): PanelFields | null {
  const only = (...names: (keyof Fields)[]): boolean => Object.keys(f).length === names.length && names.every((k) => f[k] !== undefined);
  switch (a) {
    case 'blocklevel': return only('text') && oneOf(f.text, LEVELS) ? { a, text: f.text } : null;
    case 'cookienotices': return only('on') && typeof f.on === 'boolean' ? { a, on: f.on } : null;
    case 'retention': return only('text') && oneOf(f.text, RETENTIONS) ? { a, text: f.text } : null;
    case 'webappearance': return only('text') && oneOf(f.text, APPEARANCES) ? { a, text: f.text } : null;
    case 'forcedark': return only('on') && typeof f.on === 'boolean' ? { a, on: f.on } : null;
    case 'privatetheme': return only('text') && oneOf(f.text, PRIVATE_THEMES) ? { a, text: f.text } : null;
    case 'site-level': return only('text', 'n') && isSite(f.text) && Number.isInteger(f.n) && (f.n as number) >= 0 && (f.n as number) <= 3 ? { a, text: f.text, n: f.n } : null;
    case 'site-cookies': return only('text', 'n') && isSite(f.text) && Number.isInteger(f.n) && (f.n as number) >= 0 && (f.n as number) <= 2 ? { a, text: f.text, n: f.n } : null;
    case 'site-remove': return only('text') && isSite(f.text) ? { a, text: f.text } : null;
    case 'src-toggle': return only('text', 'on') && isId(f.text) && typeof f.on === 'boolean' ? { a, text: f.text, on: f.on } : null;
    case 'src-add-url': return only('text', 'url') && oneOf(f.text, SOURCE_KINDS) && typeof f.url === 'string' && f.url.trim() !== '' && f.url.length <= MAX_URL ? { a, text: f.text, url: f.url } : null;
    case 'src-add-file': return only('text') && oneOf(f.text, SOURCE_KINDS) ? { a, text: f.text } : null;
    case 'src-update':
    case 'src-remove': return only('text') && isId(f.text) ? { a, text: f.text } : null;
    case 'delete-data': {
      if (!only('id', 'n', 'on')) return null;
      const bits = f.id as number;
      const range = f.n as number;
      const all = (1 << DELETE_KINDS.length) - 1;
      if (!Number.isInteger(bits) || bits <= 0 || bits > all || !Number.isInteger(range) || range < 0 || range >= DELETE_RANGES.length || typeof f.on !== 'boolean') return null;
      // Saved sign-ins are only ever sent with the explicit confirmation, and the confirmation only with them.
      if (((bits & LOGINS_BIT) !== 0) !== f.on) return null;
      return { a, id: bits, n: range, on: f.on };
    }
    default: return null;
  }
}

// ---- Words ----

/** "3 rules", "1 rule", "No rules (switched off)". */
export function rulesText(source: SourceRow): string {
  if (!source.enabled) return 'Off';
  return source.rules === 1 ? '1 rule' : `${source.rules.toLocaleString('en-GB')} rules`;
}

/** The second line of a list row: what it is, where it came from, how big. */
export function sourceDetail(source: SourceRow): string {
  const where = source.origin === 'builtin' ? 'Built in' : source.origin === 'filters' ? 'filters.txt in your profile' : source.origin === 'file' ? 'From a file' : hostOf(source.url);
  return `${SOURCE_KIND_LABELS[source.kind]} · ${where} · ${rulesText(source)}`;
}

/** The host of a list address, for display ("lists.example"). */
export function hostOf(url: string): string {
  const match = /^https:\/\/([^/?#]+)/.exec(url);
  return match?.[1] ?? url;
}

/** The sentence under "Blocking level" for the current choice. */
export function levelDescription(level: Level, cookieNotices: boolean): string {
  const base = LEVEL_HINTS[level];
  return level === 'none' && cookieNotices ? 'Nothing is blocked. Cookie notices are still hidden.' : base;
}

/** The line for a page-count readout: "3 blocked on this page · 40 this session". */
export function blockedText(page: number, session: number): string {
  return `${page.toLocaleString('en-GB')} blocked on this page · ${session.toLocaleString('en-GB')} this session`;
}

/** What a typed site becomes for the per-site list, or `null`. Same rules as the allow list. */
export function siteFromInput(input: string): string | null {
  let text = input.trim().toLowerCase();
  text = text.replace(/^[a-z][a-z0-9+.-]*:\/\//, '');
  text = text.split(/[/?#]/)[0] ?? '';
  text = text.replace(/:\d+$/, '').replace(/^www\./, '').replace(/\.$/, '');
  return isSite(text) ? text : null;
}

/** Whether a list address typed by the user could be fetched: https, a host with a dot, no user name or secret in it. */
export function listUrlProblem(input: string): string | null {
  const text = input.trim();
  if (text === '') return 'Enter the address of a filter list.';
  if (text.length > MAX_URL) return 'That address is too long.';
  if (CONTROL.test(text)) return 'That is not a web address.';
  if (!/^https:\/\//i.test(text)) return 'Lists are only fetched over https.';
  const rest = text.slice('https://'.length);
  const authority = rest.split(/[/?#]/)[0] ?? '';
  if (authority.includes('@')) return 'An address with a user name in it is not accepted.';
  if (authority.startsWith('[')) return 'Only public web sites are fetched, not local or numeric addresses.';
  if (authority.includes(':')) return 'An address with its own port is not accepted.';
  if (!authority.includes('.') || /^[0-9.]+$/.test(authority)) return 'Only public web sites are fetched, not local or numeric addresses.';
  return null;
}
