// Pure logic for downloads (docs/design/downloads.md): the strict wire contracts (entry object,
// bubble page, downloads page snapshot), Chrome-style byte, speed and time-left words, the status
// line per state, file kinds, which actions a row offers, the toolbar ring and the row patch
// plan. No DOM here; test/downloads.mjs covers all of it.
import { MAX_ID, type DlButtonMsg } from './protocol';
import { dayHeading as historyDayHeading, matchRanges } from './history-page-logic';

export { matchRanges };

// ---- Wire: the entry object ----

export const DL_STATES = ['active', 'paused', 'done', 'failed', 'cancelled', 'missing'] as const;
export type DlState = typeof DL_STATES[number];

export interface DlEntry {
  readonly id: number;
  /** File name only, no path. */
  readonly name: string;
  readonly host: string;
  /** The source http(s) address (Rust truncates it), or '' when there is none to show. */
  readonly url: string;
  readonly state: DlState;
  readonly got: number;
  /** Bytes, -1 when unknown. */
  readonly total: number;
  /** Seconds left, -1 when unknown. */
  readonly eta: number;
  /** Bytes per second now (0 when not active). */
  readonly bps: number;
  /** Short failure reason ('' unless failed). */
  readonly err: string;
  /** The engine can resume it. */
  readonly resume: boolean;
  /** `HH:MM`, local time it finished (or started, while active). */
  readonly clock: string;
}

/** Rust caps characters (200, 120, 200, 60); UTF-16 may need twice the units. */
const MAX_NAME_UNITS = 400;
const MAX_HOST_UNITS = 240;
const MAX_URL_UNITS = 400;
const MAX_ERR_UNITS = 120;
/** Byte counts and times stay exact integers in JS. */
const MAX_COUNT = Number.MAX_SAFE_INTEGER;
export const ENTRY_KEYS = ['id', 'name', 'host', 'url', 'state', 'got', 'total', 'eta', 'bps', 'err', 'resume', 'clock'] as const;
const CLOCK_RE = /^([01]\d|2[0-3]):[0-5]\d$/;
const DATE_RE = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const exactKeys = (value: RecordValue, keys: readonly string[]): boolean => {
  const own = Object.keys(value);
  return own.length === keys.length && own.every((key) => keys.includes(key));
};
const isStrUpTo = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const isInt = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);

/** One entry, every key present and nothing else; `null` for anything Bowser would never send. */
export function parseEntry(value: unknown): DlEntry | null {
  if (!isRecord(value) || !exactKeys(value, ENTRY_KEYS)) return null;
  const { id, name, host, url, state, got, total, eta, bps, err, resume, clock } = value;
  if (!isInt(id, 1, MAX_ID) || !oneOf(state, DL_STATES) || typeof resume !== 'boolean') return null;
  if (!isStrUpTo(name, MAX_NAME_UNITS) || name.trim() === '' || /[\\/\u0000-\u001f]/.test(name)) return null;
  if (!isStrUpTo(host, MAX_HOST_UNITS) || !isStrUpTo(url, MAX_URL_UNITS) || !isStrUpTo(err, MAX_ERR_UNITS)) return null;
  if (url !== '' && !/^https?:\/\//i.test(url)) return null;
  if (!isInt(got, 0, MAX_COUNT) || !isInt(total, -1, MAX_COUNT) || !isInt(eta, -1, MAX_COUNT) || !isInt(bps, 0, MAX_COUNT)) return null;
  if (typeof clock !== 'string' || !CLOCK_RE.test(clock)) return null;
  // Only a failed download has a reason, and only an active one moves bytes.
  if (state !== 'failed' && err !== '') return null;
  if (state !== 'active' && bps !== 0) return null;
  return { id, name, host, url, state, got, total, eta, bps, err, resume, clock };
}

/** Entries in order with unique ids, at most `max`; `null` when any one is malformed. */
function parseEntries(value: unknown, max: number): DlEntry[] | null {
  if (!Array.isArray(value) || value.length > max) return null;
  const seen = new Set<number>();
  const out: DlEntry[] = [];
  for (const item of value) {
    const entry = parseEntry(item);
    if (!entry || seen.has(entry.id)) return null;
    seen.add(entry.id);
    out.push(entry);
  }
  return out;
}

// ---- Wire: the bubble (popup host, page `downloads`) ----

/** The most rows the bubble lists. */
export const BUBBLE_ITEMS = 5;
export interface BubbleMessage {
  readonly t: 'downloads';
  readonly tok: number;
  /** Opened by a download start: closes itself after AUTO_CLOSE_MS unless the user is in it. */
  readonly auto: boolean;
  readonly items: readonly DlEntry[];
  /** Entries beyond the ones shown. */
  readonly more: number;
  /** Active downloads, all of them. */
  readonly active: number;
}
const BUBBLE_KEYS = ['v', 't', 'tok', 'auto', 'items', 'more', 'active'];

export function parseBubble(raw: unknown): BubbleMessage | null {
  if (!isRecord(raw) || raw.t !== 'downloads' || raw.v !== 1 || !exactKeys(raw, BUBBLE_KEYS)) return null;
  const { tok, auto, more, active } = raw;
  if (!isInt(tok, 0, MAX_ID) || typeof auto !== 'boolean' || !isInt(more, 0, MAX_ID) || !isInt(active, 0, MAX_ID)) return null;
  const items = parseEntries(raw.items, BUBBLE_ITEMS);
  if (!items) return null;
  return { t: 'downloads', tok, auto, items, more, active };
}

// ---- Wire: the downloads page snapshot (`snap`, p `downloadspage`) ----

export const PAGE_STATES = ['on', 'error', 'guest'] as const;
export type PageState = typeof PAGE_STATES[number];
export const PAGE_ACKS = ['none', 'removed', 'restored', 'cleared', 'failed'] as const;
export type PageAck = typeof PAGE_ACKS[number];
/** The search field's limit (Rust refuses longer text). */
export const MAX_QUERY = 256;
/** Bowser keeps 500 entries; anything longer is not a Bowser snapshot. */
const MAX_PAGE_ENTRIES = 1000;

export interface DlDay {
  /** `Today`, `Yesterday` or a `YYYY-MM-DD` date. */
  readonly label: string;
  readonly date: string;
  readonly count: number;
  readonly entries: readonly DlEntry[];
}
export interface DownloadsSnapshot {
  readonly state: PageState;
  readonly query: string;
  readonly days: readonly DlDay[];
  /** Older rows were left out to keep the message small. */
  readonly more: boolean;
  readonly undo: boolean;
  readonly ack: PageAck;
  /** Active rows (downloading or paused) across the whole list. */
  readonly active: number;
}
const SNAPSHOT_KEYS = ['state', 'query', 'rows', 'more', 'undo', 'ack', 'active'];
const DAY_KEYS = ['label', 'date', 'count'];

/** Rejects partial, extended or inconsistent snapshots before anything is drawn. */
export function parseDownloadsSnapshot(value: unknown): DownloadsSnapshot | null {
  if (!isRecord(value) || !exactKeys(value, SNAPSHOT_KEYS)) return null;
  const v = value;
  if (!oneOf(v.state, PAGE_STATES) || !oneOf(v.ack, PAGE_ACKS)) return null;
  if (!isStrUpTo(v.query, MAX_QUERY * 2) || typeof v.more !== 'boolean' || typeof v.undo !== 'boolean') return null;
  if (!isInt(v.active, 0, MAX_ID) || !Array.isArray(v.rows)) return null;
  // An unreadable list shows nothing and has nothing to undo.
  if (v.state === 'error' && (v.rows.length > 0 || v.undo)) return null;
  const days: Array<{ label: string; date: string; count: number; entries: DlEntry[] }> = [];
  const seen = new Set<number>();
  let total = 0;
  for (const row of v.rows) {
    if (!isRecord(row)) return null;
    if (exactKeys(row, DAY_KEYS)) {
      if (!isStrUpTo(row.label, 32) || row.label === '' || typeof row.date !== 'string' || !DATE_RE.test(row.date)) return null;
      if (!isInt(row.count, 1, MAX_PAGE_ENTRIES)) return null;
      days.push({ label: row.label, date: row.date, count: row.count, entries: [] });
      continue;
    }
    const day = days[days.length - 1];
    const entry = parseEntry(row);
    if (!day || !entry || seen.has(entry.id)) return null;
    seen.add(entry.id);
    if (++total > MAX_PAGE_ENTRIES) return null;
    day.entries.push(entry);
  }
  if (days.some((day) => day.entries.length !== day.count)) return null;
  return { state: v.state, query: v.query, days, more: v.more, undo: v.undo, ack: v.ack, active: v.active };
}

// ---- Words: bytes, speed, time left ----

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

/**
 * Chrome's byte words (ui::FormatBytes): 1024 steps, one decimal below 100 of a unit, none above,
 * so the width of the number barely changes while it counts up. `512 B`, `1.5 KB`, `12.3 MB`,
 * `456 MB`, `1.2 GB`.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '';
  if (bytes < 1024) return `${Math.floor(bytes)} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) { value /= 1024; unit += 1; }
  // Rounded down, so a count never shows more than arrived and never jumps back.
  const text = value < 100 ? (Math.floor(value * 10) / 10).toFixed(1) : String(Math.floor(value));
  return `${text} ${UNITS[unit]}`;
}

export function formatSpeed(bps: number): string {
  return bps > 0 ? `${formatBytes(bps)}/s` : '';
}

/** `14 sec left`, `3 min left`, `2 hr left`, `3 days left`; '' when unknown. */
export function formatEta(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '';
  if (secs < 60) return `${Math.max(1, Math.round(secs))} sec left`;
  if (secs < 3600) return `${Math.min(59, Math.max(1, Math.round(secs / 60)))} min left`;
  if (secs < 86400) return `${Math.min(23, Math.max(1, Math.round(secs / 3600)))} hr left`;
  const days = Math.max(1, Math.round(secs / 86400));
  return days === 1 ? '1 day left' : `${days} days left`;
}

/** `12.3 MB of 45.6 MB`, or just what arrived when the size is unknown. */
export function progressText(got: number, total: number): string {
  return total >= 0 ? `${formatBytes(got)} of ${formatBytes(total)}` : formatBytes(got);
}

/** The bullet Chrome's bubble puts between parts of a status line. */
export const SEP = ' • ';

/**
 * The status line under a file name. `page` adds the speed to an active row (the page has the
 * room; Chrome's page shows it, the bubble does not).
 */
export function statusText(entry: Pick<DlEntry, 'state' | 'got' | 'total' | 'eta' | 'bps' | 'err'>, where: 'bubble' | 'page' = 'bubble'): string {
  switch (entry.state) {
    case 'active': {
      const left = entry.total >= 0 ? formatEta(entry.eta) : '';
      const text = `${progressText(entry.got, entry.total)}${left ? `, ${left}` : ''}`;
      const speed = where === 'page' ? formatSpeed(entry.bps) : '';
      return speed ? `${speed}${SEP}${text}` : text;
    }
    case 'paused': return `Paused, ${progressText(entry.got, entry.total)}`;
    case 'done': {
      const size = formatBytes(entry.total >= 0 ? entry.total : entry.got);
      return where === 'page' ? size : `${size}${SEP}Done`;
    }
    case 'failed': return entry.err.trim() !== '' ? `Failed, ${entry.err.trim()}` : 'Failed';
    case 'cancelled': return 'Cancelled';
    case 'missing': return 'File moved or deleted';
  }
}

/** How the status line is coloured: failures are warm, gone or cancelled files are quiet. */
export function statusTone(state: DlState): 'normal' | 'warn' | 'quiet' {
  return state === 'failed' ? 'warn' : state === 'cancelled' || state === 'missing' ? 'quiet' : 'normal';
}

/** How far along a row is, 0..1, or `null` when there is no bar (finished) or no size (indeterminate). */
export function fraction(entry: Pick<DlEntry, 'state' | 'got' | 'total'>): number | null {
  if (entry.state !== 'active' && entry.state !== 'paused') return null;
  if (entry.total <= 0) return entry.total === 0 ? 1 : null;
  return Math.max(0, Math.min(1, entry.got / entry.total));
}

/** Whether a row carries a progress bar (active or paused). */
export function hasBar(state: DlState): boolean {
  return state === 'active' || state === 'paused';
}

// ---- Files ----

export const FILE_KINDS = ['image', 'video', 'audio', 'archive', 'document', 'code', 'program', 'other'] as const;
export type FileKind = typeof FILE_KINDS[number];

const KIND_EXTENSIONS: Readonly<Record<Exclude<FileKind, 'other'>, readonly string[]>> = {
  image: ['png', 'jpg', 'jpeg', 'jfif', 'gif', 'webp', 'bmp', 'svg', 'ico', 'tif', 'tiff', 'heic', 'heif', 'avif', 'psd', 'raw', 'dng', 'cr2', 'nef'],
  video: ['mp4', 'm4v', 'mkv', 'mov', 'avi', 'webm', 'wmv', 'flv', 'mpg', 'mpeg', '3gp', 'm2ts', 'mts', 'ogv'],
  audio: ['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg', 'oga', 'opus', 'wma', 'aif', 'aiff', 'mid', 'midi', 'alac'],
  archive: ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'tbz2', 'xz', 'txz', 'zst', 'lz', 'lzma', 'cab', 'iso', 'img', 'vhd', 'vhdx', 'wim'],
  document: ['pdf', 'doc', 'docx', 'odt', 'rtf', 'txt', 'md', 'xls', 'xlsx', 'ods', 'csv', 'tsv', 'ppt', 'pptx', 'odp', 'epub', 'mobi', 'pages', 'key', 'numbers', 'tex', 'log', 'xps', 'oxps', 'one'],
  code: ['js', 'mjs', 'cjs', 'ts', 'tsx', 'jsx', 'json', 'html', 'htm', 'css', 'scss', 'py', 'rs', 'go', 'java', 'kt', 'c', 'h', 'cpp', 'hpp', 'cc', 'cs', 'rb', 'php', 'sh', 'ps1', 'psm1', 'bat', 'cmd', 'sql', 'xml', 'yaml', 'yml', 'toml', 'ini', 'swift', 'lua', 'ipynb', 'wasm'],
  program: ['exe', 'msi', 'msix', 'msixbundle', 'appx', 'appxbundle', 'appinstaller', 'apk', 'aab', 'dmg', 'pkg', 'deb', 'rpm', 'appimage', 'jar', 'com', 'scr', 'crx', 'vsix', 'xpi'],
};
const KIND_OF = new Map<string, FileKind>(Object.entries(KIND_EXTENSIONS).flatMap(([kind, list]) => list.map((ext) => [ext, kind as FileKind] as const)));
/** Two-part extensions kept whole, so `.tar.gz` never loses its `.tar`. */
const DOUBLE_EXTENSIONS = ['tar.gz', 'tar.bz2', 'tar.xz', 'tar.zst', 'tar.lz'];

/** A file name split so the extension can stay on screen while the stem truncates. */
export function splitName(name: string): { stem: string; ext: string } {
  const lower = name.toLowerCase();
  for (const double of DOUBLE_EXTENSIONS) {
    if (lower.endsWith(`.${double}`) && name.length > double.length + 1) return { stem: name.slice(0, -(double.length + 1)), ext: name.slice(-(double.length + 1)) };
  }
  const dot = name.lastIndexOf('.');
  // No extension, a dot file (`.bashrc`), or a "suffix" too long to be an extension.
  if (dot <= 0 || dot === name.length - 1 || name.length - dot > 11 || /\s/.test(name.slice(dot))) return { stem: name, ext: '' };
  return { stem: name.slice(0, dot), ext: name.slice(dot) };
}

/**
 * A name split for middle elision: `head` truncates with an ellipsis, `tail` (the stem's last few
 * characters and the extension) always shows, so `Quarterly report final (approved) v3.pdf` reads
 * `Quarterly report fi… v3.pdf` in a narrow row. Short stems keep only the extension in the tail.
 */
export function nameParts(name: string): { head: string; tail: string } {
  const { stem, ext } = splitName(name);
  const points = Array.from(stem);
  const keep = points.length > 16 ? 4 : 0;
  return { head: points.slice(0, points.length - keep).join(''), tail: points.slice(points.length - keep).join('') + ext };
}

export function fileKind(name: string): FileKind {
  const { ext } = splitName(name);
  const last = ext.toLowerCase().split('.').pop() ?? '';
  return KIND_OF.get(last) ?? 'other';
}

// ---- Actions ----

export const DL_ACTIONS = ['pause', 'resume', 'cancel', 'open', 'show', 'remove', 'copy', 'retry'] as const;
export type DlAction = typeof DL_ACTIONS[number];
/** `dlact` `n` per row action (docs/design/downloads.md). */
export const ACTION_CODE: Readonly<Record<DlAction, number>> = { pause: 0, resume: 1, cancel: 2, open: 3, show: 4, remove: 5, copy: 6, retry: 7 };
/** `dlact` `n` for list-level actions, always with id 0. */
export const LIST_ACTION = { page: 8, folder: 9, clear: 10 } as const;

export const ACTION_LABEL: Readonly<Record<DlAction, string>> = {
  pause: 'Pause', resume: 'Resume', cancel: 'Cancel', open: 'Open', show: 'Show in folder', remove: 'Remove from list', copy: 'Copy link', retry: 'Retry',
};

/**
 * What a row offers, by state: `primary` buttons always show, `secondary` ones join them on hover
 * or focus. Opening a finished file is the row itself (its name), not a button. The same rules as
 * Rust's `DownloadManager::allowed`: Copy link and Retry need a recorded address, Remove only a
 * download that is no longer running.
 */
export function actionsFor(entry: Pick<DlEntry, 'state' | 'resume' | 'url'>): { primary: DlAction[]; secondary: DlAction[] } {
  const linked = entry.url !== '';
  const copy: DlAction[] = linked ? ['copy'] : [];
  switch (entry.state) {
    case 'active': return { primary: ['pause', 'cancel'], secondary: copy };
    case 'paused': return { primary: ['resume', 'cancel'], secondary: copy };
    case 'done': return { primary: ['show'], secondary: [...copy, 'remove'] };
    case 'failed':
      if (entry.resume) return { primary: ['resume'], secondary: [...copy, 'remove'] };
      return linked ? { primary: ['retry'], secondary: [...copy, 'remove'] } : { primary: ['remove'], secondary: [] };
    case 'cancelled': return linked ? { primary: ['retry'], secondary: [...copy, 'remove'] } : { primary: ['remove'], secondary: [] };
    case 'missing': return { primary: ['remove'], secondary: copy };
  }
}

/** Whether `action` is legal for the row right now (Rust checks it again). */
export function canAct(entry: Pick<DlEntry, 'state' | 'resume' | 'url'>, action: DlAction): boolean {
  if (action === 'open') return entry.state === 'done';
  const { primary, secondary } = actionsFor(entry);
  return primary.includes(action) || secondary.includes(action);
}

/** Delete removes a row from the list once it is no longer running. */
export function removable(state: DlState): boolean {
  return state === 'done' || state === 'failed' || state === 'cancelled' || state === 'missing';
}

// ---- Toolbar button ----

/**
 * The aggregate the ring shows over `entries`: received over total of every active download,
 * 0-100, or -1 while any of them has no known size (the ring sweeps). Paused downloads count.
 */
export function aggregatePct(entries: ReadonlyArray<Pick<DlEntry, 'state' | 'got' | 'total'>>): number {
  const running = entries.filter((entry) => hasBar(entry.state));
  if (running.length === 0) return -1;
  let got = 0;
  let total = 0;
  for (const entry of running) {
    if (entry.total < 0) return -1;
    got += Math.min(entry.got, entry.total);
    total += entry.total;
  }
  if (total === 0) return 100;
  return Math.max(0, Math.min(100, Math.floor((got / total) * 100)));
}

/** `stroke-dashoffset` of the ring arc drawn with `pathLength="100"`: 100 is empty, 0 is full. */
export function ringOffset(pct: number): number {
  if (!Number.isFinite(pct) || pct < 0) return 75;
  return 100 - Math.max(0, Math.min(100, pct));
}

/** What the ring does for a `dlbtn` message. */
export function ringMode(msg: Pick<DlButtonMsg, 'state' | 'pct'>): 'none' | 'progress' | 'sweep' | 'paused' {
  if (msg.state === 'active') return msg.pct < 0 ? 'sweep' : 'progress';
  if (msg.state === 'paused') return 'paused';
  return 'none';
}

/** The button's accessible name (its tooltip is shorter, see buttonTip). */
export function buttonLabel(msg: Pick<DlButtonMsg, 'state' | 'n' | 'pct'>): string {
  const pct = msg.pct >= 0 ? `, ${msg.pct}%` : '';
  switch (msg.state) {
    case 'active': return `Downloads, ${msg.n === 1 ? '1 download' : `${msg.n} downloads`} in progress${pct}`;
    case 'paused': return `Downloads, paused${pct}`;
    case 'done': return 'Downloads, download finished';
    case 'failed': return 'Downloads, a download failed';
    case 'idle': return 'Downloads';
  }
}

export function buttonTip(msg: Pick<DlButtonMsg, 'state' | 'n' | 'pct'>): string {
  switch (msg.state) {
    case 'active': return msg.n > 1 ? `${msg.n} downloads${msg.pct >= 0 ? `, ${msg.pct}%` : ''}` : msg.pct >= 0 ? `Downloading, ${msg.pct}%` : 'Downloading';
    case 'paused': return 'Downloads paused';
    case 'done': return 'Download finished';
    case 'failed': return 'Download failed';
    case 'idle': return 'Downloads';
  }
}

/** Rust and the page agree on the slot the button takes from the pill (28px + 4px gap). */
export const DL_BUTTON_SLOT = 32;

// ---- Live patching ----

/** What changed between two versions of a row, so a re-post patches only that. */
export interface EntryChange {
  readonly name: boolean;
  readonly state: boolean;
  readonly status: boolean;
  readonly bar: boolean;
  readonly actions: boolean;
}
export function entryChange(prev: DlEntry | undefined, next: DlEntry, where: 'bubble' | 'page' = 'bubble'): EntryChange {
  if (!prev) return { name: true, state: true, status: true, bar: true, actions: true };
  const actions = (entry: DlEntry): string => { const a = actionsFor(entry); return `${a.primary.join()}|${a.secondary.join()}`; };
  return {
    name: prev.name !== next.name || prev.host !== next.host || prev.url !== next.url || prev.clock !== next.clock,
    state: prev.state !== next.state,
    status: statusText(prev, where) !== statusText(next, where),
    bar: fraction(prev) !== fraction(next) || prev.state !== next.state,
    actions: actions(prev) !== actions(next),
  };
}

/**
 * How the list moves from `prev` to `next` ids: which rows arrive, which leave and which stay.
 * Rows that stay are patched in place (never re-created), so a running bar or animation keeps
 * going; arriving and leaving rows animate, and the rest glide (FLIP).
 */
export function diffIds(prev: readonly number[], next: readonly number[]): { added: number[]; removed: number[]; kept: number[] } {
  const before = new Set(prev);
  const after = new Set(next);
  return {
    added: next.filter((id) => !before.has(id)),
    removed: prev.filter((id) => !after.has(id)),
    kept: next.filter((id) => before.has(id)),
  };
}

/**
 * Keeps one action per row and button from being sent twice before Bowser answers (a double
 * click, a held key, a click while the 4/s re-post is in flight): the same key is refused until
 * `hold` ms have passed or `clear` is called by the next message.
 */
export class ActionGate {
  private readonly sent = new Map<string, number>();
  constructor(private readonly hold = 600, private readonly now: () => number = () => Date.now()) {}
  /** True (and remembered) when `key` may go out now. */
  take(key: string): boolean {
    const at = this.now();
    const last = this.sent.get(key);
    if (last !== undefined && at - last < this.hold) return false;
    this.sent.set(key, at);
    return true;
  }
  clear(): void {
    this.sent.clear();
  }
}

// ---- Page words ----

export function countText(count: number): string {
  return count === 1 ? '1 download' : `${count} downloads`;
}

/** The quiet line under the page title. */
export function summaryText(s: Pick<DownloadsSnapshot, 'state' | 'query' | 'active' | 'more'>, total: number): string {
  if (s.state === 'error') return '';
  const q = s.query.trim();
  if (q !== '') return total === 0 ? `No downloads match “${q}”` : `${countText(total)} found for “${q}”`;
  if (total === 0) return '';
  const running = s.active > 0 ? `${SEP}${s.active} in progress` : '';
  return `${countText(total)}${s.more ? ' shown' : ''}${running}`;
}

export function dayHeading(day: Pick<DlDay, 'label' | 'date'>, year: number): { title: string; caption: string } {
  return historyDayHeading(day, year);
}

export interface EmptyView { readonly kind: 'empty' | 'nomatch' | 'guest' | 'error'; readonly title: string; readonly body: string }
/** The empty state for a snapshot with no rows, or `null` when there are rows. */
export function emptyView(s: Pick<DownloadsSnapshot, 'state' | 'query' | 'days'>): EmptyView | null {
  if (s.state === 'error') return { kind: 'error', title: 'Downloads couldn’t be read', body: 'The download list for this profile could not be opened. Your files are still in their folder. Try again in a moment.' };
  if (s.days.length > 0) return null;
  const q = s.query.trim();
  if (q !== '') return { kind: 'nomatch', title: `No matches for “${q}”`, body: 'Try part of the file name or the site it came from.' };
  if (s.state === 'guest') return { kind: 'guest', title: 'Nothing downloaded in this guest window', body: 'Files you download here are saved to your downloads folder. This list is forgotten when the window closes.' };
  return { kind: 'empty', title: 'No downloads yet', body: 'Files you download appear here, newest first. Press Ctrl+J to come back.' };
}

/** The bubble's footer note when more downloads exist than it lists. */
export function moreText(more: number): string {
  return more > 0 ? `+${more} more` : '';
}

/** Polite announcement when a row's state changes under the user (finished, failed). */
export function announceText(entry: Pick<DlEntry, 'name' | 'state' | 'err'>): string {
  switch (entry.state) {
    case 'done': return `${entry.name} finished downloading`;
    case 'failed': return `${entry.name} failed${entry.err ? `: ${entry.err}` : ''}`;
    case 'cancelled': return `${entry.name} cancelled`;
    case 'paused': return `${entry.name} paused`;
    case 'missing': return `${entry.name} was moved or deleted`;
    case 'active': return `${entry.name} downloading`;
  }
}

/** Row ids in the order the page lists them. */
export function pageOrder(s: Pick<DownloadsSnapshot, 'days'>): number[] {
  return s.days.flatMap((day) => day.entries.map((entry) => entry.id));
}
