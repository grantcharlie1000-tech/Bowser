// Typed bridge for the toolbar, tab strip and left rail pages. Mirrors docs/html-chrome-protocol.md (v1).
// Page-controlled text reaches the DOM only through textContent or attributes (see view.ts, tabs.ts, rail.ts).
import { MAX_RAIL_ITEMS, isRailIcon, isRailKey, type RailIconName } from './rail-logic';
import type { InteractionEvent } from './interaction-diagnostic';
import { validZoom } from './zoom-logic';
// --- split view ---
import { parseSplitInbound, type SplitCommand, type SplitInbound } from './split-protocol';
export type { PairMsg, PrivatePageMsg, SplitCommand, SplitDropMsg, SplitInbound, SplitPickMsg } from './split-protocol';
// --- end split view ---
// --- address bar, search and bookmarks (src/chrome_host/address_page.rs) ---
import { ADDR_FLAG_KEYS, SEARCH_FIELD_MODES, type AddrFlags, type SearchFieldMode } from './address-logic';
// --- end address bar, search and bookmarks ---
// --- tab ghost (docs/design/tab-drag.md) ---
import { parseTabGhost, type TabDragCommand, type TabGhostMsg } from './tab-drag-protocol';
import { parseLook, type Look } from './window-logic'; // --- window and panels (thread T9) ---
export type { TabDragCommand, TabGhostMsg } from './tab-drag-protocol';
// --- end tab ghost ---

export const PROTOCOL_VERSION = 1;
// Raised from 2048 so the settings page's `save` payload (every profile setting) fits; mirrors
// chrome_host::protocol::MAX_TEXT_CHARS.
export const MAX_TEXT = 4096;
const MAX_LABEL = 512;
/** Rail button label (also its tooltip and letter-tile source). */
const MAX_RAIL_LABEL = 64;
const MAX_COLOR = 64;
const MAX_WIRE = 16 * 1024;
const MAX_INBOUND = 256 * 1024;
const MAX_SEQ = 0x7fffffff;
export const MAX_ID = 0xffffffff;
export const MAX_TITLE = 512;
/** Most items in one `tabs` message; Rust sends at most this many. */
export const MAX_TABS = 512;
const FAV_PREFIX = 'data:image/png;base64,';
export const MAX_FAV_URI = 24 * 1024;
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;

export type NavAction = 'back' | 'forward' | 'reload' | 'stop';
export type OpenTarget = 'menu' | 'site' | 'newtab' | 'split' | 'zoomreset' | 'reader' | 'translate' | 'feeds';
/** The reader view button: hidden, offered (the page looks like an article) or on. */
export type ReaderPill = 'none' | 'available' | 'on';
/** The translate button: hidden, offered (the page is in another language) or on (showing the translation). */
export type TranslatePill = 'none' | 'available' | 'on';
export type CaptionAction = 'toggleMaximize' | 'systemMenu';
export type Security = 'secure' | 'notsecure' | 'certproblem' | 'internal' | 'unknown' | 'none';
export type TabAction = 'select' | 'close' | 'menu' | 'pin' | 'mute' | 'dblclick';
export type GroupAction = 'toggle' | 'menu';
export type RailAction = 'click' | 'menu' | 'hover';
export type PanelKind = 'panel' | 'hist' | 'bm' | 'cust' | 'notes' | 'set' | 'priv' | 'ren' | 'hpage' | 'dpage' | 'spage' | 'addr' | 'sess' | 'feed';
export type PanelToken = 'history' | 'bookmarks' | 'customize' | 'notes' | 'settings' | 'privacy' | 'rename' | 'historypage' | 'downloadspage' | 'extensions' | 'commands' | 'protection' | 'startpage' | 'address' | 'addressdlg' | 'bookmarkbar' | 'sessions' | 'pagetools' | 'feeds';
export interface PanelFields {
  a: string;
  id?: number;
  parent?: number;
  before?: number;
  n?: number;
  on?: boolean;
  text?: string;
  url?: string;
}

export const THEME_TOKENS = [
  'bg',
  'fill',
  'border',
  'focusBorder',
  'text',
  'placeholder',
  'completion',
  'selection',
  'hover',
  'progress',
] as const;
export type ThemeToken = (typeof THEME_TOKENS)[number];
export type Theme = Partial<Record<ThemeToken, string>>;

// JS -> Rust commands (sent as flat JSON text).
export type Command =
  | { v: 1; t: 'ready' }
  | { v: 1; t: 'interaction'; e: InteractionEvent; n: number }
  | { v: 1; t: 'nav'; a: NavAction }
  | { v: 1; t: 'edit'; seq: number; text: string }
  | { v: 1; t: 'submit'; text: string }
  | { v: 1; t: 'escape' }
  | { v: 1; t: 'focus'; on: boolean }
  | { v: 1; t: 'open'; a: OpenTarget }
  | { v: 1; t: 'drag' }
  | { v: 1; t: 'caption'; a: CaptionAction }
  | { v: 1; t: 'tab'; a: TabAction; id: number }
  | { v: 1; t: 'tabmove'; id: number; before: number }
  // --- tab stacks (T10) --- a held tab dropped on another after resting over it; the wheel over the workspaces.
  | { v: 1; t: 'tabstack'; id: number; onto: number }
  | { v: 1; t: 'railwheel'; d: number }
  // --- tab options --- the strip's selection (comma-separated wire ids) and its Closed tabs button.
  | { v: 1; t: 'tabsel'; ids: string }
  | { v: 1; t: 'closedtabs' }
  // --- end tab options ---
  | { v: 1; t: 'group'; a: GroupAction; key: number }
  // --- vertical sidebar (lane S) ---
  | { v: 1; t: 'side'; a: 'workspace' | 'newtab' | 'collapse' }
  // --- end vertical sidebar ---
  // --- toolbars and feeds (thread T13) --- a command button of the address bar or the tab bar: its item id and its place in the bar's list.
  | { v: 1; t: 'tool'; b: 'address' | 'tabs'; k: string; i: number }
  // --- end toolbars and feeds ---
  | { v: 1; t: 'rail'; a: RailAction; k: string; r?: string }
  | { v: 1; t: 'railmove'; k: string; before: string }
  | ({ v: 1; t: PanelKind } & PanelFields)
  | { v: 1; t: 'size'; tok: number; w: number; h: number; c?: string }
  | { v: 1; t: 'pick'; tok: number; id: number }
  | { v: 1; t: 'dismiss'; tok: number }
  // --- popup surfaces (lane PP) ---
  | { v: 1; t: 'act'; tok: number; id: number; n: number }
  | { v: 1; t: 'compose'; tok: number; name: string; url?: string }
  | { v: 1; t: 'composeopen'; a: 'workspace-new' | 'app-add' | 'app-edit'; id?: number; name?: string; url?: string; r: string }
  // --- end popup surfaces ---
  // --- hover layer ---
  | { v: 1; t: 'tip'; text: string; r: string }
  | { v: 1; t: 'tipoff' }
  | { v: 1; t: 'hover'; k: HoverTargetKind; id: number; r: string }
  | { v: 1; t: 'hover'; k: 'none' }
  | ({ v: 1; t: 'hact'; tok: number } & HoverActFields)
  // --- end hover layer ---
  // --- omnibox suggest ---
  | { v: 1; t: 'suggestPick'; seq: number; i: number }
  | { v: 1; t: 'suggestPreview'; seq: number; i: number }
  | { v: 1; t: 'suggestEnd' }
  // --- end omnibox suggest ---
  // --- downloads (docs/design/downloads.md) ---
  | { v: 1; t: 'dl'; a: 'toggle'; text: string }
  | { v: 1; t: 'ext'; a: 'open'; text: string }
  | { v: 1; t: 'dlact'; tok: number; id: number; n: number }
  // --- end downloads ---
  // --- split view ---
  | SplitCommand
  // --- end split view ---
  | TabDragCommand; // --- tab ghost ---

// --- hover layer ---
/** Longest tooltip text Rust accepts, in code points. */
export const MAX_TIP = 256;
/** Largest coordinate or size in a hover rectangle, in CSS pixels. */
export const MAX_HOVER_COORD = 16384;
/** Most member rows, swatches and markers Rust sends in one hover message. */
export const MAX_HOVER_ROWS = 12;
export const MAX_HOVER_SWATCHES = 32;
export const HOVER_MARKERS = 5;
/** Longest island name, in code points (`MAX_GROUP_NAME_CHARS`). */
export const MAX_ISLAND_NAME = 64;
const MAX_HOVER_HOST = 253;
const MAX_HOVER_ROW_INDEX = 63;
const MAX_HOVER_SWATCH_INDEX = 31;
export type HoverTargetKind = 'tab' | 'ball';
export type HoverActFields =
  | { a: 'marker'; n: number }
  | { a: 'row'; n: number }
  | { a: 'swatch'; n: number }
  | { a: 'name'; text: string }
  | { a: 'next' | 'close' | 'focus' | 'dismiss' };
export interface HoverRowMsg {
  readonly title: string;
  readonly active: boolean;
  readonly linked: boolean;
  /** PNG data: URI, or '' for the placeholder dot. */
  readonly fav: string;
}
export interface HoverTooltipMsg { readonly t: 'hover'; readonly tok: number; readonly kind: 'tooltip'; readonly text: string }
export interface HoverCardMsg {
  readonly t: 'hover';
  readonly tok: number;
  readonly kind: 'card';
  readonly title: string;
  readonly host: string;
  /** Marker id 0-4, or -1 for none. */
  readonly marker: number;
  /** Marker swatch colours ('' = theme accent). */
  readonly markers: readonly string[];
  readonly sleeping: boolean;
  readonly fav: string;
  readonly rows: readonly HoverRowMsg[];
  readonly more: number;
  // --- tab options --- sent only when "Show a thumbnail in tab previews" / "Show memory use" are on and known.
  /** A small JPEG of the page as a data: URI. */
  readonly thumb?: string;
  /** Memory of the tab's page process in megabytes. */
  readonly mem?: number;
  // --- end tab options ---
}
export interface HoverIslandMsg {
  readonly t: 'hover';
  readonly tok: number;
  readonly kind: 'island';
  readonly name: string;
  readonly hint: string;
  readonly slot: number;
  readonly swatches: readonly string[];
  readonly rows: readonly HoverRowMsg[];
  readonly more: number;
  readonly close: string;
}
export type HoverMsg = HoverTooltipMsg | HoverCardMsg | HoverIslandMsg;
export interface HoverHideMsg { readonly t: 'hoverhide' }
// --- end hover layer ---

// Rust -> JS messages (validated by parseInbound).
export interface StateMsg {
  readonly t: 'state';
  readonly scale: number;
  readonly pillx?: number;
  readonly pillw?: number;
  readonly menux?: number;
  readonly solid: boolean;
  readonly dark: boolean;
  readonly back: boolean;
  readonly forward: boolean;
  readonly loading: boolean;
  /** The window shows a split view. Absent in older hosts. */
  readonly split?: boolean;
  /** The page the tab shows is bookmarked. Absent in older hosts. */
  readonly bookmarked?: boolean;
  readonly progress: number;
  readonly security: Security;
  readonly label: string;
  /** `show_load_progress` setting. Absent in older hosts, which means shown. */
  readonly showProgress?: boolean;
  /** A permission request on the current page was denied (passive toolbar indicator). */
  readonly permblocked?: boolean;
  /** The page zoom of the active tab in percent (25 to 500). Absent in older hosts; 100 means no chip. */
  readonly zoom?: number;
  /** `show_zoom_indicator` setting: whether the zoom chip may show at all. Absent in older hosts,
   * which means shown. */
  readonly showZoom?: boolean;
  /** The active tab is a private tab (the pill shows the private indicator). Absent in older hosts. */
  readonly private?: boolean;
  /** The reader view button. Absent in older hosts, which means hidden. */
  readonly reader?: ReaderPill;
  /** The translate button. Absent in older hosts, which means hidden. */
  readonly translate?: TranslatePill;
  /** `font_size`, as a percentage of the default chrome font size. Absent in older hosts, which
   * means 100. */
  readonly fontScale?: number;
  /** `ui_font`; empty or absent means the page's own default font-family. */
  readonly uiFont?: string;
  // --- layout (thread T6) ---
  /** `ui_density`: list rows in the sidebar, panels and pages. Absent in older hosts (regular). */
  readonly density?: UiDensity;
  /** `compact_menus`: shorter menu rows. Absent in older hosts (off). */
  readonly compactMenus?: boolean;
  // --- end layout ---
  /** Window appearance options and whether the window is in the background (window-logic.ts). Absent in older hosts (stock look). */
  readonly look?: Look; // --- window and panels (thread T9) ---
  // --- toolbars and feeds (thread T13) --- the address bar's buttons; all four absent in older hosts (every button, in its usual place).
  /** `toolbar_buttons`: the buttons in order, as a comma list. */
  readonly items?: string;
  /** The command buttons among `items`, with the names the registry gives them. */
  readonly tools?: readonly ToolButtonMsg[];
  /** CSS px the always-there buttons at the end of the bar take from the address field. */
  readonly extra?: number;
  /** How many feeds the page in front offers; 0 hides the feed button. */
  readonly feeds?: number;
  // --- end toolbars and feeds ---
  readonly theme: Theme;
}
/** A command button on a bar: its item id (`cmd:new-tab`, `chain:2`) and the name it shows. */
export interface ToolButtonMsg { readonly k: string; readonly l: string; readonly i: number }
/** Most command buttons on one bar (Rust `MAX_COMMAND_ITEMS`). */
export const MAX_TOOLS = 4;
const TOOL_ITEM_RE = /^[a-z0-9:-]{1,64}$/;
function parseTools(value: unknown): ToolButtonMsg[] | null {
  if (!Array.isArray(value) || value.length > MAX_TOOLS) return null;
  const out: ToolButtonMsg[] = [];
  for (const row of value) {
    if (!isRecord(row) || !hasOnlyKeys(row, ['k', 'l', 'i']) || typeof row.k !== 'string' || !TOOL_ITEM_RE.test(row.k) || !isStr(row.l, 64) || !isInt(row.i, 0, 13)) return null;
    out.push({ k: row.k, l: row.l, i: row.i });
  }
  return out;
}
// --- layout (thread T6) ---
export const UI_DENSITIES = ['regular', 'compact'] as const;
export type UiDensity = typeof UI_DENSITIES[number];
// --- end layout ---
export interface SetTextMsg {
  readonly t: 'setText';
  readonly text: string;
  readonly select: boolean;
  /** `minimal_url`: the full address the focused field holds, when it differs from `text`. */
  readonly full?: string;
}
export interface FocusOmniboxMsg {
  readonly t: 'focusOmnibox';
  readonly select: boolean;
}
// --- omnibox suggest ---
/** Most rows in one `suggest` message, the default row included (Rust MAX_SUGGEST_ITEMS). */
export const MAX_SUGGEST_ITEMS = 20;
const MAX_SUGGEST_TEXT = 256;
const MAX_SUGGEST_WIDTH = 4000;
export const SUGGEST_KINDS = ['search', 'url', 'hist', 'bm', 'tab', 'calc'] as const;
export type SuggestKind = (typeof SUGGEST_KINDS)[number];
export interface SuggestItem {
  readonly k: SuggestKind;
  /** What the omnibox shows while the row is selected. */
  readonly text: string;
  readonly title: string;
  /** Display form of the address; empty for a search row. */
  readonly url: string;
  /** A PNG data: URI, or '' when none is known. */
  readonly fav: string;
  /** Show the kind badge on this row (`dd_always_badge`). */
  readonly bd?: boolean;
}
/** The row list for edit `seq`. `tok` is 0 for the toolbar's copy; no items closes the list. */
export interface SuggestMsg {
  readonly t: 'suggest';
  readonly tok: number;
  readonly seq: number;
  /** Dropdown width in CSS pixels (0 in the toolbar's copy). */
  readonly w: number;
  readonly sel: number;
  /** The typed text the rows answer ('' in the toolbar's copy); matches in rows are emphasised. */
  readonly q: string;
  readonly items: readonly SuggestItem[];
}
/** The keyboard selection moved (hover layer only). */
export interface SuggestSelMsg {
  readonly t: 'suggestSel';
  readonly seq: number;
  readonly sel: number;
}
/**
 * Whether the dropdown is on screen under the pill (toolbar only), so the pill joins it as one
 * surface. Sent when the popup shows and once it is fully hidden; never per keystroke.
 */
export interface SuggestOpenMsg {
  readonly t: 'suggestOpen';
  readonly on: boolean;
}
// --- end omnibox suggest ---
// --- address bar, search and bookmarks (src/chrome_host/address_page.rs) ---
/** How the address field behaves and looks (toolbar only); sent at start and whenever it changes. */
export type AddrMsg = { readonly t: 'addr' } & AddrFlags;
/** One engine in the search field's list. */
export interface FieldEngine { readonly n: string; readonly k: string }
/** The search field's mode and engine list (toolbar only). `engine` indexes `engines`. */
export interface SearchFieldMsg {
  readonly t: 'sfield';
  readonly mode: SearchFieldMode;
  readonly icon: boolean;
  readonly nick: boolean;
  readonly suggest: boolean;
  readonly keep: boolean;
  readonly engine: number;
  readonly engines: readonly FieldEngine[];
}
/** A search-field drop-down row was picked with the mouse and its search ran (Rust `SEARCH_DONE_JSON`). */
export interface SearchDoneMsg { readonly t: 'sdone' }
/** Most engines the search field lists (Rust `MAX_FIELD_ENGINES`). */
export const MAX_FIELD_ENGINES = 40;
function parseAddr(raw: Record<string, unknown>): AddrMsg | null {
  const keys = ['v', 't', ...ADDR_FLAG_KEYS];
  if (!hasOnlyKeys(raw, keys) || !hasAllKeys(raw, keys) || raw.v !== 1) return null;
  for (const key of ADDR_FLAG_KEYS) {
    if (key === 'base' ? !isStr(raw.base, 253) : !isBool(raw[key])) return null;
  }
  return { t: 'addr', ...(Object.fromEntries(ADDR_FLAG_KEYS.map((key) => [key, raw[key]])) as unknown as AddrFlags) };
}
function parseSearchField(raw: Record<string, unknown>): SearchFieldMsg | null {
  const keys = ['v', 't', 'mode', 'icon', 'nick', 'suggest', 'keep', 'engine', 'engines'];
  if (!hasOnlyKeys(raw, keys) || !hasAllKeys(raw, keys) || raw.v !== 1) return null;
  const { mode, icon, nick, suggest, keep, engine, engines } = raw;
  if (!isOneOf(mode, SEARCH_FIELD_MODES) || !isBool(icon) || !isBool(nick) || !isBool(suggest) || !isBool(keep)) return null;
  if (!Array.isArray(engines) || engines.length === 0 || engines.length > MAX_FIELD_ENGINES || !isInt(engine, 0, engines.length - 1)) return null;
  const list: FieldEngine[] = [];
  for (const item of engines) {
    if (!isRecord(item) || !hasOnlyKeys(item, ['n', 'k']) || !hasAllKeys(item, ['n', 'k']) || !isStr(item.n, 80) || !isStr(item.k, 32)) return null;
    list.push({ n: item.n, k: item.k });
  }
  return { t: 'sfield', mode, icon, nick, suggest, keep, engine, engines: list };
}
/** A command of the address field or the search field (`{t:"addr",a,...}`), checked like every panel command. */
export function sendAddr(fields: PanelFields): boolean {
  return sendPanel('addr', fields);
}
// --- end address bar, search and bookmarks ---
// --- extensions (docs/extensions.md "Managing extensions") ---
/** The toolbar's extensions button (toolbar only): shown while an enabled extension has a popup. */
export interface ExtButtonMsg {
  readonly t: 'extbtn';
  readonly on: boolean;
  /** Enabled extensions with a popup. */
  readonly n: number;
  /** The only one's name when `n` is 1, else empty. */
  readonly name: string;
}
/** Longest extension name Rust sends (`extensions_page::MAX_NAME_CHARS`). */
export const MAX_EXTENSION_NAME = 128;
/** At most this many extensions are installed in one profile (`extensions::MAX_INSTALLED`). */
export const MAX_EXTENSIONS = 64;
// --- end extensions ---
// --- downloads (docs/design/downloads.md) ---
export const DL_BUTTON_STATES = ['idle', 'active', 'paused', 'done', 'failed'] as const;
export type DlButtonState = typeof DL_BUTTON_STATES[number];
/** The toolbar's download button (toolbar only). */
export interface DlButtonMsg {
  readonly t: 'dlbtn';
  /** Show the button. */
  readonly on: boolean;
  /** What the icon shows; `done`/`failed` are unseen finished or failed downloads. */
  readonly state: DlButtonState;
  /** Active downloads. */
  readonly n: number;
  /** Aggregate progress 0-100, -1 unknown (the ring sweeps). */
  readonly pct: number;
  /** Counts download starts; the button plays its start animation when it changes. */
  readonly burst: number;
}
// --- end downloads ---
/** Whether the main menu is open (toolbar only): the menu button shows it while it is up. */
export interface MenuOpenMsg {
  readonly t: 'menuOpen';
  readonly on: boolean;
}
export interface CompletionMsg {
  readonly t: 'completion';
  readonly seq: number;
  readonly suffix: string;
}
export interface TabItem {
  readonly id: number;
  readonly title: string;
  readonly loading: boolean;
  readonly pinned: boolean;
  readonly muted: boolean;
  /** A private (InPrivate) tab: never grouped, nothing it does is saved. */
  readonly private: boolean;
  /** Marker colour index 0-7, or -1 for none. */
  readonly marker: number;
  /** Group key, 0 when ungrouped. */
  readonly g: number;
  /** Group leader (the item that carries the group fields). */
  readonly gl: boolean;
  readonly gname?: string;
  /** Island colour index 0-7, or -1 for the theme accent. Leader only. */
  readonly gcol?: number;
  readonly gcollapsed?: boolean;
  readonly gsize?: number;
  /** A favicon is cached for this id (see the `fav` message). */
  readonly fav: boolean;
  // --- tab options --- each is sent only when set (Settings > Tabs).
  /** Changed while out of sight and not looked at since. */
  readonly unread?: boolean;
  /** The count a page title announces ("(3) Inbox"), 1 to 999. */
  readonly notify?: number;
  /** The page is asleep: the icon is dimmed. */
  readonly asleep?: boolean;
  /** Page load progress, 0 to 100. */
  readonly pct?: number;
  /** Seconds until the tab's periodic reload. */
  readonly rl?: number;
  /** Not used for longer than the inactive threshold. */
  readonly idle?: boolean;
  // --- end tab options ---
}
export interface TabsMsg {
  readonly t: 'tabs';
  /** Active tab id, 0 when none. */
  readonly active: number;
  readonly items: readonly TabItem[];
}
export interface FavMsg {
  readonly t: 'fav';
  readonly id: number;
  readonly uri: string;
}
export interface RailItem {
  /** Rail key (see RAIL_KEY_RE in rail-logic.ts). */
  readonly k: string;
  readonly icon: RailIconName;
  readonly label: string;
  readonly active: boolean;
  readonly dot: boolean;
  /** Safe CSS colour, or '' for the theme accent. */
  readonly accent: string;
  /** A favicon is cached for this key (see the `railfav` message). */
  readonly fav: boolean;
  /** --- tab stacks and workspaces (T10) --- the workspace's tab count badge (absent: none). */
  readonly n?: number;
  /** The emoji a workspace shows instead of its glyph. */
  readonly emoji?: string;
}
export interface RailMsg {
  readonly t: 'rail';
  readonly items: readonly RailItem[];
}
export interface RailFavMsg {
  readonly t: 'railfav';
  readonly k: string;
  readonly uri: string;
}
export interface PanelShowMsg { readonly t: 'panelshow'; readonly p: PanelToken }
// --- rail flyouts ---
/** The panel page is shown as a rail flyout; `caret` is the caret centre in CSS px from the surface top, or -1. */
export interface FlyoutMsg { readonly t: 'flyout'; readonly caret: number }
export const MAX_FLYOUT_CARET = 4000;
// --- end rail flyouts ---
/** `side` is the far side the tabs sit on (`right` for a vertical sidebar, `bottom` for a strip); absent, the near one. */
export interface LayoutMsg { readonly t: 'layout'; readonly mode: 'vertical' | 'horizontal'; readonly side?: 'right' | 'bottom' }
// --- tab options ---
export const LOAD_INDICATORS = ['spinner', 'bar', 'favicon', 'none'] as const;
export type LoadIndicator = typeof LOAD_INDICATORS[number];
/** What the strip needs from the Tabs settings; sent again whenever any of it changes. */
export interface TabOptsMsg {
  readonly t: 'tabopts';
  readonly closeLeft: boolean;
  readonly closeAlways: boolean;
  readonly scroll: boolean;
  /** Narrowest the active tab gets, in pixels; 0 leaves it to the strip. */
  readonly minWidth: number;
  readonly indicator: LoadIndicator;
  readonly wheel: boolean;
  readonly separator: boolean;
  readonly alignClose: boolean;
  readonly selection: boolean;
  /** --- wave-1 fixes --- the active tab joins a Ctrl or Shift click selection. */
  readonly selectionActive: boolean;
  readonly removeSpacing: boolean;
  readonly maximised: boolean;
  /** Tabs in the closed-tabs list; 0 hides the Closed tabs button. */
  readonly closed: number;
  /** --- tab stacks (T10) --- a held tab resting over another for `stackDelay` ms stacks on drop. */
  readonly stackDrag: boolean;
  readonly stackDelay: number;
  /** --- two-level stacks (T10b) --- the second tab row Rust laid out (`chrome::layout::StackRow`). */
  readonly stackRow: StackRowSize;
  // --- toolbars and feeds (thread T13) ---
  /** `toolbar_tabs` lists `closed-tabs`: the Closed tabs button may show at all. */
  readonly closedButton: boolean;
  /** The tab bar's command buttons, in order. */
  readonly tools: readonly ToolButtonMsg[];
  // --- end toolbars and feeds ---
}
export const STACK_ROWS = ['off', 'regular', 'compact'] as const;
export type StackRowSize = typeof STACK_ROWS[number];
export const MAX_SELECTION = 512;
/** The strip's selection, as the comma list Rust parses. False when the ids are not tab ids. */
export function sendTabSelection(ids: readonly number[]): boolean {
  if (ids.length > MAX_SELECTION || !ids.every((id) => isInt(id, 0, MAX_ID))) return false;
  return send({ v: 1, t: 'tabsel', ids: ids.join(',') });
}
export function sendClosedTabs(): boolean {
  return send({ v: 1, t: 'closedtabs' });
}
// --- tab stacks (T10) ---
/** Most notches one `railwheel` carries (Rust's `MAX_RAIL_WHEEL`). */
export const MAX_RAIL_WHEEL = 8;
/** The held tab `id` was dropped on tab `onto` after resting over it. */
export function sendTabStack(id: number, onto: number): boolean {
  return isInt(id, 1, MAX_ID) && isInt(onto, 1, MAX_ID) && id !== onto && send({ v: 1, t: 'tabstack', id, onto });
}
/** The wheel turned `d` notches (down positive) over the workspaces. */
export function sendRailWheel(d: number): boolean {
  return Number.isInteger(d) && d !== 0 && Math.abs(d) <= MAX_RAIL_WHEEL && send({ v: 1, t: 'railwheel', d });
}
// --- end tab stacks ---
// --- toolbars and feeds (thread T13) ---
/** A command button of the address bar or the tab bar: Rust runs it only if `item` is at `index` in that bar's list. */
export function sendTool(bar: 'address' | 'tabs', item: string, index: number): boolean {
  if (!TOOL_ITEM_RE.test(item) || !isInt(index, 0, 13)) return false;
  return send({ v: 1, t: 'tool', b: bar, k: item, i: index });
}
// --- end toolbars and feeds ---
// --- end tab options ---
// --- vertical sidebar (lane S) ---
export interface WorkspaceMsg {
  readonly t: 'workspace';
  readonly name: string;
  readonly icon: RailIconName;
  /** --- tab stacks and workspaces (T10) --- shown instead of the glyph. */
  readonly emoji?: string;
  /** Workspaces are off: the switcher hides. */
  readonly off?: boolean;
}
// --- end vertical sidebar ---
// --- settings page (docs/design/settings-page.md) ---
/** Sections of `bowser://settings/<section>`, the same list as `internal_page::Section` in Rust. */
export const SETTINGS_SECTIONS = ['profile', 'appearance', 'tabs', 'stacks', 'startpage', 'privacy', 'performance', 'pagetools', 'address', 'search', 'bookmarks', 'keyboard', 'commands', 'downloads', 'media', 'extensions', 'updates', 'about'] as const;
export type SettingsSection = typeof SETTINGS_SECTIONS[number];
/** Show `s` on the settings page (`''` is the top). Sent when Bowser navigates the tab, never as an echo. */
export interface SettingsGoMsg { readonly t: 'settingsgo'; readonly s: SettingsSection | '' }
// --- end settings page ---
export type SnapshotValue = string | number | boolean | null | readonly SnapshotValue[] | { readonly [key: string]: SnapshotValue };
export interface SnapshotMsg { readonly t: 'snap'; readonly p: PanelToken; readonly d: { readonly [key: string]: SnapshotValue } }
export type Inbound =
  | { readonly t: 'diagnosticInteraction'; readonly enabled: true }
  | StateMsg
  | SetTextMsg
  | FocusOmniboxMsg
  | CompletionMsg
  | TabsMsg
  | TabOptsMsg // --- tab options ---
  | FavMsg
  | RailMsg
  | RailFavMsg
  | PanelShowMsg
  | FlyoutMsg // --- rail flyouts ---
  | LayoutMsg
  // --- vertical sidebar (lane S) ---
  | WorkspaceMsg
  // --- end vertical sidebar ---
  // --- hover layer ---
  | HoverMsg
  | HoverHideMsg
  // --- end hover layer ---
  // --- omnibox suggest ---
  | SuggestMsg
  | SuggestSelMsg
  | SuggestOpenMsg
  // --- end omnibox suggest ---
  | MenuOpenMsg
  | DlButtonMsg // --- downloads ---
  | ExtButtonMsg // --- extensions ---
  | AddrMsg // --- address bar, search and bookmarks ---
  | SearchFieldMsg
  | SearchDoneMsg
  // --- split view ---
  | SplitInbound
  // --- end split view ---
  | SettingsGoMsg // --- settings page ---
  | TabGhostMsg // --- tab ghost ---
  | SnapshotMsg;

interface WebViewLike {
  postMessage(message: string): void;
  addEventListener(type: 'message', listener: (ev: { readonly data: unknown }) => void): void;
}

declare global {
  interface Window {
    chrome?: { webview?: WebViewLike };
  }
}

// Exactly the colours `is_safe_css_colour` accepts in Rust: `#` with 3, 4, 6 or 8 hex digits, or
// `rgb(`/`rgba(` over digits, commas, dots, spaces and percent signs with at least one digit.
const HEX_COLOR_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const RGB_COLOR_RE = /^rgba?\((?=[0-9,. %]*[0-9])[0-9,. %]*\)$/;

function isSafeColour(v: string): boolean {
  return HEX_COLOR_RE.test(v) || RGB_COLOR_RE.test(v);
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function isBool(v: unknown): v is boolean {
  return typeof v === 'boolean';
}

function isStr(v: unknown, max: number): v is string {
  return typeof v === 'string' && v.length <= max;
}

function isInt(v: unknown, min: number, max: number): v is number {
  return typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
}

function isOneOf<T extends string>(v: unknown, list: readonly T[]): v is T {
  return typeof v === 'string' && (list as readonly string[]).includes(v);
}

function hasOnlyKeys(v: Record<string, unknown>, keys: readonly string[]): boolean {
  return Object.keys(v).every((k) => keys.includes(k));
}

function hasAllKeys(v: Record<string, unknown>, keys: readonly string[]): boolean {
  return keys.every((k) => k in v);
}

const READER_PILL: readonly ReaderPill[] = ['none', 'available', 'on'];
const TRANSLATE_PILL: readonly TranslatePill[] = ['none', 'available', 'on'];
const SECURITY: readonly Security[] = ['secure', 'notsecure', 'certproblem', 'internal', 'unknown', 'none'];
const PANEL_TOKENS: readonly PanelToken[] = ['history', 'bookmarks', 'customize', 'notes', 'settings', 'privacy', 'rename', 'historypage', 'downloadspage', 'extensions', 'commands', 'protection', 'startpage', 'address', 'addressdlg', 'bookmarkbar', 'sessions', 'pagetools', 'feeds'];
/** Lower-case words joined by single hyphens (`workspace-create`), at most 24 characters. */
const PANEL_ACTION_RE = /^(?=.{1,24}$)[a-z]+(?:-[a-z]+)*$/;
const MAX_SNAPSHOT_DEPTH = 8;
const MAX_SNAPSHOT_KEYS = 4096;
const MAX_SNAPSHOT_OBJECT_KEYS = 128;
const MAX_SNAPSHOT_ARRAY_ITEMS = 4096;

/** Only validated, flat commands can cross to the Rust panel parser. */
export function sendPanel(t: PanelKind, fields: PanelFields): boolean {
  if (!PANEL_ACTION_RE.test(fields.a)) return false;
  const keys = Object.keys(fields);
  if (keys.length > 4 || keys.some(k => !['a', 'id', 'parent', 'before', 'n', 'on', 'text', 'url'].includes(k))) return false;
  for (const key of ['id', 'parent', 'before'] as const) {
    const value = fields[key];
    if (value !== undefined && !isInt(value, 0, Number.MAX_SAFE_INTEGER)) return false;
  }
  if (fields.n !== undefined && !isInt(fields.n, -0x80000000, 0x7fffffff)) return false;
  if (fields.on !== undefined && !isBool(fields.on)) return false;
  if (fields.text !== undefined && !isStr(fields.text, MAX_TEXT)) return false;
  if (fields.url !== undefined && !isStr(fields.url, MAX_TEXT)) return false;
  return send({ v: 1, t, ...fields });
}

// --- vertical sidebar (lane S) ---
export function sendSide(a: 'workspace' | 'newtab' | 'collapse'): boolean {
  return (a === 'workspace' || a === 'newtab' || a === 'collapse') && send({ v: 1, t: 'side', a });
}
// --- end vertical sidebar ---

/**
 * The cards of a menu that has submenus, in CSS pixels of the popup page: the parent card (Rust
 * anchors it and clips the window to it), and the part of a child card that shows, while there is one.
 */
export interface PopupCards {
  px: number;
  py: number;
  pw: number;
  ph: number;
  cx?: number;
  cy?: number;
  cw?: number;
  ch?: number;
}

export function sendPopupSize(tok: number, w: number, h: number, cards?: PopupCards): boolean {
  if (!isInt(tok, 0, MAX_ID) || !isInt(w, 1, 4000) || !isInt(h, 1, 4000)) return false;
  if (!cards) return send({ v: 1, t: 'size', tok, w, h });
  const { px, py, pw, ph, cx, cy, cw, ch } = cards;
  if (![px, py].every((n) => isInt(n, 0, 4000)) || ![pw, ph].every((n) => isInt(n, 1, 4000))) return false;
  // One string keeps the message inside the six-field cap (v, t, tok, w, h, c): the parent alone, or
  // the parent and then the child.
  if (cx === undefined && cy === undefined && cw === undefined && ch === undefined) {
    return send({ v: 1, t: 'size', tok, w, h, c: `${px},${py},${pw},${ph}` });
  }
  if (![cx, cy].every((n) => isInt(n, 0, 4000)) || ![cw, ch].every((n) => isInt(n, 1, 4000))) return false;
  const c = `${px},${py},${pw},${ph},${cx},${cy},${cw},${ch}`;
  return send({ v: 1, t: 'size', tok, w, h, c });
}
export function sendPopupPick(tok: number, id: number): boolean {
  return isInt(tok, 0, MAX_ID) && isInt(id, 0, MAX_ID) && send({ v: 1, t: 'pick', tok, id });
}
export function sendPopupDismiss(tok: number): boolean {
  return isInt(tok, 0, MAX_ID) && send({ v: 1, t: 'dismiss', tok });
}
// --- popup surfaces (lane PP) ---
/** Largest `n` an `act` may carry (Rust: MAX_ACT_CHOICE). */
export const MAX_ACT_CHOICE = 7;
/** In-place action on an open popup page (site info control); the popup stays open. */
export function sendPopupAct(tok: number, id: number, n: number): boolean {
  return isInt(tok, 0, MAX_ID) && isInt(id, 0, MAX_ID) && isInt(n, 0, MAX_ACT_CHOICE) && send({ v: 1, t: 'act', tok, id, n });
}

/** Longest composer name Rust accepts (MAX_COMPOSER_NAME_CHARS). */
export const MAX_COMPOSE_NAME = 128;
/**
 * Enter or the confirm row of an open composer page: the name and, for web apps, the address as
 * typed. Rust validates both again and answers with the page (an error, or `done`).
 */
export function sendPopupCompose(tok: number, name: string, url?: string): boolean {
  if (!isInt(tok, 0, MAX_ID) || !isStr(name, MAX_COMPOSE_NAME) || (url !== undefined && !isStr(url, MAX_TEXT))) return false;
  return send(url === undefined ? { v: 1, t: 'compose', tok, name } : { v: 1, t: 'compose', tok, name, url });
}

export interface ComposerOpenFields {
  /** The web app an edit changes. */
  readonly id?: number;
  /** Prefill (a preset): both or neither. */
  readonly name?: string;
  readonly url?: string;
}
/**
 * A Customize flyout button asks Rust for an anchored composer, opening below-left of `rect` (the
 * button, in the panel's CSS pixels).
 */
export function sendComposerOpen(
  mode: 'workspace-new' | 'app-add' | 'app-edit',
  rect: Parameters<typeof rectWire>[0],
  fields: ComposerOpenFields = {},
): boolean {
  const r = rectWire(rect);
  if (r === null || !isRectWire(r)) return false;
  if ((mode === 'app-edit') !== (fields.id !== undefined)) return false;
  if (fields.id !== undefined && !isInt(fields.id, 0, MAX_ID)) return false;
  if (fields.name !== undefined && !isStr(fields.name, MAX_COMPOSE_NAME)) return false;
  if (fields.url !== undefined && !isStr(fields.url, MAX_TEXT)) return false;
  if (mode === 'workspace-new' && (fields.name !== undefined || fields.url !== undefined)) return false;
  return send({ v: 1, t: 'composeopen', a: mode, ...fields, r });
}
// --- end popup surfaces ---

// --- downloads (docs/design/downloads.md) ---
/** Largest `n` a `dlact` may carry: 0-7 row actions, 8-10 list actions (id 0). */
export const MAX_DLACT = 10;
/** The toolbar's download button was clicked; `rect` is the button in toolbar CSS px. */
export function sendDownloadToggle(rect: Parameters<typeof rectWire>[0]): boolean {
  const text = rectWire(rect);
  return text !== null && isRectWire(text) && send({ v: 1, t: 'dl', a: 'toggle', text });
}
/** A row or list action on the downloads bubble (`tok` is the page's). */
export function sendDownloadAct(tok: number, id: number, n: number): boolean {
  return isInt(tok, 0, MAX_ID) && isInt(id, 0, MAX_ID) && isInt(n, 0, MAX_DLACT) && (n < 8 ? id > 0 : id === 0) && send({ v: 1, t: 'dlact', tok, id, n });
}

/** The toolbar's extensions button was clicked; Rust opens the popup or the list of them under `rect`. */
export function sendExtensionsButton(rect: Parameters<typeof rectWire>[0]): boolean {
  const text = rectWire(rect);
  return text !== null && isRectWire(text) && send({ v: 1, t: 'ext', a: 'open', text });
}

function parseExtButton(raw: Record<string, unknown>): ExtButtonMsg | null {
  const keys = ['v', 't', 'on', 'n', 'name'];
  if (!hasOnlyKeys(raw, keys) || !hasAllKeys(raw, keys) || raw.v !== 1) return null;
  const { on, n, name } = raw;
  if (!isBool(on) || !isInt(n, 0, MAX_EXTENSIONS) || !isStr(name, MAX_EXTENSION_NAME * 2) || Array.from(name).length > MAX_EXTENSION_NAME) return null;
  // Shown exactly when there is something to open; a name only for a single one.
  if (on !== n > 0 || (name !== '') !== (n === 1)) return null;
  return { t: 'extbtn', on, n, name };
}

function parseDlButton(raw: Record<string, unknown>): DlButtonMsg | null {
  const keys = ['v', 't', 'on', 'state', 'n', 'pct', 'burst'];
  if (!hasOnlyKeys(raw, keys) || !hasAllKeys(raw, keys) || raw.v !== 1) return null;
  const { on, state, n, pct, burst } = raw;
  if (!isBool(on) || !isOneOf(state, DL_BUTTON_STATES) || !isInt(n, 0, MAX_ID) || !isInt(pct, -1, 100) || !isInt(burst, 0, MAX_ID)) return null;
  return { t: 'dlbtn', on, state, n, pct, burst };
}
// --- end downloads ---

// --- hover layer ---
/** Code-point-safe cap, matching Rust's character count. */
function capCodePoints(text: string, max: number): string {
  const points = Array.from(text);
  return points.length <= max ? text : points.slice(0, max).join('');
}

/** `"x,y,w,h"` for a client rectangle: edges clamped to 0..MAX_HOVER_COORD, rounded outwards. */
export function rectWire(rect: { readonly left: number; readonly top: number; readonly right: number; readonly bottom: number }): string | null {
  const values = [rect.left, rect.top, rect.right, rect.bottom];
  if (!values.every(Number.isFinite)) return null;
  const clamp = (v: number): number => Math.min(MAX_HOVER_COORD, Math.max(0, v));
  const x = clamp(Math.floor(rect.left));
  const y = clamp(Math.floor(rect.top));
  const w = clamp(Math.ceil(rect.right) - x);
  const h = clamp(Math.ceil(rect.bottom) - y);
  return `${x},${y},${w},${h}`;
}

const RECT_RE = /^(0|[1-9][0-9]{0,4}),(0|[1-9][0-9]{0,4}),(0|[1-9][0-9]{0,4}),(0|[1-9][0-9]{0,4})$/;
/** The grammar Rust's `HoverRect::parse` accepts. */
export function isRectWire(value: string): boolean {
  const match = RECT_RE.exec(value);
  return match !== null && match.slice(1).every(part => Number(part) <= MAX_HOVER_COORD);
}

// --- rail flyouts ---
/** A rail button click, menu or flyout hover, with the button's rectangle when it is known. */
export function sendRail(a: RailAction, k: string, rect?: Parameters<typeof rectWire>[0]): boolean {
  const r = rect ? rectWire(rect) : null;
  if (a === 'hover' && r === null) return false;
  return send(r === null ? { v: 1, t: 'rail', a, k } : { v: 1, t: 'rail', a, k, r });
}
// --- end rail flyouts ---

export function sendTip(text: string, rect: Parameters<typeof rectWire>[0]): boolean {
  const capped = capCodePoints(text, MAX_TIP);
  const r = rectWire(rect);
  return capped.trim() !== '' && r !== null && send({ v: 1, t: 'tip', text: capped, r });
}
export function sendTipOff(): boolean {
  return send({ v: 1, t: 'tipoff' });
}
export function sendHover(k: HoverTargetKind | 'none', id = 0, rect?: Parameters<typeof rectWire>[0]): boolean {
  if (k === 'none') return send({ v: 1, t: 'hover', k });
  const r = rect ? rectWire(rect) : null;
  return (k === 'tab' || k === 'ball') && isInt(id, 0, MAX_ID) && r !== null && send({ v: 1, t: 'hover', k, id, r });
}
export function sendHoverAct(tok: number, fields: HoverActFields): boolean {
  if (!isInt(tok, 0, MAX_ID)) return false;
  switch (fields.a) {
    case 'marker': if (!isInt(fields.n, 0, HOVER_MARKERS - 1)) return false; break;
    case 'row': if (!isInt(fields.n, 0, MAX_HOVER_ROW_INDEX)) return false; break;
    case 'swatch': if (!isInt(fields.n, 0, MAX_HOVER_SWATCH_INDEX)) return false; break;
    case 'name': if (typeof fields.text !== 'string' || Array.from(fields.text).length > MAX_ISLAND_NAME) return false; break;
    case 'next': case 'close': case 'focus': case 'dismiss': break;
    default: return false;
  }
  return send({ v: 1, t: 'hact', tok, ...fields });
}

// --- omnibox suggest ---
const SUGGEST_ITEM_KEYS = ['k', 'text', 'title', 'url', 'fav', 'bd'];
function parseSuggestItem(v: unknown): SuggestItem | null {
  if (!isRecord(v) || !hasOnlyKeys(v, SUGGEST_ITEM_KEYS) || !['k', 'text', 'title', 'url'].every(key => key in v)) return null;
  const { k, text, title, url } = v;
  if (!isOneOf(k, SUGGEST_KINDS)) return null;
  if (!isStr(text, MAX_SUGGEST_TEXT) || !isStr(title, MAX_TITLE) || !isStr(url, MAX_SUGGEST_TEXT)) return null;
  const fav = 'fav' in v ? v.fav : '';
  if (fav !== '' && !isFavUri(fav)) return null;
  if ('bd' in v && !isBool(v.bd)) return null;
  return { k, text, title, url, fav: fav as string, ...(v.bd === true ? { bd: true } : {}) };
}

/** Strict validation of a `suggest` message. */
function parseSuggestMsg(raw: Record<string, unknown>): SuggestMsg | null {
  if (!hasOnlyKeys(raw, ['v', 't', 'tok', 'seq', 'w', 'sel', 'q', 'items']) || raw.v !== 1) return null;
  const { tok, seq, w, sel, q, items } = raw;
  if (!isInt(tok, 0, MAX_ID) || !isInt(seq, 0, MAX_SEQ) || !isInt(w, 0, MAX_SUGGEST_WIDTH)) return null;
  // Rust caps the query in UTF-16 units (twice the row text limit).
  if (!isStr(q, MAX_SUGGEST_TEXT * 2)) return null;
  if (!Array.isArray(items) || items.length > MAX_SUGGEST_ITEMS || !isInt(sel, 0, MAX_SUGGEST_ITEMS - 1)) return null;
  if (items.length > 0 && sel >= items.length) return null;
  // One bad row (say an over-long one) must not take the whole list down with it: keep the valid
  // rows, and move the selection with them (to the first row if its own was dropped). A list whose
  // rows are all invalid is rejected.
  const out: SuggestItem[] = [];
  let keptSel = items.length === 0 ? sel : 0;
  items.forEach((v, index) => {
    const item = parseSuggestItem(v);
    if (!item) return;
    if (index === sel) keptSel = out.length;
    out.push(item);
  });
  if (items.length > 0 && out.length === 0) return null;
  return { t: 'suggest', tok, seq, w, sel: keptSel, q, items: out };
}

export function sendSuggestPick(seq: number, i: number): boolean {
  return isInt(seq, 0, MAX_SEQ) && isInt(i, 0, MAX_SUGGEST_ITEMS - 1) && send({ v: 1, t: 'suggestPick', seq, i });
}
export function sendSuggestPreview(seq: number, i: number): boolean {
  return isInt(seq, 0, MAX_SEQ) && isInt(i, 0, MAX_SUGGEST_ITEMS - 1) && send({ v: 1, t: 'suggestPreview', seq, i });
}
export function sendSuggestEnd(): boolean {
  return send({ v: 1, t: 'suggestEnd' });
}
// --- end omnibox suggest ---

const HOVER_ROW_KEYS = ['title', 'active', 'linked', 'fav'];
function parseHoverRows(v: unknown): HoverRowMsg[] | null {
  if (!Array.isArray(v) || v.length > MAX_HOVER_ROWS) return null;
  const rows: HoverRowMsg[] = [];
  for (const row of v) {
    if (!isRecord(row) || !hasOnlyKeys(row, HOVER_ROW_KEYS) || !hasAllKeys(row, HOVER_ROW_KEYS)) return null;
    const { title, active, linked, fav } = row;
    if (!isStr(title, MAX_TITLE) || !isBool(active) || !isBool(linked) || (fav !== '' && !isFavUri(fav))) return null;
    rows.push({ title, active, linked, fav: fav as string });
  }
  return rows;
}
function parseColours(v: unknown, max: number): string[] | null {
  if (!Array.isArray(v) || v.length > max) return null;
  const out: string[] = [];
  for (const colour of v) {
    if (!isStr(colour, MAX_COLOR) || (colour !== '' && !isSafeColour(colour))) return null;
    out.push(colour);
  }
  return out;
}
const TOOLTIP_KEYS = ['v', 't', 'tok', 'kind', 'text'];
const CARD_KEYS = ['v', 't', 'tok', 'kind', 'title', 'host', 'marker', 'markers', 'sleeping', 'fav', 'rows', 'more'];
/** Longest thumbnail data: URI Rust sends (36,000 bytes of JPEG as base64). */
export const MAX_THUMB_URI = 48 * 1024;
const THUMB_PREFIX = 'data:image/jpeg;base64,';
function isThumbUri(v: unknown): v is string {
  return typeof v === 'string' && v.length <= MAX_THUMB_URI && v.startsWith(THUMB_PREFIX) && BASE64_RE.test(v.slice(THUMB_PREFIX.length));
}
const ISLAND_KEYS = ['v', 't', 'tok', 'kind', 'name', 'hint', 'slot', 'swatches', 'rows', 'more', 'close'];

/** Strict validation of a Rust `hover` message (tooltip, card or island content). */
export function parseHoverMsg(raw: Record<string, unknown>): HoverMsg | null {
  if (raw.v !== 1 || !isInt(raw.tok, 1, MAX_ID)) return null;
  const tok = raw.tok;
  switch (raw.kind) {
    case 'tooltip': {
      if (!hasOnlyKeys(raw, TOOLTIP_KEYS) || !hasAllKeys(raw, TOOLTIP_KEYS) || !isStr(raw.text, MAX_TIP * 2) || Array.from(raw.text).length > MAX_TIP) return null;
      return { t: 'hover', tok, kind: 'tooltip', text: raw.text };
    }
    case 'card': {
      if (!hasOnlyKeys(raw, [...CARD_KEYS, 'thumb', 'mem']) || !hasAllKeys(raw, CARD_KEYS)) return null;
      if (('thumb' in raw && !isThumbUri(raw.thumb)) || ('mem' in raw && !isInt(raw.mem, 0, 99999))) return null; // --- tab options ---
      const { title, host, marker, sleeping, fav, more } = raw;
      const markers = parseColours(raw.markers, HOVER_MARKERS);
      const rows = parseHoverRows(raw.rows);
      if (!isStr(title, MAX_TITLE) || !isStr(host, MAX_HOVER_HOST) || !isInt(marker, -1, HOVER_MARKERS - 1)) return null;
      if (!isBool(sleeping) || (fav !== '' && !isFavUri(fav)) || !isInt(more, 0, MAX_ID) || !markers || !rows) return null;
      return {
        t: 'hover', tok, kind: 'card', title, host, marker, markers, sleeping, fav: fav as string, rows, more,
        ...(typeof raw.thumb === 'string' ? { thumb: raw.thumb } : {}), // --- tab options ---
        ...(typeof raw.mem === 'number' ? { mem: raw.mem } : {}),
      };
    }
    case 'island': {
      if (!hasOnlyKeys(raw, ISLAND_KEYS) || !hasAllKeys(raw, ISLAND_KEYS)) return null;
      const { name, hint, slot, more, close } = raw;
      const swatches = parseColours(raw.swatches, MAX_HOVER_SWATCHES);
      const rows = parseHoverRows(raw.rows);
      if (!isStr(name, MAX_ISLAND_NAME * 2) || Array.from(name).length > MAX_ISLAND_NAME) return null;
      if (!isStr(hint, MAX_ISLAND_NAME * 2) || Array.from(hint).length > MAX_ISLAND_NAME) return null;
      if (!isStr(close, MAX_ISLAND_NAME * 2) || !isInt(more, 0, MAX_ID) || !swatches || !rows) return null;
      if (!isInt(slot, 0, Math.max(0, swatches.length - 1))) return null;
      return { t: 'hover', tok, kind: 'island', name, hint, slot, swatches, rows, more, close };
    }
    default:
      return null;
  }
}
// --- end hover layer ---

function validSnapshot(value: unknown, depth: number, budget: { keys: number }): value is SnapshotValue {
  if (depth > MAX_SNAPSHOT_DEPTH) return false;
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true;
  if (typeof value === 'number') return Number.isFinite(value);
  if (Array.isArray(value)) {
    if (value.length > MAX_SNAPSHOT_ARRAY_ITEMS) return false;
    return value.every(item => validSnapshot(item, depth + 1, budget));
  }
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  budget.keys += keys.length;
  if (keys.length > MAX_SNAPSHOT_OBJECT_KEYS || budget.keys > MAX_SNAPSHOT_KEYS) return false;
  return keys.every(key => validSnapshot(value[key], depth + 1, budget));
}

/** Sends one command. Returns false when the host bridge is missing or the message is too large. */
export function send(cmd: Command): boolean {
  const wv = window.chrome?.webview;
  if (!wv) return false;
  const wire = JSON.stringify(cmd);
  if (wire.length > MAX_WIRE) return false;
  wv.postMessage(wire);
  return true;
}

/** Registers the listener for host messages; invalid or unknown messages are dropped. */
export function onInbound(listener: (msg: Inbound) => void): void {
  const wv = window.chrome?.webview;
  if (!wv) return;
  wv.addEventListener('message', (ev) => {
    const msg = parseInbound(ev.data);
    if (msg) listener(msg);
  });
}

function parseTheme(v: unknown): Theme {
  const out: Theme = {};
  if (!isRecord(v)) return out;
  for (const token of THEME_TOKENS) {
    const colour = v[token];
    if (isStr(colour, MAX_COLOR) && isSafeColour(colour)) out[token] = colour;
  }
  return out;
}

const TAB_KEYS = ['id', 'title', 'loading', 'pinned', 'muted', 'private', 'marker', 'g', 'gl', 'fav', 'gname', 'gcol', 'gcollapsed', 'gsize', 'unread', 'notify', 'asleep', 'pct', 'rl', 'idle'];
const LEADER_KEYS = ['gname', 'gcol', 'gcollapsed', 'gsize'];

/** Strict validation of one `tabs` item. Ids must be new within the message (`seen`). */
function parseTabItem(v: unknown, seen: ReadonlySet<number>): TabItem | null {
  if (!isRecord(v) || !hasOnlyKeys(v, TAB_KEYS)) return null;
  const { id, title, loading, pinned, muted, private: priv, marker, g, gl, fav } = v;
  if (!isInt(id, 0, MAX_ID) || seen.has(id)) return null;
  if (!isStr(title, MAX_TITLE) || !isBool(loading) || !isBool(pinned) || !isBool(muted) || !isBool(priv) || !isBool(fav)) return null;
  if (!isInt(marker, -1, 7) || !isInt(g, 0, MAX_ID) || !isBool(gl)) return null;
  // --- tab options --- optional marks: absent means off.
  if (('unread' in v && !isBool(v.unread)) || ('asleep' in v && !isBool(v.asleep)) || ('idle' in v && !isBool(v.idle))) return null;
  if (('notify' in v && !isInt(v.notify, 1, 999)) || ('pct' in v && !isInt(v.pct, 0, 100)) || ('rl' in v && !isInt(v.rl, 0, 86400))) return null;
  const marks = {
    ...(v.unread === true ? { unread: true } : {}),
    ...(typeof v.notify === 'number' ? { notify: v.notify } : {}),
    ...(v.asleep === true ? { asleep: true } : {}),
    ...(typeof v.pct === 'number' ? { pct: v.pct } : {}),
    ...(typeof v.rl === 'number' ? { rl: v.rl } : {}),
    ...(v.idle === true ? { idle: true } : {}),
  };
  // --- end tab options ---
  const base = { id, title, loading, pinned, muted, private: priv, marker, g, gl, fav, ...marks };
  if (!gl) {
    // Group fields are leader-only: a member must not carry them.
    if (LEADER_KEYS.some((k) => k in v)) return null;
    return base;
  }
  const { gname, gcol, gcollapsed, gsize } = v;
  if (g === 0) return null;
  if (!isStr(gname, MAX_TITLE) || !isInt(gcol, -1, 7) || !isBool(gcollapsed) || !isInt(gsize, 1, MAX_ID)) return null;
  return { ...base, gname, gcol, gcollapsed, gsize };
}

const RAIL_ITEM_KEYS = ['k', 'icon', 'label', 'active', 'dot', 'accent', 'fav'];
/** --- tab stacks and workspaces (T10) --- keys an item carries only when set. */
const RAIL_ITEM_OPTIONAL = ['n', 'emoji'];
/** Longest workspace emoji in bytes (Rust's `MAX_EMOJI_BYTES`; at most eight characters). */
const MAX_EMOJI = 32;
/** A workspace emoji: short, no spaces or control characters (Rust's `clean_emoji`). */
export function isWorkspaceEmoji(v: unknown): v is string {
  return typeof v === 'string' && v.length > 0 && new TextEncoder().encode(v).length <= MAX_EMOJI && Array.from(v).length <= 8 && !/[\s\u0000-\u001f\u007f]/.test(v);
}

/** A PNG data: URI with base64 payload, at most MAX_FAV_URI characters. */
function isFavUri(v: unknown): v is string {
  if (!isStr(v, MAX_FAV_URI) || !v.startsWith(FAV_PREFIX)) return false;
  return BASE64_RE.test(v.slice(FAV_PREFIX.length));
}

/** Strict validation of one `rail` item: every key present, none extra, key unique within the message. */
function parseRailItem(v: unknown, seen: ReadonlySet<string>): RailItem | null {
  if (!isRecord(v) || !hasOnlyKeys(v, [...RAIL_ITEM_KEYS, ...RAIL_ITEM_OPTIONAL]) || !hasAllKeys(v, RAIL_ITEM_KEYS)) return null;
  const { k, icon, label, active, dot, accent, fav } = v;
  if (typeof k !== 'string' || !isRailKey(k) || seen.has(k)) return null;
  if (!isRailIcon(icon) || !isStr(label, MAX_RAIL_LABEL) || !isBool(active) || !isBool(dot) || !isBool(fav)) return null;
  if (!isStr(accent, MAX_COLOR) || (accent !== '' && !isSafeColour(accent))) return null;
  // --- tab stacks and workspaces (T10) ---
  if ('n' in v && !isInt(v.n, 1, 9999)) return null;
  if ('emoji' in v && !isWorkspaceEmoji(v.emoji)) return null;
  return { k, icon, label, active, dot, accent, fav, ...('n' in v ? { n: v.n as number } : {}), ...('emoji' in v ? { emoji: v.emoji as string } : {}) };
}

/** Strictly validates a host message. Accepts an object or its JSON text; returns null for anything unexpected. */
export function parseInbound(data: unknown): Inbound | null {
  let raw: unknown = data;
  if (typeof raw === 'string') {
    if (new TextEncoder().encode(raw).length > MAX_INBOUND) return null;
    try {
      raw = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (!isRecord(raw)) return null;
  if (typeof data !== 'string') {
    try { if (new TextEncoder().encode(JSON.stringify(raw)).length > MAX_INBOUND) return null; }
    catch { return null; }
  }

  switch (raw.t) {
    case 'diagnosticInteraction':
      if (!hasOnlyKeys(raw, ['v', 't', 'enabled']) || raw.v !== 1 || raw.enabled !== true) return null;
      return { t: 'diagnosticInteraction', enabled: true };
    case 'panelshow': {
      if (!hasOnlyKeys(raw, ['v', 't', 'p']) || raw.v !== 1 || !isOneOf(raw.p, PANEL_TOKENS)) return null;
      return { t: 'panelshow', p: raw.p };
    }
    // --- rail flyouts ---
    case 'flyout': {
      if (!hasOnlyKeys(raw, ['v', 't', 'caret']) || raw.v !== 1 || !isInt(raw.caret, -1, MAX_FLYOUT_CARET)) return null;
      return { t: 'flyout', caret: raw.caret };
    }
    // --- end rail flyouts ---
    case 'layout': {
      if (!hasOnlyKeys(raw, ['v', 't', 'mode', 'side']) || raw.v !== 1 || !isOneOf(raw.mode, ['vertical', 'horizontal'] as const)) return null;
      if (raw.side === undefined) return { t: 'layout', mode: raw.mode };
      const far = raw.mode === 'vertical' ? 'right' : 'bottom';
      if (raw.side !== far) return null;
      return { t: 'layout', mode: raw.mode, side: far };
    }
    // --- vertical sidebar (lane S) ---
    case 'workspace': {
      if (!hasOnlyKeys(raw, ['v', 't', 'name', 'icon', 'emoji', 'off']) || raw.v !== 1 || !isStr(raw.name, MAX_RAIL_LABEL) || !isRailIcon(raw.icon) || !raw.icon.startsWith('g:')) return null;
      // --- tab stacks and workspaces (T10) --- optional emoji and off flag.
      if ('emoji' in raw && !isWorkspaceEmoji(raw.emoji)) return null;
      if ('off' in raw && raw.off !== true) return null;
      return { t: 'workspace', name: raw.name, icon: raw.icon, ...('emoji' in raw ? { emoji: raw.emoji as string } : {}), ...('off' in raw ? { off: true } : {}) };
    }
    // --- end vertical sidebar ---
    // --- hover layer ---
    case 'hover':
      return parseHoverMsg(raw);
    case 'hoverhide':
      return hasOnlyKeys(raw, ['v', 't']) && raw.v === 1 ? { t: 'hoverhide' } : null;
    // --- end hover layer ---
    // --- omnibox suggest ---
    case 'suggest':
      return parseSuggestMsg(raw);
    case 'suggestSel': {
      if (!hasOnlyKeys(raw, ['v', 't', 'seq', 'sel']) || raw.v !== 1) return null;
      const { seq, sel } = raw;
      if (!isInt(seq, 0, MAX_SEQ) || !isInt(sel, 0, MAX_SUGGEST_ITEMS - 1)) return null;
      return { t: 'suggestSel', seq, sel };
    }
    case 'suggestOpen':
      if (!hasOnlyKeys(raw, ['v', 't', 'on']) || raw.v !== 1 || !isBool(raw.on)) return null;
      return { t: 'suggestOpen', on: raw.on };
    // --- end omnibox suggest ---
    case 'menuOpen':
      if (!hasOnlyKeys(raw, ['v', 't', 'on']) || raw.v !== 1 || !isBool(raw.on)) return null;
      return { t: 'menuOpen', on: raw.on };
    // --- downloads ---
    case 'dlbtn':
      return parseDlButton(raw);
    case 'extbtn':
      return parseExtButton(raw);
    // --- address bar, search and bookmarks ---
    case 'addr':
      return parseAddr(raw);
    case 'sfield':
      return parseSearchField(raw);
    case 'sdone':
      return hasOnlyKeys(raw, ['v', 't']) && raw.v === 1 ? { t: 'sdone' } : null;
    // --- end address bar, search and bookmarks ---
    // --- end downloads ---
    // --- split view ---
    case 'pair':
    case 'splitpick':
    case 'splitdrop':
    case 'splithide':
    case 'privatepage':
      return parseSplitInbound(raw);
    // --- end split view ---
    case 'tabghost': // --- tab ghost ---
      return parseTabGhost(raw);
    // --- settings page ---
    case 'settingsgo': {
      if (!hasOnlyKeys(raw, ['v', 't', 's']) || raw.v !== 1 || !(raw.s === '' || isOneOf(raw.s, SETTINGS_SECTIONS))) return null;
      return { t: 'settingsgo', s: raw.s };
    }
    // --- end settings page ---
    case 'snap': {
      if (!hasOnlyKeys(raw, ['v', 't', 'p', 'd']) || raw.v !== 1 || !isOneOf(raw.p, PANEL_TOKENS) || !isRecord(raw.d)) return null;
      const budget = { keys: 0 };
      if (!validSnapshot(raw.d, 1, budget)) return null;
      return { t: 'snap', p: raw.p, d: raw.d as SnapshotMsg['d'] };
    }
    case 'state': {
      if (!hasOnlyKeys(raw, ['t', 'scale', 'pillx', 'pillw', 'menux', 'solid', 'dark', 'back', 'forward', 'loading', 'split', 'bookmarked', 'progress', 'security', 'label', 'showProgress', 'permblocked', 'zoom', 'showZoom', 'private', 'reader', 'translate', 'fontScale', 'uiFont', 'density', 'compactMenus', 'look', 'items', 'tools', 'extra', 'feeds', 'theme'])) return null;
      // --- toolbars and feeds (thread T13) --- the four come together or not at all.
      const barKeys = ['items', 'tools', 'extra', 'feeds'] as const;
      const barCount = barKeys.filter((key) => key in raw).length;
      if (barCount !== 0 && barCount !== barKeys.length) return null;
      if (barCount !== 0 && (!isStr(raw.items, 512) || parseTools(raw.tools) === null || !isInt(raw.extra, 0, 400) || !isInt(raw.feeds, 0, 9))) return null;
      // --- end toolbars and feeds ---
      if ('showProgress' in raw && !isBool(raw.showProgress)) return null;
      if ('permblocked' in raw && !isBool(raw.permblocked)) return null;
      if ('zoom' in raw && !validZoom(raw.zoom)) return null;
      if ('showZoom' in raw && !isBool(raw.showZoom)) return null;
      if ('private' in raw && !isBool(raw.private)) return null;
      if ('reader' in raw && !isOneOf(raw.reader, READER_PILL)) return null;
      if ('translate' in raw && !isOneOf(raw.translate, TRANSLATE_PILL)) return null;
      if ('fontScale' in raw && !isInt(raw.fontScale, 50, 200)) return null;
      if ('uiFont' in raw && !isStr(raw.uiFont, 64)) return null;
      if ('density' in raw && !isOneOf(raw.density, UI_DENSITIES)) return null; // --- layout (thread T6) ---
      if ('compactMenus' in raw && !isBool(raw.compactMenus)) return null;
      if ('look' in raw && parseLook(raw.look) === null) return null; // --- window and panels (thread T9) ---
      const { scale, solid, dark, back, forward, loading, progress, security, label } = raw;
      if (typeof scale !== 'number' || !Number.isFinite(scale)) return null;
      const placementKeys = ['pillx', 'pillw', 'menux'] as const;
      const placementCount = placementKeys.filter((key) => key in raw).length;
      if (placementCount !== 0 && placementCount !== placementKeys.length) return null;
      if (placementCount !== 0 && placementKeys.some((key) => !isInt(raw[key], 0, Number.MAX_SAFE_INTEGER))) return null;
      if (!isBool(solid) || !isBool(dark) || !isBool(back) || !isBool(forward) || !isBool(loading)) return null;
      if (!isInt(progress, 0, 100)) return null;
      if (('split' in raw && !isBool(raw.split)) || ('bookmarked' in raw && !isBool(raw.bookmarked))) return null;
      if (!isOneOf(security, SECURITY)) return null;
      if (!isStr(label, MAX_LABEL)) return null;
      return {
        t: 'state',
        scale,
        ...(placementCount === 3 ? { pillx: raw.pillx as number, pillw: raw.pillw as number, menux: raw.menux as number } : {}),
        solid,
        dark,
        back,
        forward,
        loading,
        ...(typeof raw.split === 'boolean' ? { split: raw.split } : {}),
        ...(typeof raw.bookmarked === 'boolean' ? { bookmarked: raw.bookmarked } : {}),
        progress,
        security,
        label,
        ...(isBool(raw.showProgress) ? { showProgress: raw.showProgress } : {}),
        ...(isBool(raw.permblocked) ? { permblocked: raw.permblocked } : {}),
        ...(typeof raw.zoom === 'number' ? { zoom: raw.zoom } : {}),
        ...(isBool(raw.showZoom) ? { showZoom: raw.showZoom } : {}),
        ...(isBool(raw.private) ? { private: raw.private } : {}),
        ...(isOneOf(raw.reader, READER_PILL) ? { reader: raw.reader } : {}),
        ...(isOneOf(raw.translate, TRANSLATE_PILL) ? { translate: raw.translate } : {}),
        ...(isInt(raw.fontScale, 50, 200) ? { fontScale: raw.fontScale } : {}),
        ...(isStr(raw.uiFont, 64) ? { uiFont: raw.uiFont } : {}),
        ...(isOneOf(raw.density, UI_DENSITIES) ? { density: raw.density } : {}), // --- layout (thread T6) ---
        ...(isBool(raw.compactMenus) ? { compactMenus: raw.compactMenus } : {}),
        ...('look' in raw ? { look: parseLook(raw.look)! } : {}), // --- window and panels (thread T9) ---
        ...(barCount === 4 ? { items: raw.items as string, tools: parseTools(raw.tools) as ToolButtonMsg[], extra: raw.extra as number, feeds: raw.feeds as number } : {}), // --- toolbars and feeds ---
        theme: parseTheme(raw.theme),
      };
    }
    case 'setText': {
      const { text, select, full } = raw;
      if (typeof text !== 'string' || !isBool(select)) return null;
      if (full !== undefined && typeof full !== 'string') return null;
      return { t: 'setText', text: truncateCodePoint(text, MAX_TEXT), select, ...(typeof full === 'string' ? { full: truncateCodePoint(full, MAX_TEXT) } : {}) };
    }
    case 'focusOmnibox': {
      const { select } = raw;
      if (!isBool(select)) return null;
      return { t: 'focusOmnibox', select };
    }
    case 'completion': {
      const { seq, suffix } = raw;
      if (!isInt(seq, 0, MAX_SEQ) || !isStr(suffix, MAX_TEXT)) return null;
      return { t: 'completion', seq, suffix };
    }
    case 'tabs': {
      if (!hasOnlyKeys(raw, ['v', 't', 'active', 'items']) || raw.v !== 1) return null;
      const { active, items } = raw;
      if (!isInt(active, 0, MAX_ID) || !Array.isArray(items) || items.length > MAX_TABS) return null;
      const seen = new Set<number>();
      const out: TabItem[] = [];
      for (const v of items) {
        const item = parseTabItem(v, seen);
        if (!item) return null;
        seen.add(item.id);
        out.push(item);
      }
      return { t: 'tabs', active, items: out };
    }
    // --- tab options ---
    case 'tabopts': {
      const keys = ['v', 't', 'closeLeft', 'closeAlways', 'scroll', 'minWidth', 'indicator', 'wheel', 'separator', 'alignClose', 'selection', 'selectionActive', 'removeSpacing', 'maximised', 'closed', 'stackDrag', 'stackDelay', 'stackRow', 'closedButton', 'tools'];
      if (!hasOnlyKeys(raw, keys) || !hasAllKeys(raw, keys) || raw.v !== 1) return null;
      const { closeLeft, closeAlways, scroll, minWidth, indicator, wheel, separator, alignClose, selection, selectionActive, removeSpacing, maximised, closed, stackDrag, stackDelay, stackRow, closedButton } = raw;
      if (![closeLeft, closeAlways, scroll, wheel, separator, alignClose, selection, selectionActive, removeSpacing, maximised, stackDrag, closedButton].every(isBool)) return null;
      if (!isInt(minWidth, 0, 400) || !isOneOf(indicator, LOAD_INDICATORS) || !isInt(closed, 0, 99) || !isInt(stackDelay, 0, 2000)) return null;
      if (!isOneOf(stackRow, STACK_ROWS)) return null; // --- two-level stacks (T10b) ---
      const tools = parseTools(raw.tools); // --- toolbars and feeds (thread T13) ---
      if (tools === null) return null;
      return { t: 'tabopts', closeLeft, closeAlways, scroll, minWidth, indicator, wheel, separator, alignClose, selection, selectionActive, removeSpacing, maximised, closed, stackDrag, stackDelay, stackRow, closedButton, tools } as TabOptsMsg;
    }
    // --- end tab options ---
    case 'fav': {
      if (!hasOnlyKeys(raw, ['v', 't', 'id', 'uri']) || raw.v !== 1) return null;
      const { id, uri } = raw;
      if (!isInt(id, 0, MAX_ID) || !isFavUri(uri)) return null;
      return { t: 'fav', id, uri };
    }
    case 'rail': {
      if (!hasOnlyKeys(raw, ['v', 't', 'items']) || raw.v !== 1) return null;
      const { items } = raw;
      if (!Array.isArray(items) || items.length > MAX_RAIL_ITEMS) return null;
      const seen = new Set<string>();
      const out: RailItem[] = [];
      for (const v of items) {
        const item = parseRailItem(v, seen);
        if (!item) return null;
        seen.add(item.k);
        out.push(item);
      }
      return { t: 'rail', items: out };
    }
    case 'railfav': {
      if (!hasOnlyKeys(raw, ['v', 't', 'k', 'uri']) || raw.v !== 1) return null;
      const { k, uri } = raw;
      if (typeof k !== 'string' || !isRailKey(k) || !isFavUri(uri)) return null;
      return { t: 'railfav', k, uri };
    }
    default:
      return null;
  }
}

function truncateCodePoint(text: string, max: number): string {
  if (text.length <= max) return text;
  let end = max;
  const last = text.charCodeAt(end - 1);
  if (last >= 0xd800 && last <= 0xdbff) end -= 1;
  return text.slice(0, end);
}
