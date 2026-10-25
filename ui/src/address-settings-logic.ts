// Pure logic of the settings page's "Address bar", "Search engine" and "Bookmarks" sections
// (src/chrome_host/address_page.rs): the snapshot contract, the rows the nav search indexes, the
// command texts, and the rules for the source list and the engine editor. No DOM here;
// test/address.mjs covers all of it.
import type { RowSpec } from './settings-page-logic';
import type { SnapshotMsg, SnapshotValue } from './protocol';

export const SOURCE_IDS = ['direct', 'bookmarks', 'nicknames', 'search', 'search-history', 'history', 'calculator'] as const;
export type SourceId = typeof SOURCE_IDS[number];
export const SOURCE_LABELS: Readonly<Record<SourceId, { readonly label: string; readonly hint: string }>> = {
  direct: { label: 'Direct match', hint: 'What you typed as an address or a search, and the page it completes to' },
  bookmarks: { label: 'Bookmarks', hint: 'Bookmarks whose title or address match' },
  nicknames: { label: 'Bookmark nicknames', hint: 'The bookmark whose nickname you typed' },
  search: { label: 'Search suggestions', hint: 'From the engine, only after its nickname' },
  'search-history': { label: 'Search history', hint: 'Searches you typed before' },
  history: { label: 'History and open tabs', hint: 'Pages you visited and tabs you have open' },
  calculator: { label: 'Calculator', hint: 'The answer to a sum, worked out on this device' },
};

export const SEARCH_FIELD_MODES = ['off', 'field', 'button'] as const;
export type SearchFieldMode = typeof SEARCH_FIELD_MODES[number];
export const BAR_POSITIONS = ['top', 'bottom'] as const;
export type BarPosition = typeof BAR_POSITIONS[number];
export const BAR_DISPLAYS = ['text-icon', 'text', 'icon', 'bookmark-icon'] as const;
export type BarDisplay = typeof BAR_DISPLAYS[number];
export const BAR_DISPLAY_LABELS: Readonly<Record<BarDisplay, string>> = {
  'text-icon': 'Text and icon', text: 'Text', icon: 'Icon', 'bookmark-icon': 'Bookmark icon only',
};
export const MIN_RESULTS = 1;
export const MAX_RESULTS = 20;
export const MIN_CONFIRM = 2;
export const MAX_CONFIRM = 100;
const NICK_RE = /^[a-z0-9_-]{0,16}$/;
const SUFFIX_RE = /^(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)*)?$/;
export const MAX_ENGINES = 40;

/** Every option the page shows, as the typed object Rust sends under `s`. */
export interface AddressValues {
  address_bar_show: boolean; address_autocomplete: boolean; address_show_full: boolean; address_select_on_activate: boolean;
  address_copy_encoded: boolean; address_qr_button: boolean;
  dd_show_while_typing: boolean; dd_always_badge: boolean; dd_typed_history: boolean; dd_typed_include_searches: boolean; dd_history_button: boolean;
  dd_max_results: number; dd_prioritise_bookmarks: boolean; dd_prioritise_direct: boolean; dd_sources: string;
  sec_always_https: boolean; sec_strip_js: boolean; sec_highlight_base: boolean; domain_expansion_suffix: string;
  search_field: SearchFieldMode; search_show_nickname: boolean; search_new_tab: boolean; search_show_icon: boolean; search_in_address: boolean;
  search_keep_engine: boolean; search_keep_text: boolean; search_selection_background: boolean; search_suggest_address: boolean;
  search_suggest_field: boolean; search_last_engine: string;
  nick_bookmarks: string; nick_history: string; nick_tabs: string;
  bm_open_new_tab: boolean; bm_panel_single_click: boolean; bm_confirm_open: number; bm_fetch_metadata: boolean; bm_always_dialog: boolean;
  bm_bar_folder: number; bm_bar_position: BarPosition; bm_bar_display: BarDisplay; bar_groups: string;
}
export type AddressKey = keyof AddressValues;

const BOOL_KEYS = [
  'address_bar_show', 'address_autocomplete', 'address_show_full', 'address_select_on_activate', 'address_copy_encoded', 'address_qr_button',
  'dd_show_while_typing', 'dd_always_badge', 'dd_typed_history', 'dd_typed_include_searches', 'dd_history_button', 'dd_prioritise_bookmarks', 'dd_prioritise_direct',
  'sec_always_https', 'sec_strip_js', 'sec_highlight_base', 'search_show_nickname', 'search_new_tab', 'search_show_icon', 'search_in_address',
  'search_keep_engine', 'search_keep_text', 'search_selection_background', 'search_suggest_address', 'search_suggest_field',
  'bm_open_new_tab', 'bm_panel_single_click', 'bm_fetch_metadata', 'bm_always_dialog',
] as const;
const TEXT_KEYS = ['dd_sources', 'domain_expansion_suffix', 'search_last_engine', 'nick_bookmarks', 'nick_history', 'nick_tabs', 'bar_groups'] as const;
const INT_KEYS = ['dd_max_results', 'bm_confirm_open', 'bm_bar_folder'] as const;
const ENUM_KEYS = ['search_field', 'bm_bar_position', 'bm_bar_display'] as const;
export const ADDRESS_KEYS: readonly string[] = [...BOOL_KEYS, ...TEXT_KEYS, ...INT_KEYS, ...ENUM_KEYS];

export interface EngineView {
  readonly id: string;
  readonly name: string;
  readonly nick: string;
  readonly url: string;
  readonly post: string;
  readonly suggest: string;
  readonly image: string;
  readonly imagePost: string;
  readonly builtin: boolean;
}
export interface FolderView { readonly id: number; readonly title: string; readonly depth: number }

export interface AddressSnapshot {
  readonly s: AddressValues;
  readonly engines: readonly EngineView[];
  readonly def: string;
  readonly priv: string;
  readonly img: string;
  readonly folders: readonly FolderView[];
  readonly guest: boolean;
  readonly ack: 'none' | 'saved' | 'failed';
  readonly error: string;
}

const isRecord = (v: unknown): v is Record<string, SnapshotValue> => typeof v === 'object' && v !== null && !Array.isArray(v);
const str = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max && !/[\u0000-\u001f\u007f]/.test(v);
const ID_RE = /^[a-z0-9_-]{0,32}$/;

function parseValues(v: unknown): AddressValues | null {
  if (!isRecord(v) || Object.keys(v).length !== ADDRESS_KEYS.length) return null;
  for (const key of BOOL_KEYS) if (typeof v[key] !== 'boolean') return null;
  for (const key of INT_KEYS) if (typeof v[key] !== 'number' || !Number.isInteger(v[key]) || (v[key] as number) < 0) return null;
  for (const key of TEXT_KEYS) if (!str(v[key], 4096)) return null;
  const mode = v.search_field, pos = v.bm_bar_position, display = v.bm_bar_display;
  if (!(SEARCH_FIELD_MODES as readonly unknown[]).includes(mode) || !(BAR_POSITIONS as readonly unknown[]).includes(pos) || !(BAR_DISPLAYS as readonly unknown[]).includes(display)) return null;
  const results = v.dd_max_results as number;
  if (results < MIN_RESULTS || results > MAX_RESULTS) return null;
  return v as unknown as AddressValues;
}

function parseEngine(v: unknown): EngineView | null {
  if (!isRecord(v)) return null;
  const { id, name, nick, url, post, suggest, image, imagePost, builtin } = v;
  if (!str(id, 32) || id === '' || !ID_RE.test(id) || !str(name, 120) || !str(nick, 32) || !str(url, 1100) || !str(post, 600) || !str(suggest, 1100) || !str(image, 1100) || !str(imagePost, 600)) return null;
  if (typeof builtin !== 'boolean') return null;
  return { id, name, nick, url, post, suggest, image, imagePost, builtin };
}

/** Strict validation of the `address` snapshot; `null` for anything partial, extended or malformed. */
export function parseAddressSnapshot(d: SnapshotMsg['d']): AddressSnapshot | null {
  const keys = ['s', 'engines', 'def', 'priv', 'img', 'folders', 'guest', 'ack', 'error'];
  if (Object.keys(d).length !== keys.length || Object.keys(d).some((key) => !keys.includes(key))) return null;
  const s = parseValues(d.s);
  if (!s || !Array.isArray(d.engines) || d.engines.length === 0 || d.engines.length > MAX_ENGINES) return null;
  const engines: EngineView[] = [];
  for (const raw of d.engines) {
    const engine = parseEngine(raw);
    if (!engine || engines.some((e) => e.id === engine.id)) return null;
    engines.push(engine);
  }
  if (!str(d.def, 32) || !str(d.priv, 32) || !str(d.img, 32) || !engines.some((e) => e.id === d.def)) return null;
  if (d.priv !== '' && !engines.some((e) => e.id === d.priv)) return null;
  if (!Array.isArray(d.folders) || d.folders.length > 200) return null;
  const folders: FolderView[] = [];
  for (const raw of d.folders) {
    if (!isRecord(raw) || typeof raw.id !== 'number' || !Number.isInteger(raw.id) || raw.id < 1 || !str(raw.title, 200) || typeof raw.depth !== 'number' || !Number.isInteger(raw.depth) || raw.depth < 0 || raw.depth > 16) return null;
    folders.push({ id: raw.id, title: raw.title, depth: raw.depth });
  }
  if (typeof d.guest !== 'boolean' || (d.ack !== 'none' && d.ack !== 'saved' && d.ack !== 'failed') || !str(d.error, 400)) return null;
  return { s, engines, def: d.def as string, priv: d.priv as string, img: d.img as string, folders, guest: d.guest, ack: d.ack, error: d.error as string };
}

// ---- commands ----

/** The `set` fields that change one option. `null` for a value Rust would refuse. */
export function setFields(key: AddressKey, value: boolean | number | string): { a: 'addrset'; text: string } | null {
  if (!ADDRESS_KEYS.includes(key)) return null;
  const text = String(value);
  if (/[\n\r\t]/.test(text) || text.length > 2048) return null;
  if (key === 'dd_max_results' && !(Number.isInteger(value) && (value as number) >= MIN_RESULTS && (value as number) <= MAX_RESULTS)) return null;
  if (key === 'bm_confirm_open' && !(value === 0 || (Number.isInteger(value) && (value as number) >= MIN_CONFIRM && (value as number) <= MAX_CONFIRM))) return null;
  if (key === 'domain_expansion_suffix' && !isSuffix(text)) return null;
  if ((key === 'nick_bookmarks' || key === 'nick_history' || key === 'nick_tabs') && !isNickWord(text)) return null;
  return { a: 'addrset', text: `${key}=${text}` };
}

export function isSuffix(text: string): boolean {
  return text.length <= 24 && SUFFIX_RE.test(text.replace(/^\./, ''));
}
export function isNickWord(text: string): boolean {
  return NICK_RE.test(text.replace(/^@/, '').toLowerCase());
}

// ---- the source list ----

/** The enabled sources, in row order, from the `dd_sources` text. */
export function sourceList(text: string): SourceId[] {
  return text.split(',').filter((id): id is SourceId => (SOURCE_IDS as readonly string[]).includes(id));
}

/** Every source in the order the editor lists them: the enabled ones in row order, then the rest. */
export function sourceRows(enabled: readonly SourceId[]): Array<{ readonly id: SourceId; readonly on: boolean }> {
  return [...enabled.map((id) => ({ id, on: true })), ...SOURCE_IDS.filter((id) => !enabled.includes(id)).map((id) => ({ id, on: false }))];
}

/** `dd_sources` text after switching `id` on (at the end of the enabled ones) or off. */
export function toggleSource(enabled: readonly SourceId[], id: SourceId, on: boolean): string {
  const rest = enabled.filter((item) => item !== id);
  return (on ? [...rest, id] : rest).join(',');
}

/** `dd_sources` text after moving enabled source `id` one place up (-1) or down (+1). */
export function moveSource(enabled: readonly SourceId[], id: SourceId, delta: -1 | 1): string {
  const list = [...enabled];
  const from = list.indexOf(id);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= list.length) return list.join(',');
  [list[from], list[to]] = [list[to]!, list[from]!];
  return list.join(',');
}

// ---- the engine editor ----

export interface EngineDraft {
  name: string;
  nick: string;
  url: string;
  post: string;
  suggest: string;
  image: string;
  imagePost: string;
}
export const EMPTY_ENGINE: EngineDraft = { name: '', nick: '', url: '', post: '', suggest: '', image: '', imagePost: '' };

export function draftOfEngine(engine: EngineView): EngineDraft {
  return { name: engine.name, nick: engine.nick, url: engine.url, post: engine.post, suggest: engine.suggest, image: engine.image, imagePost: engine.imagePost };
}

const hasControl = (text: string): boolean => /[\u0000-\u001f\u007f]/.test(text);
const placeholders = (text: string): number => text.split('%s').length - 1;
function httpsTemplate(text: string): boolean {
  if (text.length > 1024 || hasControl(text) || /\s/.test(text)) return false;
  try {
    const url = new URL(text.replaceAll('%s', 'bowserquerymarker'));
    return url.protocol === 'https:' && url.hostname !== '' && url.username === '' && url.password === '';
  } catch {
    return false;
  }
}

/** The first thing wrong with an engine draft, in the page's words, or null when Bowser will take it. */
export function engineProblem(draft: EngineDraft, others: readonly EngineView[], selfId: string | null): string | null {
  const name = draft.name.trim();
  if (name === '' || name.length > 40 || hasControl(name)) return 'Give the engine a name of up to 40 characters.';
  const nick = draft.nick.trim().toLowerCase();
  if (nick.length > 16 || !/^[a-z0-9._-]*$/.test(nick)) return 'A nickname is up to 16 letters, digits, dots, dashes or underscores.';
  const clash = others.find((e) => e.id !== selfId && nick !== '' && e.nick === nick);
  if (clash) return `The nickname is already used by ${clash.name}.`;
  const url = draft.url.trim(), post = draft.post.trim(), suggest = draft.suggest.trim(), image = draft.image.trim(), imagePost = draft.imagePost.trim();
  if (!httpsTemplate(url)) return 'The search address must be an https:// address.';
  if (post === '') {
    if (placeholders(url) !== 1) return 'The search address needs %s once, where the search text goes.';
  } else if (post.length > 512 || hasControl(post) || placeholders(post) !== 1) {
    return 'POST parameters need %s once, where the search text goes.';
  }
  if (suggest !== '' && (!httpsTemplate(suggest) || placeholders(suggest) !== 1)) return 'The suggestions address must be https:// and hold %s once.';
  if (image !== '' && !httpsTemplate(image)) return 'The image search address must be an https:// address.';
  if (image === '') {
    if (imagePost !== '') return 'Image POST parameters need an image search address.';
  } else if (imagePost === '') {
    if (placeholders(image) !== 1) return 'The image search address needs %s once, where the image address goes.';
  } else if (imagePost.length > 512 || hasControl(imagePost) || placeholders(imagePost) !== 1) {
    return 'Image POST parameters need %s once, where the image address goes.';
  }
  return null;
}

const draftFields = (d: EngineDraft): string[] => [d.name.trim(), d.nick.trim().toLowerCase(), d.url.trim(), d.post.trim(), d.suggest.trim(), d.image.trim(), d.imagePost.trim()];

export type EngineOp =
  | { readonly op: 'add'; readonly draft: EngineDraft }
  | { readonly op: 'update'; readonly id: string; readonly draft: EngineDraft }
  | { readonly op: 'remove' | 'default' | 'image'; readonly id: string }
  | { readonly op: 'private'; readonly id: string }
  | { readonly op: 'move'; readonly id: string; readonly index: number }
  | { readonly op: 'restore' };

/** The `set` fields of an engine change: the operation, then tab-separated fields. */
export function engineFields(change: EngineOp): { a: 'engine'; text: string } | null {
  let parts: string[];
  switch (change.op) {
    case 'add': parts = ['add', ...draftFields(change.draft)]; break;
    case 'update': parts = ['update', change.id, ...draftFields(change.draft)]; break;
    case 'remove': case 'default': case 'image': parts = [change.op, change.id]; break;
    case 'private': parts = ['private', change.id]; break;
    case 'move': parts = ['move', change.id, String(change.index)]; break;
    case 'restore': parts = ['restore']; break;
  }
  if (change.op !== 'add' && change.op !== 'update' && change.op !== 'restore' && change.op !== 'private' && !ID_RE.test(change.id)) return null;
  if (change.op === 'private' && !ID_RE.test(change.id)) return null;
  return { a: 'engine', text: parts.join('\t') };
}

/** Engine ids ordered as the list shows them after moving `id` to `index`. */
export function moved(engines: readonly EngineView[], id: string, index: number): string[] {
  const ids = engines.map((e) => e.id).filter((x) => x !== id);
  ids.splice(Math.max(0, Math.min(ids.length, index)), 0, id);
  return ids;
}

/**
 * The name in the search-engine menu. The stored default id is `optimised`: DuckDuckGo’s HTML
 * search with sponsored results off (`k1=-1`). The id stays, so settings.txt is unchanged.
 * The separate `duckduckgo` engine is the main site, so the menu can tell the two apart.
 */
export function engineChoiceLabel(id: string, name: string): string {
  if (id === 'optimised') return 'DuckDuckGo';
  if (id === 'duckduckgo') return 'DuckDuckGo.com';
  return name;
}

/** The label an engine shows in lists: its name, and its nickname when asked. */
export function engineLabel(engine: Pick<EngineView, 'name' | 'nick'>, showNickname: boolean): string {
  return showNickname && engine.nick !== '' ? `${engine.name} (${engine.nick})` : engine.name;
}

/** The monogram a search-field button shows for an engine (its first letter, upper case). */
export function monogram(name: string): string {
  const first = Array.from(name.trim())[0] ?? '?';
  return first.toUpperCase();
}

// ---- rows (the nav search index) ----

const S = (id: string, section: RowSpec['section'], label: string, description: string, keywords: string): RowSpec => ({ id, section, label, description, keywords });

export const ADDRESS_ROWS: readonly RowSpec[] = [
  S('addrShow', 'address', 'Show the address field', 'Off, the address field appears only while it has focus (Ctrl+L). The search field and page buttons stay.', 'address bar hide show url field toolbar omnibox'),
  S('addrAutocomplete', 'address', 'Suggest sites as you type', 'Completes an address you have visited before.', 'autocomplete inline completion address typing history complete'),
  S('addrFull', 'address', 'Show the full address', 'Keeps https:// and www. in the address instead of hiding them.', 'full url scheme www https protocol show address'),
  S('addrSelect', 'address', 'Select the address when the field is activated', 'Clicking or tabbing into the field selects all of its text.', 'select all on click focus activation address'),
  S('addrEncoded', 'address', 'Copy and cut the encoded address', 'Copying the whole address puts its percent-encoded form on the clipboard.', 'copy cut encoded url percent encoding clipboard'),
  S('addrQr', 'address', 'Show a QR code button', 'A button in the address field shows a QR code for the page, made on this device.', 'qr code generator share phone scan address bar'),
  S('ddTyping', 'address', 'Show the drop-down while typing', 'Off, the list opens only when you press the Down arrow.', 'dropdown suggestions list typing omnibox'),
  S('ddBadge', 'address', 'Always show a badge on drop-down items', 'Each suggestion names its kind: bookmark, tab, history, search.', 'badge icon suggestions kind dropdown'),
  S('ddTypedHistory', 'address', 'Show typed history when the field is focused', 'Lists what you typed before while the field is empty.', 'typed history dropdown focus recent addresses'),
  S('ddTypedSearches', 'address', 'Include searches in typed history', 'Searches you typed appear beside addresses.', 'typed history search queries include'),
  S('ddHistoryButton', 'address', 'Show a typed history button', 'A button at the end of the address field opens the typed history.', 'history button arrow dropdown typed'),
  S('ddMax', 'address', 'Maximum results', 'The most rows the drop-down shows, the first one included.', 'maximum results rows number suggestions limit'),
  S('ddPriorityBookmarks', 'address', 'Prioritise bookmarks', 'Bookmark rows come before the other sources.', 'prioritise bookmarks first suggestions order'),
  S('ddPriorityDirect', 'address', 'Prioritise the direct match', 'A page whose address is exactly what you typed comes right after the first row.', 'prioritise direct match exact address suggestions'),
  S('ddSources', 'address', 'Suggestion sources and order', 'Turn sources on or off and move them up or down.', 'sources order priority bookmarks nicknames search suggestions history calculator'),
  S('secHttps', 'address', 'Always use HTTPS', 'Addresses you type open over HTTPS when they can.', 'https secure upgrade http always security local private'),
  S('secJs', 'address', 'Remove JavaScript from pasted addresses', 'A pasted javascript: prefix is removed, so a pasted script is text, not code.', 'javascript paste strip security xss self'),
  S('secBase', 'address', 'Highlight the base domain', 'Emphasises only the main domain of the address, so a lookalike host stands out.', 'highlight base domain security phishing emphasis'),
  S('addrExpansion', 'address', 'Domain expansion', 'Ctrl+Enter turns one word into www.word.suffix. Empty turns it off.', 'domain expansion ctrl enter suffix com www'),
  S('engines', 'search', 'Search engines', 'Add, edit, order and remove the engines the address bar and the search field use.', 'search engines add edit remove reorder nickname url suggest post image restore defaults'),
  S('searchDefault', 'search', 'Search engine', 'DuckDuckGo, with no sponsored results. Type img before a query for images.', 'default engine optimised google duckduckgo bing search address bar'),
  S('searchPrivate', 'search', 'Private window search engine', 'Used in private tabs. Default means the engine above.', 'private window search engine incognito'),
  S('searchImage', 'search', 'Image search engine', 'Used by “Search for this image” on a picture.', 'image search engine reverse picture'),
  S('searchField', 'search', 'Search field', 'A separate field beside the address field, as a text field or a button.', 'search field toolbar button text box'),
  S('searchNickname', 'search', 'Show nicknames in the engine list', 'Each engine in the list and the drop-down shows its nickname.', 'nickname engine list dropdown show'),
  S('searchIcon', 'search', 'Show the engine icon', 'The search field shows the engine chosen.', 'engine icon search field'),
  S('searchKeepEngine', 'search', 'Keep the last selected engine', 'The search field starts with the engine you used last.', 'keep last engine search field remember'),
  S('searchKeepText', 'search', 'Keep typed text after searching', 'The search field is not cleared after a search.', 'keep text search field clear after'),
  S('searchSuggestField', 'search', 'Show engine suggestions in the search field', 'The engine is asked for suggestions as you type in the search field.', 'search suggestions search field engine network'),
  S('searchNewTab', 'search', 'Always search in a new tab', 'Searches open in a new tab instead of the current one.', 'search new tab always'),
  S('searchInAddress', 'search', 'Search from the address field', 'Off, text that is not an address is not searched there. Engine nicknames still work.', 'search address field omnibox enable'),
  S('searchSelection', 'search', 'Search for selected text in a background tab', 'The page’s “Search for …” menu item opens its results without leaving the page.', 'selection search background tab context menu'),
  S('searchSuggestAddress', 'search', 'Show engine suggestions in the address field', 'After an engine’s nickname, the engine’s own suggestions appear. Nothing is sent before.', 'search suggestions address nickname engine network'),
  S('nickBookmarks', 'search', 'Bookmarks scope nickname', 'Type it in the address field, then a word, to search only bookmarks.', 'nickname scope bookmarks @bookmarks address field'),
  S('nickHistory', 'search', 'History scope nickname', 'Type it in the address field, then a word, to search only history.', 'nickname scope history @history address field'),
  S('nickTabs', 'search', 'Tabs scope nickname', 'Type it in the address field, then a word, to find an open tab.', 'nickname scope tabs @tabs address field'),
  S('bmNewTab', 'bookmarks', 'Open bookmarks in a new tab', 'Choosing a bookmark opens it beside the current tab.', 'bookmark new tab open'),
  S('bmSingleClick', 'bookmarks', 'Open panel items with a single click', 'Off, bookmarks in the panel open with a double click.', 'bookmark panel single click double open'),
  S('bmConfirm', 'bookmarks', 'Ask before opening many bookmarks', 'Opening a folder of this many bookmarks or more asks first. 0 never asks.', 'bookmark confirm open many folder tabs warning'),
  S('bmMetadata', 'bookmarks', 'Fetch bookmark details from the page', 'A new bookmark takes its title and description from the page’s own metadata.', 'bookmark metadata title description fetch page'),
  S('bmDialog', 'bookmarks', 'Always open the bookmark dialog', 'Ctrl+D opens the dialog to choose a name, folder and nickname instead of saving at once.', 'bookmark dialog ctrl d edit name folder'),
  S('bmBarFolder', 'appearance', 'Bookmarks bar folder', 'The folder whose bookmarks the bar shows.', 'bookmarks bar folder show which'),
  S('bmBarPosition', 'appearance', 'Bookmarks bar position', 'At the top, under the toolbar, or at the bottom of the window.', 'bookmarks bar top bottom position'),
  S('bmBarDisplay', 'appearance', 'Bookmarks bar display', 'What each bookmark on the bar shows.', 'bookmarks bar display text icon favicon'),
];
