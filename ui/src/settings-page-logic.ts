// Pure logic for the settings page (?view=settings, docs/design/settings-page.md): the snapshot
// contract, the exact-match save payload, the section and row index the nav and the search use,
// and the words that depend on state. No DOM here; test/settings-page.mjs covers all of it.
import { MAX_TEXT, SETTINGS_SECTIONS, type SettingsSection } from './protocol';

export { SETTINGS_SECTIONS, type SettingsSection };

const SEARCH_ENGINES = ['optimised', 'google', 'duckduckgo'] as const;
export type SearchEngine = typeof SEARCH_ENGINES[number];
export const EFFICIENCY_MODES = ['auto', 'on', 'off'] as const;
export type EfficiencyMode = typeof EFFICIENCY_MODES[number];
export type TrackingPrevention = 'balanced' | 'strict';
/** Browser layouts, in the order the page lists them. `horizontal` is the saved key of "toolbar above tabs". */
export const TAB_LAYOUTS = ['horizontal', 'tabs-top', 'single-row', 'vertical'] as const;
export type TabLayout = typeof TAB_LAYOUTS[number];
export const TAB_LAYOUT_LABELS: Readonly<Record<TabLayout, string>> = {
  horizontal: 'Toolbar above tabs',
  'tabs-top': 'Tabs above toolbar',
  'single-row': 'Single row',
  vertical: 'Vertical',
};
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
export const BOOKMARKS_BAR_MODES = ['always', 'never', 'newtab'] as const;
export type BookmarksBarMode = typeof BOOKMARKS_BAR_MODES[number];
export const SIDE_PANEL_POSITIONS = ['left', 'right'] as const;
export type SidePanelPosition = typeof SIDE_PANEL_POSITIONS[number];
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
  frameless: boolean;
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
  /** `''` means the Windows display language, else a 2-3 letter lower-case code. Its own command. */
  translateTarget: string;
  /** The codes `translateTarget` may be set to, in the order the page lists them. */
  translateTargets: string[];
}

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isIntBetween = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
const isStrUpTo = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);
const KEYS = ['tabLayout', 'sidebarWidth', 'sleepEnabled', 'sleepAfterSecs', 'blockerEnabled', 'blockerAllow', 'searchEngine', 'searchFallback',
  'automaticTabGrouping', 'efficiencyMode', 'efficiencyActive', 'trackingPrevention', 'autoUpdate', 'historyEnabled', 'guest', 'profile', 'avatar',
  'version', 'update', 'updateVersion', 'lastCheck', 'checkResult', 'ack', 'error', 'readable',
  'theme', 'themeAccent', 'toolbarButtons', 'centredAddressBar', 'minimalUrl', 'showLoadProgress', 'frameless', 'roundedContentFrame', 'showHomeButton',
  'bookmarksBar', 'bookmarksBarTabGroups', 'sidePanelPosition', 'tabPreviewImages', 'tabPreviewMemory', 'fontSize', 'uiFont', 'defaultZoom',
  'showZoomIndicator', 'newTabBesideActive', 'ctrlTabMru', 'shiftRightClickContext', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs', 'pinNewTabGroups',
  'splitEdgeDrop', 'blockYoutubeVideoAds', 'defaultBrowser', 'downloadDir', 'downloadAsk',
  'translateOffer', 'translateAuto', 'translateTarget', 'translateTargets'];
/** A language code the translation settings carry: two or three lower-case letters. */
const LANGUAGE_RE = /^[a-z]{2,3}$/;
/** The most target languages Rust may list. */
export const MAX_TRANSLATE_TARGETS = 64;
/** English names of the languages Bowser can translate into; a code not listed here shows as itself. */
const LANGUAGE_NAMES: Readonly<Record<string, string>> = {
  en: 'English', de: 'German', es: 'Spanish', fr: 'French', it: 'Italian', nl: 'Dutch', pl: 'Polish', pt: 'Portuguese', tr: 'Turkish',
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
  };
}

/** The profile settings `save` carries (everything else in the snapshot is read-only). */
export type Draft = Pick<SettingsSnapshot,
  | 'tabLayout' | 'sidebarWidth' | 'sleepEnabled' | 'sleepAfterSecs' | 'blockerEnabled' | 'blockerAllow' | 'searchEngine' | 'automaticTabGrouping'
  | 'efficiencyMode' | 'trackingPrevention' | 'theme' | 'themeAccent' | 'toolbarButtons' | 'centredAddressBar' | 'minimalUrl' | 'showLoadProgress'
  | 'frameless' | 'roundedContentFrame' | 'showHomeButton' | 'bookmarksBar' | 'bookmarksBarTabGroups' | 'sidePanelPosition' | 'tabPreviewImages'
  | 'tabPreviewMemory' | 'fontSize' | 'uiFont' | 'defaultZoom' | 'showZoomIndicator' | 'newTabBesideActive' | 'ctrlTabMru' | 'shiftRightClickContext'
  | 'ctrlShiftCCopyLink' | 'ctrlSVerticalTabs' | 'pinNewTabGroups' | 'splitEdgeDrop' | 'blockYoutubeVideoAds'
>;

export function draftOf(snapshot: SettingsSnapshot): Draft {
  const {
    tabLayout, sidebarWidth, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow, searchEngine, automaticTabGrouping, efficiencyMode,
    trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless, roundedContentFrame,
    showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont, defaultZoom,
    showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups, splitEdgeDrop,
    blockYoutubeVideoAds,
  } = snapshot;
  return {
    tabLayout, sidebarWidth, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow: [...blockerAllow], searchEngine, automaticTabGrouping,
    efficiencyMode, trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless,
    roundedContentFrame, showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont,
    defaultZoom, showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups,
    splitEdgeDrop, blockYoutubeVideoAds,
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
  const text = lines.join('\n');
  return text.length <= MAX_TEXT ? text : null;
}

// ---- Sections and rows (the nav, the search index and the deep links) ----

export interface SectionSpec { readonly key: SettingsSection; readonly title: string; readonly nav: string; readonly lead: string }
export const SECTIONS: readonly SectionSpec[] = [
  { key: 'profile', title: 'You and Bowser', nav: 'You and Bowser', lead: 'The profile these settings belong to.' },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: 'Layout, colours, the toolbar, tabs, text size and page translation.' },
  { key: 'privacy', title: 'Privacy and security', nav: 'Privacy and security', lead: 'Blocking, tracking prevention, history, site permissions and browsing data.' },
  { key: 'performance', title: 'Performance', nav: 'Performance', lead: 'Save memory and power in the background.' },
  { key: 'search', title: 'Search engine', nav: 'Search engine', lead: 'What the address bar searches with.' },
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
}

/** The note under a row whose change waits for a restart. */
export const RESTART_NOTE = 'Applies after restart';
/** The note under a row (or a whole group of rows) Bowser saves but does not act on yet. */
export const NOT_YET_NOTE = 'Has no effect yet. Bowser keeps your choice for when it does.';
export const NOT_YET_GROUP_NOTE = 'These have no effect yet. Bowser keeps your choices for when they do.';

/** Every row the page draws, in page order. The view builds a control per id. */
export const ROWS: readonly RowSpec[] = [
  { id: 'profile', section: 'profile', label: 'Profile', description: 'Settings on this page apply to this profile only.', keywords: 'account person name avatar guest' },
  { id: 'profiles', section: 'profile', label: 'Manage profiles', description: 'Switch, create, rename or delete profiles.', keywords: 'switch new guest delete rename' },
  { id: 'import', section: 'profile', label: 'Import from another browser', description: 'Bring bookmarks and other data over from a browser on this PC.', keywords: 'chrome edge firefox migrate transfer bookmarks' },
  { id: 'layout', section: 'appearance', label: 'Browser layout', description: 'Where the toolbar and the tabs sit.', keywords: 'tabs toolbar vertical horizontal sidebar single row top' },
  { id: 'sidebarWidth', section: 'appearance', label: 'Sidebar width', description: 'Width of the tab sidebar in the vertical layout.', keywords: 'vertical tabs size wide narrow pixels' },
  { id: 'theme', section: 'appearance', label: 'Theme', description: 'Helium colours is Bowser’s own palette. System follows Windows light, dark and accent.', keywords: 'dark light colour scheme helium system' },
  { id: 'themeAccent', section: 'appearance', label: 'Accent colour', description: 'Overrides the theme’s accent colour. Reset to use the default.', keywords: 'colour accent override custom theme reset' },
  { id: 'toolbarButtons', section: 'appearance', label: 'Customise your toolbar', description: 'Choose which buttons appear in the toolbar.', keywords: 'back forward reload bookmark newtab split reader site information buttons customize' },
  { id: 'showHomeButton', section: 'appearance', label: 'Show the Home button', description: 'Adds a Home button to the toolbar.', keywords: 'home button toolbar' },
  { id: 'centredAddressBar', section: 'appearance', label: 'Centre the address bar', description: 'Keeps the address bar centred in the toolbar.', keywords: 'address bar centre centered position' },
  { id: 'minimalUrl', section: 'appearance', label: 'Show a simplified address while unfocused', description: 'Shows the page’s registrable host until you focus the address bar, then the full address.', keywords: 'url simplified host domain minimal address bar focus' },
  { id: 'showLoadProgress', section: 'appearance', label: 'Show a loading progress bar', description: 'A thin bar tracks page loads in the toolbar.', keywords: 'progress loading bar indicator' },
  { id: 'grouping', section: 'appearance', label: 'Group tabs by site', description: 'Tabs from one site share an island. Groups you make by hand stay.', keywords: 'automatic tab groups islands domain', restart: true },
  { id: 'newTabBesideActive', section: 'appearance', label: 'Open new tabs next to the active tab', description: 'A new tab opens right after the tab you’re on, not at the end.', keywords: 'new tab position beside active next' },
  { id: 'ctrlTabMru', section: 'appearance', label: 'Cycle tabs most-recently-used first with Ctrl+Tab', description: 'Ctrl+Tab steps through tabs in the order you last used them.', keywords: 'ctrl tab cycle most recently used mru switch' },
  { id: 'splitEdgeDrop', section: 'appearance', label: 'Allow dropping a dragged tab on the window edge to split', description: 'Dragging a tab to the edge of the window opens it in a split view.', keywords: 'split view drag drop edge tab' },
  { id: 'tabPreviewImages', section: 'appearance', label: 'Show a thumbnail in tab previews', description: 'Tab hover previews include a snapshot of the page.', keywords: 'tab preview thumbnail hover image', notYet: true },
  { id: 'tabPreviewMemory', section: 'appearance', label: 'Show memory use in tab previews', description: 'Tab hover previews show how much memory the tab is using.', keywords: 'tab preview memory usage hover', notYet: true },
  { id: 'pinNewTabGroups', section: 'appearance', label: 'Pin new tab groups to the bookmarks bar', description: 'New tab groups are pinned to the bookmarks bar.', keywords: 'pin tab groups bookmarks bar', notYet: true },
  { id: 'ctrlSVerticalTabs', section: 'appearance', label: 'Ctrl+S collapses or expands the sidebar in the vertical layout', description: 'Only does anything in the vertical layout; otherwise Ctrl+S reaches the page as normal.', keywords: 'ctrl s sidebar collapse expand vertical layout shortcut', notYet: true },
  { id: 'roundedContentFrame', section: 'appearance', label: 'Rounded corners around the page', description: 'Rounds the corners of the page area.', keywords: 'rounded corners content frame page' },
  { id: 'frameless', section: 'appearance', label: 'Frameless window', description: 'Hides the browser’s toolbar and tabs until the pointer nears the top of the window.', keywords: 'frameless hide toolbar tabs immersive', notYet: true },
  { id: 'bookmarksBar', section: 'appearance', label: 'Bookmarks bar', description: 'Choose when the bookmarks bar is shown.', keywords: 'bookmarks bar show hide newtab always never', notYet: true },
  { id: 'bookmarksBarTabGroups', section: 'appearance', label: 'Show tab groups in the bookmarks bar', description: 'Tab groups appear alongside bookmarks in the bar.', keywords: 'tab groups bookmarks bar islands', notYet: true },
  { id: 'sidePanelPosition', section: 'appearance', label: 'Side panel position', description: 'Which edge the side panels (history, bookmarks, notes and more) are docked to.', keywords: 'side panel left right dock position helium', restart: true },
  { id: 'fontSize', section: 'appearance', label: 'Font size', description: 'The base text size across Bowser’s own pages.', keywords: 'font size small medium large text' },
  { id: 'uiFont', section: 'appearance', label: 'Customise fonts', description: 'A font family to use instead of the system font.', keywords: 'font family typeface custom ui' },
  { id: 'defaultZoom', section: 'appearance', label: 'Default zoom', description: 'The starting zoom for new pages, not the current page’s own zoom, which is set separately.', keywords: 'zoom default starting percent' },
  { id: 'showZoomIndicator', section: 'appearance', label: 'Show the zoom indicator', description: 'Shows the zoom level in the toolbar when it isn’t 100%.', keywords: 'zoom indicator toolbar percent' },
  { id: 'translateOffer', section: 'appearance', label: 'Offer to translate pages in other languages', description: 'Shows a button in the toolbar. Nothing is downloaded or changed until you click it. Translation runs on this device; page text never leaves it.', keywords: 'translate translation language foreign page' },
  { id: 'translateAuto', section: 'appearance', label: 'Translate automatically on sites set to always translate', description: 'Only once the language files are already on this device.', keywords: 'translate translation language foreign page automatic always' },
  { id: 'translateTarget', section: 'appearance', label: 'Translate into', description: '', keywords: 'translate translation language foreign page target windows' },
  { id: 'shiftRightClickContext', section: 'appearance', label: 'Shift+Right Click shows the system menu', description: 'Shift+Right Click always shows the system menu over Bowser’s own tab and rail menus.', keywords: 'shift right click context menu system', notYet: true },
  { id: 'ctrlShiftCCopyLink', section: 'appearance', label: 'Ctrl+Shift+C copies the address', description: 'Ctrl+Shift+C copies the current page’s address.', keywords: 'ctrl shift c copy link address shortcut', notYet: true },
  { id: 'blocker', section: 'privacy', label: 'Block ads and trackers', description: 'Stops known ad and tracking requests before they load.', keywords: 'adblock ad blocker shield trackers', restart: true },
  { id: 'blockYoutubeVideoAds', section: 'privacy', label: 'Block YouTube video ads', description: 'Skips in-video ads on YouTube.', keywords: 'youtube video ads block skip' },
  { id: 'allow', section: 'privacy', label: 'Sites the blocker skips', description: 'The blocker never blocks on these sites.', keywords: 'allow list exceptions whitelist adblock', restart: true },
  { id: 'tracking', section: 'privacy', label: 'Tracking prevention', description: 'Balanced is the default. Strict blocks more trackers but can break some sites.', keywords: 'privacy cookies strict balanced fingerprinting', restart: true },
  { id: 'history', section: 'privacy', label: 'Save browsing history', description: 'Pages you visit are listed in History and suggested in the address bar.', keywords: 'history visits record' },
  { id: 'dashboard', section: 'privacy', label: 'Privacy dashboard', description: 'Clear browsing data, review site permissions and blocking statistics.', keywords: 'clear cookies cache data permissions camera microphone location notifications statistics' },
  { id: 'efficiency', section: 'performance', label: 'Efficiency mode', description: 'Auto saves power on battery or in Battery Saver: hidden tabs sleep sooner and background tabs use less memory.', keywords: 'battery power eco energy saver laptop' },
  { id: 'sleep', section: 'performance', label: 'Sleep hidden tabs', description: 'Hidden tabs are put to sleep after a delay to free memory, sooner under memory pressure.', keywords: 'memory discard suspend hibernate inactive' },
  { id: 'sleepAfter', section: 'performance', label: 'Sleep after', description: 'How long a tab stays hidden before it sleeps.', keywords: 'delay timeout minutes memory' },
  { id: 'engine', section: 'search', label: 'Search engine', description: 'Used for searches typed in the address bar. Type img before a query for images.', keywords: 'google duckduckgo optimised default address bar omnibox images' },
  { id: 'downloadDir', section: 'downloads', label: 'Location', description: '', keywords: 'download folder directory save path where change' },
  { id: 'downloadAsk', section: 'downloads', label: 'Ask where to save each file before downloading', description: 'When this is off, files go straight to the location above.', keywords: 'save as prompt dialog ask where download location' },
  { id: 'media', section: 'media', label: 'Media support', description: 'Runs on this device only and sends nothing.', keywords: 'video codec h264 vp9 av1 hevc widevine playready drm netflix hardware decode' },
  { id: 'extensions', section: 'extensions', label: 'Installed extensions', description: 'Turn extensions on or off, open a popup, see what each one declares, or remove one.', keywords: 'extension add-on addon plugin unpacked crx file manifest permissions popup install remove uninstall disable enable' },
  { id: 'autoUpdate', section: 'updates', label: 'Update automatically', description: 'Checks in the background and installs the update the next time Bowser starts. Applies to every profile.', keywords: 'auto update upgrade version' },
  { id: 'updateStatus', section: 'updates', label: 'Bowser updates', description: '', keywords: 'check for updates restart version upgrade' },
  { id: 'version', section: 'about', label: 'Version', description: '', keywords: 'about build release' },
  { id: 'file', section: 'about', label: 'Settings file', description: 'Every setting here is saved in this profile’s settings.txt, which you can also edit by hand.', keywords: 'settings.txt advanced edit text' },
  { id: 'reset', section: 'about', label: 'Reset settings', description: 'Puts layout, tabs, blocking, performance and search back to their defaults. History, updates and bookmarks are not changed.', keywords: 'defaults restore factory start over' },
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

export const LAYOUT_HINTS: Readonly<Record<TabLayout, string>> = {
  horizontal: 'The address bar on top, tabs underneath.',
  'tabs-top': 'Tabs on top, the address bar underneath.',
  'single-row': 'Everything in one row, for small screens.',
  vertical: 'Tabs in a sidebar on the left.',
};
