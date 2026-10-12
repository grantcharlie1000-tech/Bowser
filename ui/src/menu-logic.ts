export type MenuEntry =
  | { separator: true; label?: string }
  | { section: string }
  | { id: number; label: string; enabled: boolean; checked: boolean; shortcut?: string; icon?: string; fav?: number; badge?: MenuBadge; submenu?: MenuEntry[] };

/** A coloured disc in the icon slot: a profile avatar (one glyph) or a colour swatch (empty). */
export interface MenuBadge { avatar: string; accent: string }

export type MenuItem = Extract<MenuEntry, { id: number }>;

/** Profile card above the main menu's list. `accent` is a `#rrggbb` colour. */
export interface MenuHeader { name: string; caption: string; avatar: string; accent: string }

export interface MenuMessage {
  t: 'menu';
  tok: number;
  kind: string;
  header?: MenuHeader;
  footer?: string;
  /** PNG data: URIs of the favicons rows refer to by index (`fav`). */
  favs?: string[];
  entries: MenuEntry[];
}

/**
 * Icon names a row may carry: the whole family in icons.gen.ts (test/menu.mjs fails when the two
 * drift, and when a Rust `.glyph("...")` is not in the family).
 */
export const MENU_ICONS: readonly string[] = [
  'app-tile', 'arrow-left', 'arrow-right', 'back', 'bell', 'bookmark', 'camera', 'caption-close', 'caption-maximize',
  'caption-minimize', 'caption-restore', 'check', 'chevron-down', 'chevron-left', 'chevron-right', 'chevron-up', 'clock',
  'close', 'command', 'cookie', 'copy', 'customize', 'dots', 'download', 'droplet', 'edit', 'extensions', 'external', 'eye',
  'find', 'folder', 'forward', 'gear', 'grid', 'grip', 'heart', 'history', 'home', 'info', 'key', 'link', 'location', 'lock', 'menu',
  'mic', 'more', 'more-horizontal', 'more-vertical', 'mute', 'new-tab', 'notes', 'palette', 'pencil', 'person',
  'picture-in-picture', 'pin', 'pip', 'plus', 'print', 'profile', 'puzzle', 'refresh', 'reload', 'reset', 'search', 'settings',
  'shield', 'sidebar', 'sidebar-toggle', 'split', 'split-view', 'star', 'stop', 'tab-close', 'tab-group', 'trash', 'unlock',
  'volume', 'warning', 'workspace', 'workspace-bolt', 'workspace-book', 'workspace-briefcase', 'workspace-cart',
  'workspace-code', 'workspace-game', 'workspace-globe', 'workspace-heart', 'workspace-home', 'workspace-leaf',
  'workspace-music', 'workspace-star', 'zoom-in', 'zoom-out',
];

/** Most favicons one menu carries, and the longest data: URI of one (the tab strip's limit). */
export const MAX_MENU_FAVS = 48;
const MAX_FAV_URI = 24 * 1024;
const FAV_URI = /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/;

/** Kinds whose footer is real information; for every other kind it only names the menu (aria-label). */
const FOOTER_KINDS: readonly string[] = ['main', 'site-info', 'profile-delete', 'import-offer'];

export function footerVisible(kind: string): boolean {
  return FOOTER_KINDS.includes(kind);
}

/** Whether any row of the page has an icon, favicon or badge, so rows without one reserve the column. */
export function hasLeadColumn(entries: readonly MenuEntry[]): boolean {
  return entries.some(entry => 'id' in entry && (entry.icon !== undefined || entry.fav !== undefined || entry.badge !== undefined));
}

/** Whether any row has a check or a submenu chevron, so every row reserves the trailing column. */
export function hasTrailColumn(entries: readonly MenuEntry[]): boolean {
  return entries.some(entry => 'id' in entry && (entry.checked || entry.submenu !== undefined));
}

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** WCAG relative luminance of `#rrggbb`. */
export function luminance(hex: string): number {
  const n = Number.parseInt(hex.slice(1), 16);
  return 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255);
}

/** The text colour (black or white) that reads better on the `#rrggbb` fill. */
export function onFill(hex: string): '#000000' | '#ffffff' {
  const l = luminance(hex);
  return (l + 0.05) / 0.05 >= 1.05 / (l + 0.05) ? '#000000' : '#ffffff';
}

/** Typed characters within this many milliseconds of each other build one typeahead string. */
export const TYPEAHEAD_MS = 700;

export function isSelectable(entry: MenuEntry | undefined): entry is MenuItem {
  return !!entry && !('separator' in entry) && !('section' in entry) && entry.enabled;
}

export function stepMenu(entries: readonly MenuEntry[], from: number, direction: -1 | 1): number {
  if (entries.length === 0) return -1;
  const start = from < 0 ? (direction === 1 ? -1 : 0) : from;
  for (let n = 1; n <= entries.length; n += 1) {
    const index = ((start + direction * n) % entries.length + entries.length) % entries.length;
    if (isSelectable(entries[index])) return index;
  }
  return -1;
}

export function edgeMenu(entries: readonly MenuEntry[], end: boolean): number {
  const indices = end ? [...entries.keys()].reverse() : entries.keys();
  for (const index of indices) {
    if (isSelectable(entries[index])) return index;
  }
  return -1;
}

/**
 * The first enabled row at or after `from` (or strictly after it when `after` is set), wrapping,
 * whose label starts with `text`, ignoring case and leading space. -1 when none does.
 */
export function typeahead(entries: readonly MenuEntry[], from: number, text: string, after: boolean): number {
  if (entries.length === 0 || text.length === 0) return -1;
  const needle = text.toLocaleLowerCase();
  const start = Math.max(from, -1) + (after ? 1 : 0);
  for (let n = 0; n < entries.length; n += 1) {
    const index = (((start + n) % entries.length) + entries.length) % entries.length;
    const entry = entries[index];
    if (isSelectable(entry) && entry.label.trimStart().toLocaleLowerCase().startsWith(needle)) return index;
  }
  return -1;
}

/** One drill-in page: the top-level list or a submenu reached from the row labelled `label`. */
export interface MenuPage { readonly entries: readonly MenuEntry[]; readonly label: string; selected: number }

/** Keyboard state of an open menu. Pages are a stack; the last one is on screen. */
export interface MenuNav { pages: MenuPage[]; typed: string; typedAt: number }

/**
 * What a key or click did: nothing, moved the selection, changed page (redraw and resize), picked a
 * command, or dismissed the menu.
 */
export type MenuOutcome =
  | { kind: 'none' }
  | { kind: 'select' }
  | { kind: 'page' }
  | { kind: 'pick'; id: number }
  | { kind: 'dismiss' };

const NONE: MenuOutcome = { kind: 'none' };
const SELECT: MenuOutcome = { kind: 'select' };
const PAGE: MenuOutcome = { kind: 'page' };

export function createNav(entries: readonly MenuEntry[]): MenuNav {
  return { pages: [{ entries, label: '', selected: edgeMenu(entries, false) }], typed: '', typedAt: -Infinity };
}

export function currentPage(nav: MenuNav): MenuPage {
  const page = nav.pages[nav.pages.length - 1];
  if (!page) throw new Error('menu has no page');
  return page;
}

/** Runs the row at `index`: a submenu opens as a new page, a command is picked. */
export function activate(nav: MenuNav, index: number): MenuOutcome {
  const page = currentPage(nav);
  const entry = page.entries[index];
  if (!isSelectable(entry)) return NONE;
  page.selected = index;
  if (entry.submenu) {
    nav.pages.push({ entries: entry.submenu, label: entry.label, selected: edgeMenu(entry.submenu, false) });
    nav.typed = '';
    return PAGE;
  }
  return { kind: 'pick', id: entry.id };
}

/** Leaves a submenu page. False on the top page. */
export function back(nav: MenuNav): boolean {
  if (nav.pages.length <= 1) return false;
  nav.pages.pop();
  nav.typed = '';
  return true;
}

export interface KeyMods { ctrl?: boolean; alt?: boolean; meta?: boolean; shift?: boolean }

/** Applies one keydown (`KeyboardEvent.key`) at time `now` in milliseconds. */
export function menuKey(nav: MenuNav, key: string, mods: KeyMods, now: number): MenuOutcome {
  const page = currentPage(nav);
  const move = (index: number): MenuOutcome => {
    if (index < 0 || index === page.selected) return NONE;
    page.selected = index;
    return SELECT;
  };
  switch (key) {
    case 'ArrowDown': return move(stepMenu(page.entries, page.selected, 1));
    case 'ArrowUp': return move(stepMenu(page.entries, page.selected, -1));
    case 'Tab': return move(stepMenu(page.entries, page.selected, mods.shift ? -1 : 1));
    case 'Home': case 'PageUp': return move(edgeMenu(page.entries, false));
    case 'End': case 'PageDown': return move(edgeMenu(page.entries, true));
    case 'Enter': case ' ': return activate(nav, page.selected);
    case 'ArrowRight': {
      const entry = page.entries[page.selected];
      return isSelectable(entry) && entry.submenu ? activate(nav, page.selected) : NONE;
    }
    case 'ArrowLeft': case 'Backspace': return back(nav) ? PAGE : NONE;
    case 'Escape': return { kind: 'dismiss' };
    default: break;
  }
  if (key.length !== 1 || mods.ctrl || mods.alt || mods.meta) return NONE;
  if (now - nav.typedAt > TYPEAHEAD_MS) nav.typed = '';
  nav.typedAt = now;
  const ch = key.toLocaleLowerCase();
  // Repeating one letter cycles through the rows starting with it; more letters refine the match.
  const cycling = nav.typed.length > 0 && [...nav.typed].every(c => c === ch);
  nav.typed += ch;
  const found = cycling
    ? typeahead(page.entries, page.selected, ch, true)
    : typeahead(page.entries, page.selected, nav.typed, nav.typed.length === 1);
  return move(found);
}

function validBadge(value: unknown): value is MenuBadge {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const b = value as Record<string, unknown>;
  return Object.keys(b).every(key => key === 'avatar' || key === 'accent') &&
    typeof b.avatar === 'string' && b.avatar.length <= 4 && typeof b.accent === 'string' && /^#[0-9a-f]{6}$/.test(b.accent);
}

function validHeader(value: unknown): value is MenuHeader {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const h = value as Record<string, unknown>;
  return Object.keys(h).every(key => ['name', 'caption', 'avatar', 'accent'].includes(key)) &&
    typeof h.name === 'string' && h.name.length <= 128 && typeof h.caption === 'string' && h.caption.length <= 128 &&
    typeof h.avatar === 'string' && h.avatar.length <= 4 && typeof h.accent === 'string' && /^#[0-9a-f]{6}$/.test(h.accent);
}

export function validMenuPayload(value: unknown): value is MenuMessage {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const msg = value as Record<string, unknown>;
  if (Object.keys(msg).some(key => !['v', 't', 'tok', 'kind', 'header', 'footer', 'favs', 'entries'].includes(key)) || msg.v !== 1 || msg.t !== 'menu' ||
      !Number.isSafeInteger(msg.tok) || (msg.tok as number) < 0 || (msg.tok as number) > 0xffffffff ||
      typeof msg.kind !== 'string' || msg.kind.length < 1 || msg.kind.length > 32 || !Array.isArray(msg.entries) || msg.entries.length > 128) return false;
  if (msg.header !== undefined && !validHeader(msg.header)) return false;
  if (msg.footer !== undefined && (typeof msg.footer !== 'string' || msg.footer.length > 256)) return false;
  if (msg.favs !== undefined && (!Array.isArray(msg.favs) || msg.favs.length > MAX_MENU_FAVS ||
      msg.favs.some(uri => typeof uri !== 'string' || uri.length > MAX_FAV_URI || !FAV_URI.test(uri)))) return false;
  const favCount = Array.isArray(msg.favs) ? msg.favs.length : 0;
  let count = 0;
  const validEntries = (entries: unknown[], depth: number): boolean => {
    if (depth > 8) return false;
    for (const raw of entries) {
      count += 1;
      if (count > 256 || !raw || typeof raw !== 'object' || Array.isArray(raw)) return false;
      const item = raw as Record<string, unknown>;
      if (item.separator === true) {
        if (Object.keys(item).some(key => !['separator', 'label'].includes(key))) return false;
        if (item.label !== undefined && (typeof item.label !== 'string' || item.label.length > 128)) return false;
        continue;
      }
      if (typeof item.section === 'string') {
        if (Object.keys(item).length !== 1 || item.section.length > 128) return false;
        continue;
      }
      if (Object.keys(item).some(key => !['id', 'label', 'enabled', 'checked', 'shortcut', 'icon', 'fav', 'badge', 'submenu'].includes(key)) ||
          !Number.isSafeInteger(item.id) || (item.id as number) < 0 || (item.id as number) > 0xffffffff || typeof item.label !== 'string' || item.label.length > 128 ||
          typeof item.enabled !== 'boolean' || typeof item.checked !== 'boolean') return false;
      if (item.shortcut !== undefined && (typeof item.shortcut !== 'string' || item.shortcut.length > 48)) return false;
      if (item.icon !== undefined && (typeof item.icon !== 'string' || !MENU_ICONS.includes(item.icon))) return false;
      if (item.fav !== undefined && (!Number.isSafeInteger(item.fav) || (item.fav as number) < 0 || (item.fav as number) >= favCount)) return false;
      if (item.badge !== undefined && !validBadge(item.badge)) return false;
      if (item.submenu !== undefined && (!Array.isArray(item.submenu) || item.submenu.length > 128 || !validEntries(item.submenu, depth + 1))) return false;
    }
    return true;
  };
  return validEntries(msg.entries, 0);
}
