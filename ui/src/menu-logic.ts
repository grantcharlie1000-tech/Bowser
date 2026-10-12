export type MenuEntry =
  | { separator: true; label?: string }
  | { section: string }
  | { id: number; label: string; enabled: boolean; checked: boolean; shortcut?: string; icon?: string; submenu?: MenuEntry[] };

export type MenuItem = Extract<MenuEntry, { id: number }>;

/** Profile card above the main menu's list. `accent` is a `#rrggbb` colour. */
export interface MenuHeader { name: string; caption: string; avatar: string; accent: string }

export interface MenuMessage {
  t: 'menu';
  tok: number;
  kind: string;
  header?: MenuHeader;
  footer?: string;
  entries: MenuEntry[];
}

export const MENU_ICONS: readonly string[] = ['plus', 'person', 'grid', 'key', 'shield', 'gear', 'close', 'check', 'lock', 'unlock', 'warning', 'info', 'mic', 'camera', 'location', 'bell', 'cookie', 'reset'];

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
  if (Object.keys(msg).some(key => !['v', 't', 'tok', 'kind', 'header', 'footer', 'entries'].includes(key)) || msg.v !== 1 || msg.t !== 'menu' ||
      !Number.isSafeInteger(msg.tok) || (msg.tok as number) < 0 || (msg.tok as number) > 0xffffffff ||
      typeof msg.kind !== 'string' || msg.kind.length < 1 || msg.kind.length > 32 || !Array.isArray(msg.entries) || msg.entries.length > 128) return false;
  if (msg.header !== undefined && !validHeader(msg.header)) return false;
  if (msg.footer !== undefined && (typeof msg.footer !== 'string' || msg.footer.length > 256)) return false;
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
      if (Object.keys(item).some(key => !['id', 'label', 'enabled', 'checked', 'shortcut', 'icon', 'submenu'].includes(key)) ||
          !Number.isSafeInteger(item.id) || (item.id as number) < 0 || (item.id as number) > 0xffffffff || typeof item.label !== 'string' || item.label.length > 128 ||
          typeof item.enabled !== 'boolean' || typeof item.checked !== 'boolean') return false;
      if (item.shortcut !== undefined && (typeof item.shortcut !== 'string' || item.shortcut.length > 48)) return false;
      if (item.icon !== undefined && (typeof item.icon !== 'string' || !MENU_ICONS.includes(item.icon))) return false;
      if (item.submenu !== undefined && (!Array.isArray(item.submenu) || item.submenu.length > 128 || !validEntries(item.submenu, depth + 1))) return false;
    }
    return true;
  };
  return validEntries(msg.entries, 0);
}
