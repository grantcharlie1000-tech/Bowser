// --- omnibox suggest ---
// Pure rules for the omnibox dropdown, shared by the toolbar (keyboard) and the hover view (rows).
import type { IconName } from './dom';
import type { SuggestKind } from './protocol';

/** Row after `sel` when the arrow `key` is pressed in a list of `count` rows; wraps at both ends. */
export function moveSuggestSelection(sel: number, count: number, key: 'ArrowDown' | 'ArrowUp'): number {
  if (count <= 0) return 0;
  const step = key === 'ArrowDown' ? 1 : count - 1;
  return (Math.max(0, Math.min(count - 1, sel)) + step) % count;
}

/** Text the omnibox shows for row `sel`: the user's own text for the default row, else the row's. */
export function suggestPreviewText(typed: string, sel: number, rowText: string): string {
  return sel === 0 ? typed : rowText;
}

/** Icon shown in a row that has no favicon: what picking the row does, in the family's glyphs. */
export function suggestGlyph(kind: SuggestKind): IconName {
  switch (kind) {
    case 'search': return 'search';
    case 'hist': return 'history';
    case 'bm': return 'bookmark';
    case 'tab': return 'tab-switch';
    default: return 'globe';
  }
}

/** Trailing caption for rows whose action differs from "go to this page". */
export function suggestAction(kind: SuggestKind): string {
  return kind === 'tab' ? 'Switch to tab' : '';
}

const asciiLower = (code: number): number => (code >= 65 && code <= 90 ? code + 32 : code);
const stripPrefix = (text: string, prefix: string): string | null =>
  text.length >= prefix.length && suggestMatch(text.slice(0, prefix.length), prefix) !== null ? text.slice(prefix.length) : null;

/**
 * What the rows are matched against: the typed text trimmed, without a leading scheme and then a
 * leading `www.` (the same needle Rust's ranking uses, `omnibox::strip_typed_prefixes`).
 */
export function suggestNeedle(typed: string): string {
  const text = typed.trim();
  const rest = stripPrefix(text, 'https://') ?? stripPrefix(text, 'http://') ?? text;
  return stripPrefix(rest, 'www.') ?? rest;
}

/**
 * The first ASCII-case-insensitive occurrence of `needle` in `text` as [start, end) UTF-16 offsets,
 * or null. Folding only ASCII keeps every offset valid (no length-changing case maps), as Rust does.
 */
export function suggestMatch(text: string, needle: string): readonly [number, number] | null {
  const n = needle.length;
  if (n === 0 || n > text.length) return null;
  outer: for (let at = 0; at + n <= text.length; at += 1) {
    for (let i = 0; i < n; i += 1) {
      if (asciiLower(text.charCodeAt(at + i)) !== asciiLower(needle.charCodeAt(i))) continue outer;
    }
    return [at, at + n];
  }
  return null;
}

/** The part of a row's `text` to emphasise for `needle`: its first match, unless that is all of it. */
export function suggestEmphasis(text: string, needle: string): readonly [number, number] | null {
  const hit = suggestMatch(text, needle);
  return hit && !(hit[0] === 0 && hit[1] === text.length) ? hit : null;
}
// --- end omnibox suggest ---
