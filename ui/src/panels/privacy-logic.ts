export const KINDS = ['camera', 'microphone', 'location', 'notifications', 'popups'] as const;
export type Kind = (typeof KINDS)[number];
/** Display names of the kinds, in the order Rust sends them (src/chrome_host/panels/privacy.rs KIND_ORDER). */
export const KIND_LABEL: Readonly<Record<Kind, string>> = { camera: 'Camera', microphone: 'Microphone', location: 'Location', notifications: 'Notifications', popups: 'Popups' };
export const CHOICES = ['allow', 'block', 'default'] as const;
export type Choice = (typeof CHOICES)[number];
export const CHOICE_LABEL: Readonly<Record<Choice, string>> = { allow: 'Allow', block: 'Block', default: 'Default' };

export type SiteRow = { origin: string; current: boolean; kinds: Choice[] };
export type DeniedRow = { origin: string; kind: Kind; ago: number; n: number };
export type PrivacySnapshot = {
  profile: string; guest: boolean;
  stats: { allTime: number; session: number; currentSite: number; topSites: { host: string; count: number }[]; days: { day: string; count: number }[] };
  site: string | null; security: 'secure' | 'notsecure' | 'certproblem' | 'internal' | 'unknown' | 'none';
  blocking: boolean | null; tracking: string | null; smartScreen: boolean | null;
  /** The level Settings will apply on the next start; `tracking` is what the engine reports now. */
  trackingSetting: 'balanced' | 'strict'; efficiencyActive: boolean;
  blockerSetting: boolean; sleeping: boolean; sleepAfter: number;
  sites: SiteRow[]; denied: DeniedRow[]; status: string | null;
};

/** Most sites in one snapshot (Rust: MAX_SITES) and most recent denied requests (Rust: DENIED_LOG_CAP). */
export const MAX_SITES = 1000;
export const MAX_DENIED = 50;
/** Longest origin text on the wire (Rust: MAX_ORIGIN_CHARS). */
export const MAX_ORIGIN = 256;
/** An origin as Rust writes it: http or https, a host, an optional port, nothing else. */
const ORIGIN_RE = /^https?:\/\/[^\s/?#@\\]+$/;
export const validOrigin = (v: unknown): v is string => typeof v === 'string' && v.length <= MAX_ORIGIN && ORIGIN_RE.test(v);

const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const exact = (v: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(v).every(k => keys.includes(k)) && keys.every(k => k in v);
const count = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const str = (v: unknown, max = 512): v is string => typeof v === 'string' && v.length <= max;
const security = ['secure', 'notsecure', 'certproblem', 'internal', 'unknown', 'none'];
const isKind = (v: unknown): v is Kind => typeof v === 'string' && (KINDS as readonly string[]).includes(v);
const isChoice = (v: unknown): v is Choice => typeof v === 'string' && (CHOICES as readonly string[]).includes(v);

/** Exact keys and bounded primitive values are required before any snapshot reaches the DOM. */
export function parsePrivacySnapshot(value: unknown): PrivacySnapshot | null {
  const keys = ['profile','guest','stats','site','security','blocking','tracking','trackingSetting','efficiencyActive','smartScreen','blockerSetting','sleeping','sleepAfter','sites','denied','status'];
  if (!record(value) || !exact(value, keys) || !str(value.profile) || typeof value.guest !== 'boolean' || !record(value.stats) || !exact(value.stats, ['allTime','session','currentSite','topSites','days']) || !count(value.stats.allTime) || !count(value.stats.session) || !count(value.stats.currentSite) || !Array.isArray(value.stats.topSites) || !Array.isArray(value.stats.days) || value.stats.topSites.length > 5 || value.stats.days.length > 7 || !(value.site === null || str(value.site)) || !security.includes(value.security as string) || !(value.blocking === null || typeof value.blocking === 'boolean') || !(value.tracking === null || str(value.tracking, 64)) || (value.trackingSetting !== 'balanced' && value.trackingSetting !== 'strict') || typeof value.efficiencyActive !== 'boolean' || !(value.smartScreen === null || typeof value.smartScreen === 'boolean') || typeof value.blockerSetting !== 'boolean' || typeof value.sleeping !== 'boolean' || !count(value.sleepAfter) || !Array.isArray(value.sites) || value.sites.length > MAX_SITES || !Array.isArray(value.denied) || value.denied.length > MAX_DENIED || !(value.status === null || str(value.status, 512))) return null;
  const topSites: PrivacySnapshot['stats']['topSites'] = [];
  for (const raw of value.stats.topSites) { if (!record(raw) || !exact(raw, ['host','count']) || !str(raw.host) || !count(raw.count)) return null; topSites.push({ host: raw.host, count: raw.count }); }
  const days: PrivacySnapshot['stats']['days'] = [];
  for (const raw of value.stats.days) { if (!record(raw) || !exact(raw, ['day','count']) || !/^\d{4}-\d{2}-\d{2}$/.test(String(raw.day)) || !count(raw.count)) return null; days.push({ day: raw.day as string, count: raw.count }); }
  const sites: SiteRow[] = [];
  const seen = new Set<string>();
  for (const raw of value.sites) {
    if (!record(raw) || !exact(raw, ['origin','current','kinds']) || !validOrigin(raw.origin) || typeof raw.current !== 'boolean' || !Array.isArray(raw.kinds) || raw.kinds.length !== KINDS.length || !raw.kinds.every(isChoice) || seen.has(raw.origin)) return null;
    seen.add(raw.origin);
    sites.push({ origin: raw.origin, current: raw.current, kinds: [...raw.kinds] as Choice[] });
  }
  const denied: DeniedRow[] = [];
  for (const raw of value.denied) {
    if (!record(raw) || !exact(raw, ['origin','kind','ago','n']) || !validOrigin(raw.origin) || !isKind(raw.kind) || !count(raw.ago) || !count(raw.n)) return null;
    denied.push({ origin: raw.origin, kind: raw.kind, ago: raw.ago, n: raw.n });
  }
  return { ...value, stats: { allTime: value.stats.allTime, session: value.stats.session, currentSite: value.stats.currentSite, topSites, days }, sites, denied } as PrivacySnapshot;
}

/** Static "Private tabs" note: [label, description] rows, rendered as plain setting rows. */
export const PRIVATE_TABS_TITLE = 'Private tabs';
export const PRIVATE_TABS_NOTE: readonly (readonly [string, string])[] = [
  ['Open a private tab', 'Press Ctrl+Shift+P, or choose New private tab in the main menu, a tab’s menu or the command palette.'],
  ['Nothing saved to disk', 'Private tabs keep no history, cookies, site data, permission choices or blocker statistics on disk. Only a bookmark you add yourself is kept.'],
  ['Cookies and site data', 'Held in memory only, and erased when your last private tab closes.'],
  ['Restore and groups', 'Private tabs are not restored after a restart or crash, are never grouped, and are left out of saved sessions and workspaces.'],
  ['Protection', 'Strict tracking prevention and the blocker are always on. Per-site blocker exceptions are ignored.'],
  ['Permissions and downloads', 'Every permission request and download in a private tab is blocked.'],
  ['Still visible', 'Websites you visit, and your internet provider, employer or school, can still see your activity.'],
  ['Close them all', 'Choose Close all private tabs in the main menu, a private tab’s menu or the new private tab page.'],
];

type Fields = { id?: number; n?: number; on?: boolean; text?: string };
/** Flat panel fields for one dashboard command, or null when they are not exactly what Rust accepts. */
export function privacyCommand(a: string, fields: Fields = {}): ({ a: string } & Fields) | null {
  const only = (...names: (keyof Fields)[]): boolean => Object.keys(fields).every(k => (names as string[]).includes(k)) && names.every(k => fields[k] !== undefined);
  if (a === 'clear-stats') return Object.keys(fields).length ? null : { a };
  if (a === 'perm-reset-all' || a === 'denied-clear') return Object.keys(fields).length ? null : { a };
  if (a === 'perm-set' && only('id', 'n', 'text') && Number.isInteger(fields.id) && (fields.id as number) >= 0 && (fields.id as number) < KINDS.length && Number.isInteger(fields.n) && (fields.n as number) >= 0 && (fields.n as number) < CHOICES.length && validOrigin(fields.text)) return { a, id: fields.id, n: fields.n, text: fields.text };
  if (a === 'perm-reset' && only('text') && validOrigin(fields.text)) return { a, text: fields.text };
  return null;
}

/** The command for choosing `choice` for `kind` on `origin`. */
export const setPermissionCommand = (origin: string, kind: Kind, choice: Choice) => privacyCommand('perm-set', { id: KINDS.indexOf(kind), n: CHOICES.indexOf(choice), text: origin });

/** Sites whose origin contains every word of `query` (case-insensitive). An empty query keeps all. */
export function filterSites(sites: readonly SiteRow[], query: string): SiteRow[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [...sites];
  return sites.filter(site => words.every(word => site.origin.toLowerCase().includes(word)));
}

/** Whether a site has any stored choice (a Reset site button does something). */
export const hasStored = (site: SiteRow): boolean => site.kinds.some(choice => choice !== 'default');

/** Total stored choices across every site, shown in the Reset all confirmation. */
export const storedCount = (sites: readonly SiteRow[]): number => sites.reduce((total, site) => total + site.kinds.filter(choice => choice !== 'default').length, 0);

/** "just now", "5 min ago", "3 h ago", "2 d ago": the age of a denied request. */
export function agoText(seconds: number): string {
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

/** The sentence under a denied request's origin. */
export function deniedText(row: DeniedRow): string {
  const what: Record<Kind, string> = { camera: 'Camera', microphone: 'Microphone', location: 'Location', notifications: 'Notifications', popups: 'Popup' };
  return `${what[row.kind]} blocked${row.n > 1 ? ` (${row.n} times)` : ''} · ${agoText(row.ago)}`;
}
