// Site information page model: payload validation and the act ids Rust declares (site_info.rs).
// Pure (no DOM), tested in test/siteinfo.mjs against the Rust fixture.
import type { Security } from './protocol';

export type PermissionKey = 'camera' | 'mic' | 'location' | 'notifications' | 'popups';
export type PermissionState = 'allow' | 'block' | 'default';

export interface SitePermission {
  readonly id: number;
  readonly k: PermissionKey;
  readonly label: string;
  readonly state: PermissionState;
}

export interface SiteInfoMessage {
  readonly t: 'siteinfo';
  readonly tok: number;
  readonly kind: string;
  readonly host: string;
  readonly origin: string;
  readonly security: Security;
  readonly summary: string;
  readonly perms: readonly SitePermission[];
  readonly cookies: { readonly state: 'count' | 'unavailable' | 'none'; readonly n: number; readonly clear: boolean };
  readonly blocker: { readonly state: 'on' | 'off' | 'unavailable'; readonly n: number };
  readonly tracking: string;
  /** Page zoom in percent (25 to 500); 100 shows no zoom row. */
  readonly zoom: number;
  readonly reset: boolean;
  readonly blockedHere: boolean;
  /** The tab is private: no permission rows or blocker toggle (Rust declares no act for them). */
  readonly private?: boolean;
}

/** Shown in place of the permission rows on a private tab. */
export const PRIVATE_SITE_NOTE = 'Private tab: every permission request is blocked and nothing about this site is saved.';

/** Choice values of a permission row's `act` (Rust: CHOICES). Default stores nothing: blocked. */
export const CHOICES: readonly PermissionState[] = ['allow', 'block', 'default'];
export const CHOICE_LABEL: Readonly<Record<PermissionState, string>> = { allow: 'Allow', block: 'Block', default: 'Default' };
/** Fixed act ids (Rust: ACT_CLEAR_COOKIES, ACT_BLOCKER, ACT_RESET). */
export const ACT_CLEAR_COOKIES = 10;
export const ACT_BLOCKER = 11;
export const ACT_RESET = 12;
/** Rust: ACT_ZOOM_RESET. Declared only while the page is zoomed. */
export const ACT_ZOOM_RESET = 13;
/** Page zoom limits in percent (Rust: site_prefs::ZOOM_MIN, ZOOM_MAX; zoom-logic.ts). */
const ZOOM_MIN = 25;
const ZOOM_MAX = 500;

const SECURITY: readonly Security[] = ['secure', 'notsecure', 'certproblem', 'internal', 'unknown', 'none'];
const KEYS: readonly PermissionKey[] = ['camera', 'mic', 'location', 'notifications', 'popups'];
const MAX_TEXT = 256;
const PAGE_KEYS = ['v', 't', 'tok', 'kind', 'host', 'origin', 'security', 'summary', 'perms', 'cookies', 'blocker', 'tracking', 'zoom', 'reset', 'blockedHere'];

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
function onlyKeys(v: Record<string, unknown>, keys: readonly string[]): boolean {
  return Object.keys(v).every(key => keys.includes(key)) && keys.every(key => key in v);
}
function isStr(v: unknown, max = MAX_TEXT): v is string {
  return typeof v === 'string' && v.length <= max;
}
function isInt(v: unknown, min: number, max: number): v is number {
  return typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
}
function isOneOf<T extends string>(v: unknown, list: readonly T[]): v is T {
  return typeof v === 'string' && (list as readonly string[]).includes(v);
}

/**
 * Strict check of a `siteinfo` message: every key present, nothing extra, closed value sets.
 * `private` is the one optional key (Rust sends it only as `true`); when present it must be a bool.
 */
export function validSiteInfoPayload(raw: unknown): raw is SiteInfoMessage {
  if (!isRecord(raw) || raw.t !== 'siteinfo' || raw.v !== 1) return false;
  const hasPrivate = 'private' in raw;
  if (hasPrivate && typeof raw.private !== 'boolean') return false;
  if (!onlyKeys(raw, hasPrivate ? [...PAGE_KEYS, 'private'] : PAGE_KEYS)) return false;
  if (!isInt(raw.zoom, ZOOM_MIN, ZOOM_MAX)) return false;
  if (!isInt(raw.tok, 0, 0xffffffff) || !isStr(raw.kind, 32) || !isStr(raw.host) || !isStr(raw.origin) || !isStr(raw.summary) || !isStr(raw.tracking, 64)) return false;
  if (!isOneOf(raw.security, SECURITY) || typeof raw.reset !== 'boolean' || typeof raw.blockedHere !== 'boolean') return false;
  if (!Array.isArray(raw.perms) || raw.perms.length > KEYS.length) return false;
  const ids = new Set<number>();
  for (const perm of raw.perms as unknown[]) {
    if (!isRecord(perm) || !onlyKeys(perm, ['id', 'k', 'label', 'state'])) return false;
    if (!isInt(perm.id, 1, 9) || ids.has(perm.id) || !isOneOf(perm.k, KEYS) || !isStr(perm.label, 64) || !isOneOf(perm.state, CHOICES)) return false;
    ids.add(perm.id);
  }
  const cookies = raw.cookies;
  if (!isRecord(cookies) || !onlyKeys(cookies, ['state', 'n', 'clear'])) return false;
  if (!isOneOf(cookies.state, ['count', 'unavailable', 'none'] as const) || !isInt(cookies.n, 0, Number.MAX_SAFE_INTEGER) || typeof cookies.clear !== 'boolean') return false;
  const blocker = raw.blocker;
  if (!isRecord(blocker) || !onlyKeys(blocker, ['state', 'n'])) return false;
  return isOneOf(blocker.state, ['on', 'off', 'unavailable'] as const) && isInt(blocker.n, 0, 0xffffffff);
}

/** Caption under "Cookies". */
export function cookieText(cookies: SiteInfoMessage['cookies']): string {
  switch (cookies.state) {
    case 'count': return cookies.n === 1 ? '1 cookie sent to this site' : `${cookies.n} cookies sent to this site`;
    case 'unavailable': return 'Unavailable right now';
    default: return 'Not used on this page';
  }
}

/** Caption under "Tracker blocker". A private tab's blocker is always on (no toggle). */
export function blockerText(blocker: SiteInfoMessage['blocker'], priv = false): string {
  switch (blocker.state) {
    case 'on': {
      const lead = priv ? 'Always on' : 'On';
      return blocker.n === 1 ? `${lead} · 1 request blocked here` : `${lead} · ${blocker.n} requests blocked here`;
    }
    case 'off': return 'Off for this site';
    default: return 'Unavailable';
  }
}

/** The "a request was blocked" notice; a private tab has nothing to allow. */
export function blockedNoticeText(priv: boolean): string {
  return priv
    ? 'This page asked for a permission and was blocked. Private tabs block every request.'
    : 'This page asked for a permission and was blocked. Allow it below to use it here.';
}
