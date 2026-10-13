export type PrivacySnapshot = {
  profile: string; guest: boolean;
  stats: { allTime: number; session: number; currentSite: number; topSites: { host: string; count: number }[]; days: { day: string; count: number }[] };
  site: string | null; security: 'secure' | 'notsecure' | 'certproblem' | 'internal' | 'unknown' | 'none';
  blocking: boolean | null; tracking: string | null; smartScreen: boolean | null;
  blockerSetting: boolean; sleeping: boolean; sleepAfter: number;
  permissions: { origin: string; permission: string; decision: string }[]; status: string | null;
};
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const exact = (v: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(v).every(k => keys.includes(k)) && keys.every(k => k in v);
const count = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const str = (v: unknown, max = 512): v is string => typeof v === 'string' && v.length <= max;
const security = ['secure', 'notsecure', 'certproblem', 'internal', 'unknown', 'none'];
const decisions = ['allow', 'block'];
/** Exact keys and bounded primitive values are required before any snapshot reaches the DOM. */
export function parsePrivacySnapshot(value: unknown): PrivacySnapshot | null {
  const keys = ['profile','guest','stats','site','security','blocking','tracking','smartScreen','blockerSetting','sleeping','sleepAfter','permissions','status'];
  if (!record(value) || !exact(value, keys) || !str(value.profile) || typeof value.guest !== 'boolean' || !record(value.stats) || !exact(value.stats, ['allTime','session','currentSite','topSites','days']) || !count(value.stats.allTime) || !count(value.stats.session) || !count(value.stats.currentSite) || !Array.isArray(value.stats.topSites) || !Array.isArray(value.stats.days) || value.stats.topSites.length > 5 || value.stats.days.length > 7 || !(value.site === null || str(value.site)) || !security.includes(value.security as string) || !(value.blocking === null || typeof value.blocking === 'boolean') || !(value.tracking === null || str(value.tracking, 64)) || !(value.smartScreen === null || typeof value.smartScreen === 'boolean') || typeof value.blockerSetting !== 'boolean' || typeof value.sleeping !== 'boolean' || !count(value.sleepAfter) || !Array.isArray(value.permissions) || value.permissions.length > 1024 || !(value.status === null || str(value.status, 512))) return null;
  const topSites: PrivacySnapshot['stats']['topSites'] = [];
  for (const raw of value.stats.topSites) { if (!record(raw) || !exact(raw, ['host','count']) || !str(raw.host) || !count(raw.count)) return null; topSites.push({ host: raw.host, count: raw.count }); }
  const days: PrivacySnapshot['stats']['days'] = [];
  for (const raw of value.stats.days) { if (!record(raw) || !exact(raw, ['day','count']) || !/^\d{4}-\d{2}-\d{2}$/.test(String(raw.day)) || !count(raw.count)) return null; days.push({ day: raw.day as string, count: raw.count }); }
  const permissions: PrivacySnapshot['permissions'] = [];
  for (const raw of value.permissions) { if (!record(raw) || !exact(raw, ['origin','permission','decision']) || !str(raw.origin) || !str(raw.permission, 128) || !decisions.includes(raw.decision as string)) return null; permissions.push({ origin: raw.origin, permission: raw.permission, decision: raw.decision as string }); }
  return { ...value, stats: { allTime: value.stats.allTime, session: value.stats.session, currentSite: value.stats.currentSite, topSites, days }, permissions } as PrivacySnapshot;
}

/** Static "Private tabs" note: [label, description] rows, rendered as plain setting rows. */
export const PRIVATE_TABS_TITLE = 'Private tabs';
export const PRIVATE_TABS_NOTE: readonly (readonly [string, string])[] = [
  ['Open a private tab', 'Press Ctrl+Shift+P, or choose New private tab in the main menu.'],
  ['Nothing saved to disk', 'Private tabs keep no history, cookies, site data, permission choices or blocker statistics on disk.'],
  ['Cookies and site data', 'Held in memory only, and discarded when the last private tab closes.'],
  ['Restore and groups', 'Private tabs are not restored after a restart or crash, and are never grouped.'],
  ['Protection', 'Strict tracking prevention and the blocker are always on. Per-site blocker exceptions are ignored.'],
  ['Permissions and downloads', 'Every permission request and download in a private tab is blocked.'],
];

type Fields = { id?: number; n?: number; on?: boolean };
export function privacyCommand(a: string, fields: Fields = {}): ({ a: string } & Fields) | null {
  if (a === 'clear-stats') return Object.keys(fields).length ? null : { a };
  if (a === 'clear-data' && Number.isInteger(fields.id) && (fields.id as number) >= 0 && Number.isInteger(fields.n) && (fields.n as number) >= 0 && fields.on === undefined) return { a, ...fields };
  if (a === 'permission' && Number.isInteger(fields.id) && (fields.id as number) >= 0 && Number.isInteger(fields.n) && [0, 1, 2].includes(fields.n as number) && fields.on === undefined) return { a, ...fields };
  return null;
}
