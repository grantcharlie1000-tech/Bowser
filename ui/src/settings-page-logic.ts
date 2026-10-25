// Pure logic for the settings page (?view=settings, docs/design/settings-page.md): the snapshot
// contract, the exact-match save payload, the section and row index the nav and the search use,
// and the words that depend on state. No DOM here; test/settings-page.mjs covers all of it.
import { MAX_SETTINGS_SAVE, SETTINGS_SECTIONS, utf8Bytes, type SettingsSection } from './protocol';
// --- start page ---
import { parseStartSettings, startSaveLines, type StartSettings } from './start-page-logic';
// --- end start page ---
// --- address bar, search and bookmarks (src/chrome_host/address_page.rs) ---
import { ADDRESS_ROWS } from './address-settings-logic';
import { PAGE_TOOLS_CORE, PAGE_TOOLS_ROWS } from './page-tools-logic'; // --- page tools (T7) ---
// --- end address bar, search and bookmarks ---
import { cardSpecs, parseTabOptions, tabOptionRows, type TabOptionValues } from './tab-options-logic';
// --- theme options ---
import { THEME_ROWS, parseThemeOptions, type ThemeOptionValues } from './theme-options-logic';
// --- end theme options ---
// --- toolbars and feeds (thread T13) --- the buttons of the three bars (bars-logic.ts) and the feed options
import { ADDRESS_ITEMS, DEFAULT_ADDRESS, itemsOf, validItems } from './bars-logic';
import { FEED_ROWS, REFRESH_CHOICES } from './feeds-settings-logic';
// --- end toolbars and feeds ---

export { SETTINGS_SECTIONS, type SettingsSection };

const SEARCH_ENGINES = ['optimised', 'google', 'duckduckgo'] as const;
export type SearchEngine = typeof SEARCH_ENGINES[number];
export const EFFICIENCY_MODES = ['auto', 'on', 'off'] as const;
export type EfficiencyMode = typeof EFFICIENCY_MODES[number];
export type TrackingPrevention = 'balanced' | 'strict';
// --- layout (thread T6) --- the layouts (six now), the Layout block's `layout` object and its presets live in layout-logic.ts.
import {
  LAYOUT_HINTS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_ARIA, TAB_LAYOUT_LABELS, layoutSaveLines, parseLayoutSettings,
  type LayoutSettings, type SidePanelPosition, type TabLayout,
} from './layout-logic';
export { LAYOUT_HINTS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_ARIA, TAB_LAYOUT_LABELS, type LayoutSettings, type SidePanelPosition, type TabLayout };
// --- end layout ---
// --- window and panels (thread T9) ---
import { parseWindowSettings, windowSaveLines, type WindowSettings } from './window-logic';
export type { WindowSettings };
// --- end window and panels ---
export const MIN_SIDEBAR = 180;
export const MAX_SIDEBAR = 360;
export const MIN_SLEEP = 30;
export const MAX_SLEEP = 604800;
export const MAX_ALLOW = 500;
/** The allow list's joined length in UTF-8 bytes, commas included (settings::MAX_BLOCKER_ALLOW_BYTES): 500 sites of up to 40 characters. */
export const MAX_ALLOW_BYTES = 20 * 1024;
/** Whether the joined allow list fits `MAX_ALLOW_BYTES`. */
export function allowFits(list: readonly string[]): boolean {
  return utf8Bytes(list.join(',')) <= MAX_ALLOW_BYTES;
}

export const THEME_MODES = ['helium', 'system'] as const;
export type ThemeMode = typeof THEME_MODES[number];
/** The address bar's own buttons, in the order the editor offers them (bars-logic.ts); command buttons come on top. */
export const TOOLBAR_BUTTONS = ADDRESS_ITEMS;
export type ToolbarButton = typeof TOOLBAR_BUTTONS[number];
/** What the address bar shows until it is changed: today's look (bars-logic.ts, thread T13). */
export const DEFAULT_TOOLBAR_BUTTONS = DEFAULT_ADDRESS.join(',');
// --- auto-hide (T11) --- the bars "Auto-hide toolbars and tabs" can take out of the layout, in page order
// (src/auto_hide.rs BAR_TABLE). The status bar joins when the layout work adds one.
export const AUTO_HIDE_BARS = ['tabs', 'address', 'bookmarks', 'panel'] as const;
export type AutoHideBar = typeof AUTO_HIDE_BARS[number];
export const AUTO_HIDE_BAR_LABELS: Readonly<Record<AutoHideBar, string>> = {
  tabs: 'Tab bar',
  address: 'Address bar',
  bookmarks: 'Bookmarks bar',
  panel: 'Panel',
};
/** What hides when nobody has chosen: the three bars above and beside the page. */
export const DEFAULT_AUTO_HIDE_BARS = 'tabs,address,bookmarks';
/** The bars of a saved list, `''` meaning none. */
export function autoHideBarsList(text: string): AutoHideBar[] {
  return text === '' ? [] : (text.split(',') as AutoHideBar[]);
}
/** The saved spelling of a set of bars: catalog order, each once. */
export function autoHideBarsText(bars: Iterable<AutoHideBar>): string {
  const set = new Set(bars);
  return AUTO_HIDE_BARS.filter((bar) => set.has(bar)).join(',');
}
/** Exactly the spelling Rust saves: every bar known, none twice, catalog order. */
function isAutoHideBarsText(value: unknown): value is string {
  return typeof value === 'string' && autoHideBarsText(autoHideBarsList(value)) === value && autoHideBarsList(value).every((bar) => (AUTO_HIDE_BARS as readonly string[]).includes(bar));
}
// --- end auto-hide ---
export const BOOKMARKS_BAR_MODES = ['always', 'never', 'newtab'] as const;
export type BookmarksBarMode = typeof BOOKMARKS_BAR_MODES[number];
export const FONT_SIZES = ['small', 'medium', 'large'] as const;
export type FontSize = typeof FONT_SIZES[number];
export const DEFAULT_BROWSER_STATES = ['yes', 'no', 'unknown'] as const;
export type DefaultBrowserState = typeof DEFAULT_BROWSER_STATES[number];
export const MIN_ZOOM = 25;
export const MAX_ZOOM = 500;
export const MAX_UI_FONT = 64;
const ACCENT_RE = /^#[0-9a-f]{6}$/;
const CONTROL_CHAR_RE = /[\u0000-\u001f\u007f]/;

/** The ids of `text` (a `toolbar_buttons`-style comma list), `''` meaning none. */
export function toolbarButtonsList(text: string): string[] {
  return text === '' ? [] : text.split(',');
}

/** No duplicates, every id one of the address bar's buttons or a command button, within the bar's limits; any order. */
function isToolbarButtonsText(value: unknown): value is string {
  if (typeof value !== 'string') return false;
  if (value === '') return true;
  const ids = value.split(',');
  return ids.every((id) => id !== '') && validItems('address', ids);
}

function isThemeAccent(value: unknown): value is string {
  return typeof value === 'string' && (value === '' || ACCENT_RE.test(value));
}

function isUiFont(value: unknown): value is string {
  return typeof value === 'string' && value.length <= MAX_UI_FONT && !CONTROL_CHAR_RE.test(value);
}

const UPDATE_STATES = ['unavailable', 'checking', 'ready', 'idle'] as const;
const LAST_CHECKS = ['none', 'uptodate', 'failed', 'other'] as const;
const CHECK_RESULTS = ['none', 'staged', 'uptodate', 'unverified', 'busy', 'unavailable', 'failed'] as const;
const ACKS = ['none', 'saved', 'failed'] as const;
export type UpdateState = typeof UPDATE_STATES[number];
export type LastCheck = typeof LAST_CHECKS[number];
export type CheckResult = typeof CHECK_RESULTS[number];
export type Ack = typeof ACKS[number];

export interface SettingsSnapshot {
  tabLayout: TabLayout;
  sidebarWidth: number;
  /** Vertical tabs stay open and push the page. Off, they are a hover rail over the page. */
  sidebarPinned: boolean;
  sleepEnabled: boolean;
  sleepAfterSecs: number;
  blockerEnabled: boolean;
  blockerAllow: string[];
  searchEngine: SearchEngine | null;
  searchFallback: 'default' | 'custom';
  automaticTabGrouping: boolean;
  efficiencyMode: EfficiencyMode;
  trackingPrevention: TrackingPrevention;
  theme: ThemeMode;
  /** `''` (no override) or `#` plus six lower-case hex digits. */
  themeAccent: string;
  /** Comma list of the address bar's buttons (`bars-logic.ts`), each at most once; `''` for none. */
  toolbarButtons: string;
  centredAddressBar: boolean;
  minimalUrl: boolean;
  showLoadProgress: boolean;
  /** The master "Auto-hide toolbars and tabs" switch (the key keeps its old name). */
  frameless: boolean;
  /** Comma list of `AUTO_HIDE_BARS`, each at most once in that order; `''` for none. */
  autoHideBars: string;
  /** "Always auto-hide in full screen". */
  autoHideFullscreen: boolean;
  roundedContentFrame: boolean;
  showHomeButton: boolean;
  bookmarksBar: BookmarksBarMode;
  bookmarksBarTabGroups: boolean;
  sidePanelPosition: SidePanelPosition;
  tabPreviewImages: boolean;
  tabPreviewMemory: boolean;
  fontSize: FontSize;
  /** `''` for the system font. */
  uiFont: string;
  defaultZoom: number;
  showZoomIndicator: boolean;
  newTabBesideActive: boolean;
  ctrlTabMru: boolean;
  shiftRightClickContext: boolean;
  ctrlShiftCCopyLink: boolean;
  ctrlSVerticalTabs: boolean;
  pinNewTabGroups: boolean;
  splitEdgeDrop: boolean;
  blockYoutubeVideoAds: boolean;
  /** Read-only from here on: state the page shows but never sends back in `save`. */
  efficiencyActive: boolean;
  /** Install-wide and applied at once, so never part of `save`. */
  autoUpdate: boolean;
  historyEnabled: boolean;
  guest: boolean;
  profile: string;
  avatar: string;
  version: string;
  update: UpdateState;
  updateVersion: string;
  lastCheck: LastCheck;
  checkResult: CheckResult;
  ack: Ack;
  error: string;
  readable: boolean;
  /** Whether Bowser is currently the HKCU default http handler. Read-only, never part of `save`. */
  defaultBrowser: DefaultBrowserState;
  /** Where downloads are saved (a display path). Changed only through the native folder picker. */
  downloadDir: string;
  /** "Ask where to save each file before downloading"; its own command, never part of `save`. */
  downloadAsk: boolean;
  /** "Offer to translate pages in other languages"; its own command, never part of `save`. */
  translateOffer: boolean;
  /** "Translate automatically on sites set to always translate"; its own command. */
  translateAuto: boolean;
  /** The Tabs section's options (`tab-options-logic.ts`); each change is its own `tabopt` command. */
  tabOptions: TabOptionValues;
  /** The Appearance area's theme options (`theme-options-logic.ts`); each change is its own `themeopt` command. */
  themeOptions: ThemeOptionValues;
  /** `''` means the Windows display language, else a 2-3 letter lower-case code. Its own command. */
  translateTarget: string;
  /** The codes `translateTarget` may be set to, in the order the page lists them. */
  translateTargets: string[];
  // --- start page ---
  /** The start page, speed dial, widget, homepage and startup settings (`start_*` keys of settings.txt). */
  start: StartSettings;
  // --- end start page ---
  // --- tab stacks, tiling and workspaces (T10) ---
  /** Sites that open in a named workspace, and the workspaces a rule can name. Each change is its own `wsrule` command. */
  workspaceRules: WorkspaceRules;
  // --- end tab stacks ---
  // --- layout (thread T6) ---
  /** The Layout block's own options (`layout_prefs.rs`); the tab layout and panel side are the older keys above. */
  layout: LayoutSettings;
  // --- end layout ---
  // --- window and panels (thread T9) ---
  /** Window appearance and panel options (`window_prefs.rs`). */
  window: WindowSettings;
  // --- end window and panels ---
  // --- toolbars and feeds (thread T13) --- the tab bar's and panel bar's buttons and the feed options (each its own `baropt` command).
  bars: BarsSettings;
  // --- end toolbars and feeds ---
}

/** The tab bar and panel bar lists and the feed options. The address bar's list is `toolbarButtons`. */
export interface BarsSettings {
  /** Comma list of the tab bar's buttons. */
  tabs: string;
  /** Comma list of the panel bar's extra buttons. */
  panel: string;
  feedsDetect: boolean;
  /** Minutes: one of `REFRESH_CHOICES`. */
  feedsRefresh: number;
}

function parseBarsSettings(value: unknown): BarsSettings | null {
  if (!isRecord(value) || Object.keys(value).length !== 4) return null;
  const { tabs, panel, feedsDetect, feedsRefresh } = value;
  if (typeof tabs !== 'string' || typeof panel !== 'string' || typeof feedsDetect !== 'boolean') return null;
  if (!(tabs === '' || validItems('tabs', itemsOf(tabs))) || !(panel === '' || validItems('panel', itemsOf(panel)))) return null;
  if (typeof feedsRefresh !== 'number' || !(REFRESH_CHOICES as readonly number[]).includes(feedsRefresh)) return null;
  return { tabs, panel, feedsDetect, feedsRefresh };
}

// --- tab stacks, tiling and workspaces (T10) ---
export interface WorkspaceRule { readonly site: string; readonly workspace: string }
export interface WorkspaceRules { readonly rules: readonly WorkspaceRule[]; readonly workspaces: readonly string[] }
/** Most rules (Rust's `workspace_rules::MAX_RULES`). */
export const MAX_WORKSPACE_RULES = 64;
const MAX_WORKSPACE_NAME = 128;
const isWorkspaceName = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= MAX_WORKSPACE_NAME && !/[\u0000-\u001f\u007f]/.test(value);
/** Strict check of the snapshot's `workspaceRules`. */
export function parseWorkspaceRules(value: unknown): WorkspaceRules | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (Object.keys(record).length !== 2 || !Array.isArray(record.rules) || !Array.isArray(record.workspaces)) return null;
  if (record.rules.length > MAX_WORKSPACE_RULES || record.workspaces.length > 128) return null;
  const rules: WorkspaceRule[] = [];
  for (const raw of record.rules as unknown[]) {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null;
    const rule = raw as Record<string, unknown>;
    if (Object.keys(rule).length !== 2 || typeof rule.site !== 'string' || normaliseSite(rule.site) !== rule.site || !isWorkspaceName(rule.workspace)) return null;
    rules.push({ site: rule.site, workspace: rule.workspace });
  }
  if (!(record.workspaces as unknown[]).every(isWorkspaceName)) return null;
  return { rules, workspaces: [...(record.workspaces as string[])] };
}
/** The `wsrule` text that adds a rule (`null` for a site or workspace Bowser would refuse). */
export function addRuleText(site: string, workspace: string): string | null {
  const plain = ruleSite(site);
  return plain === null || !isWorkspaceName(workspace.trim()) ? null : `add\t${plain}\t${workspace.trim()}`;
}
/** A typed site a rule can hold: a plain host with a dot (Rust's `workspace_rules::normalise_site`), or `null`. */
export function ruleSite(site: string): string | null {
  const plain = normaliseSite(site);
  return plain !== null && plain.includes('.') ? plain : null;
}
/** The `wsrule` text that removes the rule for `site`. */
export function removeRuleText(site: string): string {
  return `del\t${site}`;
}
/** Whether a typed site already has a rule. */
export function ruleExists(rules: readonly WorkspaceRule[], site: string): boolean {
  const plain = ruleSite(site);
  return plain !== null && rules.some((rule) => rule.site === plain);
}
// --- end tab stacks ---

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isIntBetween = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
const isStrUpTo = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);
const KEYS = ['tabLayout', 'sidebarWidth', 'sidebarPinned', 'sleepEnabled', 'sleepAfterSecs', 'blockerEnabled', 'blockerAllow', 'searchEngine', 'searchFallback',
  'automaticTabGrouping', 'efficiencyMode', 'efficiencyActive', 'trackingPrevention', 'autoUpdate', 'historyEnabled', 'guest', 'profile', 'avatar',
  'version', 'update', 'updateVersion', 'lastCheck', 'checkResult', 'ack', 'error', 'readable',
  'theme', 'themeAccent', 'toolbarButtons', 'centredAddressBar', 'minimalUrl', 'showLoadProgress', 'frameless', 'autoHideBars', 'autoHideFullscreen', 'roundedContentFrame', 'showHomeButton',
  'bookmarksBar', 'bookmarksBarTabGroups', 'sidePanelPosition', 'tabPreviewImages', 'tabPreviewMemory', 'fontSize', 'uiFont', 'defaultZoom',
  'showZoomIndicator', 'newTabBesideActive', 'ctrlTabMru', 'shiftRightClickContext', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs', 'pinNewTabGroups',
  'splitEdgeDrop', 'blockYoutubeVideoAds', 'defaultBrowser', 'downloadDir', 'downloadAsk',
  'translateOffer', 'translateAuto', 'translateTarget', 'translateTargets', 'start', 'tabOptions', 'workspaceRules' /* --- tab stacks (T10) --- */,
  'layout' /* --- layout (thread T6) --- */, 'themeOptions', 'window' /* --- window and panels (thread T9) --- */,
  'bars' /* --- toolbars and feeds (thread T13) --- */];
/** A language code the translation settings carry: two or three lower-case letters. */
const LANGUAGE_RE = /^[a-z]{2,3}$/;
/** The most target languages Rust may list. */
export const MAX_TRANSLATE_TARGETS = 64;
/** English names of the languages Bowser can translate into (the table in src/translate/mod.rs); a code not listed here shows as itself. */
const LANGUAGE_NAMES: Readonly<Record<string, string>> = {
  en: 'English', es: 'Spanish', fr: 'French', de: 'German', it: 'Italian', pt: 'Portuguese', nl: 'Dutch', pl: 'Polish', ru: 'Russian', uk: 'Ukrainian',
  tr: 'Turkish', ar: 'Arabic', he: 'Hebrew', fa: 'Persian', hi: 'Hindi', bn: 'Bengali', ur: 'Urdu', ta: 'Tamil', te: 'Telugu', mr: 'Marathi',
  gu: 'Gujarati', kn: 'Kannada', ml: 'Malayalam', pa: 'Punjabi', th: 'Thai', vi: 'Vietnamese', id: 'Indonesian', ms: 'Malay', fil: 'Filipino',
  zh: 'Chinese', ja: 'Japanese', ko: 'Korean', sv: 'Swedish', da: 'Danish', no: 'Norwegian', fi: 'Finnish', cs: 'Czech', sk: 'Slovak',
  hu: 'Hungarian', ro: 'Romanian', bg: 'Bulgarian', el: 'Greek', hr: 'Croatian', sr: 'Serbian', sl: 'Slovenian', lt: 'Lithuanian', lv: 'Latvian',
  et: 'Estonian', is: 'Icelandic', ga: 'Irish', cy: 'Welsh', sq: 'Albanian', ca: 'Catalan', eu: 'Basque', gl: 'Galician', af: 'Afrikaans',
  sw: 'Swahili', mk: 'Macedonian', be: 'Belarusian', ka: 'Georgian', hy: 'Armenian', az: 'Azerbaijani', kk: 'Kazakh',
};
export const TRANSLATE_TARGET_SYSTEM_LABEL = 'My Windows language';
export function languageName(code: string): string {
  return Object.hasOwn(LANGUAGE_NAMES, code) ? LANGUAGE_NAMES[code]! : code;
}
/** The "Translate into" options: the Windows language (value `''`) first, then one per code. */
export function translateTargetOptions(codes: readonly string[]): Array<{ value: string; label: string }> {
  return [{ value: '', label: TRANSLATE_TARGET_SYSTEM_LABEL }, ...codes.map((code) => ({ value: code, label: languageName(code) }))];
}
/** The longest download folder Rust sends (`MAX_PATH`). */
export const MAX_DOWNLOAD_DIR = 260;
/** A site the blocker skips: no whitespace, no controls, no comma (the file separator). */
const SITE_RE = /^[^\s\u0000-\u001f\u007f,]{1,253}$/u;

/** Rejects partial or extended snapshots before anything is drawn. */
export function parseSettingsSnapshot(value: unknown): SettingsSnapshot | null {
  if (!isRecord(value) || Object.keys(value).length !== KEYS.length || Object.keys(value).some((key) => !KEYS.includes(key))) return null;
  const v = value;
  if (!oneOf(v.tabLayout, TAB_LAYOUTS) || !isIntBetween(v.sidebarWidth, MIN_SIDEBAR, MAX_SIDEBAR) || typeof v.sidebarPinned !== 'boolean') return null;
  if (typeof v.sleepEnabled !== 'boolean' || !isIntBetween(v.sleepAfterSecs, MIN_SLEEP, MAX_SLEEP)) return null;
  if (typeof v.blockerEnabled !== 'boolean' || !Array.isArray(v.blockerAllow) || v.blockerAllow.length > MAX_ALLOW) return null;
  if (!v.blockerAllow.every((site) => typeof site === 'string' && SITE_RE.test(site)) || !allowFits(v.blockerAllow as string[])) return null;
  if (!(v.searchEngine === null || oneOf(v.searchEngine, SEARCH_ENGINES))) return null;
  if (v.searchFallback !== 'default' && v.searchFallback !== 'custom') return null;
  if (v.searchEngine !== null && v.searchFallback !== 'default') return null;
  if (typeof v.automaticTabGrouping !== 'boolean' || !oneOf(v.efficiencyMode, EFFICIENCY_MODES) || typeof v.efficiencyActive !== 'boolean') return null;
  if (v.trackingPrevention !== 'balanced' && v.trackingPrevention !== 'strict') return null;
  for (const key of ['autoUpdate', 'historyEnabled', 'guest', 'readable'] as const) if (typeof v[key] !== 'boolean') return null;
  if (v.guest === true && v.historyEnabled === true) return null;
  if (!isStrUpTo(v.profile, 64) || !isStrUpTo(v.avatar, 4) || !isStrUpTo(v.version, 32) || !isStrUpTo(v.updateVersion, 32) || !isStrUpTo(v.error, 240)) return null;
  if (!oneOf(v.update, UPDATE_STATES) || !oneOf(v.lastCheck, LAST_CHECKS) || !oneOf(v.checkResult, CHECK_RESULTS) || !oneOf(v.ack, ACKS)) return null;
  if (!oneOf(v.theme, THEME_MODES) || !isThemeAccent(v.themeAccent) || !isToolbarButtonsText(v.toolbarButtons)) return null;
  for (const key of ['centredAddressBar', 'minimalUrl', 'showLoadProgress', 'frameless', 'roundedContentFrame', 'showHomeButton'] as const) {
    if (typeof v[key] !== 'boolean') return null;
  }
  if (!isAutoHideBarsText(v.autoHideBars) || typeof v.autoHideFullscreen !== 'boolean') return null; // --- auto-hide (T11) ---
  if (!oneOf(v.bookmarksBar, BOOKMARKS_BAR_MODES) || typeof v.bookmarksBarTabGroups !== 'boolean') return null;
  if (!oneOf(v.sidePanelPosition, SIDE_PANEL_POSITIONS)) return null;
  if (typeof v.tabPreviewImages !== 'boolean' || typeof v.tabPreviewMemory !== 'boolean') return null;
  if (!oneOf(v.fontSize, FONT_SIZES) || !isUiFont(v.uiFont) || !isIntBetween(v.defaultZoom, MIN_ZOOM, MAX_ZOOM)) return null;
  for (const key of ['showZoomIndicator', 'newTabBesideActive', 'ctrlTabMru', 'shiftRightClickContext', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs',
    'pinNewTabGroups', 'splitEdgeDrop', 'blockYoutubeVideoAds'] as const) {
    if (typeof v[key] !== 'boolean') return null;
  }
  if (!oneOf(v.defaultBrowser, DEFAULT_BROWSER_STATES)) return null;
  // A display path: no control characters (it is drawn as text, never used as one).
  if (!isStrUpTo(v.downloadDir, MAX_DOWNLOAD_DIR * 2) || /[\u0000-\u001f\u007f]/.test(v.downloadDir) || typeof v.downloadAsk !== 'boolean') return null;
  if (typeof v.translateOffer !== 'boolean' || typeof v.translateAuto !== 'boolean') return null;
  if (typeof v.translateTarget !== 'string' || !(v.translateTarget === '' || LANGUAGE_RE.test(v.translateTarget))) return null;
  if (!Array.isArray(v.translateTargets) || v.translateTargets.length > MAX_TRANSLATE_TARGETS) return null;
  if (!v.translateTargets.every((code) => typeof code === 'string' && LANGUAGE_RE.test(code))) return null;
  const start = parseStartSettings(v.start); // --- start page ---
  if (start === null) return null;
  const tabOptions = parseTabOptions(v.tabOptions);
  if (!tabOptions) return null;
  const workspaceRules = parseWorkspaceRules(v.workspaceRules); // --- tab stacks (T10) ---
  if (!workspaceRules) return null;
  const layout = parseLayoutSettings(v.layout); // --- layout (thread T6) ---
  if (layout === null) return null;
  const themeOptions = parseThemeOptions(v.themeOptions); // --- theme options ---
  if (!themeOptions) return null;
  const window = parseWindowSettings(v.window); // --- window and panels (thread T9) ---
  if (window === null) return null;
  const bars = parseBarsSettings(v.bars); // --- toolbars and feeds (thread T13) ---
  if (bars === null) return null;
  return {
    tabLayout: v.tabLayout,
    sidebarWidth: v.sidebarWidth,
    sidebarPinned: v.sidebarPinned as boolean,
    sleepEnabled: v.sleepEnabled,
    sleepAfterSecs: v.sleepAfterSecs,
    blockerEnabled: v.blockerEnabled,
    blockerAllow: [...(v.blockerAllow as string[])],
    searchEngine: v.searchEngine as SearchEngine | null,
    searchFallback: v.searchFallback,
    automaticTabGrouping: v.automaticTabGrouping,
    efficiencyMode: v.efficiencyMode,
    trackingPrevention: v.trackingPrevention,
    theme: v.theme,
    themeAccent: v.themeAccent,
    toolbarButtons: v.toolbarButtons,
    centredAddressBar: v.centredAddressBar as boolean,
    minimalUrl: v.minimalUrl as boolean,
    showLoadProgress: v.showLoadProgress as boolean,
    frameless: v.frameless as boolean,
    autoHideBars: v.autoHideBars,
    autoHideFullscreen: v.autoHideFullscreen as boolean,
    roundedContentFrame: v.roundedContentFrame as boolean,
    showHomeButton: v.showHomeButton as boolean,
    bookmarksBar: v.bookmarksBar,
    bookmarksBarTabGroups: v.bookmarksBarTabGroups as boolean,
    sidePanelPosition: v.sidePanelPosition,
    tabPreviewImages: v.tabPreviewImages as boolean,
    tabPreviewMemory: v.tabPreviewMemory as boolean,
    fontSize: v.fontSize,
    uiFont: v.uiFont,
    defaultZoom: v.defaultZoom,
    showZoomIndicator: v.showZoomIndicator as boolean,
    newTabBesideActive: v.newTabBesideActive as boolean,
    ctrlTabMru: v.ctrlTabMru as boolean,
    shiftRightClickContext: v.shiftRightClickContext as boolean,
    ctrlShiftCCopyLink: v.ctrlShiftCCopyLink as boolean,
    ctrlSVerticalTabs: v.ctrlSVerticalTabs as boolean,
    pinNewTabGroups: v.pinNewTabGroups as boolean,
    splitEdgeDrop: v.splitEdgeDrop as boolean,
    blockYoutubeVideoAds: v.blockYoutubeVideoAds as boolean,
    efficiencyActive: v.efficiencyActive,
    autoUpdate: v.autoUpdate as boolean,
    historyEnabled: v.historyEnabled as boolean,
    guest: v.guest as boolean,
    profile: v.profile,
    avatar: v.avatar,
    version: v.version,
    update: v.update,
    updateVersion: v.updateVersion,
    lastCheck: v.lastCheck,
    checkResult: v.checkResult,
    ack: v.ack,
    error: v.error,
    readable: v.readable as boolean,
    defaultBrowser: v.defaultBrowser,
    downloadDir: v.downloadDir,
    downloadAsk: v.downloadAsk,
    translateOffer: v.translateOffer,
    translateAuto: v.translateAuto,
    translateTarget: v.translateTarget,
    translateTargets: [...(v.translateTargets as string[])],
    start,
    tabOptions,
    workspaceRules,
    layout, // --- layout (thread T6) ---
    themeOptions,
    window, // --- window and panels (thread T9) ---
    bars, // --- toolbars and feeds (thread T13) ---
  };
}

/** The profile settings `save` carries (everything else in the snapshot is read-only). */
export type Draft = Pick<SettingsSnapshot,
  | 'tabLayout' | 'sidebarWidth' | 'sidebarPinned' | 'sleepEnabled' | 'sleepAfterSecs' | 'blockerEnabled' | 'blockerAllow' | 'searchEngine' | 'automaticTabGrouping'
  | 'efficiencyMode' | 'trackingPrevention' | 'theme' | 'themeAccent' | 'toolbarButtons' | 'centredAddressBar' | 'minimalUrl' | 'showLoadProgress'
  | 'frameless' | 'autoHideBars' | 'autoHideFullscreen' | 'roundedContentFrame' | 'showHomeButton' | 'bookmarksBar' | 'bookmarksBarTabGroups' | 'sidePanelPosition' | 'tabPreviewImages'
  | 'tabPreviewMemory' | 'fontSize' | 'uiFont' | 'defaultZoom' | 'showZoomIndicator' | 'newTabBesideActive' | 'ctrlTabMru' | 'shiftRightClickContext'
  | 'ctrlShiftCCopyLink' | 'ctrlSVerticalTabs' | 'pinNewTabGroups' | 'splitEdgeDrop' | 'blockYoutubeVideoAds' | 'start'
  | 'layout' // --- layout (thread T6) ---
  | 'window' // --- window and panels (thread T9) ---
>;

export function draftOf(snapshot: SettingsSnapshot): Draft {
  const {
    tabLayout, sidebarWidth, sidebarPinned, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow, searchEngine, automaticTabGrouping, efficiencyMode,
    trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless, autoHideBars, autoHideFullscreen, roundedContentFrame,
    showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont, defaultZoom,
    showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups, splitEdgeDrop,
    blockYoutubeVideoAds, start, layout, window,
  } = snapshot;
  return {
    tabLayout, sidebarWidth, sidebarPinned, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow: [...blockerAllow], searchEngine, automaticTabGrouping,
    efficiencyMode, trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless, autoHideBars, autoHideFullscreen,
    roundedContentFrame, showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont,
    defaultZoom, showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups,
    splitEdgeDrop, blockYoutubeVideoAds, start: { ...start, startupPages: [...start.startupPages] },
    layout: { ...layout }, // --- layout (thread T6) ---
    window: { ...window }, // --- window and panels (thread T9) ---
  };
}

/**
 * The exact-match `save` text: every profile setting once, in a fixed order, the search engine only
 * when one is chosen. `null` for any value Bowser would refuse.
 */
export function saveText(draft: Draft): string | null {
  if (!oneOf(draft.tabLayout, TAB_LAYOUTS) || !isIntBetween(draft.sidebarWidth, MIN_SIDEBAR, MAX_SIDEBAR)) return null;
  if (!isIntBetween(draft.sleepAfterSecs, MIN_SLEEP, MAX_SLEEP) || draft.blockerAllow.length > MAX_ALLOW || !allowFits(draft.blockerAllow)) return null;
  if (!draft.blockerAllow.every((site) => SITE_RE.test(site))) return null;
  if (!oneOf(draft.efficiencyMode, EFFICIENCY_MODES) || (draft.trackingPrevention !== 'balanced' && draft.trackingPrevention !== 'strict')) return null;
  if (draft.searchEngine !== null && !oneOf(draft.searchEngine, SEARCH_ENGINES)) return null;
  if (!oneOf(draft.theme, THEME_MODES) || !isThemeAccent(draft.themeAccent) || !isToolbarButtonsText(draft.toolbarButtons)) return null;
  if (!isAutoHideBarsText(draft.autoHideBars)) return null; // --- auto-hide (T11) ---
  if (!oneOf(draft.bookmarksBar, BOOKMARKS_BAR_MODES) || !oneOf(draft.sidePanelPosition, SIDE_PANEL_POSITIONS)) return null;
  if (!oneOf(draft.fontSize, FONT_SIZES) || !isUiFont(draft.uiFont) || !isIntBetween(draft.defaultZoom, MIN_ZOOM, MAX_ZOOM)) return null;
  const lines = [
    `tab_layout=${draft.tabLayout}`,
    `sidebar_width=${draft.sidebarWidth}`,
    `sidebar_collapsed=${draft.sidebarPinned ? 'false' : 'true'}`,
    `sleep_enabled=${draft.sleepEnabled}`,
    `sleep_after_secs=${draft.sleepAfterSecs}`,
    `blocker_enabled=${draft.blockerEnabled}`,
    `blocker_allow=${draft.blockerAllow.join(',')}`,
    `automatic_tab_grouping=${draft.automaticTabGrouping}`,
    `efficiency_mode=${draft.efficiencyMode}`,
    `tracking_prevention=${draft.trackingPrevention}`,
  ];
  if (draft.searchEngine !== null) lines.push(`search_engine=${draft.searchEngine}`);
  lines.push(
    `theme=${draft.theme}`,
    `theme_accent=${draft.themeAccent}`,
    `toolbar_buttons=${draft.toolbarButtons}`,
    `centred_address_bar=${draft.centredAddressBar}`,
    `minimal_url=${draft.minimalUrl}`,
    `show_load_progress=${draft.showLoadProgress}`,
    `frameless=${draft.frameless}`,
    `auto_hide_bars=${draft.autoHideBars}`,
    `auto_hide_fullscreen=${draft.autoHideFullscreen}`,
    `rounded_content_frame=${draft.roundedContentFrame}`,
    `show_home_button=${draft.showHomeButton}`,
    `bookmarks_bar=${draft.bookmarksBar}`,
    `bookmarks_bar_tab_groups=${draft.bookmarksBarTabGroups}`,
    `side_panel_position=${draft.sidePanelPosition}`,
    `tab_preview_images=${draft.tabPreviewImages}`,
    `tab_preview_memory=${draft.tabPreviewMemory}`,
    `font_size=${draft.fontSize}`,
    `ui_font=${draft.uiFont}`,
    `default_zoom=${draft.defaultZoom}`,
    `show_zoom_indicator=${draft.showZoomIndicator}`,
    `new_tab_beside_active=${draft.newTabBesideActive}`,
    `ctrl_tab_mru=${draft.ctrlTabMru}`,
    `shift_right_click_context=${draft.shiftRightClickContext}`,
    `ctrl_shift_c_copy_link=${draft.ctrlShiftCCopyLink}`,
    `ctrl_s_vertical_tabs=${draft.ctrlSVerticalTabs}`,
    `pin_new_tab_groups=${draft.pinNewTabGroups}`,
    `split_edge_drop=${draft.splitEdgeDrop}`,
    `block_youtube_video_ads=${draft.blockYoutubeVideoAds}`,
  );
  // --- start page --- the start page and startup settings ride in the same payload
  const startLines = startSaveLines(draft.start);
  if (startLines === null) return null;
  lines.push(...startLines);
  // --- end start page ---
  // --- layout (thread T6) --- the Layout block's options follow
  const layoutLines = layoutSaveLines(draft.layout);
  if (layoutLines === null) return null;
  lines.push(...layoutLines);
  // --- end layout ---
  lines.push(...windowSaveLines(draft.window)); // --- window and panels (thread T9) ---
  const text = lines.join('\n');
  return utf8Bytes(text) <= MAX_SETTINGS_SAVE ? text : null;
}

// ---- Sections and rows (the nav, the search index and the deep links) ----

export interface SectionSpec { readonly key: SettingsSection; readonly title: string; readonly nav: string; readonly lead: string; readonly aliases?: string }
export const SECTIONS: readonly SectionSpec[] = [
  { key: 'profile', title: 'Profile', nav: 'Profile', lead: '' },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: '' },
  { key: 'tabs', title: 'Tabs', nav: 'Tabs', lead: '' },
  // --- T10 ---
  { key: 'stacks', title: 'Groups', nav: 'Groups', lead: '', aliases: 'tab groups stacking tiling workspaces' },
  { key: 'startpage', title: 'Start page', nav: 'Start page', lead: '' },
  { key: 'privacy', title: 'Privacy', nav: 'Privacy', lead: '', aliases: 'privacy and security' },
  { key: 'performance', title: 'Performance', nav: 'Performance', lead: '' },
  { key: 'pagetools', title: 'Page tools', nav: 'Page tools', lead: '' }, // --- page tools (T7) ---
  { key: 'address', title: 'Address', nav: 'Address', lead: '', aliases: 'address bar' },
  { key: 'search', title: 'Search', nav: 'Search', lead: '' },
  { key: 'bookmarks', title: 'Bookmarks', nav: 'Bookmarks', lead: '' },
  { key: 'keyboard', title: 'Keyboard', nav: 'Keyboard', lead: '' },
  { key: 'commands', title: 'Commands', nav: 'Commands', lead: '', aliases: 'quick commands' },
  { key: 'downloads', title: 'Downloads', nav: 'Downloads', lead: '' },
  { key: 'media', title: 'Media', nav: 'Media', lead: '' },
  { key: 'extensions', title: 'Extensions', nav: 'Extensions', lead: '' },
  { key: 'updates', title: 'Updates', nav: 'Updates', lead: '' },
  { key: 'about', title: 'About', nav: 'About', lead: '' },
];

export function isSection(value: string): value is SettingsSection {
  return (SETTINGS_SECTIONS as readonly string[]).includes(value);
}

export interface RowSpec {
  readonly id: string;
  readonly section: SettingsSection;
  readonly label: string;
  /** A fuller accessible name when the visible label is terse. Never shown. */
  readonly aria?: string;
  readonly description: string;
  /** Extra words the search matches (never shown). Old labels and descriptions live here. */
  readonly keywords: string;
  /** The change waits for a restart. */
  readonly restart?: boolean;
  /** Saved, but Bowser does not act on it yet: the page says so instead of pretending. */
  readonly notYet?: boolean;
  /** Where the row is drawn (`rowTier`); a row that sets none gets the table's choice. */
  readonly tier?: RowTier;
}

// --- settings slim --- the page shows a short core list per area. The long tail of each area sits
// behind one "More options" disclosure, the big editors open as sub-pages from one row, and the rows
// that do nothing yet sit in "Not active yet" at the end. Rust's sixteen section keys stay (deep links,
// `settingsgo`); an area groups one or more of them under one heading and one nav item.

/** `core`: always shown. `more`: behind the area's "More options". `page`: its own sub-page, opened from a row. `later`: "Not active yet". */
export type RowTier = 'core' | 'more' | 'page' | 'later';

export interface AreaSpec {
  readonly key: SettingsSection;
  readonly title: string;
  readonly nav: string;
  readonly lead: string;
  readonly aliases?: string;
  /** The Rust sections drawn in this area, in order; the first is the area's own key. */
  readonly parts: readonly SettingsSection[];
}

const sectionSpec = (key: SettingsSection): SectionSpec => SECTIONS.find((spec) => spec.key === key)!;
const single = (key: SettingsSection): AreaSpec => {
  const spec = sectionSpec(key);
  return { key, title: spec.title, nav: spec.nav, lead: spec.lead, parts: [key] };
};

/**
 * The page's areas, in nav order. Fewer headings than Rust's section keys: everyday choices first,
 * then how Bowser looks, tabs, privacy, browsing, shortcuts, extensions, and about.
 * A deep link to any Rust section still lands in the area that contains it.
 */
export const AREAS: readonly AreaSpec[] = [
  { key: 'profile', title: 'General', nav: 'General', lead: '', parts: ['profile', 'search', 'startpage', 'downloads'] },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: '', parts: ['appearance'] },
  { key: 'tabs', title: 'Tabs', nav: 'Tabs', lead: '', parts: ['tabs', 'stacks'] },
  { key: 'privacy', title: 'Privacy', nav: 'Privacy', lead: '', parts: ['privacy', 'performance'] },
  { key: 'address', title: 'Browsing', nav: 'Browsing', lead: '', parts: ['address', 'bookmarks', 'pagetools'] },
  { key: 'keyboard', title: 'Shortcuts', nav: 'Shortcuts', lead: '', aliases: 'keyboard shortcuts quick commands', parts: ['keyboard', 'commands'] },
  single('extensions'),
  single('media'),
  { key: 'about', title: 'About', nav: 'About', lead: '', parts: ['updates', 'about'] },
];

/** The area a Rust section is drawn in. */
export function areaOf(section: SettingsSection): AreaSpec {
  return AREAS.find((area) => area.parts.includes(section)) ?? AREAS[0]!;
}

/** The title of a part inside a merged area (the old section's own title). */
export function partTitle(section: SettingsSection): string {
  return sectionSpec(section).title;
}

export const MORE_LABEL = 'More';
export const LATER_TITLE = 'Inactive';
export const LATER_LEAD = '';
export const LATER_LABEL = 'Show';

/** "12 more settings": the disclosure's description. */
export function moreCountText(count: number): string {
  return count === 1 ? '1 more setting' : `${count} more settings`;
}

/**
 * Rows a person changes often enough to see without opening anything. Every other row is `more`.
 * Each area keeps at most ten of these, counting the rows that open a sub-page.
 * Light or dark sits in the main Appearance view; the rest of the theme editor stays behind More options.
 * Default page zoom, the feed list, private-window search and the finer tab rules are behind More options.
 */
const CORE_ROWS: ReadonlySet<string> = new Set([
  'profile', 'import',
  'layoutPreset', 'layoutBars' /* --- layout (thread T6): the presets and the three edges; the rest of the block is `more` --- */,
  'theme', 'themeSchedule', 'themeAccent', 'websiteAppearance', 'bookmarksBar', 'fontSize', 'translateOffer',
  'feedsOpen', // --- toolbars and feeds (thread T13) ---
  'tabs_new_tab_page', 'tabs_new_tab_url',
  'tabs_stack_style', 'tabs_workspaces', // --- T10 ---
  'startupMode', 'startupPages', 'startHomepage',
  'blocker', 'tracking', 'history',
  'efficiency', 'sleep',
  'addrAutocomplete', 'secHttps', 'searchDefault', 'bmNewTab',
  'kbEnabled',
  ...PAGE_TOOLS_CORE, // --- page tools (T7) ---
  'downloadDir', 'downloadAsk', 'media', 'extensions', 'autoUpdate', 'updateStatus', 'version', 'file', 'reset', 'defaultBrowser',
]);

export interface PageSpec {
  /** The opening row's button. */
  readonly button: string;
  /** Accessible name when the button is a single word. */
  readonly aria: string;
  /** Put the opening row after this row (or opening row) instead of where the editor was. */
  readonly after?: string;
}

/** The big editors and blocks that open as sub-pages, each from one row. */
export const PAGE_ROWS: Readonly<Record<string, PageSpec>> = {
  engines: { button: 'Edit', aria: 'Edit search engines' },
  siteRules: { button: 'Edit', aria: 'Edit sites', after: 'history' },
  blockLists: { button: 'Edit', aria: 'Edit lists', after: 'siteRules' },
  deleteData: { button: 'Clear', aria: 'Clear browsing data', after: 'blockLists' },
  dashboard: { button: 'Open', aria: 'Open dashboard', after: 'deleteData' },
  toolbarButtons: { button: 'Edit', aria: 'Edit toolbars' }, // --- toolbars and feeds (thread T13) ---
  kbEditor: { button: 'Edit', aria: 'Edit shortcuts' },
  chains: { button: 'Edit', aria: 'Edit chains' },
  workspaceRules: { button: 'Edit', aria: 'Edit rules' }, // --- T10 ---
};

/** Where a row is drawn. A row Bowser does not act on yet is always `later`, whatever else it says. */
export function rowTier(row: RowSpec): RowTier {
  if (row.notYet) return 'later';
  if (row.tier) return row.tier;
  if (Object.hasOwn(PAGE_ROWS, row.id)) return 'page';
  return CORE_ROWS.has(row.id) ? 'core' : 'more';
}
// --- end settings slim ---

/** The note under a row whose change waits for a restart. */
export const RESTART_NOTE = 'Applies after restart';
/** The note under a row (or a whole group of rows) Bowser saves but does not act on yet. */
export const NOT_YET_NOTE = 'Has no effect yet. Bowser keeps your choice for when it does.';
export const NOT_YET_GROUP_NOTE = 'These have no effect yet. Bowser keeps your choices for when they do.';

// --- wave-1 integration --- the Tabs section: T1's options card by card, with the older tab rows
// (Ctrl+Tab order, tab previews) at the end of its Tab bar card, where settings-page.ts draws them.
const OLDER_TAB_ROWS: readonly RowSpec[] = [
  { id: 'ctrlTabMru', section: 'tabs', label: 'Ctrl+Tab', aria: 'Cycle tabs most recently used first with Ctrl+Tab', description: '', keywords: 'ctrl tab cycle cycling most recently used mru switch order Cycle tabs most-recently-used first with Ctrl+Tab Ctrl+Tab steps through tabs in the order you last used them. Off, it follows the tab order.' },
  { id: 'tabPreviewImages', section: 'tabs', label: 'Preview images', aria: 'Show a thumbnail in tab previews', description: '', keywords: 'tab preview thumbnail hover image popup Show a thumbnail in tab previews Tab hover previews include a snapshot of the page.' },
  { id: 'tabPreviewMemory', section: 'tabs', label: 'Preview memory', aria: 'Show memory use in tab previews', description: '', keywords: 'tab preview memory usage hover popup Show memory use in tab previews Tab hover previews show how much memory the tab is using.' },
];
const TABS_SECTION_ROWS: readonly RowSpec[] = (() => {
  const rows = tabOptionRows();
  const lastBar = cardSpecs('bar').at(-1)?.key;
  const at = rows.findIndex((row) => row.id === lastBar) + 1;
  return [...rows.slice(0, at), ...OLDER_TAB_ROWS, ...rows.slice(at)];
})();
// --- end wave-1 integration ---

/** Every row the page draws, in page order. The view builds a control per id. */
export const ROWS: readonly RowSpec[] = [
  { id: 'profile', section: 'profile', label: 'Profile', aria: 'This profile', description: '', keywords: 'account person name avatar guest you and bowser profile This profile Settings on this page apply to this profile only.' },
  { id: 'profiles', section: 'profile', label: 'Profiles', aria: 'Manage profiles', description: '', keywords: 'switch new guest delete rename Manage profiles Switch, create, rename or delete profiles.' },
  { id: 'import', section: 'profile', label: 'Import', aria: 'Import bookmarks', description: '', keywords: 'chrome edge firefox migrate transfer bookmarks import Import bookmarks Bring bookmarks over from another browser on this PC.' },
  // --- layout (thread T6) --- the Layout block: presets and the three edges in view, the rest behind More options.
  { id: 'layoutPreset', section: 'appearance', label: 'Presets', description: '', keywords: 'layout preset presets simple classic vertical left right bottom saved thumbnails arrangement tabs top side Presets A starting arrangement. Change any part of it and Bowser keeps yours as Saved.' },
  { id: 'layoutBars', section: 'appearance', label: 'Bar positions', aria: 'Tab bar, address bar and panels', description: '', keywords: 'tab bar position top left right bottom address bar side panel dock helium edge title row window buttons Tab bar, address bar and panels Which edge each one sits on.' },
  { id: 'layout', section: 'appearance', label: 'Top style', aria: 'Tab bar style at the top', description: '', keywords: 'tabs toolbar horizontal single row top browser layout Tab bar style at the top How the tabs and the address bar share the top of the window.' },
  { id: 'sidebarWidth', section: 'appearance', label: 'Sidebar width', description: '', keywords: 'vertical tabs size wide narrow pixels Sidebar width Width of the tab sidebar in the vertical layouts.' },
  { id: 'sidebarPinned', section: 'appearance', label: 'Pin sidebar', aria: 'Pin vertical tabs', description: '', keywords: 'vertical tabs pin sidebar hover expand overlay edge rail icons Pin vertical tabs Keeps vertical tabs open beside the page. Off, they are a narrow rail that opens on hover.' },
  { id: 'tabBarVisible', section: 'appearance', label: 'Tab bar', aria: 'Show the tab bar', description: '', keywords: 'tab bar show hide tabs visible Show the tab bar Off, the tabs are reached with Ctrl+Tab, the tab list and Quick commands.' },
  { id: 'addressBarVisible', section: 'appearance', label: 'Address bar', aria: 'Show the address bar row', description: '', keywords: 'address bar show hide toolbar row visible ctrl l Show the address bar row The whole row: navigation, the address field and the menu. Off, Ctrl+L brings it back until the address field loses focus.' },
  { id: 'showTitleBar', section: 'appearance', label: 'Title bar', aria: 'Show the title bar when the tab bar is on a side', description: '', keywords: 'title bar window buttons caption vertical side page title Show the title bar when the tab bar is on a side A row above the address bar with the page title and the window buttons. With the address bar along the bottom, the row is always there.' },
  { id: 'statusBar', section: 'appearance', label: 'Status bar', description: '', keywords: 'status bar info overlay link address hover hide Status bar Bowser has no status bar of its own. The address of a link under the pointer shows in a small overlay at the bottom of the page; Hidden turns that off.' },
  { id: 'uiDensity', section: 'appearance', label: 'Density', aria: 'User interface density', description: '', keywords: 'density regular compact rows spacing dense User interface density Compact shortens the rows of the tab sidebar, the panels and Bowser’s own pages.' },
  { id: 'compactMenus', section: 'appearance', label: 'Compact menus', aria: 'Compact menu layout', description: '', keywords: 'compact menus dense rows menu layout Compact menu layout Shorter rows in Bowser’s menus.' },
  { id: 'uiZoom', section: 'appearance', label: 'UI zoom', aria: 'User interface zoom', description: '', keywords: 'ui zoom scale interface size percent slider text User interface zoom Scales the text of Bowser’s bars, panels and pages, on top of the font size. Select the value to type one.' },
  // --- end layout ---
  { id: 'theme', section: 'appearance', label: 'Palette', aria: 'Colour palette', description: '', keywords: 'dark light colour scheme helium system prefer system colour windows accent palette theme Colour palette Bowser’s own colours, or your Windows accent colour.' },
  { id: 'themeAccent', section: 'appearance', label: 'Accent', aria: 'Accent colour', description: '', keywords: 'colour accent override custom theme reset Accent colour A colour for buttons and highlights. Reset uses the default.' },
  { id: 'websiteAppearance', section: 'appearance', label: 'Websites', description: '', keywords: 'dark light auto mode prefers color scheme websites page theme appearance Websites Ask sites for a light page, a dark page, or the same as Windows.' },
  { id: 'forceDark', section: 'appearance', label: 'Force dark', aria: 'Force dark theme on all websites', description: '', keywords: 'force dark mode night websites invert auto dark Force dark theme on all websites Darkens pages that have no dark theme of their own, in open tabs at once and in the pages you open next.' },
  { id: 'privateTheme', section: 'appearance', label: 'Private theme', aria: 'Private tab theme', description: '', keywords: 'private incognito inprivate theme dark light window colour Private tab theme How Bowser’s own windows look while a private tab is shown: the same as normal, light or dark.' },
  // --- theme options --- the schedule, custom colours, colouring mode, page accent, contrast and the theme file.
  ...THEME_ROWS,
  // --- end theme options ---
  // --- toolbars and feeds (thread T13) --- the editor for the three bars (a sub-page); it replaces the old checklist.
  { id: 'toolbarButtons', section: 'appearance', label: 'Toolbars', aria: 'Customise your toolbars', description: '', keywords: 'toolbar toolbars buttons customize customise address bar tab bar panel bar back forward reload bookmark new tab split reader site information downloads extensions command chain button add remove move reset Customise your toolbars Choose the buttons on the address bar, the tab bar and the panel bar, and put a command or a chain of commands on any of them.' },
  { id: 'showHomeButton', section: 'appearance', label: 'Home button', aria: 'Show the Home button', description: '', keywords: 'home button toolbar rail sidebar Show the Home button Adds a Home button to the side rail, beside the tabs.' },
  { id: 'centredAddressBar', section: 'appearance', label: 'Centre address', aria: 'Centre the address bar', description: '', keywords: 'address bar centre centered position Centre the address bar Keeps the address bar centred in the toolbar.' },
  { id: 'minimalUrl', section: 'appearance', label: 'Short address', aria: 'Show a simplified address while unfocused', description: '', keywords: 'url simplified host domain minimal address bar focus Show a simplified address while unfocused Shows the page’s site and path, without the query or fragment, until you focus the address bar, then the full address.' },
  { id: 'showLoadProgress', section: 'appearance', label: 'Load progress', aria: 'Show a loading progress bar', description: '', keywords: 'progress loading bar indicator Show a loading progress bar A thin bar tracks page loads in the toolbar.' },
  { id: 'grouping', section: 'appearance', label: 'Group by site', aria: 'Group tabs by site', description: '', keywords: 'automatic tab groups islands domain Group tabs by site Tabs from one site share an island. Groups you make by hand stay.', restart: true },
  // --- wave-1 integration --- "Open new tabs next to the active tab" is Tabs > New tab position (the
  // same `new_tab_beside_active` key), and the Ctrl+Tab order and tab previews sit in Tabs > Tab bar
  // (TABS_SECTION_ROWS below).
  // --- T10 --- drag and drop tiling, beside the other tiling options.
  { id: 'splitEdgeDrop', section: 'stacks', label: 'Edge split', aria: 'Drop a dragged tab on the window edge to split', description: '', keywords: 'split view drag drop edge tab tiling tile Allow dropping a dragged tab on the window edge to split Dragging a tab to the edge of the window opens it in a split view.' },
  { id: 'pinNewTabGroups', section: 'appearance', label: 'Pin groups', aria: 'Pin new tab groups to the bookmarks bar', description: '', keywords: 'pin tab groups bookmarks bar Pin new tab groups to the bookmarks bar A tab group formed from now on is pinned to the bookmarks bar. Needs “Show tab groups in the bookmarks bar”.' },
  { id: 'ctrlSVerticalTabs', section: 'keyboard', label: 'Ctrl+S', aria: 'Ctrl+S collapses or expands the sidebar in the vertical layout', description: '', keywords: 'ctrl s sidebar collapse expand vertical layout shortcut Ctrl+S collapses or expands the sidebar in the vertical layout Only does anything in the vertical layout; otherwise Ctrl+S reaches the page as normal.' },
  // --- window and panels (thread T9) --- each row below acts at once; all sit behind More options. Not here, on purpose:
  // "open settings in a tab" and "open popups in tabs" (Bowser already always does), "show title bar" (Layout's
  // title row), "reset toolbars" (the toolbar editor) and the Windows and Reading list panels (Bowser has neither).
  { id: 'winAnimations', section: 'appearance', label: 'Animation', aria: 'Use animation', description: '', keywords: 'animation motion transitions reduced motion prefers reduced motion slide Use animation Bowser’s own transitions and the panel slide. Off, they snap. Bowser also stays still when Windows has animations off.' },
  { id: 'winRangeButtons', section: 'appearance', label: 'Steppers', aria: 'Buttons instead of sliders', description: '', keywords: 'sliders range buttons stepper plus minus Buttons instead of sliders Range controls on this page, such as the sidebar width and the interface zoom, show minus and plus buttons instead of a slider.' },
  { id: 'winFade', section: 'appearance', label: 'Fade inactive', aria: 'Fade colours in background windows', description: '', keywords: 'fade colours background window inactive dim grey desaturate Fade colours in background windows Bowser’s bars and panels lose their colour while another window is in front.' },
  { id: 'winScrollbars', section: 'appearance', label: 'Scrollbars', aria: 'Simple scrollbars', description: '', keywords: 'scrollbars simple thin slim overlay Simple scrollbars Thin, quiet scrollbars in Bowser’s own pages and panels. Web pages keep theirs.' },
  { id: 'winMonoIcons', section: 'appearance', label: 'Plain icons', aria: 'Monochrome settings icons', description: '', keywords: 'monochrome icons settings colour accent plain Monochrome settings icons The icons in this page’s list draw in plain text colour, without the accent.' },
  { id: 'panelCloseButton', section: 'appearance', label: 'Close button', aria: 'Show the close button on panels', description: '', keywords: 'panel close button x header Show the close button on panels The X in a panel’s header. Off, Escape and the panel’s rail button still close it.' },
  { id: 'panelFolderCounts', section: 'appearance', label: 'Folder counts', aria: 'Show folder counters', description: '', keywords: 'folder counters count number bookmarks panel items Show folder counters How many items a bookmarks folder holds, beside its name.' },
  { id: 'panelAutoClose', section: 'appearance', label: 'Auto-close', aria: 'Close floating panels when you click elsewhere', description: '', keywords: 'floating panel auto close dismiss click outside overlay flyout Close floating panels when you click elsewhere Bookmarks and Customize float over the page. Off, they stay until you close them.' },
  { id: 'winReset', section: 'appearance', label: 'Reset window', aria: 'Reset window options', description: '', keywords: 'reset window defaults restore Restores defaults Reset window options Puts animation, sliders, fading, scrollbars and icons back to their defaults.' },
  { id: 'panelReset', section: 'appearance', label: 'Reset panels', aria: 'Reset panel options', description: '', keywords: 'reset panels defaults restore Restores defaults Reset panel options Puts the close button, folder counters and auto-close back to their defaults.' },
  // --- end window and panels ---
  { id: 'roundedContentFrame', section: 'appearance', label: 'Rounded page', aria: 'Rounded corners around the page', description: '', keywords: 'rounded corners content frame page Rounded corners around the page Rounds the corners of the page area.' },
  // --- auto-hide (T11) --- the master switch is the old `frameless` key (core); the other two sit behind "More options".
  { id: 'frameless', section: 'appearance', label: 'Auto-hide', aria: 'Auto-hide toolbars and tabs', description: '', keywords: 'auto hide autohide frameless toolbar tabs immersive fullscreen full screen bars edge Auto-hide toolbars and tabs Hides the bars you choose until the pointer rests at the edge of the window. Ctrl+F11 turns it on or off.', tier: 'more' },
  { id: 'autoHideBars', section: 'appearance', label: 'Hidden bars', aria: 'Bars to hide', description: '', keywords: 'auto hide bars tabs address bookmarks panel status Bars to hide The tab bar and address bar share the top edge in the horizontal layouts, so either one hides both. The side panel opens over the page instead of pushing it.', tier: 'more' },
  { id: 'autoHideFullscreen', section: 'appearance', label: 'Full screen', aria: 'Always auto-hide in full screen', description: '', keywords: 'auto hide full screen fullscreen f11 video Always auto-hide in full screen In full screen (F11, or a video) every bar hides, even with the switch above off.', tier: 'more' },
  // --- end auto-hide ---
  { id: 'bookmarksBar', section: 'appearance', label: 'Bookmarks bar', description: '', keywords: 'bookmarks bar show hide newtab always never Bookmarks bar When the bookmarks bar is shown.' },
  { id: 'bookmarksBarTabGroups', section: 'appearance', label: 'Groups on bar', aria: 'Show tab groups in the bookmarks bar', description: '', keywords: 'tab groups bookmarks bar islands Show tab groups in the bookmarks bar Tab groups pinned to the bar appear after the bookmarks. Select one to go to it.' },
  ...ADDRESS_ROWS.filter((row) => row.section === 'appearance'),
  // --- layout (thread T6) --- the side panel's edge is the Panels choice of the "Tab bar, address bar and panels" row (layoutBars) above; it acts at once.
  { id: 'fontSize', section: 'appearance', label: 'Text size', description: '', keywords: 'font size small medium large text Text size The size of text on Bowser’s own pages.' },
  { id: 'uiFont', section: 'appearance', label: 'Font', aria: 'Customise fonts', description: '', keywords: 'font family typeface custom ui Customise fonts A font family to use instead of the system font.' },
  { id: 'defaultZoom', section: 'appearance', label: 'Page zoom', description: '', keywords: 'zoom default starting percent page Page zoom The zoom new pages start at.' },
  { id: 'showZoomIndicator', section: 'appearance', label: 'Zoom indicator', aria: 'Show the zoom indicator', description: '', keywords: 'zoom indicator toolbar percent Show the zoom indicator Shows the zoom level in the toolbar when it isn’t 100%.' },
  { id: 'translateOffer', section: 'appearance', label: 'Offer translate', aria: 'Offer to translate pages in other languages', description: '', keywords: 'translate translation language foreign page Offer to translate pages in other languages Shows a button in the toolbar when a page is in another language. Nothing is sent until you click it. Pages are translated by Google Translate, so their text is sent to Google.' },
  { id: 'translateAuto', section: 'appearance', label: 'Auto-translate', aria: 'Translate automatically on sites set to always translate', description: '', keywords: 'translate translation language foreign page automatic always Translate automatically on sites set to always translate Sites set to always translate are translated as soon as they load, which sends their text to Google Translate.' },
  { id: 'translateTarget', section: 'appearance', label: 'Translate to', aria: 'Translate into', description: '', keywords: 'translate translation language foreign page target windows Translate into' },
  { id: 'shiftRightClickContext', section: 'keyboard', label: 'Shift+right-click', aria: 'Shift+right click shows the system menu', description: '', keywords: 'shift right click context menu system Shift+Right Click shows the system menu Shift+Right Click always shows the system menu over Bowser’s own tab and rail menus.' },
  { id: 'ctrlShiftCCopyLink', section: 'keyboard', label: 'Ctrl+Shift+C', aria: 'Ctrl+Shift+C copies the address', description: '', keywords: 'ctrl shift c copy link address shortcut Ctrl+Shift+C copies the address Ctrl+Shift+C copies the current page’s address.' },
  // The Tabs section's rows come from its own table (tab-options-logic.ts).
  ...TABS_SECTION_ROWS,
  // --- tab stacks, tiling and workspaces (T10) ---
  { id: 'workspaceRules', section: 'stacks', label: 'Rules', aria: 'Workspace rules', description: '', keywords: 'workspace rules sites open automatically named workspace domain Workspace rules Sites that open in a workspace of their own: a link, an address or a bookmark of the site moves its tab there.' },
  // --- start page --- (docs/design/start-page.md)
  { id: 'startGroupReopen', section: 'startpage', label: 'Start group', aria: 'Open the start page on', description: '', keywords: 'speed dial group last first new tab start page reopen Open the start page on Which speed dial group a new tab shows first.' },
  { id: 'startTitles', section: 'startpage', label: 'Titles', aria: 'Show titles', description: '', keywords: 'speed dial titles names always never needed labels Show titles When a speed dial shows its name. When needed shows it under tiles without a chosen image, and on hover for the rest.' },
  { id: 'startAddButton', section: 'startpage', label: 'Add button', aria: 'Show the add button', description: '', keywords: 'speed dial add button plus tile new site Show the add button A tile at the end of each group for adding a site.' },
  { id: 'startColumns', section: 'startpage', label: 'Columns', aria: 'Maximum columns', description: '', keywords: 'speed dial columns row grid width Maximum columns The most speed dials in a row. Fewer show when the window is narrow.' },
  { id: 'startThumbSize', section: 'startpage', label: 'Tile size', description: '', keywords: 'speed dial thumbnail size tiny small medium large huge tiles Tile size How large each speed dial tile is.' },
  { id: 'startDragReorder', section: 'startpage', label: 'Reorder', aria: 'Drag to reorder', description: '', keywords: 'speed dial drag drop reorder move sort Drag to reorder Drag speed dials to reorder them, or onto a folder to put them inside it. Alt and the arrow keys do the same.' },
  { id: 'startHomepage', section: 'startpage', label: 'Homepage', description: '', keywords: 'home page homepage address url Homepage The Home button opens this. Leave it empty for the start page.' },
  { id: 'startupMode', section: 'startpage', label: 'Startup', aria: 'On startup', description: '', keywords: 'startup launch open start page last session where you left off homepage specific pages restore when bowser starts On startup What opens when Bowser starts.' },
  { id: 'startupPages', section: 'startpage', label: 'Startup pages', aria: 'Pages to open at startup', description: '', keywords: 'startup pages specific urls tabs launch Pages to open at startup One address per line, up to eight. Used when Bowser starts with specific pages.' },
  { id: 'startWidgets', section: 'startpage', label: 'Dashboard', aria: 'Show the dashboard', description: '', keywords: 'widgets dashboard top sites sticky note date privacy statistics webpage Show the dashboard Widgets under the speed dials: top sites, bookmarks, a sticky note, the date, privacy statistics and a web page.' },
  { id: 'startWidgetColumns', section: 'startpage', label: 'Widget columns', description: '', keywords: 'widgets columns row dashboard Widget columns The most widgets in a row, 1 to 4.' },
  { id: 'startWidgetHeight', section: 'startpage', label: 'Height', aria: 'Widget height', description: '', keywords: 'widgets height tall regular dashboard Widget height Regular or tall widgets.' },
  { id: 'startWidgetBg', section: 'startpage', label: 'Background', aria: 'Widget background', description: '', keywords: 'widgets background solid transparent translucent custom colour Widget background How a widget is filled: solid, semi-transparent, transparent or a colour you choose.' },
  { id: 'startWidgetColor', section: 'startpage', label: 'Colour', aria: 'Custom widget colour', description: '', keywords: 'widgets colour color custom background Custom widget colour Used when the widget background is Custom.' },
  { id: 'startWidgetDrag', section: 'startpage', label: 'Rearrange', aria: 'Drag to rearrange widgets', description: '', keywords: 'widgets drag rearrange reorder move Drag to rearrange widgets Drag a widget by its handle, or press Alt and an arrow key on it.' },
  // --- end start page ---
  { id: 'blocker', section: 'privacy', label: 'Blocking', description: '', keywords: 'adblock ad blocker shield trackers ads none level block blocking Blocking None, trackers, or trackers and ads. A site can have its own level.' },
  { id: 'cookieNotices', section: 'privacy', label: 'Cookie notices', aria: 'Hide cookie notices', description: '', keywords: 'cookie warning banner consent gdpr popup annoyance i don’t care about cookies easylist Hide cookie notices Hides cookie-consent banners on pages, with or without blocking. A site can switch it off.' },
  { id: 'blockYoutubeVideoAds', section: 'privacy', label: 'YouTube ads', aria: 'Block YouTube video ads', description: '', keywords: 'youtube video ads block skip Block YouTube video ads Skips in-video ads on YouTube where the blocking level is Trackers and ads, and in private tabs. At Trackers or None, YouTube ads still play.' },
  { id: 'allow', section: 'privacy', label: 'Skipped sites', aria: 'Sites the blocker skips', description: '', keywords: 'allow list exceptions whitelist adblock Sites the blocker skips The blocker never blocks on these sites.' },
  { id: 'tracking', section: 'privacy', label: 'Tracking', aria: 'Tracking prevention', description: '', keywords: 'privacy cookies strict balanced fingerprinting Tracking prevention Balanced is the default. Strict blocks more trackers but can break some sites.', restart: true },
  { id: 'history', section: 'privacy', label: 'History', aria: 'Save history', description: '', keywords: 'history visits record browsing save Save history Pages you visit are listed in History and suggested in the address bar.' },
  { id: 'historyRetention', section: 'privacy', label: 'Keep history', aria: 'Keep history for', description: '', keywords: 'history retention expire delete old days week month year forever session Keep history for Older visits are deleted from the history list. Session only clears it when Bowser closes.' },
  { id: 'siteRules', section: 'privacy', label: 'Site rules', aria: 'Sites with their own choices', description: '', keywords: 'site exceptions per site level blocking cookie notices list shield Sites with their own choices Give one site its own blocking level or cookie-notice choice.' },
  { id: 'blockLists', section: 'privacy', label: 'Block lists', aria: 'Blocking lists', description: '', keywords: 'sources filter lists easylist easyprivacy subscription url file add cookie list Blocking lists Switch lists on or off, add one from an address or a file, or remove one you added.' },
  { id: 'deleteData', section: 'privacy', label: 'Clear data', aria: 'Clear browsing data', description: '', keywords: 'clear delete browsing data history cookies cache storage logins sign-ins autofill site settings hour day week Cannot be undone Clear browsing data Delete history, cookies, cache and other site data for a period you choose.' },
  { id: 'dashboard', section: 'privacy', label: 'Dashboard', aria: 'Privacy dashboard', description: '', keywords: 'clear cookies cache data permissions camera microphone location notifications statistics Privacy dashboard Review site permissions and blocking statistics.' },
  { id: 'efficiency', section: 'performance', label: 'Efficiency', description: '', keywords: 'battery power eco energy saver laptop efficiency mode Efficiency On battery, hidden tabs sleep sooner and use less memory.' },
  { id: 'sleep', section: 'performance', label: 'Sleep tabs', aria: 'Sleep unused tabs', description: '', keywords: 'memory discard suspend hibernate inactive hidden tabs Sleep unused tabs Hidden tabs go to sleep after a while to free memory.' },
  { id: 'sleepAfter', section: 'performance', label: 'Sleep after', description: '', keywords: 'delay timeout minutes memory Sleep after How long a tab stays hidden before it sleeps.' },
  ...ADDRESS_ROWS.filter((row) => row.section !== 'appearance'),
  ...PAGE_TOOLS_ROWS, // --- page tools (T7) ---
  ...FEED_ROWS, // --- toolbars and feeds (thread T13) ---
  // --- keyboard and quick commands (docs/commands.md) ---
  { id: 'kbEnabled', section: 'keyboard', label: 'Shortcuts', aria: 'Keyboard shortcuts', description: '', keywords: 'keyboard shortcuts enable disable hotkeys keybindings keys ctrl shift k Keyboard shortcuts Turn Bowser’s shortcuts on or off. Quick commands stays available.' },
  { id: 'kbSingleKey', section: 'keyboard', label: 'Single-key', aria: 'Allow single-key shortcuts', description: '', keywords: 'single key shortcuts letter number plain keys Allow single-key shortcuts Lets a letter, number or punctuation key be a shortcut on its own. They work only from Bowser’s own bars, never while you type in a page or a text field.' },
  { id: 'kbAltMenu', section: 'keyboard', label: 'Alt menu', aria: 'Alt opens the menu', description: '', keywords: 'alt key menu windows Alt opens the menu Pressing and releasing Alt on its own opens the Bowser menu. Off, it does nothing.' },
  { id: 'kbFocusCycling', section: 'keyboard', label: 'F6 focus', aria: 'F6 moves between the address bar and the page', description: '', keywords: 'f6 focus cycle address bar page tab F6 moves between the address bar and the page F6 jumps from the address bar to the page, and back. Off, F6 always goes to the address bar.' },
  { id: 'kbSites', section: 'keyboard', label: 'Site shortcuts', aria: 'Prioritise website shortcuts on these sites', description: '', keywords: 'website shortcuts priority sites page keys web app gmail docs Prioritise website shortcuts on these sites On a site listed here, shortcuts the page uses go to the page. The browser keeps the shortcuts marked Browser priority below. The “Prioritise website shortcuts on this site” command adds or removes the site you are on.' },
  { id: 'kbEditor', section: 'keyboard', label: 'Bindings', aria: 'Edit shortcuts', description: '', keywords: 'shortcuts assign rebind remap clear restore reset cheat sheet ctrl f1 keybindings browser priority Shortcuts Give a command several shortcuts, clear them or restore the defaults. Ctrl+F1 shows the cheat sheet. Editing shortcuts such as Ctrl+C and Ctrl+V cannot be taken.' },
  { id: 'qcTypes', section: 'commands', label: 'Result types', description: '', keywords: 'quick commands palette results types order prefix bookmark note tab history command action workspace calculator Result types Choose which kinds of result Quick commands shows and in what order. Type bkm:, tb:, cmd: and similar before a search to look in one place. Shift or the right arrow completes the top result.' },
  { id: 'qcMax', section: 'commands', label: 'Result limit', aria: 'Visible results', description: '', keywords: 'quick commands palette results limit maximum number Visible results The most results shown for a typed search.' },
  { id: 'qcKeepLast', section: 'commands', label: 'Remember search', aria: 'Keep the last typed search', description: '', keywords: 'quick commands palette remember last typed value query Keep the last typed search Quick commands opens with what you typed last time, selected, so the first key you press replaces it.' },
  { id: 'qcNewTab', section: 'commands', label: 'Open in tab', aria: 'Open links in a new tab', description: '', keywords: 'quick commands palette new tab bookmark history open link Open links in a new tab A bookmark or history entry picked in Quick commands opens in a new tab, not the one you are on.' },
  { id: 'qcNotesPanel', section: 'commands', label: 'Notes panel', aria: 'Open notes in the Notes panel', description: '', keywords: 'quick commands palette notes manager panel copy Open notes in the Notes panel Picking a note opens the Notes panel; the panel does not yet go to the picked note. Off, it copies the note’s text instead.' },
  { id: 'qcNickname', section: 'commands', label: 'Nicknames', aria: 'Open a bookmark when its nickname is typed', description: '', keywords: 'quick commands palette bookmark nickname automatic open match keyword Open a bookmark when its nickname is typed Typing a bookmark’s whole nickname in Quick commands opens it at once, without pressing Enter. Give a bookmark a nickname in its bookmark dialog.' },
  { id: 'chains', section: 'commands', label: 'Chains', aria: 'Command chains', description: '', keywords: 'command chains macro sequence delay milliseconds test clone create edit delete automation Command chains Run several commands one after another, with a delay in milliseconds before each. Run a chain from Quick commands, give it a shortcut in Keyboard, or test it here.' },
  // --- end keyboard and quick commands ---
  { id: 'downloadDir', section: 'downloads', label: 'Folder', aria: 'Download folder', description: '', keywords: 'download folder directory save path where change location Download folder' },
  { id: 'downloadAsk', section: 'downloads', label: 'Ask where', aria: 'Ask where to save each file', description: '', keywords: 'save as prompt dialog ask where download location before downloading Ask where to save each file Off, files go straight to the folder above.' },
  { id: 'media', section: 'media', label: 'Playback', aria: 'Media support', description: '', keywords: 'video codec h264 vp9 av1 hevc widevine playready drm netflix hardware decode Media support Runs on this device only and sends nothing.' },
  { id: 'extensions', section: 'extensions', label: 'Extensions', description: '', keywords: 'extension add-on addon plugin unpacked crx file manifest permissions popup install remove uninstall disable enable installed Extensions Turn extensions on or off, or remove one.' },
  { id: 'autoUpdate', section: 'updates', label: 'Automatic', aria: 'Update automatically', description: '', keywords: 'auto update upgrade version Update automatically Checks in the background and installs the update the next time Bowser starts. Applies to every profile.' },
  { id: 'updateStatus', section: 'updates', label: 'Status', aria: 'Bowser updates', description: '', keywords: 'check for updates restart version upgrade Bowser updates' },
  { id: 'version', section: 'about', label: 'Version', description: '', keywords: 'about build release Version' },
  { id: 'file', section: 'about', label: 'Settings file', description: '', keywords: 'settings.txt advanced edit text shortcuts engines history workspace blocker updates Settings file Most settings are saved in this profile’s settings.txt, which you can also edit by hand. Kept in their own files: shortcuts and command chains, search engines, the history switch, workspace rules, and blocker rules and lists. Automatic updates are shared by all profiles.' },
  { id: 'reset', section: 'about', label: 'Reset', aria: 'Reset settings', description: '', keywords: 'defaults restore Restores defaults factory start over tracking prevention Reset settings Puts layout, appearance, toolbars, tabs, start page, address bar, search engines, bookmarks bar, page tools, feeds, blocking and website appearance, performance, downloads and translation back to their defaults. Tracking prevention goes back to its default the next time Bowser starts. Not changed: how long history is kept, your history and bookmarks, the Keyboard section and Quick commands, extensions, site permissions and updates.' },
  { id: 'defaultBrowser', section: 'about', label: 'Default browser', description: '', keywords: 'default browser make bowser windows handler http https web links Default browser Open links from other apps in Bowser.' },
];

/** Lower-case, accent-free, single-spaced text for matching. */
export function fold(text: string): string {
  return text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/\s+/g, ' ').trim();
}

/** Ids of the rows whose words contain every word of `query`; `null` for an empty query (show all). */
export function matchRows(query: string, rows: readonly RowSpec[] = ROWS): Set<string> | null {
  const words = fold(query).split(' ').filter(Boolean);
  if (words.length === 0) return null;
  const hits = new Set<string>();
  for (const row of rows) {
    const area = areaOf(row.section);
    const sectionSpec = SECTIONS.find((spec) => spec.key === row.section);
    const section = sectionSpec?.title ?? '';
    const hay = fold(`${row.label} ${row.aria ?? ''} ${row.description} ${row.keywords} ${section} ${sectionSpec?.aliases ?? ''} ${area.title} ${area.nav} ${area.aliases ?? ''}`);
    if (words.every((word) => hay.includes(word))) hits.add(row.id);
  }
  return hits;
}

// ---- Simple settings (a UI choice only; nothing here is written to settings.txt) ----

/** Where the page remembers Simple versus All. A viewer convenience, like which More options are open. */
export const SETTINGS_VIEW_KEY = 'bowser.settings.view';
export type SettingsView = 'simple' | 'all';

/** `all` only when that exact word was saved. Anything else, including a first visit, is Simple. */
export function readSettingsView(raw: string | null): SettingsView {
  return raw === 'all' ? 'all' : 'simple';
}

/** Existing rows the simple page lifts out of All settings, in the order it shows them. */
export const SIMPLE_MOVED: readonly string[] = ['searchDefault', 'startHomepage', 'startupMode', 'themeAccent', 'import', 'deleteData', 'defaultBrowser'];

export const SIMPLE_TITLE = 'Settings';
export const SIMPLE_LEAD = '';
export const SIMPLE_ALL_LABEL = 'All settings';

/** Light, dark, or follow Windows. `null` when a time-of-day schedule is in use, so the control does not pretend. */
export function simpleThemeChoice(schedule: string, look: string): 'light' | 'dark' | 'system' | null {
  if (schedule === 'system') return 'system';
  if (schedule === 'none') return look === 'dark' ? 'dark' : 'light';
  return null;
}

/** Top or side. `null` when the tabs sit along the bottom, which is neither. */
export function simpleTabChoice(edge: 'top' | 'left' | 'right' | 'bottom'): 'top' | 'side' | null {
  if (edge === 'left' || edge === 'right') return 'side';
  if (edge === 'top') return 'top';
  return null;
}

/**
 * Where the words of `query` occur in `text` (shown text only), as merged [start, end) ranges
 * over `text` itself. Matching ignores case; accents are compared as written.
 */
export function highlightRanges(text: string, query: string): Array<[number, number]> {
  const words = query.toLowerCase().split(/\s+/).filter((word) => word.length > 0);
  const lower = text.toLowerCase();
  const ranges: Array<[number, number]> = [];
  for (const word of words) {
    let from = 0;
    for (;;) {
      const at = lower.indexOf(word, from);
      if (at < 0) break;
      ranges.push([at, at + word.length]);
      from = at + word.length;
    }
  }
  ranges.sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [];
  for (const range of ranges) {
    const last = merged[merged.length - 1];
    if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
    else merged.push([range[0], range[1]]);
  }
  return merged;
}

export function resultCountText(count: number, query: string): string {
  if (count === 0) return `No settings match “${query.trim()}”`;
  return count === 1 ? '1 setting found' : `${count} settings found`;
}

// ---- Values and words ----

/** Sleep delays the select offers; a stored value that is not one of them is shown as its own option. */
export const SLEEP_PRESETS: readonly number[] = [30, 60, 300, 900, 1800, 3600, 14400, 86400, 604800];

export function durationLabel(secs: number): string {
  const plural = (n: number, unit: string): string => `${n} ${unit}${n === 1 ? '' : 's'}`;
  if (secs % 86400 === 0) return plural(secs / 86400, 'day');
  if (secs % 3600 === 0) return plural(secs / 3600, 'hour');
  if (secs % 60 === 0) return plural(secs / 60, 'minute');
  return plural(secs, 'second');
}

export function sleepOptions(current: number): number[] {
  const list = [...SLEEP_PRESETS];
  if (!list.includes(current) && current >= MIN_SLEEP && current <= MAX_SLEEP) list.push(current);
  return list.sort((a, b) => a - b);
}

/**
 * A typed site for the allow list: trimmed and lower-cased, a scheme, path, port or `www.` prefix
 * dropped. `null` when what is left could not be a host.
 */
export function normaliseSite(input: string): string | null {
  let text = input.trim().toLowerCase();
  text = text.replace(/^[a-z][a-z0-9+.-]*:\/\//, '');
  text = text.split(/[/?#]/)[0] ?? '';
  text = text.replace(/:\d+$/, '').replace(/^www\./, '').replace(/\.$/, '');
  if (!/^(?=.{1,253}$)[a-z0-9¡-￿](?:[a-z0-9¡-￿-]*[a-z0-9¡-￿])?(?:\.[a-z0-9¡-￿](?:[a-z0-9¡-￿-]*[a-z0-9¡-￿])?)*$/u.test(text)) return null;
  return SITE_RE.test(text) ? text : null;
}

export type AddSite = { readonly list: string[] } | { readonly error: string };

export function addSite(list: readonly string[], input: string): AddSite {
  const site = normaliseSite(input);
  if (site === null) return { error: 'Enter a site.' };
  if (list.includes(site)) return { error: `${site} is already on the list.` };
  if (list.length >= MAX_ALLOW) return { error: `The list holds at most ${MAX_ALLOW} sites.` };
  const next = [...list, site];
  if (!allowFits(next)) return { error: `The list is full: its sites can use at most ${MAX_ALLOW_BYTES} characters in all.` };
  return { list: next };
}

export interface UpdateView {
  readonly title: string;
  readonly detail: string;
  readonly action: 'check' | 'restart' | null;
  readonly busy: boolean;
  readonly tone: 'ok' | 'warn' | 'info';
}

/** What the update card says and offers. */
export function updateView(s: Pick<SettingsSnapshot, 'update' | 'updateVersion' | 'lastCheck' | 'checkResult' | 'version' | 'autoUpdate'>): UpdateView {
  if (s.update === 'unavailable') return { title: 'Off', detail: '', action: null, busy: false, tone: 'info' };
  if (s.update === 'checking') return { title: 'Checking…', detail: s.version, action: null, busy: true, tone: 'info' };
  if (s.update === 'ready') {
    const which = s.updateVersion !== '' ? s.updateVersion : 'Update';
    return { title: which, detail: '', action: 'restart', busy: false, tone: 'ok' };
  }
  const result: Record<CheckResult, string> = {
    none: '',
    staged: 'Downloaded',
    uptodate: 'Up to date',
    unverified: 'Not verified',
    busy: 'Checking',
    unavailable: 'Off',
    failed: 'Check failed',
  };
  if (s.checkResult !== 'none') {
    const warn = s.checkResult === 'failed' || s.checkResult === 'unverified';
    return { title: result[s.checkResult], detail: s.version, action: 'check', busy: false, tone: warn ? 'warn' : 'ok' };
  }
  const title = s.lastCheck === 'uptodate' ? 'Up to date' : s.lastCheck === 'failed' ? 'Check failed' : s.version;
  const detail = s.autoUpdate ? '' : 'Off';
  return { title, detail, action: 'check', busy: false, tone: s.lastCheck === 'failed' ? 'warn' : 'ok' };
}

