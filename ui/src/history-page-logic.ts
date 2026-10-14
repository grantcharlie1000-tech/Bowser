// Pure logic for the history page (?view=history, docs/design/history-page.md): the strict
// snapshot contract (chrome_host/history_page.rs), day headings, the address line, search marks,
// keyboard movement and the words that depend on state. No DOM here; test/history-page.mjs covers it.
import { MAX_ID } from './protocol';
import { isFavUri } from './panels/kit';

export const HISTORY_STATES = ['on', 'off', 'guest', 'error'] as const;
export type HistoryState = typeof HISTORY_STATES[number];
export const HISTORY_ACKS = ['none', 'removed', 'restored', 'failed'] as const;
export type HistoryAck = typeof HISTORY_ACKS[number];
/** The search field's limit; Rust refuses longer text (`MAX_QUERY_CHARS`). */
export const MAX_QUERY = 256;
/** Rust caps titles at 120 and addresses at 200 characters; UTF-16 may need twice the units. */
const MAX_TITLE_UNITS = 240;
const MAX_URL_UNITS = 400;
const MAX_HOST_UNITS = 128;
/** History keeps at most 500 addresses; anything longer is not a Bowser snapshot. */
const MAX_ENTRIES = 1000;

export interface HistoryEntry {
  readonly id: number;
  readonly title: string;
  readonly host: string;
  readonly url: string;
  readonly clock: string;
}
export interface HistoryDay {
  /** `Today`, `Yesterday` or a `YYYY-MM-DD` date (Rust's day bucket). */
  readonly label: string;
  /** The local date of the group, `YYYY-MM-DD`. */
  readonly date: string;
  readonly count: number;
  readonly entries: readonly HistoryEntry[];
}
export interface HistorySnapshot {
  readonly state: HistoryState;
  readonly query: string;
  readonly days: readonly HistoryDay[];
  /** Older rows were left out to keep the message small. */
  readonly more: boolean;
  readonly undo: boolean;
  readonly ack: HistoryAck;
  readonly favs: Readonly<Record<string, string>>;
}

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const exactKeys = (value: RecordValue, keys: readonly string[]): boolean => {
  const own = Object.keys(value);
  return own.length === keys.length && own.every((key) => keys.includes(key));
};
const isStrUpTo = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);
const DATE_RE = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;
const CLOCK_RE = /^([01]\d|2[0-3]):[0-5]\d$/;
const SNAPSHOT_KEYS = ['state', 'query', 'rows', 'more', 'undo', 'ack', 'favs'];
const DAY_KEYS = ['label', 'date', 'count'];
const ENTRY_KEYS = ['id', 'title', 'host', 'url', 'clock'];

/** Rejects partial, extended or inconsistent snapshots before anything is drawn. */
export function parseHistorySnapshot(value: unknown): HistorySnapshot | null {
  if (!isRecord(value) || !exactKeys(value, SNAPSHOT_KEYS)) return null;
  const v = value;
  if (!oneOf(v.state, HISTORY_STATES) || !oneOf(v.ack, HISTORY_ACKS)) return null;
  if (!isStrUpTo(v.query, MAX_QUERY * 2) || typeof v.more !== 'boolean' || typeof v.undo !== 'boolean') return null;
  if (!Array.isArray(v.rows) || !isRecord(v.favs)) return null;
  // Nothing is listed unless history is recorded (a guest or a switched-off profile shows none).
  if (v.state !== 'on' && (v.rows.length > 0 || v.undo)) return null;
  const days: Array<{ label: string; date: string; count: number; entries: HistoryEntry[] }> = [];
  const seen = new Set<number>();
  let total = 0;
  for (const row of v.rows) {
    if (!isRecord(row)) return null;
    if (exactKeys(row, DAY_KEYS)) {
      if (!isStrUpTo(row.label, 32) || row.label === '' || typeof row.date !== 'string' || !DATE_RE.test(row.date)) return null;
      if (typeof row.count !== 'number' || !Number.isSafeInteger(row.count) || row.count < 1) return null;
      days.push({ label: row.label, date: row.date, count: row.count, entries: [] });
      continue;
    }
    if (!exactKeys(row, ENTRY_KEYS)) return null;
    const day = days[days.length - 1];
    if (!day) return null; // an entry before any day heading
    const { id, title, host, url, clock } = row;
    if (typeof id !== 'number' || !Number.isSafeInteger(id) || id < 1 || id > MAX_ID || seen.has(id)) return null;
    if (!isStrUpTo(title, MAX_TITLE_UNITS) || !isStrUpTo(host, MAX_HOST_UNITS) || !isStrUpTo(url, MAX_URL_UNITS) || typeof clock !== 'string' || !CLOCK_RE.test(clock)) return null;
    if (!/^https?:\/\//i.test(url)) return null;
    seen.add(id);
    if (++total > MAX_ENTRIES) return null;
    day.entries.push({ id, title, host, url, clock });
  }
  if (days.some((day) => day.entries.length !== day.count)) return null;
  const favs: Record<string, string> = {};
  for (const [host, uri] of Object.entries(v.favs)) {
    if (host === '' || host.length > MAX_HOST_UNITS || !isFavUri(uri)) return null;
    favs[host] = uri;
  }
  return { state: v.state, query: v.query, days, more: v.more, undo: v.undo, ack: v.ack, favs };
}

// ---- Words ----

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

/** `Wednesday 14 October`, with the year when it is not `year`. Empty for a malformed date. */
export function longDate(date: string, year: number): string {
  if (!DATE_RE.test(date)) return '';
  const [y, m, d] = date.split('-').map(Number) as [number, number, number];
  const weekday = WEEKDAYS[new Date(Date.UTC(y, m - 1, d)).getUTCDay()] ?? '';
  return `${weekday} ${d} ${MONTHS[m - 1]}${y === year ? '' : ` ${y}`}`;
}

/** A day group's heading and the quieter date beside it (Today and Yesterday keep their date). */
export function dayHeading(day: Pick<HistoryDay, 'label' | 'date'>, year: number): { title: string; caption: string } {
  const long = longDate(day.date, year);
  if (day.label === 'Today' || day.label === 'Yesterday') return { title: day.label, caption: long };
  return { title: long || day.label, caption: '' };
}

export function pagesText(count: number): string {
  return count === 1 ? '1 page' : `${count} pages`;
}

/** What the search found, for the polite live region and the result line. */
export function resultText(count: number, query: string): string {
  const q = query.trim();
  if (q === '') return '';
  if (count === 0) return `No pages match “${q}”`;
  return `${pagesText(count)} found for “${q}”`;
}

/** The address line under a title: the host, then the rest of the address (scheme and `www.` dropped). */
export function addressParts(url: string, host: string): { host: string; rest: string } {
  let text = url.replace(/^https?:\/\//i, '');
  text = text.replace(/^www\./i, '');
  if (host !== '' && text.toLowerCase().startsWith(host.toLowerCase())) {
    const rest = text.slice(host.length).replace(/^\/$/, '');
    return { host, rest: rest.replace(/\/$/, '') };
  }
  return { host: host || text, rest: '' };
}

/** A row's name: its title, else its host, else its address. */
export function rowName(entry: Pick<HistoryEntry, 'title' | 'host' | 'url'>): string {
  return entry.title.trim() || entry.host || entry.url;
}

/**
 * Where `query` occurs in `text`, as [start, end) ranges. Bowser matches the whole trimmed query
 * as one case-insensitive substring (productivity::search_entries), so the marks do too. Text whose
 * lower-case form changes length (a few scripts) is left unmarked rather than marked wrongly.
 */
export function matchRanges(text: string, query: string): Array<[number, number]> {
  const q = query.trim().toLowerCase();
  if (q === '') return [];
  const lower = text.toLowerCase();
  if (lower.length !== text.length) return [];
  const ranges: Array<[number, number]> = [];
  for (let from = 0; ;) {
    const at = lower.indexOf(q, from);
    if (at < 0) break;
    ranges.push([at, at + q.length]);
    from = at + q.length;
  }
  return ranges;
}

/** A stable tone (0-7, the group palette) for a host's letter tile. */
export function tileTone(host: string): number {
  let hash = 0;
  for (const ch of host) hash = (Math.imul(hash, 31) + (ch.codePointAt(0) ?? 0)) >>> 0;
  return hash % 8;
}

// ---- Keyboard ----

/** The row index a list key moves to from `current` (-1: none yet), clamped; `null` when the key is not a list key. */
export function moveIndex(count: number, current: number, key: string, page = 10): number | null {
  if (count <= 0) return null;
  switch (key) {
    case 'ArrowDown': return current < 0 ? 0 : Math.min(count - 1, current + 1);
    case 'ArrowUp': return current < 0 ? count - 1 : Math.max(0, current - 1);
    case 'Home': return 0;
    case 'End': return count - 1;
    case 'PageDown': return current < 0 ? Math.min(count - 1, page - 1) : Math.min(count - 1, current + page);
    case 'PageUp': return current < 0 ? 0 : Math.max(0, current - page);
    default: return null;
  }
}

/** After `id` is removed from `ids`, the row that takes the focus: the next one, else the previous. */
export function focusAfterRemoval(ids: readonly number[], id: number): number | null {
  const at = ids.indexOf(id);
  if (at < 0) return null;
  return ids[at + 1] ?? ids[at - 1] ?? null;
}

/** Ctrl or Cmd click, a middle click, and Ctrl/Shift+Enter open an entry in a new tab. */
export function opensNewTab(event: { ctrlKey: boolean; metaKey: boolean; shiftKey?: boolean; button?: number }): boolean {
  return event.ctrlKey || event.metaKey || event.shiftKey === true || event.button === 1;
}

/** The empty state for a snapshot with no rows, or `null` when there are rows. */
export interface EmptyView { readonly kind: 'empty' | 'nomatch' | 'off' | 'guest' | 'error'; readonly title: string; readonly body: string }
export function emptyView(s: Pick<HistorySnapshot, 'state' | 'query' | 'days'>): EmptyView | null {
  switch (s.state) {
    case 'off':
      return { kind: 'off', title: 'History is off', body: 'Bowser isn’t saving the pages you visit in this profile. Turn on “Save browsing history” in Settings to see them here.' };
    case 'guest':
      return { kind: 'guest', title: 'Guest windows never save history', body: 'Pages you visit in this guest window are forgotten when it closes. Nothing is written to disk.' };
    case 'error':
      return { kind: 'error', title: 'History couldn’t be read', body: 'The history file for this profile could not be opened. Try again in a moment.' };
    case 'on':
      break;
  }
  if (s.days.length > 0) return null;
  const q = s.query.trim();
  if (q !== '') return { kind: 'nomatch', title: `No matches for “${q}”`, body: 'Try fewer letters, or part of the address.' };
  return { kind: 'empty', title: 'No history yet', body: 'Pages you visit will appear here, grouped by day.' };
}
