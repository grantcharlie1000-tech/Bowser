// Pure parts of the Feeds panel (the dialog page `?view=dialog&p=feeds`): the strict snapshot contract
// with Rust (src/chrome_host/panels/feeds.rs, `snapshot_json`) and the commands the page may send. A
// command never carries an address to open: it names a subscription and an item by id and hash. The only
// address a command carries is the one the user typed or picked to preview or subscribe to.
// No DOM here; test/feeds.mjs covers all of it.

export type FeedKind = 'article' | 'video' | 'podcast';
export type Media = '' | 'audio' | 'video';

export interface FeedRow {
  readonly id: number;
  readonly title: string;
  readonly unread: number;
  readonly total: number;
  /** Seconds since 1970 of the last good fetch; 0 before the first. */
  readonly fetched: number;
  readonly error: string;
  readonly kind: FeedKind;
}
export interface ItemRow {
  readonly feed: number;
  /** The item's hash: with `feed`, what a command names it by. */
  readonly h: number;
  readonly title: string;
  readonly summary: string;
  readonly published: number;
  readonly read: boolean;
  readonly media: Media;
  readonly duration: string;
  /** The item opens on the cookie-free YouTube host. */
  readonly watch: boolean;
  /** The subscription's name, when the list mixes subscriptions. */
  readonly source: string;
}
export interface PreviewItem { readonly title: string; readonly summary: string; readonly published: number }
export interface CandidateRow { readonly url: string; readonly title: string }
export interface PreviewData {
  readonly url: string;
  readonly title: string;
  readonly site: string;
  readonly subscribed: boolean;
  readonly items: readonly PreviewItem[];
  readonly candidates: readonly CandidateRow[];
}
export interface PageFeed extends CandidateRow { readonly subscribed: boolean }
export interface FeedsSnapshot {
  /** A guest window keeps no subscriptions. */
  readonly allowed: boolean;
  readonly busy: boolean;
  /** The "refresh when opened" choice in minutes; 0 fetches only when Refresh is pressed. */
  readonly refresh: number;
  readonly selected: number;
  readonly unread: number;
  readonly feeds: readonly FeedRow[];
  readonly items: readonly ItemRow[];
  readonly preview: PreviewData | null;
  readonly page: readonly PageFeed[];
  readonly status: string;
  readonly error: string;
}

export const MAX_FEEDS = 100;
export const MAX_ITEMS = 120;
export const MAX_PREVIEW_ITEMS = 20;
export const MAX_ADDRESS = 2048;
const U32 = 0xffffffff;

const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const exact = (value: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(value).every((key) => keys.includes(key)) && keys.every((key) => key in value);
const count = (value: unknown, max: number): value is number => Number.isInteger(value) && (value as number) >= 0 && (value as number) <= max;
const text = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const KINDS: readonly string[] = ['article', 'video', 'podcast'];
const MEDIA: readonly string[] = ['', 'audio', 'video'];

function parseFeed(raw: unknown): FeedRow | null {
  if (!record(raw) || !exact(raw, ['id', 'title', 'unread', 'total', 'fetched', 'error', 'kind'])) return null;
  const { id, title, unread, total, fetched, error, kind } = raw;
  if (!count(id, U32) || id === 0 || !text(title, 400) || !count(unread, 100) || !count(total, 100) || !count(fetched, Number.MAX_SAFE_INTEGER) || !text(error, 800)) return null;
  if (typeof kind !== 'string' || !KINDS.includes(kind)) return null;
  return { id, title, unread, total, fetched, error, kind: kind as FeedKind };
}

function parseItem(raw: unknown): ItemRow | null {
  if (!record(raw)) return null;
  const keys = ['feed', 'h', 'title', 'summary', 'published', 'read', 'media', 'duration', 'watch'];
  if (!exact(raw, 'source' in raw ? [...keys, 'source'] : keys)) return null;
  const { feed, h, title, summary, published, read, media, duration, watch } = raw;
  if (!count(feed, U32) || feed === 0 || !count(h, U32) || h === 0 || !text(title, 800) || !text(summary, 1600) || !count(published, Number.MAX_SAFE_INTEGER)) return null;
  if (typeof read !== 'boolean' || typeof watch !== 'boolean' || typeof media !== 'string' || !MEDIA.includes(media) || !text(duration, 64)) return null;
  const source = 'source' in raw ? raw.source : '';
  if (!text(source, 480)) return null;
  return { feed, h, title, summary, published, read, media: media as Media, duration, watch, source };
}

function parsePreview(raw: unknown): PreviewData | null {
  if (!record(raw) || !exact(raw, ['url', 'title', 'site', 'subscribed', 'items', 'candidates'])) return null;
  const { url, title, site, subscribed, items, candidates } = raw;
  if (!text(url, MAX_ADDRESS * 2) || !text(title, 480) || !text(site, MAX_ADDRESS * 2) || typeof subscribed !== 'boolean') return null;
  if (!Array.isArray(items) || items.length > MAX_PREVIEW_ITEMS || !Array.isArray(candidates) || candidates.length > 8) return null;
  const outItems: PreviewItem[] = [];
  for (const row of items) {
    if (!record(row) || !exact(row, ['title', 'summary', 'published']) || !text(row.title, 800) || !text(row.summary, 1600) || !count(row.published, Number.MAX_SAFE_INTEGER)) return null;
    outItems.push({ title: row.title, summary: row.summary, published: row.published });
  }
  const outCandidates: CandidateRow[] = [];
  for (const row of candidates) {
    if (!record(row) || !exact(row, ['url', 'title']) || !text(row.url, MAX_ADDRESS * 2) || !text(row.title, 400)) return null;
    outCandidates.push({ url: row.url, title: row.title });
  }
  return { url, title, site, subscribed, items: outItems, candidates: outCandidates };
}

/** Rejects the whole snapshot unless every field is exactly what Rust sends. */
export function parseFeedsSnapshot(value: unknown): FeedsSnapshot | null {
  if (!record(value) || !exact(value, ['allowed', 'busy', 'refresh', 'selected', 'unread', 'feeds', 'items', 'preview', 'page', 'status', 'error'])) return null;
  if (typeof value.allowed !== 'boolean' || typeof value.busy !== 'boolean' || !count(value.refresh, 100000) || !count(value.selected, U32) || !count(value.unread, 100 * 100)) return null;
  if (!Array.isArray(value.feeds) || value.feeds.length > MAX_FEEDS || !Array.isArray(value.items) || value.items.length > MAX_ITEMS || !Array.isArray(value.page) || value.page.length > 8) return null;
  if (!text(value.status, 800) || !text(value.error, 960)) return null;
  const feeds: FeedRow[] = [];
  const seen = new Set<number>();
  for (const raw of value.feeds) {
    const feed = parseFeed(raw);
    if (!feed || seen.has(feed.id)) return null;
    seen.add(feed.id);
    feeds.push(feed);
  }
  const items: ItemRow[] = [];
  for (const raw of value.items) {
    const item = parseItem(raw);
    if (!item || !seen.has(item.feed)) return null;
    items.push(item);
  }
  let preview: PreviewData | null = null;
  if (value.preview !== null) {
    preview = parsePreview(value.preview);
    if (!preview) return null;
  }
  const page: PageFeed[] = [];
  for (const row of value.page) {
    if (!record(row) || !exact(row, ['url', 'title', 'subscribed']) || !text(row.url, MAX_ADDRESS * 2) || !text(row.title, 400) || typeof row.subscribed !== 'boolean') return null;
    page.push({ url: row.url, title: row.title, subscribed: row.subscribed });
  }
  return {
    allowed: value.allowed, busy: value.busy, refresh: value.refresh, selected: value.selected, unread: value.unread,
    feeds, items, preview, page, status: value.status, error: value.error,
  };
}

// ---- commands ----

export type FeedsCommand =
  | { a: 'select'; id: number }
  | { a: 'preview'; text: string }
  | { a: 'closepreview' }
  | { a: 'subscribe'; text: string }
  | { a: 'unsubscribe'; id: number }
  | { a: 'refresh'; id?: number }
  | { a: 'read'; id: number; parent: number; on: boolean }
  | { a: 'readall'; id?: number }
  | { a: 'open'; id: number; parent: number }
  | { a: 'play'; id: number; parent: number }
  | { a: 'import' };

/** What a person typed, trimmed; `null` when it is empty or longer than Bowser reads. */
export function addressOf(raw: string): string | null {
  const value = raw.trim();
  return value === '' || value.length > MAX_ADDRESS || /[\u0000-\u001f\u007f\s]/.test(value) ? null : value;
}

const id32 = (value: number): boolean => Number.isInteger(value) && value >= 1 && value <= U32;

export const selectCommand = (id: number): FeedsCommand | null => (Number.isInteger(id) && id >= 0 && id <= U32 ? { a: 'select', id } : null);
export const previewCommand = (raw: string): FeedsCommand | null => { const text = addressOf(raw); return text === null ? null : { a: 'preview', text }; };
export const subscribeCommand = (raw: string): FeedsCommand | null => { const text = addressOf(raw); return text === null ? null : { a: 'subscribe', text }; };
export const unsubscribeCommand = (id: number): FeedsCommand | null => (id32(id) ? { a: 'unsubscribe', id } : null);
export const refreshCommand = (id?: number): FeedsCommand | null => (id === undefined ? { a: 'refresh' } : id32(id) ? { a: 'refresh', id } : null);
export const readCommand = (id: number, item: number, on: boolean): FeedsCommand | null => (id32(id) && id32(item) ? { a: 'read', id, parent: item, on } : null);
export const readAllCommand = (id?: number): FeedsCommand | null => (id === undefined || id === 0 ? { a: 'readall' } : id32(id) ? { a: 'readall', id } : null);
export const openCommand = (id: number, item: number): FeedsCommand | null => (id32(id) && id32(item) ? { a: 'open', id, parent: item } : null);
export const playCommand = (id: number, item: number): FeedsCommand | null => (id32(id) && id32(item) ? { a: 'play', id, parent: item } : null);
export const importCommand = (): FeedsCommand => ({ a: 'import' });
export const closePreviewCommand = (): FeedsCommand => ({ a: 'closepreview' });

// ---- words ----

/** "now", "5m", "3h", "2d", then a date: how long ago `published` was, in one short word. */
export function ageText(published: number, now: number): string {
  if (published <= 0) return '';
  const seconds = Math.max(0, now - published);
  if (seconds < 60) return 'now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  if (seconds < 30 * 86400) return `${Math.floor(seconds / 86400)}d`;
  const date = new Date(published * 1000);
  return `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, '0')}-${String(date.getUTCDate()).padStart(2, '0')}`;
}

/** The line under an item's title: where it is from, how old it is and how long an episode runs. */
export function itemMeta(item: Pick<ItemRow, 'source' | 'published' | 'duration' | 'media'>, now: number): string {
  const parts = [item.source, ageText(item.published, now), item.media === '' ? '' : item.duration].filter((part) => part !== '');
  return parts.join(' · ');
}

/** The line under the subscriptions: how fresh they are, and what a refresh is waiting for. */
export function refreshNote(minutes: number): string {
  if (minutes === 0) return 'Feeds are fetched only when you press Refresh.';
  if (minutes < 60) return `Opening Feeds fetches the ones older than ${minutes} minutes.`;
  const hours = minutes / 60;
  return `Opening Feeds fetches the ones older than ${hours === 1 ? 'an hour' : `${hours} hours`}.`;
}

export function unreadText(count: number): string {
  return count === 0 ? 'All read' : `${count} unread`;
}

/** The accessible name of a subscription row. */
export function feedLabel(feed: Pick<FeedRow, 'title' | 'unread' | 'error'>): string {
  return `${feed.title}, ${unreadText(feed.unread).toLowerCase()}${feed.error === '' ? '' : ', could not be fetched'}`;
}

/** The mark every feed row carries for its kind: the glyph names are the icon family's. */
export function kindIcon(kind: FeedKind): 'feed' | 'video' | 'volume' {
  return kind === 'video' ? 'video' : kind === 'podcast' ? 'volume' : 'feed';
}
