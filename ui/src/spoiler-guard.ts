// --- spoiler guard ---
// Masks Spoiler Firewall spoilers in Bowser's own chrome (omnibox suggestions, history, the tab hover
// card, the command palette, tab titles). Detection is the SHARED engine (the classify and policy
// fragments the page injection runs, generated into spoiler-engine.gen.ts): no second classifier.
//
// A risky row is not removed. It is drawn as "Spoiler hidden · GTA VI" with the address host only, and
// a "Show" button reveals that one row for this view's lifetime (`RevealedRows`). Layout and keyboard
// navigation stay as they are, and Enter on a masked row still navigates: the user chose it, and the
// destination page is protected by the page engine.
import SFW from './spoiler-engine.gen';
import { activeSpoilerMode, type SpoilerMode } from './spoiler-config';

export const MASK_LABEL = 'Spoiler hidden · GTA VI';
/** What a masked tab title reads: the same words the page engine rewrites a risky document title to. */
export const TITLE_MASK = 'Spoiler-protected page';
export const SHOW_LABEL = 'Show';

type Mode = SpoilerMode | 'off';
interface Result { readonly level: string; readonly flags: object }
interface Engine {
  readonly classify: { text(text: string, ctx: { hot: number; source: string }): Result; url(url: string): Result };
  readonly policy: { decide(result: Result, opts: { mode: SpoilerMode; hot: number; kind: string }): 'shield' | 'clear' | 'hold' };
}
const engine = SFW as unknown as Engine;

const CACHE_MAX = 4000;
const cache = new Map<string, boolean>();

function remember(key: string, compute: () => boolean): boolean {
  const hit = cache.get(key);
  if (hit !== undefined) return hit;
  let risky: boolean;
  // A failure is never a clear: the row stays masked.
  try { risky = compute(); } catch { risky = true; }
  if (cache.size >= CACHE_MAX) cache.clear();
  cache.set(key, risky);
  return risky;
}

const decided = (result: Result, mode: SpoilerMode): boolean =>
  engine.policy.decide(result, { mode, hot: 0, kind: 'title' }) !== 'clear';

/** Whether `text` (a title, label or suggestion) reads as a spoiler in `mode`. Always false when off. */
export function riskyText(text: string, mode: Mode = activeSpoilerMode()): boolean {
  if (mode === 'off' || text.length < 2) return false;
  return remember(`t|${mode}|${text}`, () => decided(engine.classify.text(text, { hot: 0, source: 'title' }), mode));
}

/** Whether `url` (full, or the display form without a scheme) carries a spoiler in its host, path, query or hash. */
export function riskyUrl(url: string, mode: Mode = activeSpoilerMode()): boolean {
  if (mode === 'off' || url.length < 2) return false;
  return remember(`u|${mode}|${url}`, () => {
    const full = /^[a-z][a-z0-9+.-]*:/i.test(url) ? url : `https://${url}`;
    return decided(engine.classify.url(full), mode);
  });
}

/** The host of `url` (display or full form), or '' when it is not an address. Never a path or query. */
export function hostOnly(url: string): string {
  const text = url.trim();
  if (text === '' || /\s/.test(text)) return '';
  const rest = text.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '').replace(/^[^/?#@]*@/, '');
  return rest.split(/[/?#]/, 1)[0] ?? '';
}

export interface GuardFields {
  readonly title?: string;
  /** What the row would put in the omnibox; for a search row, the suggested query. */
  readonly text?: string;
  readonly url?: string;
}

/** Whether any shown field of a row is risky. Text equal to the user's own `typed` input is never hidden from them. */
export function rowRisky(fields: GuardFields, mode: Mode = activeSpoilerMode(), typed = ''): boolean {
  if (mode === 'off') return false;
  const bare = (value: string): string => value.trim().toLowerCase().replace(/^[a-z][a-z0-9+.-]*:\/\//, '').replace(/^www\./, '').replace(/\/$/, '');
  const own = bare(typed);
  const isOwn = (value: string): boolean => own !== '' && bare(value) === own;
  const title = fields.title ?? '';
  const text = fields.text ?? '';
  const url = fields.url ?? '';
  return (title !== '' && !isOwn(title) && riskyText(title, mode))
    || (text !== '' && !isOwn(text) && riskyText(text, mode))
    || (url !== '' && !isOwn(url) && riskyUrl(url, mode));
}

/** A tab or page title as the chrome shows it: the engine's guard wording when it is risky. */
export function guardTitle(title: string, mode: Mode = activeSpoilerMode()): string {
  return riskyText(title, mode) ? TITLE_MASK : title;
}

export interface GuardedRow {
  readonly masked: boolean;
  readonly title: string;
  /** The address to show: the original when clear or revealed, else its host only. */
  readonly url: string;
}

/** What a row shows: itself when clear or `revealed`, else the mask label and the host. */
export function guardRow(fields: GuardFields, revealed: boolean, mode: Mode = activeSpoilerMode(), typed = ''): GuardedRow {
  const title = fields.title ?? '';
  const url = fields.url ?? '';
  if (revealed || !rowRisky(fields, mode, typed)) return { masked: false, title, url };
  return { masked: true, title: MASK_LABEL, url: hostOnly(url) };
}

/** Rows the user chose to reveal, for the life of one view (never stored). Keys are caller-built. */
export class RevealedRows {
  private readonly keys = new Set<string>();
  has(key: string): boolean { return this.keys.has(key); }
  add(key: string): void { this.keys.add(key); }
  clear(): void { this.keys.clear(); }
}

/** One stable key for a row's identity, from the fields that identify it. */
export function rowKey(...parts: readonly (string | number)[]): string {
  return parts.join('\u0001');
}
// --- end spoiler guard ---
