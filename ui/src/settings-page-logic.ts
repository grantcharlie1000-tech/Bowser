// Pure logic for the settings page (?view=settings, docs/design/settings-page.md): the snapshot
// contract, the exact-match save payload, the section and row index the nav and the search use,
// and the words that depend on state. No DOM here; test/settings-page.mjs covers all of it.
import { MAX_TEXT, SETTINGS_SECTIONS, type SettingsSection } from './protocol';
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

export { SETTINGS_SECTIONS, type SettingsSection };

const SEARCH_ENGINES = ['optimised', 'google', 'duckduckgo'] as const;
export type SearchEngine = typeof SEARCH_ENGINES[number];
export const EFFICIENCY_MODES = ['auto', 'on', 'off'] as const;
export type EfficiencyMode = typeof EFFICIENCY_MODES[number];
export type TrackingPrevention = 'balanced' | 'strict';
// --- layout (thread T6) --- the layouts (six now), the Layout block's `layout` object and its presets live in layout-logic.ts.
import {
  LAYOUT_HINTS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_LABELS, layoutSaveLines, parseLayoutSettings,
  type LayoutSettings, type SidePanelPosition, type TabLayout,
} from './layout-logic';
export { LAYOUT_HINTS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_LABELS, type LayoutSettings, type SidePanelPosition, type TabLayout };
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

export const THEME_MODES = ['helium', 'system'] as const;
export type ThemeMode = typeof THEME_MODES[number];
/** The catalog of toolbar buttons, in their fixed order (also the default, full set). */
export const TOOLBAR_BUTTONS = ['back', 'forward', 'reload', 'site-info', 'bookmark', 'newtab', 'split', 'reader'] as const;
export type ToolbarButton = typeof TOOLBAR_BUTTONS[number];
export const DEFAULT_TOOLBAR_BUTTONS = TOOLBAR_BUTTONS.join(',');
// --- auto-hide (T11) --- the bars "Auto-hide toolbars and tabs" can take out of the layout, in page order
// (src/auto_hide.rs BAR_TABLE). The status bar joins when the layout work adds one.
export const AUTO_HIDE_BARS = ['tabs', 'address', 'bookmarks', 'panel'] as const;
export type AutoHideBar = typeof AUTO_HIDE_BARS[number];
export const AUTO_HIDE_BAR_LABELS: Readonly<Record<AutoHideBar, string>> = {
  tabs: 'Tab bar',
  address: 'Address bar',
  bookmarks: 'Bookmarks bar',
  panel: 'Side panel (opens over the page)',
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

/** No duplicates, every id known, order free (Rust accepts any order; the page always writes the catalog order). */
function isToolbarButtonsText(value: unknown): value is string {
  if (typeof value !== 'string') return false;
  if (value === '') return true;
  const seen = new Set<string>();
  for (const id of value.split(',')) {
    if (!(TOOLBAR_BUTTONS as readonly string[]).includes(id) || seen.has(id)) return false;
    seen.add(id);
  }
  return true;
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
  /** Comma list of `TOOLBAR_BUTTONS` ids, each at most once; `''` for none. */
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
  // --- layout (thread T6) ---
  /** The Layout block's own options (`layout_prefs.rs`); the tab layout and panel side are the older keys above. */
  layout: LayoutSettings;
  // --- end layout ---
  // --- window and panels (thread T9) ---
  /** Window appearance and panel options (`window_prefs.rs`). */
  window: WindowSettings;
  // --- end window and panels ---
}

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isIntBetween = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
const isStrUpTo = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);
const KEYS = ['tabLayout', 'sidebarWidth', 'sleepEnabled', 'sleepAfterSecs', 'blockerEnabled', 'blockerAllow', 'searchEngine', 'searchFallback',
  'automaticTabGrouping', 'efficiencyMode', 'efficiencyActive', 'trackingPrevention', 'autoUpdate', 'historyEnabled', 'guest', 'profile', 'avatar',
  'version', 'update', 'updateVersion', 'lastCheck', 'checkResult', 'ack', 'error', 'readable',
  'theme', 'themeAccent', 'toolbarButtons', 'centredAddressBar', 'minimalUrl', 'showLoadProgress', 'frameless', 'autoHideBars', 'autoHideFullscreen', 'roundedContentFrame', 'showHomeButton',
  'bookmarksBar', 'bookmarksBarTabGroups', 'sidePanelPosition', 'tabPreviewImages', 'tabPreviewMemory', 'fontSize', 'uiFont', 'defaultZoom',
  'showZoomIndicator', 'newTabBesideActive', 'ctrlTabMru', 'shiftRightClickContext', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs', 'pinNewTabGroups',
  'splitEdgeDrop', 'blockYoutubeVideoAds', 'defaultBrowser', 'downloadDir', 'downloadAsk',
  'translateOffer', 'translateAuto', 'translateTarget', 'translateTargets', 'start', 'tabOptions', 'layout' /* --- layout (thread T6) --- */, 'themeOptions',
  'window' /* --- window and panels (thread T9) --- */];
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
  if (!oneOf(v.tabLayout, TAB_LAYOUTS) || !isIntBetween(v.sidebarWidth, MIN_SIDEBAR, MAX_SIDEBAR)) return null;
  if (typeof v.sleepEnabled !== 'boolean' || !isIntBetween(v.sleepAfterSecs, MIN_SLEEP, MAX_SLEEP)) return null;
  if (typeof v.blockerEnabled !== 'boolean' || !Array.isArray(v.blockerAllow) || v.blockerAllow.length > MAX_ALLOW) return null;
  if (!v.blockerAllow.every((site) => typeof site === 'string' && SITE_RE.test(site))) return null;
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
  const layout = parseLayoutSettings(v.layout); // --- layout (thread T6) ---
  if (layout === null) return null;
  const themeOptions = parseThemeOptions(v.themeOptions); // --- theme options ---
  if (!themeOptions) return null;
  const window = parseWindowSettings(v.window); // --- window and panels (thread T9) ---
  if (window === null) return null;
  return {
    tabLayout: v.tabLayout,
    sidebarWidth: v.sidebarWidth,
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
    layout, // --- layout (thread T6) ---
    themeOptions,
    window, // --- window and panels (thread T9) ---
  };
}

/** The profile settings `save` carries (everything else in the snapshot is read-only). */
export type Draft = Pick<SettingsSnapshot,
  | 'tabLayout' | 'sidebarWidth' | 'sleepEnabled' | 'sleepAfterSecs' | 'blockerEnabled' | 'blockerAllow' | 'searchEngine' | 'automaticTabGrouping'
  | 'efficiencyMode' | 'trackingPrevention' | 'theme' | 'themeAccent' | 'toolbarButtons' | 'centredAddressBar' | 'minimalUrl' | 'showLoadProgress'
  | 'frameless' | 'autoHideBars' | 'autoHideFullscreen' | 'roundedContentFrame' | 'showHomeButton' | 'bookmarksBar' | 'bookmarksBarTabGroups' | 'sidePanelPosition' | 'tabPreviewImages'
  | 'tabPreviewMemory' | 'fontSize' | 'uiFont' | 'defaultZoom' | 'showZoomIndicator' | 'newTabBesideActive' | 'ctrlTabMru' | 'shiftRightClickContext'
  | 'ctrlShiftCCopyLink' | 'ctrlSVerticalTabs' | 'pinNewTabGroups' | 'splitEdgeDrop' | 'blockYoutubeVideoAds' | 'start'
  | 'layout' // --- layout (thread T6) ---
  | 'window' // --- window and panels (thread T9) ---
>;

export function draftOf(snapshot: SettingsSnapshot): Draft {
  const {
    tabLayout, sidebarWidth, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow, searchEngine, automaticTabGrouping, efficiencyMode,
    trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless, autoHideBars, autoHideFullscreen, roundedContentFrame,
    showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont, defaultZoom,
    showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups, splitEdgeDrop,
    blockYoutubeVideoAds, start, layout, window,
  } = snapshot;
  return {
    tabLayout, sidebarWidth, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow: [...blockerAllow], searchEngine, automaticTabGrouping,
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
  if (!isIntBetween(draft.sleepAfterSecs, MIN_SLEEP, MAX_SLEEP) || draft.blockerAllow.length > MAX_ALLOW) return null;
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
  return text.length <= MAX_TEXT ? text : null;
}

// ---- Sections and rows (the nav, the search index and the deep links) ----

export interface SectionSpec { readonly key: SettingsSection; readonly title: string; readonly nav: string; readonly lead: string }
export const SECTIONS: readonly SectionSpec[] = [
  { key: 'profile', title: 'You and Bowser', nav: 'You and Bowser', lead: 'The profile these settings belong to.' },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: 'Layout, colours, the toolbar, tabs, text size and page translation.' },
  { key: 'tabs', title: 'Tabs', nav: 'Tabs', lead: 'How tabs open, close, show their state and come back after a restart.' },
  { key: 'startpage', title: 'Start page', nav: 'Start page', lead: 'Speed dials and widgets on a new tab, your homepage, and what Bowser opens at startup.' },
  { key: 'privacy', title: 'Privacy and security', nav: 'Privacy and security', lead: 'Blocking, tracking prevention, history, site permissions and browsing data.' },
  { key: 'performance', title: 'Performance', nav: 'Performance', lead: 'Save memory and power in the background.' },
  { key: 'pagetools', title: 'Page tools', nav: 'Page tools', lead: 'Capture, moving between links with the keyboard, page zoom and reader view.' }, // --- page tools (T7) ---
  { key: 'address', title: 'Address bar', nav: 'Address bar', lead: 'How the address field behaves, what its drop-down suggests, and how it protects you.' },
  { key: 'search', title: 'Search engine', nav: 'Search engine', lead: 'The engines the address bar and the search field search with.' },
  { key: 'bookmarks', title: 'Bookmarks', nav: 'Bookmarks', lead: 'How bookmarks open and how new ones are saved.' },
  { key: 'keyboard', title: 'Keyboard', nav: 'Keyboard', lead: 'Shortcuts for every command, and how the keyboard behaves.' },
  { key: 'commands', title: 'Quick commands', nav: 'Quick commands', lead: 'What the command palette searches, and chains of commands to run with one shortcut.' },
  { key: 'downloads', title: 'Downloads', nav: 'Downloads', lead: 'Where files are saved, and whether Bowser asks first.' },
  { key: 'media', title: 'Media', nav: 'Media', lead: 'Which video formats and protected streams this device can play.' },
  { key: 'extensions', title: 'Extensions', nav: 'Extensions', lead: 'Extensions in this profile, and what each one declared when you installed it.' },
  { key: 'updates', title: 'Updates', nav: 'Updates', lead: 'How Bowser keeps itself up to date.' },
  { key: 'about', title: 'About Bowser', nav: 'About Bowser', lead: 'Version, where settings live, and starting over.' },
];

export function isSection(value: string): value is SettingsSection {
  return (SETTINGS_SECTIONS as readonly string[]).includes(value);
}

export interface RowSpec {
  readonly id: string;
  readonly section: SettingsSection;
  readonly label: string;
  readonly description: string;
  /** Extra words the search matches (never shown). */
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
  /** The Rust sections drawn in this area, in order; the first is the area's own key. */
  readonly parts: readonly SettingsSection[];
}

const sectionSpec = (key: SettingsSection): SectionSpec => SECTIONS.find((spec) => spec.key === key)!;
const single = (key: SettingsSection): AreaSpec => {
  const spec = sectionSpec(key);
  return { key, title: spec.title, nav: spec.nav, lead: spec.lead, parts: [key] };
};

/** The page's areas, in nav order: Bowser's own sections, with Tabs, Address bar and Shortcuts each taking in their neighbours. */
export const AREAS: readonly AreaSpec[] = [
  single('profile'),
  single('appearance'),
  { key: 'tabs', title: 'Tabs and start page', nav: 'Tabs and start page', lead: 'How tabs open and close, and what a new tab and a new window show.', parts: ['tabs', 'startpage'] },
  single('privacy'),
  single('performance'),
  single('pagetools'), // --- page tools (T7) ---
  { key: 'address', title: 'Address bar and search', nav: 'Address bar and search', lead: 'How the address field behaves, the engines it searches with, and how bookmarks open.', parts: ['address', 'search', 'bookmarks'] },
  { key: 'keyboard', title: 'Shortcuts and commands', nav: 'Shortcuts and commands', lead: 'Keyboard shortcuts, Quick commands and chains of commands.', parts: ['keyboard', 'commands'] },
  single('downloads'),
  single('media'),
  single('extensions'),
  single('updates'),
  single('about'),
];

/** The area a Rust section is drawn in. */
export function areaOf(section: SettingsSection): AreaSpec {
  return AREAS.find((area) => area.parts.includes(section)) ?? AREAS[0]!;
}

/** The title of a part inside a merged area (the old section's own title). */
export function partTitle(section: SettingsSection): string {
  return sectionSpec(section).title;
}

export const MORE_LABEL = 'More options';
export const LATER_TITLE = 'Not active yet';
export const LATER_LEAD = 'Bowser saves these choices, but they change nothing yet. Each one starts working when its feature arrives.';
export const LATER_LABEL = 'Show these settings';

/** "12 more settings": the disclosure's description. */
export function moreCountText(count: number): string {
  return count === 1 ? '1 more setting' : `${count} more settings`;
}

/** Rows a person changes often enough to see without opening anything. Every other row is `more`. */
const CORE_ROWS: ReadonlySet<string> = new Set([
  'profile', 'profiles', 'import',
  'layoutPreset', 'layoutBars' /* --- layout (thread T6): the presets and the three edges; the rest of the block is `more` --- */,
  // --- final --- Appearance keeps ten core rows: the theme schedule (T8) and the auto-hide switch (T11) sit behind More options.
  'theme', 'themeAccent', 'websiteAppearance', 'showHomeButton', 'bookmarksBar', 'fontSize', 'defaultZoom', 'translateOffer',
  'tabs_new_tab_page', 'tabs_new_tab_url', 'tabs_new_tab_position', 'tabs_link_background',
  'startupMode', 'startupPages', 'startHomepage',
  'blocker', 'cookieNotices', 'tracking', 'history', 'historyRetention',
  'efficiency', 'sleep', 'sleepAfter',
  'addrAutocomplete', 'addrFull', 'secHttps', 'searchDefault', 'searchPrivate', 'searchField', 'bmNewTab',
  'kbEnabled',
  ...PAGE_TOOLS_CORE, // --- page tools (T7) ---
  'downloadDir', 'downloadAsk', 'media', 'extensions', 'autoUpdate', 'updateStatus', 'version', 'file', 'reset', 'defaultBrowser',
]);

export interface PageSpec {
  /** The opening row's button. */
  readonly button: string;
  /** Put the opening row after this row (or opening row) instead of where the editor was. */
  readonly after?: string;
}

/** The big editors and blocks that open as sub-pages, each from one row. */
export const PAGE_ROWS: Readonly<Record<string, PageSpec>> = {
  engines: { button: 'Edit search engines…' },
  siteRules: { button: 'Edit sites…', after: 'cookieNotices' },
  blockLists: { button: 'Edit lists…', after: 'siteRules' },
  deleteData: { button: 'Choose what to delete…', after: 'historyRetention' },
  dashboard: { button: 'Open dashboard…', after: 'deleteData' },
  kbEditor: { button: 'Edit shortcuts…' },
  chains: { button: 'Edit chains…' },
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
  { id: 'ctrlTabMru', section: 'tabs', label: 'Cycle tabs most-recently-used first with Ctrl+Tab', description: 'Ctrl+Tab steps through tabs in the order you last used them. Off, it follows the tab order.', keywords: 'ctrl tab cycle cycling most recently used mru switch order' },
  { id: 'tabPreviewImages', section: 'tabs', label: 'Show a thumbnail in tab previews', description: 'Tab hover previews include a snapshot of the page.', keywords: 'tab preview thumbnail hover image popup' },
  { id: 'tabPreviewMemory', section: 'tabs', label: 'Show memory use in tab previews', description: 'Tab hover previews show how much memory the tab is using.', keywords: 'tab preview memory usage hover popup' },
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
  { id: 'profile', section: 'profile', label: 'Profile', description: 'Settings on this page apply to this profile only.', keywords: 'account person name avatar guest' },
  { id: 'profiles', section: 'profile', label: 'Manage profiles', description: 'Switch, create, rename or delete profiles.', keywords: 'switch new guest delete rename' },
  { id: 'import', section: 'profile', label: 'Import from another browser', description: 'Bring bookmarks and other data over from a browser on this PC.', keywords: 'chrome edge firefox migrate transfer bookmarks' },
  // --- layout (thread T6) --- the Layout block: presets and the three edges in view, the rest behind More options.
  { id: 'layoutPreset', section: 'appearance', label: 'Layout', description: 'A starting point for where the tabs, the address bar and the panels sit. Change any of them and the arrangement becomes your own, kept as Saved.', keywords: 'layout preset simple classic vertical left right bottom saved thumbnails arrangement' },
  { id: 'layoutBars', section: 'appearance', label: 'Tab bar, address bar and panels', description: 'Which edge each one sits on. The address bar along the bottom keeps a title row at the top for the window buttons.', keywords: 'tab bar position top left right bottom address bar side panel dock helium edge' },
  { id: 'layout', section: 'appearance', label: 'Tab bar style at the top', description: 'How the tabs and the address bar share the top of the window.', keywords: 'tabs toolbar horizontal single row top browser layout' },
  { id: 'sidebarWidth', section: 'appearance', label: 'Sidebar width', description: 'Width of the tab sidebar in the vertical layouts.', keywords: 'vertical tabs size wide narrow pixels' },
  { id: 'tabBarVisible', section: 'appearance', label: 'Show the tab bar', description: 'Off, the tabs are reached with Ctrl+Tab, the tab list and Quick commands.', keywords: 'tab bar show hide tabs visible' },
  { id: 'addressBarVisible', section: 'appearance', label: 'Show the address bar row', description: 'The whole row: navigation, the address field and the menu. Off, Ctrl+L brings it back until the address field loses focus.', keywords: 'address bar show hide toolbar row visible ctrl l' },
  { id: 'showTitleBar', section: 'appearance', label: 'Show the title bar when the tab bar is on a side', description: 'A row above the address bar with the page title and the window buttons. With the address bar along the bottom, the row is always there.', keywords: 'title bar window buttons caption vertical side page title' },
  { id: 'statusBar', section: 'appearance', label: 'Status bar', description: 'Bowser has no status bar of its own. The address of a link under the pointer shows in a small overlay at the bottom of the page; Hidden turns that off.', keywords: 'status bar info overlay link address hover hide' },
  { id: 'uiDensity', section: 'appearance', label: 'User interface density', description: 'Compact shortens the rows of the tab sidebar, the panels and Bowser’s own pages.', keywords: 'density regular compact rows spacing dense' },
  { id: 'compactMenus', section: 'appearance', label: 'Compact menu layout', description: 'Shorter rows in Bowser’s menus.', keywords: 'compact menus dense rows menu layout' },
  { id: 'uiZoom', section: 'appearance', label: 'User interface zoom', description: 'Scales the text of Bowser’s bars, panels and pages, on top of the font size. Select the value to type one.', keywords: 'ui zoom scale interface size percent slider text' },
  // --- end layout ---
  { id: 'theme', section: 'appearance', label: 'Theme', description: 'Helium colours is Bowser’s own palette. System uses your Windows accent colour. Light or dark follows the theme schedule.', keywords: 'dark light colour scheme helium system prefer system colour windows accent' },
  { id: 'themeAccent', section: 'appearance', label: 'Accent colour', description: 'Overrides the theme’s accent colour. Reset to use the default.', keywords: 'colour accent override custom theme reset' },
  { id: 'websiteAppearance', section: 'appearance', label: 'Website appearance', description: 'The colour scheme websites are asked to use: light, dark, or what Windows uses.', keywords: 'dark light auto mode prefers color scheme websites page theme' },
  { id: 'forceDark', section: 'appearance', label: 'Force dark theme on all websites', description: 'Darkens pages that have no dark theme of their own, in open tabs at once and in the pages you open next.', keywords: 'force dark mode night websites invert auto dark' },
  { id: 'privateTheme', section: 'appearance', label: 'Private tab theme', description: 'How Bowser’s own windows look while a private tab is shown: the same as normal, light or dark.', keywords: 'private incognito inprivate theme dark light window colour' },
  // --- theme options --- the schedule, custom colours, colouring mode, page accent, contrast and the theme file.
  ...THEME_ROWS,
  // --- end theme options ---
  { id: 'toolbarButtons', section: 'appearance', label: 'Customise your toolbar', description: 'Choose which buttons appear in the toolbar.', keywords: 'back forward reload bookmark newtab split reader site information buttons customize', notYet: true },
  { id: 'showHomeButton', section: 'appearance', label: 'Show the Home button', description: 'Adds a Home button to the side rail, beside the tabs.', keywords: 'home button toolbar rail sidebar' },
  { id: 'centredAddressBar', section: 'appearance', label: 'Centre the address bar', description: 'Keeps the address bar centred in the toolbar.', keywords: 'address bar centre centered position' },
  { id: 'minimalUrl', section: 'appearance', label: 'Show a simplified address while unfocused', description: 'Shows the page’s site and path, without the query or fragment, until you focus the address bar, then the full address.', keywords: 'url simplified host domain minimal address bar focus' },
  { id: 'showLoadProgress', section: 'appearance', label: 'Show a loading progress bar', description: 'A thin bar tracks page loads in the toolbar.', keywords: 'progress loading bar indicator' },
  { id: 'grouping', section: 'appearance', label: 'Group tabs by site', description: 'Tabs from one site share an island. Groups you make by hand stay.', keywords: 'automatic tab groups islands domain', restart: true },
  // --- wave-1 integration --- "Open new tabs next to the active tab" is Tabs > New tab position (the
  // same `new_tab_beside_active` key), and the Ctrl+Tab order and tab previews sit in Tabs > Tab bar
  // (TABS_SECTION_ROWS below).
  { id: 'splitEdgeDrop', section: 'appearance', label: 'Allow dropping a dragged tab on the window edge to split', description: 'Dragging a tab to the edge of the window opens it in a split view.', keywords: 'split view drag drop edge tab' },
  { id: 'pinNewTabGroups', section: 'appearance', label: 'Pin new tab groups to the bookmarks bar', description: 'A tab group formed from now on is pinned to the bookmarks bar. Needs “Show tab groups in the bookmarks bar”.', keywords: 'pin tab groups bookmarks bar' },
  { id: 'ctrlSVerticalTabs', section: 'keyboard', label: 'Ctrl+S collapses or expands the sidebar in the vertical layout', description: 'Only does anything in the vertical layout; otherwise Ctrl+S reaches the page as normal.', keywords: 'ctrl s sidebar collapse expand vertical layout shortcut' },
  // --- window and panels (thread T9) --- each row below acts at once; all sit behind More options. Not here, on purpose:
  // "open settings in a tab" and "open popups in tabs" (Bowser already always does), "show title bar" (Layout's
  // title row), "reset toolbars" (the toolbar editor) and the Windows and Reading list panels (Bowser has neither).
  { id: 'winAnimations', section: 'appearance', label: 'Use animation', description: 'Bowser’s own transitions and the panel slide. Off, they snap. Bowser also stays still when Windows has animations off.', keywords: 'animation motion transitions reduced motion prefers reduced motion slide' },
  { id: 'winRangeButtons', section: 'appearance', label: 'Buttons instead of sliders', description: 'Range controls on this page, such as the sidebar width and the interface zoom, show minus and plus buttons instead of a slider.', keywords: 'sliders range buttons stepper plus minus' },
  { id: 'winFade', section: 'appearance', label: 'Fade colours in background windows', description: 'Bowser’s bars and panels lose their colour while another window is in front.', keywords: 'fade colours background window inactive dim grey desaturate' },
  { id: 'winScrollbars', section: 'appearance', label: 'Simple scrollbars', description: 'Thin, quiet scrollbars in Bowser’s own pages and panels. Web pages keep theirs.', keywords: 'scrollbars simple thin slim overlay' },
  { id: 'winMonoIcons', section: 'appearance', label: 'Monochrome settings icons', description: 'The icons in this page’s list draw in plain text colour, without the accent.', keywords: 'monochrome icons settings colour accent plain' },
  { id: 'panelCloseButton', section: 'appearance', label: 'Show the close button on panels', description: 'The X in a panel’s header. Off, Escape and the panel’s rail button still close it.', keywords: 'panel close button x header' },
  { id: 'panelFolderCounts', section: 'appearance', label: 'Show folder counters', description: 'How many items a bookmarks folder holds, beside its name.', keywords: 'folder counters count number bookmarks panel items' },
  { id: 'panelAutoClose', section: 'appearance', label: 'Close floating panels when you click elsewhere', description: 'Bookmarks and Customize float over the page. Off, they stay until you close them.', keywords: 'floating panel auto close dismiss click outside overlay flyout' },
  { id: 'winReset', section: 'appearance', label: 'Reset window options', description: 'Puts animation, sliders, fading, scrollbars and icons back to their defaults.', keywords: 'reset window defaults restore' },
  { id: 'panelReset', section: 'appearance', label: 'Reset panel options', description: 'Puts the close button, folder counters and auto-close back to their defaults.', keywords: 'reset panels defaults restore' },
  // --- end window and panels ---
  { id: 'roundedContentFrame', section: 'appearance', label: 'Rounded corners around the page', description: 'Rounds the corners of the page area.', keywords: 'rounded corners content frame page' },
  // --- auto-hide (T11) --- the master switch is the old `frameless` key (core); the other two sit behind "More options".
  { id: 'frameless', section: 'appearance', label: 'Auto-hide toolbars and tabs', description: 'Hides the bars you choose until the pointer rests at the edge of the window. Ctrl+F11 turns it on or off.', keywords: 'auto hide autohide frameless toolbar tabs immersive fullscreen full screen bars edge', tier: 'more' },
  { id: 'autoHideBars', section: 'appearance', label: 'Bars to hide', description: 'The tab bar and address bar share the top edge in the horizontal layouts, so either one hides both. The side panel opens over the page instead of pushing it.', keywords: 'auto hide bars tabs address bookmarks panel status', tier: 'more' },
  { id: 'autoHideFullscreen', section: 'appearance', label: 'Always auto-hide in full screen', description: 'In full screen (F11, or a video) every bar hides, even with the switch above off.', keywords: 'auto hide full screen fullscreen f11 video', tier: 'more' },
  // --- end auto-hide ---
  { id: 'bookmarksBar', section: 'appearance', label: 'Bookmarks bar', description: 'Choose when the bookmarks bar is shown.', keywords: 'bookmarks bar show hide newtab always never' },
  { id: 'bookmarksBarTabGroups', section: 'appearance', label: 'Show tab groups in the bookmarks bar', description: 'Tab groups pinned to the bar appear after the bookmarks. Select one to go to it.', keywords: 'tab groups bookmarks bar islands' },
  ...ADDRESS_ROWS.filter((row) => row.section === 'appearance'),
  // --- layout (thread T6) --- the side panel's edge is the Panels choice of the "Tab bar, address bar and panels" row (layoutBars) above; it acts at once.
  { id: 'fontSize', section: 'appearance', label: 'Font size', description: 'The base text size across Bowser’s own pages.', keywords: 'font size small medium large text' },
  { id: 'uiFont', section: 'appearance', label: 'Customise fonts', description: 'A font family to use instead of the system font.', keywords: 'font family typeface custom ui' },
  { id: 'defaultZoom', section: 'appearance', label: 'Default zoom', description: 'The starting zoom for new pages, not the current page’s own zoom, which is set separately.', keywords: 'zoom default starting percent' },
  { id: 'showZoomIndicator', section: 'appearance', label: 'Show the zoom indicator', description: 'Shows the zoom level in the toolbar when it isn’t 100%.', keywords: 'zoom indicator toolbar percent' },
  { id: 'translateOffer', section: 'appearance', label: 'Offer to translate pages in other languages', description: 'Shows a button in the toolbar when a page is in another language. Nothing is sent until you click it. Pages are translated by Google Translate, so their text is sent to Google.', keywords: 'translate translation language foreign page' },
  { id: 'translateAuto', section: 'appearance', label: 'Translate automatically on sites set to always translate', description: 'Sites set to always translate are translated as soon as they load, which sends their text to Google Translate.', keywords: 'translate translation language foreign page automatic always' },
  { id: 'translateTarget', section: 'appearance', label: 'Translate into', description: '', keywords: 'translate translation language foreign page target windows' },
  { id: 'shiftRightClickContext', section: 'keyboard', label: 'Shift+Right Click shows the system menu', description: 'Shift+Right Click always shows the system menu over Bowser’s own tab and rail menus.', keywords: 'shift right click context menu system' },
  { id: 'ctrlShiftCCopyLink', section: 'keyboard', label: 'Ctrl+Shift+C copies the address', description: 'Ctrl+Shift+C copies the current page’s address.', keywords: 'ctrl shift c copy link address shortcut' },
  // The Tabs section's rows come from its own table (tab-options-logic.ts).
  ...TABS_SECTION_ROWS,
  // --- start page --- (docs/design/start-page.md)
  { id: 'startGroupReopen', section: 'startpage', label: 'Open the start page on', description: 'Which speed dial group a new tab shows first.', keywords: 'speed dial group last first new tab start page reopen' },
  { id: 'startTitles', section: 'startpage', label: 'Show titles', description: 'When a speed dial shows its name. When needed shows it under tiles without a chosen image, and on hover for the rest.', keywords: 'speed dial titles names always never needed labels' },
  { id: 'startAddButton', section: 'startpage', label: 'Show the add button', description: 'A tile at the end of each group for adding a site.', keywords: 'speed dial add button plus tile new site' },
  { id: 'startColumns', section: 'startpage', label: 'Maximum columns', description: 'The most speed dials in a row. Fewer show when the window is narrow.', keywords: 'speed dial columns row grid width' },
  { id: 'startThumbSize', section: 'startpage', label: 'Tile size', description: 'How large each speed dial tile is.', keywords: 'speed dial thumbnail size tiny small medium large huge tiles' },
  { id: 'startDragReorder', section: 'startpage', label: 'Drag to reorder', description: 'Drag speed dials to reorder them, or onto a folder to put them inside it. Alt and the arrow keys do the same.', keywords: 'speed dial drag drop reorder move sort' },
  { id: 'startHomepage', section: 'startpage', label: 'Homepage', description: 'The Home button opens this address, and so does a new tab when Tabs › New tab page is Homepage. Leave it empty to open the start page instead.', keywords: 'home page homepage address url' },
  { id: 'startupMode', section: 'startpage', label: 'When Bowser starts', description: 'What the first window opens with.', keywords: 'startup launch open start page last session where you left off homepage specific pages restore' },
  { id: 'startupPages', section: 'startpage', label: 'Pages to open at startup', description: 'One address per line, up to eight. Used when Bowser starts with specific pages.', keywords: 'startup pages specific urls tabs launch' },
  { id: 'startWidgets', section: 'startpage', label: 'Show the dashboard', description: 'Widgets under the speed dials: top sites, bookmarks, a sticky note, the date, privacy statistics and a web page.', keywords: 'widgets dashboard top sites sticky note date privacy statistics webpage' },
  { id: 'startWidgetColumns', section: 'startpage', label: 'Widget columns', description: 'The most widgets in a row, 1 to 4.', keywords: 'widgets columns row dashboard' },
  { id: 'startWidgetHeight', section: 'startpage', label: 'Widget height', description: 'Regular or tall widgets.', keywords: 'widgets height tall regular dashboard' },
  { id: 'startWidgetBg', section: 'startpage', label: 'Widget background', description: 'How a widget is filled: solid, semi-transparent, transparent or a colour you choose.', keywords: 'widgets background solid transparent translucent custom colour' },
  { id: 'startWidgetColor', section: 'startpage', label: 'Custom widget colour', description: 'Used when the widget background is Custom.', keywords: 'widgets colour color custom background' },
  { id: 'startWidgetDrag', section: 'startpage', label: 'Drag to rearrange widgets', description: 'Drag a widget by its handle, or press Alt and an arrow key on it.', keywords: 'widgets drag rearrange reorder move' },
  // --- end start page ---
  { id: 'blocker', section: 'privacy', label: 'Blocking level', description: 'None blocks nothing. Trackers stops tracking and measurement requests. Trackers and ads stops ads too. A site can have its own level.', keywords: 'adblock ad blocker shield trackers ads none level block' },
  { id: 'cookieNotices', section: 'privacy', label: 'Hide cookie notices', description: 'Hides cookie-consent banners on pages, with or without blocking. A site can switch it off.', keywords: 'cookie warning banner consent gdpr popup annoyance i don’t care about cookies easylist' },
  { id: 'blockYoutubeVideoAds', section: 'privacy', label: 'Block YouTube video ads', description: 'Skips in-video ads on YouTube where the blocking level is Trackers and ads, and in private tabs. At Trackers or None, YouTube ads still play.', keywords: 'youtube video ads block skip' },
  { id: 'allow', section: 'privacy', label: 'Sites the blocker skips', description: 'The blocker never blocks on these sites.', keywords: 'allow list exceptions whitelist adblock' },
  { id: 'tracking', section: 'privacy', label: 'Tracking prevention', description: 'Balanced is the default. Strict blocks more trackers but can break some sites.', keywords: 'privacy cookies strict balanced fingerprinting', restart: true },
  { id: 'history', section: 'privacy', label: 'Save browsing history', description: 'Pages you visit are listed in History and suggested in the address bar.', keywords: 'history visits record' },
  { id: 'historyRetention', section: 'privacy', label: 'Keep history for', description: 'Older visits are deleted from the history list. Session only clears it when Bowser closes.', keywords: 'history retention expire delete old days week month year forever session' },
  { id: 'siteRules', section: 'privacy', label: 'Sites with their own choices', description: 'Give one site its own blocking level or cookie-notice choice.', keywords: 'site exceptions per site level blocking cookie notices list shield' },
  { id: 'blockLists', section: 'privacy', label: 'Blocking lists', description: 'Switch lists on or off, add one from an address or a file, or remove one you added.', keywords: 'sources filter lists easylist easyprivacy subscription url file add cookie list' },
  { id: 'deleteData', section: 'privacy', label: 'Delete browsing data', description: 'Delete history, cookies, site storage, cached files, saved sign-ins, form autofill and site settings for a period you choose.', keywords: 'clear delete browsing data history cookies cache storage logins sign-ins autofill site settings hour day week' },
  { id: 'dashboard', section: 'privacy', label: 'Privacy dashboard', description: 'Review site permissions and blocking statistics.', keywords: 'clear cookies cache data permissions camera microphone location notifications statistics' },
  { id: 'efficiency', section: 'performance', label: 'Efficiency mode', description: 'Auto saves power on battery or in Battery Saver: hidden tabs sleep sooner and background tabs use less memory.', keywords: 'battery power eco energy saver laptop' },
  { id: 'sleep', section: 'performance', label: 'Sleep hidden tabs', description: 'Hidden tabs are put to sleep after a delay to free memory, sooner under memory pressure.', keywords: 'memory discard suspend hibernate inactive' },
  { id: 'sleepAfter', section: 'performance', label: 'Sleep after', description: 'How long a tab stays hidden before it sleeps.', keywords: 'delay timeout minutes memory' },
  ...ADDRESS_ROWS.filter((row) => row.section !== 'appearance'),
  ...PAGE_TOOLS_ROWS, // --- page tools (T7) ---
  // --- keyboard and quick commands (docs/commands.md) ---
  { id: 'kbEnabled', section: 'keyboard', label: 'Keyboard shortcuts', description: 'Turns the browser’s shortcuts on or off. Quick commands (Ctrl+Shift+K) keeps working, so you can always turn them back on.', keywords: 'keyboard shortcuts enable disable hotkeys keybindings keys' },
  { id: 'kbSingleKey', section: 'keyboard', label: 'Allow single-key shortcuts', description: 'Lets a letter, number or punctuation key be a shortcut on its own. They work only from Bowser’s own bars, never while you type in a page or a text field.', keywords: 'single key shortcuts letter number plain keys' },
  { id: 'kbAltMenu', section: 'keyboard', label: 'Alt opens the menu', description: 'Pressing and releasing Alt on its own opens the Bowser menu. Off, it does nothing.', keywords: 'alt key menu windows' },
  { id: 'kbFocusCycling', section: 'keyboard', label: 'F6 moves between the address bar and the page', description: 'F6 jumps from the address bar to the page, and back. Off, F6 always goes to the address bar.', keywords: 'f6 focus cycle address bar page tab' },
  { id: 'kbSites', section: 'keyboard', label: 'Prioritise website shortcuts on these sites', description: 'On a site listed here, shortcuts the page uses go to the page. The browser keeps the shortcuts marked Browser priority below. The “Prioritise website shortcuts on this site” command adds or removes the site you are on.', keywords: 'website shortcuts priority sites page keys web app gmail docs' },
  { id: 'kbEditor', section: 'keyboard', label: 'Shortcuts', description: 'Give a command several shortcuts, clear them or restore the defaults. Ctrl+F1 shows the cheat sheet. Editing shortcuts such as Ctrl+C and Ctrl+V cannot be taken.', keywords: 'shortcuts assign rebind remap clear restore reset cheat sheet ctrl f1 keybindings browser priority' },
  { id: 'qcTypes', section: 'commands', label: 'Result types', description: 'Choose which kinds of result Quick commands shows and in what order. Type bkm:, tb:, cmd: and similar before a search to look in one place. Shift or the right arrow completes the top result.', keywords: 'quick commands palette results types order prefix bookmark note tab history command action workspace calculator' },
  { id: 'qcMax', section: 'commands', label: 'Visible results', description: 'The most results shown for a typed search.', keywords: 'quick commands palette results limit maximum number' },
  { id: 'qcKeepLast', section: 'commands', label: 'Keep the last typed search', description: 'Quick commands opens with what you typed last time, selected, so the first key you press replaces it.', keywords: 'quick commands palette remember last typed value query' },
  { id: 'qcNewTab', section: 'commands', label: 'Open links in a new tab', description: 'A bookmark or history entry picked in Quick commands opens in a new tab, not the one you are on.', keywords: 'quick commands palette new tab bookmark history open link' },
  { id: 'qcNotesPanel', section: 'commands', label: 'Open notes in the Notes panel', description: 'Picking a note opens the Notes panel; the panel does not yet go to the picked note. Off, it copies the note’s text instead.', keywords: 'quick commands palette notes manager panel copy' },
  { id: 'qcNickname', section: 'commands', label: 'Open a bookmark when its nickname is typed', description: 'Typing a bookmark’s whole nickname in Quick commands opens it at once, without pressing Enter. Give a bookmark a nickname in its bookmark dialog.', keywords: 'quick commands palette bookmark nickname automatic open match keyword' },
  { id: 'chains', section: 'commands', label: 'Command chains', description: 'Run several commands one after another, with a delay in milliseconds before each. Run a chain from Quick commands, give it a shortcut in Keyboard, or test it here.', keywords: 'command chains macro sequence delay milliseconds test clone create edit delete automation' },
  // --- end keyboard and quick commands ---
  { id: 'downloadDir', section: 'downloads', label: 'Location', description: '', keywords: 'download folder directory save path where change' },
  { id: 'downloadAsk', section: 'downloads', label: 'Ask where to save each file before downloading', description: 'When this is off, files go straight to the location above.', keywords: 'save as prompt dialog ask where download location' },
  { id: 'media', section: 'media', label: 'Media support', description: 'Runs on this device only and sends nothing.', keywords: 'video codec h264 vp9 av1 hevc widevine playready drm netflix hardware decode' },
  { id: 'extensions', section: 'extensions', label: 'Installed extensions', description: 'Turn extensions on or off, open a popup, see what each one declares, or remove one.', keywords: 'extension add-on addon plugin unpacked crx file manifest permissions popup install remove uninstall disable enable' },
  { id: 'autoUpdate', section: 'updates', label: 'Update automatically', description: 'Checks in the background and installs the update the next time Bowser starts. Applies to every profile.', keywords: 'auto update upgrade version' },
  { id: 'updateStatus', section: 'updates', label: 'Bowser updates', description: '', keywords: 'check for updates restart version upgrade' },
  { id: 'version', section: 'about', label: 'Version', description: '', keywords: 'about build release' },
  { id: 'file', section: 'about', label: 'Settings file', description: 'Every setting here is saved in this profile’s settings.txt, which you can also edit by hand.', keywords: 'settings.txt advanced edit text' },
  { id: 'reset', section: 'about', label: 'Reset settings', description: 'Puts layout, appearance, tabs, start page, address bar, search engines, bookmarks bar, blocking and website appearance, performance, downloads and translation back to their defaults. Not changed: how long history is kept, your history and bookmarks, Keyboard and Quick commands, extensions, site permissions and updates.', keywords: 'defaults restore factory start over' },
  { id: 'defaultBrowser', section: 'about', label: 'Default browser', description: 'Open web links from other apps in Bowser.', keywords: 'default browser make bowser windows handler http https' },
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
    const section = SECTIONS.find((spec) => spec.key === row.section)?.title ?? '';
    const hay = fold(`${row.label} ${row.description} ${row.keywords} ${section}`);
    if (words.every((word) => hay.includes(word))) hits.add(row.id);
  }
  return hits;
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
  if (site === null) return { error: 'Enter a site such as example.com.' };
  if (list.includes(site)) return { error: `${site} is already on the list.` };
  if (list.length >= MAX_ALLOW) return { error: `The list holds at most ${MAX_ALLOW} sites.` };
  return { list: [...list, site] };
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
  if (s.update === 'unavailable') return { title: 'Updates are off for this copy', detail: 'Development and benchmark launches never update.', action: null, busy: false, tone: 'info' };
  if (s.update === 'checking') return { title: 'Checking for updates…', detail: `Bowser ${s.version}`, action: null, busy: true, tone: 'info' };
  if (s.update === 'ready') {
    const which = s.updateVersion !== '' ? `Bowser ${s.updateVersion}` : 'An update';
    return { title: `${which} is ready`, detail: 'Restart to install it. Your tabs come back after the restart.', action: 'restart', busy: false, tone: 'ok' };
  }
  const result: Record<CheckResult, string> = {
    none: '',
    staged: 'Update downloaded.',
    uptodate: 'Bowser is up to date.',
    unverified: 'A newer version could not be verified, so it was not downloaded.',
    busy: 'A check is already running.',
    unavailable: 'Development builds do not update.',
    failed: 'The update check failed. Try again later.',
  };
  if (s.checkResult !== 'none') {
    const warn = s.checkResult === 'failed' || s.checkResult === 'unverified';
    return { title: result[s.checkResult], detail: `Bowser ${s.version}`, action: 'check', busy: false, tone: warn ? 'warn' : 'ok' };
  }
  const title = s.lastCheck === 'uptodate' ? 'Bowser is up to date' : s.lastCheck === 'failed' ? 'The last update check failed' : `Bowser ${s.version}`;
  const detail = s.autoUpdate ? 'Bowser checks for updates about once an hour.' : 'Automatic updates are off. Check by hand.';
  return { title, detail, action: 'check', busy: false, tone: s.lastCheck === 'failed' ? 'warn' : 'ok' };
}

