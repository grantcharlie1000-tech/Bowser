// Pure parts of the Sessions panel (the dialog page `?view=dialog&p=sessions`): the strict snapshot
// contract with Rust (src/win32/html_dialog.rs `sessions_snapshot`) and the commands the page may send.
// No DOM here; test/sessions.mjs covers all of it.

export interface SessionItem {
  readonly name: string;
  readonly count: number;
  /** Up to three sites the session's pages are on. */
  readonly hosts: readonly string[];
}
export interface SessionsSnapshot {
  /** A guest window keeps no sessions. */
  readonly allowed: boolean;
  /** Tabs open in this window, and how many of them are selected (0 unless at least two are). */
  readonly tabs: number;
  readonly selected: number;
  /** A name nobody has used yet ("Session 3"). */
  readonly suggest: string;
  readonly items: readonly SessionItem[];
}

export const MAX_SESSIONS = 32;
export const MAX_NAME = 100;

const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const exact = (value: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(value).every((key) => keys.includes(key)) && keys.every((key) => key in value);
const count = (value: unknown, max: number): value is number => Number.isInteger(value) && (value as number) >= 0 && (value as number) <= max;
const NAME_RE = /^[^\u0000-\u001f\u007f]+$/;

/** A session name as the page may send and Rust stores: not blank, at most 100 characters, no control characters. */
export function validName(name: unknown): name is string {
  // Characters (code points), as Rust counts them: an emoji is one, not two UTF-16 units.
  return typeof name === 'string' && name.trim() !== '' && [...name].length <= MAX_NAME && NAME_RE.test(name);
}

/** Rejects the whole snapshot unless every field is exactly what Rust sends. */
export function parseSessionsSnapshot(value: unknown): SessionsSnapshot | null {
  if (!record(value) || !exact(value, ['allowed', 'tabs', 'selected', 'suggest', 'items'])) return null;
  if (typeof value.allowed !== 'boolean' || !count(value.tabs, 5000) || !count(value.selected, 5000)) return null;
  if (typeof value.suggest !== 'string' || value.suggest.length > MAX_NAME + 20) return null;
  if (!Array.isArray(value.items) || value.items.length > MAX_SESSIONS) return null;
  const names = new Set<string>();
  const items: SessionItem[] = [];
  for (const raw of value.items) {
    if (!record(raw) || !exact(raw, ['name', 'count', 'hosts']) || typeof raw.name !== 'string' || raw.name === '' || raw.name.length > 128 || names.has(raw.name)) return null;
    if (!count(raw.count, 100) || !Array.isArray(raw.hosts) || raw.hosts.length > 3 || !raw.hosts.every((host) => typeof host === 'string' && host.length <= 64)) return null;
    names.add(raw.name);
    items.push({ name: raw.name, count: raw.count, hosts: [...(raw.hosts as string[])] });
  }
  return { allowed: value.allowed, tabs: value.tabs, selected: value.selected, suggest: value.suggest, items };
}

export type SessionsCommand =
  | { a: 'save'; text: string; n: 0 | 1 }
  | { a: 'open'; text: string; n: 0 | 1 }
  | { a: 'delete'; text: string };

/** The `save` command: all the window's tabs (`selected` false) or the selected ones. Null for a name Bowser would refuse. */
export function saveCommand(name: string, selected: boolean): SessionsCommand | null {
  return validName(name) ? { a: 'save', text: name.trim(), n: selected ? 1 : 0 } : null;
}
/** The `open` command: adds the tabs to this window, or replaces the open ones. */
export function openCommand(name: string, replace: boolean): SessionsCommand | null {
  return validName(name) ? { a: 'open', text: name, n: replace ? 1 : 0 } : null;
}
export function deleteCommand(name: string): SessionsCommand | null {
  return validName(name) ? { a: 'delete', text: name } : null;
}

/** "5 tabs, on example.com, news.org and 1 more site" in one short line. */
export function summaryText(item: SessionItem, sites = item.hosts.length): string {
  const tabs = item.count === 1 ? '1 tab' : `${item.count} tabs`;
  if (item.hosts.length === 0) return tabs;
  const shown = item.hosts.slice(0, Math.max(0, sites)).join(', ');
  return `${tabs} · ${shown}`;
}

/** The line above the list: how many tabs a new session would hold. */
export function openTabsText(tabs: number, selected: number): string {
  const open = tabs === 1 ? '1 open tab' : `${tabs} open tabs`;
  return selected > 1 ? `${open}, ${selected} selected` : open;
}
