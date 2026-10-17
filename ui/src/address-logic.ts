// Pure rules of the address bar's own behaviour (docs/html-chrome-protocol.md, `addr` message): what
// Enter, Alt+Enter, Shift+Enter and Ctrl+Enter do, what a paste and a copy become, which part of an
// address is emphasised, and how the drop-down badge and typed-history list behave. No DOM here;
// test/address.mjs covers all of it.

/** The address bar's options as Rust sends them (`addr`), every field always present. */
export interface AddrFlags {
  /** The address field is drawn; off, it appears only while focused. */
  readonly show: boolean;
  /** The unfocused field shows the whole address. */
  readonly full: boolean;
  /** Activating the field selects its text. */
  readonly select: boolean;
  /** Copy and cut put the encoded address on the clipboard. */
  readonly encoded: boolean;
  /** A QR code button is shown. */
  readonly qr: boolean;
  /** A button at the end of the field opens the typed-history list. */
  readonly hbtn: boolean;
  /** Focusing an empty field lists what was typed before. */
  readonly typed: boolean;
  /** The drop-down opens as the user types. */
  readonly typing: boolean;
  /** Every drop-down row carries a kind badge. */
  readonly badge: boolean;
  /** A pasted `javascript:` prefix is removed. */
  readonly strip: boolean;
  /** Only the base domain is emphasised in the unfocused address. */
  readonly highlight: boolean;
  /** The registrable domain of the shown page's host, or ''. */
  readonly base: string;
}

export const DEFAULT_ADDR: AddrFlags = {
  show: true, full: false, select: true, encoded: true, qr: false, hbtn: false, typed: false, typing: true, badge: false, strip: true, highlight: true, base: '',
};

export const ADDR_FLAG_KEYS = ['show', 'full', 'select', 'encoded', 'qr', 'hbtn', 'typed', 'typing', 'badge', 'strip', 'highlight', 'base'] as const;

/** What a key press in the address field submits. */
export type SubmitKind = 'plain' | 'newtab' | 'expand';

export interface EnterKeys { readonly altKey: boolean; readonly shiftKey: boolean; readonly ctrlKey: boolean; readonly metaKey: boolean }

/**
 * Enter variants: Alt+Enter and Shift+Enter open a new tab, Ctrl+Enter expands a single word to a
 * domain (`example` -> `www.example.com`). Anything else with a modifier is a plain Enter, as before.
 */
export function submitKind(keys: EnterKeys): SubmitKind {
  if (keys.metaKey) return 'plain';
  if (keys.ctrlKey && !keys.altKey && !keys.shiftKey) return 'expand';
  if ((keys.altKey || keys.shiftKey) && !keys.ctrlKey) return 'newtab';
  return 'plain';
}

/** Characters browsers drop from inside a `javascript:` scheme when pasted (tabs, newlines, controls). */
const SCHEME_NOISE = /[\u0000- ]/g;

/**
 * Removes a leading `javascript:` from pasted text, however it is spaced or cased (`  JaVa\tScRiPt:`),
 * until none is left, so a pasted snippet becomes plain text instead of a script address. Text without
 * such a prefix is returned unchanged.
 */
export function stripPastedJavascript(text: string): string {
  let rest = text;
  for (let i = 0; i < 8; i += 1) {
    const lead = rest.length - rest.replace(/^[\s\u0000-\u001f]+/, '').length;
    const head = rest.slice(lead, lead + 40).replace(SCHEME_NOISE, '').toLowerCase();
    if (!head.startsWith('javascript:')) return rest;
    // Cut after the colon that ends the scheme, counting through the noise inside it.
    let seen = '';
    let cut = lead;
    while (cut < rest.length && seen !== 'javascript:') {
      const ch = rest.charAt(cut);
      if (!/[\u0000- ]/.test(ch)) seen += ch.toLowerCase();
      cut += 1;
    }
    rest = rest.slice(cut).replace(/^[\s\u0000-\u001f]+/, '');
  }
  return rest;
}

/**
 * The percent-encoded form of an http(s) address, or null when `text` is not one. Used when the
 * whole address is copied or cut with "copy encoded address" on.
 */
export function encodedAddress(text: string): string | null {
  const trimmed = text.trim();
  if (!/^https?:\/\//i.test(trimmed)) return null;
  try {
    const url = new URL(trimmed.replace(/ /g, '%20'));
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

/**
 * What a copy or cut puts on the clipboard: the encoded address when the option is on and the
 * whole field is selected and is an address, else null (the browser's own selection text stands).
 */
export function clipboardText(opts: { readonly enabled: boolean; readonly value: string; readonly selStart: number; readonly selEnd: number }): string | null {
  if (!opts.enabled || opts.selStart !== 0 || opts.selEnd !== opts.value.length || opts.value === '') return null;
  const encoded = encodedAddress(opts.value);
  return encoded !== null && encoded !== opts.value ? encoded : null;
}

export interface HostParts {
  /** Text before the base domain (`www.`, `mail.`), possibly empty. */
  readonly before: string;
  /** The base domain, or the whole host when there is none to find. */
  readonly base: string;
  /** Text after it (a port), possibly empty. */
  readonly after: string;
}

/**
 * Splits a URL authority into the part before the base domain, the base domain and what follows
 * (a port). `base` is Rust's registrable domain for the page; when it is not a suffix of the host
 * the whole host is the base.
 */
export function splitHost(authority: string, base: string): HostParts {
  const portAt = authority.lastIndexOf(':');
  const hasPort = portAt > 0 && /^\d{1,5}$/.test(authority.slice(portAt + 1)) && !authority.endsWith(']');
  const host = hasPort ? authority.slice(0, portAt) : authority;
  const after = hasPort ? authority.slice(portAt) : '';
  const lower = host.toLowerCase();
  const wanted = base.toLowerCase();
  if (wanted !== '' && lower.endsWith(wanted) && (lower.length === wanted.length || lower.charAt(lower.length - wanted.length - 1) === '.')) {
    const start = host.length - wanted.length;
    return { before: host.slice(0, start), base: host.slice(start), after };
  }
  return { before: '', base: host, after };
}

/** The glyph shown at the end of a drop-down row for its kind, when badges are always shown. */
export type BadgeKind = 'search' | 'url' | 'hist' | 'bm' | 'tab' | 'calc';

/** Whether a row gets a badge: always with the option on (except the default row), else only a bookmark with a favicon. */
export function wantsBadge(kind: BadgeKind, hasFavicon: boolean, always: boolean, isDefaultRow: boolean): boolean {
  if (always) return !isDefaultRow;
  return kind === 'bm' && hasFavicon;
}

/**
 * Whether focusing the address field shows the typed-history list: the option is on, the field has
 * no text (or all of it is selected, as on a first click) and no list is up.
 */
export function showsTypedHistoryOnFocus(opts: { readonly enabled: boolean; readonly value: string; readonly allSelected: boolean }): boolean {
  return opts.enabled && (opts.value === '' || opts.allSelected);
}

/**
 * --- wave-1 fixes --- what keyboard focus (Tab, F6, the host focusing the field) does to the address
 * text under "Select the address on activation", as a click does: `all` selects it, `end` puts the
 * caret after it, and `keep` leaves an address the user has edited alone (focus also returns when the
 * window comes back to the front, which must not move the caret).
 */
export function keyboardFocusSelection(select: boolean, edited: boolean): 'all' | 'end' | 'keep' {
  if (edited) return 'keep';
  return select ? 'all' : 'end';
}

/** The search field's modes, as `search_field` stores them. */
export const SEARCH_FIELD_MODES = ['off', 'field', 'button'] as const;
export type SearchFieldMode = typeof SEARCH_FIELD_MODES[number];
