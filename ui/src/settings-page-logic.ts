import { MAX_SETTINGS_SAVE, SETTINGS_SECTIONS, utf8Bytes, type SettingsSection } from './protocol';
import { localNameError } from './composer-logic';
import { parseStartSettings, startSaveLines, type StartSettings } from './start-page-logic';
import { ADDRESS_ROWS } from './address-settings-logic';
import { PAGE_TOOLS_CORE, PAGE_TOOLS_ROWS } from './page-tools-logic';
import { cardSpecs, parseTabOptions, tabOptionRows, type TabOptionValues } from './tab-options-logic';
import { THEME_ROWS, parseThemeOptions, type ThemeOptionValues } from './theme-options-logic';
import { ADDRESS_ITEMS, DEFAULT_ADDRESS, itemsOf, validItems } from './bars-logic';
import { FEED_ROWS, REFRESH_CHOICES } from './feeds-settings-logic';

export { SETTINGS_SECTIONS, type SettingsSection };

const SEARCH_ENGINES = ['optimised', 'google', 'duckduckgo'] as const;
export type SearchEngine = typeof SEARCH_ENGINES[number];
export const EFFICIENCY_MODES = ['auto', 'on', 'off'] as const;
export type EfficiencyMode = typeof EFFICIENCY_MODES[number];
export const EFFICIENCY_LABELS: Readonly<Record<EfficiencyMode, string>> = { auto: 'Auto', on: 'On', off: 'Off' };
export const EFFICIENCY_ARIA: Readonly<Record<EfficiencyMode, string>> = { auto: 'On battery or with Battery saver on', on: 'Always', off: 'Never' };
export type TrackingPrevention = 'balanced' | 'strict';
import {
  LAYOUT_HINTS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_ARIA, TAB_LAYOUT_LABELS, layoutSaveLines, parseLayoutSettings,
  type LayoutSettings, type SidePanelPosition, type TabLayout,
} from './layout-logic';
export { LAYOUT_HINTS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_ARIA, TAB_LAYOUT_LABELS, type LayoutSettings, type SidePanelPosition, type TabLayout };
import { parseWindowSettings, windowSaveLines, type WindowSettings } from './window-logic';
export type { WindowSettings };
export const MIN_SIDEBAR = 180;
export const MAX_SIDEBAR = 360;
export const MIN_SLEEP = 30;
export const MAX_SLEEP = 604800;
export const MAX_ALLOW = 500;
export const MAX_ALLOW_BYTES = 20 * 1024;
export function allowFits(list: readonly string[]): boolean {
  return utf8Bytes(list.join(',')) <= MAX_ALLOW_BYTES;
}

export const THEME_MODES = ['helium', 'system'] as const;
export type ThemeMode = typeof THEME_MODES[number];
export const TOOLBAR_BUTTONS = ADDRESS_ITEMS;
export type ToolbarButton = typeof TOOLBAR_BUTTONS[number];
export const DEFAULT_TOOLBAR_BUTTONS = DEFAULT_ADDRESS.join(',');
export const AUTO_HIDE_BARS = ['tabs', 'address', 'bookmarks', 'panel'] as const;
export type AutoHideBar = typeof AUTO_HIDE_BARS[number];
export const AUTO_HIDE_BAR_LABELS: Readonly<Record<AutoHideBar, string>> = {
  tabs: 'Tab bar',
  address: 'Address bar',
  bookmarks: 'Bookmarks bar',
  panel: 'Panel',
};
export const DEFAULT_AUTO_HIDE_BARS = 'tabs,address,bookmarks';
export function autoHideBarsList(text: string): AutoHideBar[] {
  return text === '' ? [] : (text.split(',') as AutoHideBar[]);
}
export function autoHideBarsText(bars: Iterable<AutoHideBar>): string {
  const set = new Set(bars);
  return AUTO_HIDE_BARS.filter((bar) => set.has(bar)).join(',');
}
function isAutoHideBarsText(value: unknown): value is string {
  return typeof value === 'string' && autoHideBarsText(autoHideBarsList(value)) === value && autoHideBarsList(value).every((bar) => (AUTO_HIDE_BARS as readonly string[]).includes(bar));
}
export const BOOKMARKS_BAR_MODES = ['always', 'never', 'newtab'] as const;
export type BookmarksBarMode = typeof BOOKMARKS_BAR_MODES[number];
export const FONT_SIZES = ['small', 'medium', 'large'] as const;
export type FontSize = typeof FONT_SIZES[number];
export const DEFAULT_BROWSER_STATES = ['yes', 'no', 'unknown'] as const;
export type DefaultBrowserState = typeof DEFAULT_BROWSER_STATES[number];
export const SPOILER_MODES = ['lockdown', 'strict', 'balanced'] as const;
export type SpoilerMode = typeof SPOILER_MODES[number];
export const SPOILER_MODE_LABELS: Readonly<Record<SpoilerMode, string>> = { lockdown: 'Lockdown', strict: 'Strict', balanced: 'Balanced' };
export const SPOILER_MODE_HINTS: Readonly<Record<SpoilerMode, string>> = {
  lockdown: 'Hides GTA VI mentions in posts and video details, including their attached media. May also hide harmless content.',
  strict: 'Checks posts, comments, video titles, descriptions and captions for GTA VI spoiler language. Recommended.',
  balanced: 'Hides clear spoilers only. A plain mention of the game, or a spoiler written in code words, can get through.',
};
export const SPOILER_LIMITS_NOTE = 'Checks written posts, comments, video titles, descriptions and accessible captions. Image pixels, video frames and audio are not scanned; image-only spoilers and coded wording can get through. Unofficial; not affiliated with Rockstar Games or Take-Two.';
export const SPOILER_OFF_WARNING = 'GTA VI spoilers will no longer be hidden on any page, including private tabs and guest windows. Pages already open are not changed.';
export const MIN_ZOOM = 25;
export const MAX_ZOOM = 500;
export const MAX_UI_FONT = 64;
const ACCENT_RE = /^#[0-9a-f]{6}$/;
const CONTROL_CHAR_RE = /[\u0000-\u001f\u007f]/;

export function toolbarButtonsList(text: string): string[] {
  return text === '' ? [] : text.split(',');
}

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
const ACKS = ['none', 'saved', 'failed', 'note'] as const;
export type UpdateState = typeof UPDATE_STATES[number];
export type LastCheck = typeof LAST_CHECKS[number];
export type CheckResult = typeof CHECK_RESULTS[number];
export type Ack = typeof ACKS[number];

export interface SettingsSnapshot {
  tabLayout: TabLayout;
  sidebarWidth: number;
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
  themeAccent: string;
  toolbarButtons: string;
  centredAddressBar: boolean;
  minimalUrl: boolean;
  showLoadProgress: boolean;
  frameless: boolean;
  autoHideBars: string;
  autoHideFullscreen: boolean;
  roundedContentFrame: boolean;
  showHomeButton: boolean;
  bookmarksBar: BookmarksBarMode;
  bookmarksBarTabGroups: boolean;
  sidePanelPosition: SidePanelPosition;
  tabPreviewImages: boolean;
  tabPreviewMemory: boolean;
  fontSize: FontSize;
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
  spoilerProtection: boolean;
  spoilerMode: SpoilerMode;
  spoilerReducedEffects: boolean;
  spoilerOcr: boolean;
  spoilerFeed: boolean;
  spoilerFeedStatus: string;
  efficiencyActive: boolean;
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
  defaultBrowser: DefaultBrowserState;
  downloadDir: string;
  downloadAsk: boolean;
  translateOffer: boolean;
  translateAuto: boolean;
  tabOptions: TabOptionValues;
  themeOptions: ThemeOptionValues;
  translateTarget: string;
  translateTargets: string[];
  start: StartSettings;
  workspaceRules: WorkspaceRules;
  profiles: ProfileList;
  layout: LayoutSettings;
  window: WindowSettings;
  bars: BarsSettings;
}

export interface BarsSettings {
  tabs: string;
  panel: string;
  feedsDetect: boolean;
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

export const PROFILE_AVATARS = ['initial', 'leaf', 'star', 'wave', 'moon'] as const;
export type ProfileAvatar = typeof PROFILE_AVATARS[number];
export const PROFILE_ACCENTS = ['blue', 'violet', 'teal', 'amber', 'rose'] as const;
export type ProfileAccent = typeof PROFILE_ACCENTS[number];
export const PROFILE_AVATAR_LABELS: Readonly<Record<ProfileAvatar, string>> = { initial: 'Initial', leaf: 'Leaf', star: 'Star', wave: 'Wave', moon: 'Moon' };
export const PROFILE_ACCENT_LABELS: Readonly<Record<ProfileAccent, string>> = { blue: 'Blue', violet: 'Violet', teal: 'Teal', amber: 'Amber', rose: 'Rose' };
export const MAX_PROFILES = 300;
export const MAX_PROFILE_NAME = 32;
export interface ProfileEntry {
  readonly id: string;
  readonly name: string;
  readonly glyph: string;
  readonly colour: string;
  readonly avatar: ProfileAvatar;
  readonly accent: ProfileAccent;
  readonly current: boolean;
}
export interface ProfileList { readonly list: readonly ProfileEntry[]; readonly avatars: readonly string[]; readonly accents: readonly string[] }
const PROFILE_ID_RE = /^(?:default|[A-Za-z0-9_-]{1,32})$/;
const COLOUR_RE = /^#[0-9a-f]{6}$/;
const HIDDEN_RE = /[\u200b-\u200f\u202a-\u202e\u2066-\u2069\ufeff]/;
export function profileNameError(name: string, profiles: readonly ProfileEntry[], exceptId?: string): string {
  const local = localNameError(name, MAX_PROFILE_NAME);
  if (local !== '') return local;
  if (HIDDEN_RE.test(name)) return 'Names cannot contain invisible characters.';
  const wanted = name.trim().toLowerCase();
  return profiles.some((p) => p.id !== exceptId && p.name.toLowerCase() === wanted) ? 'Another profile already uses that name.' : '';
}
export function parseProfiles(value: unknown): ProfileList | null {
  if (!isRecord(value) || Object.keys(value).length !== 3 || !Array.isArray(value.list) || value.list.length > MAX_PROFILES) return null;
  const { avatars, accents } = value;
  const looks = (raw: unknown, size: number, ok: (item: unknown) => boolean): raw is string[] => Array.isArray(raw) && (raw.length === 0 || raw.length === size) && raw.every(ok);
  if (!looks(avatars, PROFILE_AVATARS.length, (g) => isStrUpTo(g, 4) && g !== '') || !looks(accents, PROFILE_ACCENTS.length, (c) => typeof c === 'string' && COLOUR_RE.test(c))) return null;
  const list: ProfileEntry[] = [];
  const ids = new Set<string>();
  for (const raw of value.list as unknown[]) {
    if (!isRecord(raw)) return null;
    const p = raw;
    const keys = Object.keys(p);
    if (keys.length !== 7 || !['id', 'name', 'glyph', 'colour', 'avatar', 'accent', 'current'].every((key) => keys.includes(key))) return null;
    if (typeof p.id !== 'string' || !PROFILE_ID_RE.test(p.id) || ids.has(p.id.toLowerCase())) return null;
    if (typeof p.name !== 'string' || p.name !== p.name.trim() || profileNameError(p.name, []) !== '' || !isStrUpTo(p.glyph, 4) || p.glyph === '') return null;
    if (typeof p.colour !== 'string' || !COLOUR_RE.test(p.colour) || typeof p.current !== 'boolean') return null;
    if (!oneOf(p.avatar, PROFILE_AVATARS) || !oneOf(p.accent, PROFILE_ACCENTS)) return null;
    ids.add(p.id.toLowerCase());
    list.push({ id: p.id, name: p.name, glyph: p.glyph, colour: p.colour, avatar: p.avatar, accent: p.accent, current: p.current });
  }
  const current = list.filter((p) => p.current).length;
  if (current > 1 || (current === 1) !== (avatars.length > 0) || avatars.length > 0 !== accents.length > 0) return null;
  return { list, avatars: [...avatars], accents: [...accents] };
}
export const profileText = {
  open: (id: string): string => `open\t${id}`,
  create: (name: string): string => `new\t${name.trim()}`,
  rename: (id: string, name: string): string => `rename\t${id}\t${name.trim()}`,
  look: (id: string, avatar: ProfileAvatar, accent: ProfileAccent): string => `look\t${id}\t${avatar}\t${accent}`,
  shortcut: (id: string): string => `shortcut\t${id}`,
  remove: (id: string): string => `delete\t${id}`,
  guest: (): string => 'guest',
};

export interface WorkspaceRule { readonly site: string; readonly workspace: string }
export interface WorkspaceRules { readonly rules: readonly WorkspaceRule[]; readonly workspaces: readonly string[] }
export const MAX_WORKSPACE_RULES = 64;
const MAX_WORKSPACE_NAME = 128;
const isWorkspaceName = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= MAX_WORKSPACE_NAME && !/[\u0000-\u001f\u007f]/.test(value);
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
export function addRuleText(site: string, workspace: string): string | null {
  const plain = ruleSite(site);
  return plain === null || !isWorkspaceName(workspace.trim()) ? null : `add\t${plain}\t${workspace.trim()}`;
}
export function ruleSite(site: string): string | null {
  const plain = normaliseSite(site);
  return plain !== null && plain.includes('.') ? plain : null;
}
export function removeRuleText(site: string): string {
  return `del\t${site}`;
}
export function ruleExists(rules: readonly WorkspaceRule[], site: string): boolean {
  const plain = ruleSite(site);
  return plain !== null && rules.some((rule) => rule.site === plain);
}

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
  'splitEdgeDrop', 'blockYoutubeVideoAds', 'spoilerProtection', 'spoilerMode', 'spoilerReducedEffects', 'spoilerOcr', 'spoilerFeed', 'spoilerFeedStatus',
  'defaultBrowser', 'downloadDir', 'downloadAsk',
  'translateOffer', 'translateAuto', 'translateTarget', 'translateTargets', 'start', 'tabOptions', 'workspaceRules',
  'layout', 'themeOptions', 'window',
  'bars', 'profiles'];
const LANGUAGE_RE = /^[a-z]{2,3}$/;
export const MAX_TRANSLATE_TARGETS = 64;
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
export function translateTargetOptions(codes: readonly string[]): Array<{ value: string; label: string }> {
  return [{ value: '', label: TRANSLATE_TARGET_SYSTEM_LABEL }, ...codes.map((code) => ({ value: code, label: languageName(code) }))];
}
export const MAX_DOWNLOAD_DIR = 260;
const SITE_RE = /^[^\s\u0000-\u001f\u007f,]{1,253}$/u;

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
  if (!isAutoHideBarsText(v.autoHideBars) || typeof v.autoHideFullscreen !== 'boolean') return null;
  if (!oneOf(v.bookmarksBar, BOOKMARKS_BAR_MODES) || typeof v.bookmarksBarTabGroups !== 'boolean') return null;
  if (!oneOf(v.sidePanelPosition, SIDE_PANEL_POSITIONS)) return null;
  if (typeof v.tabPreviewImages !== 'boolean' || typeof v.tabPreviewMemory !== 'boolean') return null;
  if (!oneOf(v.fontSize, FONT_SIZES) || !isUiFont(v.uiFont) || !isIntBetween(v.defaultZoom, MIN_ZOOM, MAX_ZOOM)) return null;
  for (const key of ['showZoomIndicator', 'newTabBesideActive', 'ctrlTabMru', 'shiftRightClickContext', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs',
    'pinNewTabGroups', 'splitEdgeDrop', 'blockYoutubeVideoAds'] as const) {
    if (typeof v[key] !== 'boolean') return null;
  }
  if (typeof v.spoilerProtection !== 'boolean' || !oneOf(v.spoilerMode, SPOILER_MODES) || typeof v.spoilerReducedEffects !== 'boolean' || typeof v.spoilerOcr !== 'boolean') return null;
  if (typeof v.spoilerFeed !== 'boolean' || typeof v.spoilerFeedStatus !== 'string' || v.spoilerFeedStatus.length > 300) return null;
  if (!oneOf(v.defaultBrowser, DEFAULT_BROWSER_STATES)) return null;
  if (!isStrUpTo(v.downloadDir, MAX_DOWNLOAD_DIR * 2) || /[\u0000-\u001f\u007f]/.test(v.downloadDir) || typeof v.downloadAsk !== 'boolean') return null;
  if (typeof v.translateOffer !== 'boolean' || typeof v.translateAuto !== 'boolean') return null;
  if (typeof v.translateTarget !== 'string' || !(v.translateTarget === '' || LANGUAGE_RE.test(v.translateTarget))) return null;
  if (!Array.isArray(v.translateTargets) || v.translateTargets.length > MAX_TRANSLATE_TARGETS) return null;
  if (!v.translateTargets.every((code) => typeof code === 'string' && LANGUAGE_RE.test(code))) return null;
  const start = parseStartSettings(v.start);
  if (start === null) return null;
  const tabOptions = parseTabOptions(v.tabOptions);
  if (!tabOptions) return null;
  const workspaceRules = parseWorkspaceRules(v.workspaceRules);
  if (!workspaceRules) return null;
  const profiles = parseProfiles(v.profiles);
  if (!profiles) return null;
  const layout = parseLayoutSettings(v.layout);
  if (layout === null) return null;
  const themeOptions = parseThemeOptions(v.themeOptions);
  if (!themeOptions) return null;
  const window = parseWindowSettings(v.window);
  if (window === null) return null;
  const bars = parseBarsSettings(v.bars);
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
    spoilerProtection: v.spoilerProtection,
    spoilerMode: v.spoilerMode,
    spoilerReducedEffects: v.spoilerReducedEffects,
    spoilerOcr: v.spoilerOcr,
    spoilerFeed: v.spoilerFeed,
    spoilerFeedStatus: v.spoilerFeedStatus,
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
    profiles,
    layout,
    themeOptions,
    window,
    bars,
  };
}

export type Draft = Pick<SettingsSnapshot,
  | 'tabLayout' | 'sidebarWidth' | 'sidebarPinned' | 'sleepEnabled' | 'sleepAfterSecs' | 'blockerEnabled' | 'blockerAllow' | 'searchEngine' | 'automaticTabGrouping'
  | 'efficiencyMode' | 'trackingPrevention' | 'theme' | 'themeAccent' | 'toolbarButtons' | 'centredAddressBar' | 'minimalUrl' | 'showLoadProgress'
  | 'frameless' | 'autoHideBars' | 'autoHideFullscreen' | 'roundedContentFrame' | 'showHomeButton' | 'bookmarksBar' | 'bookmarksBarTabGroups' | 'sidePanelPosition' | 'tabPreviewImages'
  | 'tabPreviewMemory' | 'fontSize' | 'uiFont' | 'defaultZoom' | 'showZoomIndicator' | 'newTabBesideActive' | 'ctrlTabMru' | 'shiftRightClickContext'
  | 'ctrlShiftCCopyLink' | 'ctrlSVerticalTabs' | 'pinNewTabGroups' | 'splitEdgeDrop' | 'blockYoutubeVideoAds' | 'start'
  | 'spoilerProtection' | 'spoilerMode' | 'spoilerReducedEffects' | 'spoilerOcr' | 'spoilerFeed'
  | 'layout'
  | 'window'
>;

export function restingPageRows(
  draft: Pick<Draft, 'spoilerProtection' | 'tabLayout'>,
  translateOffer: boolean,
  shortcutsEnabled: boolean,
): ReadonlySet<string> {
  const rest = new Set<string>();
  if (!draft.spoilerProtection) for (const id of ['spoilerMode', 'spoilerReducedEffects']) rest.add(id);
  if (!translateOffer) rest.add('translateAuto');
  if (!shortcutsEnabled) for (const id of ['ctrlTabMru', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs']) rest.add(id);
  if (draft.tabLayout !== 'vertical' && draft.tabLayout !== 'vertical-right') rest.add('ctrlSVerticalTabs');
  return rest;
}

export function draftOf(snapshot: SettingsSnapshot): Draft {
  const {
    tabLayout, sidebarWidth, sidebarPinned, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow, searchEngine, automaticTabGrouping, efficiencyMode,
    trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless, autoHideBars, autoHideFullscreen, roundedContentFrame,
    showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont, defaultZoom,
    showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups, splitEdgeDrop,
    blockYoutubeVideoAds, spoilerProtection, spoilerMode, spoilerReducedEffects, spoilerOcr, spoilerFeed, start, layout, window,
  } = snapshot;
  return {
    tabLayout, sidebarWidth, sidebarPinned, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow: [...blockerAllow], searchEngine, automaticTabGrouping,
    efficiencyMode, trackingPrevention, theme, themeAccent, toolbarButtons, centredAddressBar, minimalUrl, showLoadProgress, frameless, autoHideBars, autoHideFullscreen,
    roundedContentFrame, showHomeButton, bookmarksBar, bookmarksBarTabGroups, sidePanelPosition, tabPreviewImages, tabPreviewMemory, fontSize, uiFont,
    defaultZoom, showZoomIndicator, newTabBesideActive, ctrlTabMru, shiftRightClickContext, ctrlShiftCCopyLink, ctrlSVerticalTabs, pinNewTabGroups,
    splitEdgeDrop, blockYoutubeVideoAds, spoilerProtection, spoilerMode, spoilerReducedEffects, spoilerOcr, spoilerFeed, start: { ...start, startupPages: [...start.startupPages] },
    layout: { ...layout },
    window: { ...window },
  };
}

export function saveText(draft: Draft): string | null {
  if (!oneOf(draft.tabLayout, TAB_LAYOUTS) || !isIntBetween(draft.sidebarWidth, MIN_SIDEBAR, MAX_SIDEBAR)) return null;
  if (!isIntBetween(draft.sleepAfterSecs, MIN_SLEEP, MAX_SLEEP) || draft.blockerAllow.length > MAX_ALLOW || !allowFits(draft.blockerAllow)) return null;
  if (!draft.blockerAllow.every((site) => SITE_RE.test(site))) return null;
  if (!oneOf(draft.efficiencyMode, EFFICIENCY_MODES) || (draft.trackingPrevention !== 'balanced' && draft.trackingPrevention !== 'strict')) return null;
  if (draft.searchEngine !== null && !oneOf(draft.searchEngine, SEARCH_ENGINES)) return null;
  if (!oneOf(draft.theme, THEME_MODES) || !isThemeAccent(draft.themeAccent) || !isToolbarButtonsText(draft.toolbarButtons)) return null;
  if (!isAutoHideBarsText(draft.autoHideBars)) return null;
  if (!oneOf(draft.bookmarksBar, BOOKMARKS_BAR_MODES) || !oneOf(draft.sidePanelPosition, SIDE_PANEL_POSITIONS)) return null;
  if (!oneOf(draft.fontSize, FONT_SIZES) || !isUiFont(draft.uiFont) || !isIntBetween(draft.defaultZoom, MIN_ZOOM, MAX_ZOOM)) return null;
  if (!oneOf(draft.spoilerMode, SPOILER_MODES)) return null;
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
    `spoiler_protection=${draft.spoilerProtection}`,
    `spoiler_mode=${draft.spoilerMode}`,
    `spoiler_reduced_effects=${draft.spoilerReducedEffects}`,
    `spoiler_ocr=false`,
    `spoiler_feed=${draft.spoilerFeed}`,
  );
  const startLines = startSaveLines(draft.start);
  if (startLines === null) return null;
  lines.push(...startLines);
  const layoutLines = layoutSaveLines(draft.layout);
  if (layoutLines === null) return null;
  lines.push(...layoutLines);
  lines.push(...windowSaveLines(draft.window));
  const text = lines.join('\n');
  return utf8Bytes(text) <= MAX_SETTINGS_SAVE ? text : null;
}

export interface SectionSpec { readonly key: SettingsSection; readonly title: string; readonly nav: string; readonly lead: string; readonly aliases?: string }
export const SECTIONS: readonly SectionSpec[] = [
  { key: 'profile', title: 'Profile', nav: 'Profile', lead: '' },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: '' },
  { key: 'tabs', title: 'Tabs', nav: 'Tabs', lead: '', aliases: 'workspaces tiling tiles split' },
  { key: 'stacks', title: 'Groups', nav: 'Groups', lead: '', aliases: 'tab groups grouping islands automatic stack stacks stacking' },
  { key: 'startpage', title: 'Start page', nav: 'Start page', lead: '' },
  { key: 'privacy', title: 'Privacy', nav: 'Privacy', lead: '', aliases: 'privacy and security' },
  { key: 'performance', title: 'Performance', nav: 'Performance', lead: '' },
  { key: 'pagetools', title: 'Page tools', nav: 'Page tools', lead: '' },
  { key: 'address', title: 'Address', nav: 'Address', lead: '', aliases: 'address bar' },
  { key: 'search', title: 'Search', nav: 'Search', lead: '' },
  { key: 'bookmarks', title: 'Bookmarks', nav: 'Bookmarks', lead: '' },
  { key: 'keyboard', title: 'Keyboard', nav: 'Keyboard', lead: '' },
  { key: 'commands', title: 'Commands', nav: 'Commands', lead: '', aliases: 'quick commands' },
  { key: 'passwords', title: 'Passwords and autofill', nav: 'Passwords', lead: '', aliases: 'autofill addresses cards payment methods credit card generator import export passkeys' },
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
  readonly aria?: string;
  readonly description: string;
  readonly keywords: string;
  readonly restart?: boolean;
  readonly reload?: boolean;
  readonly notYet?: boolean;
  readonly tier?: RowTier;
}

export type RowTier = 'core' | 'more' | 'page' | 'later';

export interface GroupSpec {
  readonly key: string;
  readonly title: string;
  readonly section: SettingsSection;
  readonly rows: readonly string[];
}

export interface AreaSpec {
  readonly key: SettingsSection;
  readonly title: string;
  readonly nav: string;
  readonly lead: string;
  readonly aliases?: string;
  readonly parts: readonly SettingsSection[];
  readonly groups: readonly GroupSpec[];
}

const sectionSpec = (key: SettingsSection): SectionSpec => SECTIONS.find((spec) => spec.key === key)!;
const group = (key: string, title: string, section: SettingsSection, rows: readonly string[]): GroupSpec => ({ key, title, section, rows });

export const AREAS: readonly AreaSpec[] = [
  { key: 'profile', title: 'General', nav: 'General', lead: '', aliases: 'you and bowser startup search engine downloads default browser', parts: ['profile', 'search', 'downloads'], groups: [
    group('profiles', 'Profiles', 'profile', ['profile', 'profiles', 'import']),
    group('startup', 'On startup', 'profile', ['startupMode', 'startupPages', 'startHomepage']),
    group('search', 'Search engine', 'search', ['searchDefault', 'searchSuggestions', 'engines', 'searchPrivate', 'searchImage', 'searchInAddress', 'searchSuggestAddress', 'searchNewTab', 'searchSelection']),
    group('searchField', 'Search field', 'search', ['searchField', 'searchNickname', 'searchIcon', 'searchKeepEngine', 'searchKeepText', 'searchSuggestField']),
    group('searchScopes', 'Search scopes', 'search', ['nickBookmarks', 'nickHistory', 'nickTabs']),
    group('downloads', 'Downloads', 'downloads', ['downloadDir', 'downloadAsk']),
    group('defaultBrowser', 'Default browser', 'profile', ['defaultBrowser']),
  ] },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: '', parts: ['appearance'], groups: [
    group('theme', 'Theme', 'appearance', ['themeMode', 'themeAccent', 'theme', 'websiteAppearance', 'themeSchedule', 'themeLook', 'themeTimes', 'themeColours', 'themeColouring', 'themeAccentPage', 'themeSaturation', 'themeContrast', 'themeFile', 'forceDark', 'privateTheme']),
    group('layout', 'Layout', 'appearance', ['layoutPreset', 'layout', 'sidebarWidth', 'sidebarPinned', 'tabBarVisible', 'addressBarVisible', 'statusBar', 'uiDensity', 'compactMenus']),
    group('toolbar', 'Toolbar', 'appearance', ['toolbarButtons', 'centredAddressBar', 'minimalUrl', 'bookmarksBar', 'showLoadProgress', 'addrFull', 'addrShow', 'addrQr']),
    group('text', 'Text and zoom', 'appearance', ['fontSize', 'defaultZoom', 'uiFont', 'uiZoom', 'showZoomIndicator', 'ptZoomScope', 'ptWheelZoom']),
    group('bookmarksBar', 'Bookmarks bar', 'appearance', ['bookmarksBarTabGroups', 'bmBarFolder', 'bmBarPosition', 'bmBarDisplay', 'pinNewTabGroups']),
    group('window', 'Window', 'appearance', ['roundedContentFrame', 'frameless', 'autoHideBars', 'autoHideFullscreen', 'winAnimations', 'winRangeButtons', 'winFade', 'winScrollbars', 'winMonoIcons', 'winReset']),
    group('panels', 'Panels', 'appearance', ['panelCloseButton', 'panelFolderCounts', 'panelAutoClose', 'panelReset']),
  ] },
  { key: 'tabs', title: 'Tabs', nav: 'Tabs', lead: '', aliases: 'workspaces tiling tiles split groups start page speed dial', parts: ['tabs', 'stacks', 'startpage'], groups: [
    group('tabBar', 'Tab bar', 'tabs', ['tabPosition', 'tabs_scroll', 'tabs_active_min_width', 'tabs_wheel_switch', 'tabs_remove_spacing_maximised', 'tabs_load_indicator', 'tabs_minimise_active', 'tabs_unread_indicator', 'tabs_title_notifications', 'tabs_dim_hibernated', 'tabs_inactive_days', 'tabPreviewImages', 'tabPreviewMemory']),
    group('newTabs', 'New tabs', 'tabs', ['tabs_new_tab_page', 'tabs_new_tab_url', 'tabs_new_tab_position', 'tabs_link_background', 'tabs_duplicate_position', 'tabs_new_tab_in_group', 'tabs_focus_page']),
    group('startLook', 'Start page', 'startpage', ['startClock', 'startSearch', 'startWallpaper', 'startWallpaperDim']),
    group('startPage', 'Speed dials', 'startpage', ['startDials', 'startThumbSize', 'startTitles', 'startColumns', 'startAddButton', 'startGroupReopen', 'startDragReorder']),
    group('startWidgets', 'Bookmark rows', 'startpage', ['startWidgets', 'startWidgetColumns', 'startWidgetBg', 'startWidgetColor', 'startWidgetDrag']),
    group('groups', 'Groups and workspaces', 'stacks', ['grouping', 'tabs_workspaces', 'workspaceRules', 'tabs_workspace_scroll', 'tabs_workspace_counts', 'splitEdgeDrop', 'tabs_tile_titles', 'tabs_tile_tight']),
    group('closing', 'Closing', 'tabs', ['tabs_close_related', 'tabs_close_activation', 'tabs_align_close_button', 'tabs_keep_window', 'tabs_confirm_close', 'tabs_double_click', 'tabs_close_left', 'tabs_close_always']),
    group('pinned', 'Pinned and restored', 'tabs', ['tabs_pinned_close', 'tabs_pinned_separator', 'tabs_pinned_restrict', 'tabs_lazy_restore', 'tabs_load_pinned']),
    group('selecting', 'Selecting and audio', 'tabs', ['tabs_selection', 'tabs_selection_active', 'tabs_mute_mode']),
  ] },
  { key: 'privacy', title: 'Privacy and security', nav: 'Privacy', lead: '', aliases: 'privacy and security', parts: ['privacy'], groups: [
    group('blocking', 'Blocking', 'privacy', ['blocker', 'cookieNotices', 'blockYoutubeVideoAds', 'blockLists', 'allow']),
    group('history', 'History and tracking', 'privacy', ['history', 'historyRetention', 'tracking']),
    group('siteData', 'Site data', 'privacy', ['deleteData', 'siteRules', 'dashboard']),
    group('security', 'Security', 'privacy', ['secHttps', 'secBase', 'secJs']),
    group('spoilers', 'Spoiler protection', 'privacy', ['spoilerProtection', 'spoilerMode', 'spoilerReducedEffects', 'spoilerFeed', 'spoilerNote']),
  ] },
  { key: 'passwords', title: 'Passwords and autofill', nav: 'Passwords', lead: '', aliases: 'autofill addresses payment cards generator import export', parts: ['passwords'], groups: [
    group('passwords', 'Passwords and autofill', 'passwords', ['passwords']),
  ] },
  { key: 'address', title: 'Browsing', nav: 'Browsing', lead: '', aliases: 'address bar bookmarks page tools translate reader screenshots feeds', parts: ['address', 'bookmarks', 'pagetools'], groups: [
    group('address', 'Address bar', 'address', ['addrAutocomplete', 'addrSelect', 'addrEncoded', 'addrExpansion']),
    group('suggestions', 'Address bar suggestions', 'address', ['ddTyping', 'ddBadge', 'ddTypedHistory', 'ddTypedSearches', 'ddHistoryButton', 'ddMax', 'ddPriorityBookmarks', 'ddPriorityDirect', 'ddSources']),
    group('bookmarks', 'Bookmarks', 'bookmarks', ['bmNewTab', 'bmSingleClick', 'bmConfirm', 'bmMetadata', 'bmDialog']),
    group('translate', 'Translation', 'pagetools', ['translateOffer', 'translateAuto', 'translateTarget']),
    group('capture', 'Screenshots', 'pagetools', ['ptCaptureDir', 'ptCaptureName', 'ptCaptureFormat']),
    group('reader', 'Reader view', 'pagetools', ['ptReaderFont', 'ptReaderSize', 'ptReaderLine', 'ptReaderWidth', 'ptReaderTheme', 'ptReaderVertical', 'ptReaderIcon', 'ptReaderTime']),
    group('pageTools', 'Page tools', 'pagetools', ['ptPip', 'ptSpatial']),
    group('feeds', 'Feeds', 'address', ['feedsOpen', 'feedsDetect', 'feedsRefresh']),
  ] },
  { key: 'keyboard', title: 'Shortcuts', nav: 'Shortcuts', lead: '', aliases: 'keyboard shortcuts quick commands', parts: ['keyboard', 'commands'], groups: [
    group('keyboard', 'Keyboard', 'keyboard', ['kbEnabled', 'kbEditor', 'kbSingleKey', 'kbAltMenu', 'kbFocusCycling', 'kbSites']),
    group('keys', 'Keys and clicks', 'keyboard', ['ctrlTabMru', 'ctrlSVerticalTabs', 'ctrlShiftCCopyLink', 'shiftRightClickContext']),
    group('commands', 'Quick commands', 'commands', ['chains', 'qcTypes', 'qcMax', 'qcKeepLast', 'qcNewTab', 'qcNotesPanel', 'qcNickname']),
  ] },
  { key: 'extensions', title: 'Extensions', nav: 'Extensions', lead: '', parts: ['extensions'], groups: [
    group('extensions', 'Extensions', 'extensions', ['extensions']),
  ] },
  { key: 'performance', title: 'System', nav: 'System', lead: '', aliases: 'performance memory battery media playback', parts: ['performance', 'media'], groups: [
    group('performance', 'Performance', 'performance', ['efficiency', 'sleep', 'sleepAfter']),
    group('media', 'Media', 'media', ['media']),
  ] },
  { key: 'about', title: 'About', nav: 'About', lead: '', parts: ['updates', 'about'], groups: [
    group('updates', 'Updates', 'updates', ['autoUpdate', 'updateStatus']),
    group('backup', 'Back up and restore', 'about', ['backupExport', 'backupImport']),
    group('about', 'About Bowser', 'about', ['version', 'file', 'reset']),
  ] },
];

export function areaOf(section: SettingsSection): AreaSpec {
  return AREAS.find((area) => area.parts.includes(section)) ?? AREAS[0]!;
}

export function partTitle(section: SettingsSection): string {
  return sectionSpec(section).title;
}

export interface Placement { readonly area: AreaSpec; readonly group: GroupSpec }
const LISTED = new Map<string, Placement>();
for (const area of AREAS) for (const spec of area.groups) for (const id of spec.rows) LISTED.set(id, { area, group: spec });

export function placementOf(id: string, section: SettingsSection): Placement {
  const listed = LISTED.get(id);
  if (listed) return listed;
  const area = areaOf(section);
  return { area, group: area.groups.find((spec) => spec.section === section) ?? area.groups[0]! };
}

export function anchorGroup(section: SettingsSection): GroupSpec | null {
  return areaOf(section).groups.find((spec) => spec.section === section) ?? null;
}

export const MORE_LABEL = 'More';
export const LATER_TITLE = 'Inactive';
export const LATER_LEAD = '';
export const LATER_LABEL = 'Show';

export function moreCountText(count: number): string {
  return count === 1 ? '1 more setting' : `${count} more settings`;
}

const CORE_ROWS: ReadonlySet<string> = new Set([
  'profile', 'profiles', 'import',
  'startupMode', 'startupPages', 'startHomepage',
  'searchDefault',
  'searchSuggestions',
  'downloadDir', 'downloadAsk', 'defaultBrowser',
  'themeMode', 'themeAccent', 'theme', 'websiteAppearance',
  'layoutPreset',
  'toolbarButtons', 'centredAddressBar', 'minimalUrl', 'bookmarksBar',
  'fontSize', 'defaultZoom',
  'tabPosition', 'tabs_new_tab_page', 'tabs_new_tab_url', 'tabs_new_tab_position',
  'grouping', 'tabs_workspaces',
  'startWallpaper', 'startDials', 'startThumbSize', 'startTitles', 'startWidgets',
  'blocker', 'cookieNotices', 'blockYoutubeVideoAds', 'history', 'historyRetention', 'tracking', 'secHttps', 'spoilerProtection',
  'addrAutocomplete', 'bmNewTab', 'translateOffer', 'feedsOpen',
  ...PAGE_TOOLS_CORE,
  'kbEnabled', 'ctrlTabMru', 'ctrlSVerticalTabs', 'ctrlShiftCCopyLink', 'shiftRightClickContext',
  'extensions', 'passwords',
  'efficiency', 'sleep', 'sleepAfter', 'media',
  'autoUpdate', 'updateStatus', 'version', 'file', 'reset',
]);

export interface PageSpec {
  readonly button: string;
  readonly aria: string;
}

export const PAGE_ROWS: Readonly<Record<string, PageSpec>> = {
  engines: { button: 'Edit', aria: 'Edit search engines' },
  siteRules: { button: 'Edit', aria: 'Edit sites' },
  blockLists: { button: 'Edit', aria: 'Edit lists' },
  deleteData: { button: 'Clear', aria: 'Clear browsing data' },
  dashboard: { button: 'Open', aria: 'Open dashboard' },
  kbEditor: { button: 'Edit', aria: 'Edit shortcuts' },
  chains: { button: 'Edit', aria: 'Edit chains' },
  workspaceRules: { button: 'Edit', aria: 'Edit rules' },
};

export function rowTier(row: RowSpec): RowTier {
  if (row.notYet) return 'later';
  if (row.tier) return row.tier;
  if (Object.hasOwn(PAGE_ROWS, row.id)) return 'page';
  return CORE_ROWS.has(row.id) ? 'core' : 'more';
}

export const RESTART_NOTE = 'Applies after restart';
export const RELOAD_NOTE = 'Applies to pages as they load. Reload an open page to apply it there.';
export const NOT_YET_NOTE = 'Has no effect yet. Bowser keeps your choice for when it does.';
export const NOT_YET_GROUP_NOTE = 'These have no effect yet. Bowser keeps your choices for when they do.';

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

const BUILT_ROWS: readonly RowSpec[] = [
  { id: 'profile', section: 'profile', label: 'Profile', aria: 'This profile', description: '', keywords: 'account person name avatar guest you and bowser profile This profile Settings on this page apply to this profile only.' },
  { id: 'profiles', section: 'profile', label: 'Profiles', aria: 'Profiles', description: '', keywords: 'switch new add create guest delete remove rename name avatar colour color shortcut desktop taskbar work personal sign-ins Manage profiles Switch, create, rename or delete profiles. Each profile keeps its own sign-ins, history, bookmarks and settings, and gets its own taskbar button.' },
  { id: 'import', section: 'profile', label: 'Import', aria: 'Import bookmarks or passwords from a file', description: '', keywords: 'chrome edge firefox migrate transfer bookmarks passwords csv html import Import bookmarks Bring bookmarks or passwords over from a file another browser exported.' },
  { id: 'layoutPreset', section: 'appearance', label: 'Presets', description: '', keywords: 'layout preset presets simple classic vertical left right bottom saved thumbnails arrangement tabs top side Presets A starting arrangement. Change any part of it and Bowser keeps yours as Saved.' },
  { id: 'layout', section: 'appearance', label: 'Top style', aria: 'Tab bar style at the top', description: '', keywords: 'tabs toolbar horizontal single row top browser layout Tab bar style at the top How the tabs and the address bar share the top of the window.' },
  { id: 'sidebarWidth', section: 'appearance', label: 'Sidebar width', description: '', keywords: 'vertical tabs size wide narrow pixels Sidebar width Width of the tab sidebar in the vertical layouts.' },
  { id: 'sidebarPinned', section: 'appearance', label: 'Pin sidebar', aria: 'Pin vertical tabs', description: '', keywords: 'vertical tabs pin sidebar hover expand overlay edge compact zen auto hide retract Pin vertical tabs Keeps the sidebar, with the address bar in it, open beside the page. Off, it hides and the page fills the window; rest the pointer on the window’s edge to slide it back over the page.' },
  { id: 'tabBarVisible', section: 'appearance', label: 'Tab bar', aria: 'Show the tab bar', description: '', keywords: 'tab bar show hide tabs visible Show the tab bar Off, the tabs are reached with Ctrl+Tab, the tab list and Quick commands.' },
  { id: 'addressBarVisible', section: 'appearance', label: 'Address bar', aria: 'Show the address bar row', description: '', keywords: 'address bar show hide toolbar row visible ctrl l Show the address bar row The whole row: navigation, the address field and the menu. Off, Ctrl+L brings it back until the address field loses focus.' },
  { id: 'statusBar', section: 'appearance', label: 'Status bar', description: '', keywords: 'status bar info overlay link address hover hide Status bar Bowser has no status bar of its own. The address of a link under the pointer shows in a small overlay at the bottom of the page; Hidden turns that off.' },
  { id: 'uiDensity', section: 'appearance', label: 'Density', aria: 'User interface density', description: '', keywords: 'density regular compact rows spacing dense User interface density Compact shortens the rows of the tab sidebar, the panels and Bowser’s own pages.' },
  { id: 'compactMenus', section: 'appearance', label: 'Compact menus', aria: 'Compact menu layout', description: '', keywords: 'compact menus dense rows menu layout Compact menu layout Shorter rows in Bowser’s menus.' },
  { id: 'uiZoom', section: 'appearance', label: 'UI zoom', aria: 'User interface zoom', description: '', keywords: 'ui zoom scale interface size percent slider text User interface zoom Scales the text of Bowser’s bars, panels and pages, on top of the font size. Select the value to type one.' },
  { id: 'themeMode', section: 'appearance', label: 'Mode', aria: 'Light, dark or system', description: '', keywords: 'theme mode light dark system night day appearance colour color scheme follow windows dark mode light mode Mode' },
  { id: 'theme', section: 'appearance', label: 'Palette', aria: 'Colour palette', description: '', keywords: 'dark light colour scheme helium system prefer system colour windows accent palette theme Colour palette Bowser’s own colours, or your Windows accent colour. Picking one puts a custom accent back to the palette’s.' },
  { id: 'themeAccent', section: 'appearance', label: 'Accent', aria: 'Accent colour', description: '', keywords: 'colour accent override custom theme reset Accent colour A colour for buttons and highlights, in place of the palette’s. Reset goes back to the palette’s accent.' },
  { id: 'websiteAppearance', section: 'appearance', label: 'Websites', description: '', keywords: 'dark light auto mode prefers color scheme websites page theme appearance Websites Ask sites for a light page, a dark page, or the same as Windows.' },
  { id: 'forceDark', section: 'appearance', label: 'Force dark', aria: 'Force dark theme on all websites', description: '', keywords: 'force dark mode night websites invert auto dark Force dark theme on all websites Darkens pages that have no dark theme of their own, in open tabs at once and in the pages you open next.' },
  { id: 'privateTheme', section: 'appearance', label: 'Private theme', aria: 'Private tab theme', description: '', keywords: 'private incognito inprivate theme dark light window colour Private tab theme How Bowser’s own windows look while a private tab is shown: the same as normal, light or dark.' },
  ...THEME_ROWS,
  { id: 'toolbarButtons', section: 'appearance', label: 'Customise your toolbar', aria: 'Customise your toolbar', description: '', keywords: 'toolbar toolbars buttons customize customise address bar tab bar back forward reload bookmark new tab split reader site information downloads extensions closed tabs command chain button add remove move reset preview Customise your toolbars Choose the buttons on the address bar and the tab bar, and put a command or a chain of commands on either.' },
  { id: 'centredAddressBar', section: 'appearance', label: 'Centre address', aria: 'Centre the address bar', description: '', keywords: 'address bar centre centered position Centre the address bar Keeps the address bar centred in the toolbar. Only where it has a row of its own above or below the tabs: not beside vertical tabs, in one row with the tabs, or under them.' },
  { id: 'minimalUrl', section: 'appearance', label: 'Short address', aria: 'Show a simplified address while unfocused', description: '', keywords: 'url simplified host domain minimal address bar focus Show a simplified address while unfocused Shows the page’s site and path, without the query or fragment, until you focus the address bar, then the full address. Off while Full address is on.' },
  { id: 'showLoadProgress', section: 'appearance', label: 'Load progress', aria: 'Show a loading progress bar', description: '', keywords: 'progress loading bar indicator Show a loading progress bar A thin bar tracks page loads in the toolbar.' },
  { id: 'grouping', section: 'stacks', label: 'Auto group', aria: 'Group tabs automatically', description: '', keywords: 'automatic auto tab groups grouping islands stacks site domain sort Group tabs automatically Tabs from one site share a group. Drag a tab into a group to add it by hand.' },
  { id: 'splitEdgeDrop', section: 'tabs', label: 'Edge split', aria: 'Drop a dragged tab on the window edge to split', description: '', keywords: 'split view drag drop edge tab tiling tile Allow dropping a dragged tab on the window edge to split Dragging a tab to the edge of the window opens it in a split view.' },
  { id: 'pinNewTabGroups', section: 'appearance', label: 'Pin groups', aria: 'Pin new tab groups to the bookmarks bar', description: '', keywords: 'pin tab groups bookmarks bar Pin new tab groups to the bookmarks bar A tab group formed from now on is pinned to the bookmarks bar. Needs “Show tab groups in the bookmarks bar”.' },
  { id: 'ctrlSVerticalTabs', section: 'keyboard', label: 'Ctrl+S', aria: 'Ctrl+S collapses or expands the sidebar in the vertical layout', description: '', keywords: 'ctrl s sidebar collapse expand vertical layout shortcut Ctrl+S hides or pins the sidebar in the vertical layout Only does anything in the vertical layout; otherwise Ctrl+S reaches the page as normal.' },
  { id: 'winAnimations', section: 'appearance', label: 'Animation', aria: 'Use animation', description: '', keywords: 'animation motion transitions reduced motion prefers reduced motion slide Use animation Bowser’s own transitions and the panel slide. Off, they snap. Bowser also stays still when Windows has animations off.' },
  { id: 'winRangeButtons', section: 'appearance', label: 'Steppers', aria: 'Buttons instead of sliders', description: '', keywords: 'sliders range buttons stepper plus minus Buttons instead of sliders Range controls on this page, such as the sidebar width and the interface zoom, show minus and plus buttons instead of a slider.' },
  { id: 'winFade', section: 'appearance', label: 'Fade inactive', aria: 'Fade colours in background windows', description: '', keywords: 'fade colours background window inactive dim grey desaturate Fade colours in background windows Bowser’s bars and panels lose their colour while another window is in front.' },
  { id: 'winScrollbars', section: 'appearance', label: 'Scrollbars', aria: 'Simple scrollbars', description: '', keywords: 'scrollbars simple thin slim overlay Simple scrollbars Thin, quiet scrollbars in Bowser’s own pages and panels. Web pages keep theirs.' },
  { id: 'winMonoIcons', section: 'appearance', label: 'Plain icons', aria: 'Monochrome settings icons', description: '', keywords: 'monochrome icons settings colour accent plain Monochrome settings icons The icon of the section you are on, in this page’s list, draws in plain text colour instead of the accent.' },
  { id: 'panelCloseButton', section: 'appearance', label: 'Close button', aria: 'Show the close button on panels', description: '', keywords: 'panel close button x header Show the close button on panels The X in a panel’s header. Off, Escape and the panel’s rail button still close it.' },
  { id: 'panelFolderCounts', section: 'appearance', label: 'Folder counts', aria: 'Show folder counters', description: '', keywords: 'folder counters count number bookmarks panel items Show folder counters How many items a bookmarks folder holds, beside its name.' },
  { id: 'panelAutoClose', section: 'appearance', label: 'Auto-close', aria: 'Close floating panels when you click elsewhere', description: '', keywords: 'floating panel auto close dismiss click outside overlay flyout Close floating panels when you click elsewhere Bookmarks and Customize float over the page. Off, they stay until you close them.' },
  { id: 'winReset', section: 'appearance', label: 'Reset window', aria: 'Reset window options', description: '', keywords: 'reset window defaults restore Restores defaults Reset window options Puts animation, sliders, fading, scrollbars and icons back to their defaults.' },
  { id: 'panelReset', section: 'appearance', label: 'Reset panels', aria: 'Reset panel options', description: '', keywords: 'reset panels defaults restore Restores defaults Reset panel options Puts the close button, folder counters and auto-close back to their defaults.' },
  { id: 'roundedContentFrame', section: 'appearance', label: 'Rounded page', aria: 'Rounded corners around the page', description: '', keywords: 'rounded corners content frame page Rounded corners around the page Rounds the corners of the page area.' },
  { id: 'frameless', section: 'appearance', label: 'Auto-hide', aria: 'Auto-hide toolbars and tabs', description: '', keywords: 'auto hide autohide frameless toolbar tabs immersive fullscreen full screen bars edge Auto-hide toolbars and tabs Hides the bars you choose until the pointer rests at the edge of the window. Ctrl+F11 turns it on or off.', tier: 'more' },
  { id: 'autoHideBars', section: 'appearance', label: 'Hidden bars', aria: 'Bars to hide', description: '', keywords: 'auto hide bars tabs address bookmarks panel status Bars to hide The tab bar and address bar share the top edge in the horizontal layouts, so either one hides both. The side panel opens over the page instead of pushing it.', tier: 'more' },
  { id: 'autoHideFullscreen', section: 'appearance', label: 'Full screen', aria: 'Always auto-hide in full screen', description: '', keywords: 'auto hide full screen fullscreen f11 video Always auto-hide in full screen In full screen (F11, or a video) every bar hides, even with the switch above off.', tier: 'more' },
  { id: 'bookmarksBar', section: 'appearance', label: 'Bookmarks bar', description: '', keywords: 'bookmarks bar show hide newtab always never Bookmarks bar When the bookmarks bar is shown.' },
  { id: 'bookmarksBarTabGroups', section: 'appearance', label: 'Groups on bar', aria: 'Show tab groups in the bookmarks bar', description: '', keywords: 'tab groups bookmarks bar islands Show tab groups in the bookmarks bar Tab groups pinned to the bar appear after the bookmarks. Select one to go to it.' },
  ...ADDRESS_ROWS.filter((row) => row.section === 'appearance'),
  { id: 'fontSize', section: 'appearance', label: 'Text size', description: '', keywords: 'font size small medium large text Text size The size of text in Bowser’s bars, menus, panels and pages. Web pages keep theirs; Page zoom sets those.' },
  { id: 'uiFont', section: 'appearance', label: 'Font', aria: 'Customise fonts', description: '', keywords: 'font family typeface custom ui Customise fonts A font family to use instead of the system font.' },
  { id: 'defaultZoom', section: 'appearance', label: 'Page zoom', description: '', keywords: 'zoom default starting percent page Page zoom The zoom new pages start at.' },
  { id: 'showZoomIndicator', section: 'appearance', label: 'Zoom indicator', aria: 'Show the zoom indicator', description: '', keywords: 'zoom indicator toolbar percent Show the zoom indicator Shows the zoom level in the toolbar when it isn’t 100%.' },
  { id: 'translateOffer', section: 'appearance', label: 'Offer translate', aria: 'Offer to translate pages in other languages', description: '', keywords: 'translate translation language foreign page Offer to translate pages in other languages Shows a button in the toolbar when a page is in another language. Nothing is sent until you click it. Right-click selected text in another language to see its translation; that text is sent when you right-click. Text is translated by Google Translate, so it is sent to Google. selection right-click context menu' },
  { id: 'translateAuto', section: 'appearance', label: 'Auto-translate', aria: 'Translate automatically on sites set to always translate', description: '', keywords: 'translate translation language foreign page automatic always Translate automatically on sites set to always translate Sites set to always translate are translated as soon as they load, which sends their text to Google Translate.' },
  { id: 'translateTarget', section: 'appearance', label: 'Translate to', aria: 'Translate into', description: '', keywords: 'translate translation language foreign page target windows Translate into' },
  { id: 'shiftRightClickContext', section: 'keyboard', label: 'Shift+right-click', aria: 'Shift+right click shows the system menu', description: '', keywords: 'shift right click context menu system Shift+Right Click shows the system menu Shift+Right Click always shows the system menu over Bowser’s own tab and rail menus.' },
  { id: 'ctrlShiftCCopyLink', section: 'keyboard', label: 'Ctrl+Shift+C', aria: 'Ctrl+Shift+C copies the address', description: '', keywords: 'ctrl shift c copy link address shortcut Ctrl+Shift+C copies the address Ctrl+Shift+C copies the current page’s address.' },
  ...TABS_SECTION_ROWS,
  { id: 'tabPosition', section: 'tabs', label: 'Tab position', aria: 'Tabs on top or down the side', description: '', keywords: 'vertical tabs horizontal tab bar side sidebar top left layout position strip Tab position' },
  { id: 'workspaceRules', section: 'tabs', label: 'Rules', aria: 'Workspace rules', description: '', keywords: 'workspace rules sites open automatically named workspace domain Workspace rules Sites that open in a workspace of their own: a link, an address or a bookmark of the site moves its tab there.' },
  { id: 'startGroupReopen', section: 'startpage', label: 'Starting group', aria: 'Speed dial group a new tab opens on', description: '', keywords: 'start group speed dial group last used first new tab start page reopen Open the start page on Which speed dial group a new tab shows first.' },
  { id: 'startTitles', section: 'startpage', label: 'Site names', aria: 'Show site names under speed dial tiles', description: '', keywords: 'speed dial titles names always never needed auto if no image hover labels Show titles When a speed dial shows its name. When needed shows it under tiles without a chosen image, and on hover for the rest.' },
  { id: 'startDials', section: 'startpage', label: 'Speed dials', aria: 'Show speed dials on new tabs', description: '', keywords: 'speed dials new tab blank start page groups sites pinned Show speed dials on new tabs Off, a new tab is blank.' },
  { id: 'startAddButton', section: 'startpage', label: 'Add site tile', aria: 'Show the Add a site tile', description: '', keywords: 'speed dial add button plus tile new site add a site Show the add button A tile at the end of each group for adding a site.' },
  { id: 'startColumns', section: 'startpage', label: 'Max columns', aria: 'Maximum speed dial columns', description: '', keywords: 'speed dial columns row grid width Maximum columns The most speed dials in a row. Fewer show when the window is narrow.' },
  { id: 'startThumbSize', section: 'startpage', label: 'Speed dial size', description: '', keywords: 'speed dial thumbnail tile size tiny small medium large huge tiles Tile size How large each speed dial tile is.' },
  { id: 'startClock', section: 'startpage', label: 'Clock', aria: 'Show the time and date on the start page', description: '', keywords: 'clock time date start page new tab Show the time and date above the search box.' },
  { id: 'startSearch', section: 'startpage', label: 'Search box', aria: 'Show a search box on the start page', description: '', keywords: 'search box bar field start page new tab google second search Show a search box on the start page. It searches with your search engine, or opens an address.' },
  { id: 'startWallpaper', section: 'startpage', label: 'Background', aria: 'Start page background', description: '', keywords: 'background wallpaper picture image photo theme start page new tab customise aurora dusk ocean forest blossom sand graphite midnight upload Start page background A picture behind the start page. Upload your own from the pencil button on the start page.' },
  { id: 'startWallpaperDim', section: 'startpage', label: 'Dim background', aria: 'Dim the start page background', description: '', keywords: 'background wallpaper dim darken contrast start page Dim the background so the text over it reads.' },
  { id: 'startDragReorder', section: 'startpage', label: 'Drag to reorder', aria: 'Drag speed dials to reorder them', description: '', keywords: 'speed dial drag drop reorder move sort Drag to reorder Drag speed dials to reorder them, or onto a folder to put them inside it. Alt and the arrow keys do the same.' },
  { id: 'startHomepage', section: 'startpage', label: 'Homepage', description: '', keywords: 'home page homepage address url Homepage The Home button opens this. Leave it empty for the start page.' },
  { id: 'startupMode', section: 'startpage', label: 'Startup', aria: 'What opens in front of your tabs at startup', description: '', keywords: 'startup launch open start page last session continue where you left off your tabs homepage specific pages restore when bowser starts On startup Your open tabs always come back. Tabs + homepage or Tabs + pages also opens those in front of them; a page already open is shown, not opened twice.' },
  { id: 'startupPages', section: 'startpage', label: 'Startup pages', aria: 'Pages to open at startup', description: '', keywords: 'startup pages specific urls tabs launch Pages to open at startup One address per line, up to eight. Used when Startup is Tabs + pages.' },
  { id: 'startWidgets', section: 'startpage', label: 'Bookmark rows', aria: 'Show rows of bookmarks on the start page', description: '', keywords: 'widgets bookmarks rows folder dashboard start page Show bookmark rows A bookmark folder\'s sites under the speed dials, as a row of icons or a list. Add one with the pencil button on the start page.' },
  { id: 'startWidgetColumns', section: 'startpage', label: 'List columns', aria: 'Bookmark lists side by side', description: '', keywords: 'widgets bookmarks list columns row dashboard The most bookmark lists side by side, 1 to 4.' },
  { id: 'startWidgetBg', section: 'startpage', label: 'List background', aria: 'Bookmark list background', description: '', keywords: 'widgets bookmarks list background solid transparent translucent custom colour How a bookmark list is filled: solid, semi-transparent, transparent or a colour you choose.' },
  { id: 'startWidgetColor', section: 'startpage', label: 'Custom colour', aria: 'Custom bookmark list colour', description: '', keywords: 'widgets bookmarks colour color custom background Custom list colour Used when the list background is Custom.' },
  { id: 'startWidgetDrag', section: 'startpage', label: 'Drag to rearrange', aria: 'Drag to rearrange bookmark rows', description: '', keywords: 'widgets bookmarks drag rearrange reorder move Drag a bookmark row by its handle, or press Alt and an arrow key on it.' },
  { id: 'blocker', section: 'privacy', label: 'Blocking', description: '', keywords: 'adblock ad blocker shield trackers ads none level block blocking Blocking None, trackers, or trackers and ads. A site can have its own level.', reload: true },
  { id: 'cookieNotices', section: 'privacy', label: 'Cookie notices', aria: 'Hide cookie notices', description: '', keywords: 'cookie warning banner consent gdpr popup annoyance i don’t care about cookies easylist Hide cookie notices Hides cookie-consent banners on pages, with or without blocking. A site can switch it off.', reload: true },
  { id: 'blockYoutubeVideoAds', section: 'privacy', label: 'YouTube ads', aria: 'Block YouTube video ads', description: '', keywords: 'youtube video ads block skip Block YouTube video ads Skips in-video ads on YouTube where the blocking level is Trackers and ads, and in private tabs. At Trackers or None, YouTube ads still play.', reload: true },
  { id: 'spoilerProtection', section: 'privacy', label: 'GTA VI spoilers', aria: 'Hide GTA VI spoilers', description: '', keywords: 'spoiler spoilers gta 6 vi grand theft auto rockstar firewall protection shield hide block leak ending Hide GTA VI spoilers Spoiler Firewall for GTA VI is built in and on by default. It covers every tab, including private tabs and guest windows.', reload: true },
  { id: 'spoilerMode', section: 'privacy', label: 'How much to hide', description: '', keywords: 'spoiler mode lockdown strict balanced gta firewall level hide How much to hide', reload: true },
  { id: 'spoilerReducedEffects', section: 'privacy', label: 'Simple covers', aria: 'Simpler spoiler covers', description: '', keywords: 'spoiler reduced effects motion blur animation performance accessibility gta Simpler spoiler covers Opaque covers with no blur or animation. Lighter on slow computers.', reload: true },
  { id: 'spoilerFeed', section: 'privacy', label: 'Term updates', aria: 'Download spoiler term updates', description: '', keywords: 'spoiler term updates feed word list download signed gta names missions Download spoiler term updates Fetches a small signed word list about every 12 hours so new names and missions are covered before Bowser itself updates. It sends no browsing data and can only add protection.' },
  { id: 'spoilerNote', section: 'privacy', label: 'Spoiler limits', aria: SPOILER_LIMITS_NOTE, description: '', keywords: 'spoiler limits limitations images screenshots video notifications unofficial gta What spoiler protection cannot do ' + SPOILER_LIMITS_NOTE },
  { id: 'allow', section: 'privacy', label: 'Skipped sites', aria: 'Sites the blocker skips', description: '', keywords: 'allow list exceptions whitelist adblock Sites the blocker skips The blocker never blocks on these sites.', reload: true },
  { id: 'tracking', section: 'privacy', label: 'Tracking', aria: 'Tracking prevention', description: '', keywords: 'privacy cookies strict balanced fingerprinting Tracking prevention Balanced is the default. Strict blocks more trackers but can break some sites. Applies at once, to open tabs too.' },
  { id: 'history', section: 'privacy', label: 'History', aria: 'Save history', description: '', keywords: 'history visits record browsing save Save history Pages you visit are listed in History and suggested in the address bar.' },
  { id: 'historyRetention', section: 'privacy', label: 'Keep history', aria: 'Keep history for', description: '', keywords: 'history retention expire delete old days week month year forever session Keep history for Older visits are deleted from the history list. Session only clears it when Bowser closes.' },
  { id: 'siteRules', section: 'privacy', label: 'Site rules', aria: 'Sites with their own choices', description: '', keywords: 'site exceptions per site level blocking cookie notices list shield Sites with their own choices Give one site its own blocking level or cookie-notice choice.' },
  { id: 'blockLists', section: 'privacy', label: 'Block lists', aria: 'Blocking lists', description: '', keywords: 'sources filter lists easylist easyprivacy subscription url file add cookie list Blocking lists Switch lists on or off, add one from an address or a file, or remove one you added.' },
  { id: 'deleteData', section: 'privacy', label: 'Clear data', aria: 'Clear browsing data', description: '', keywords: 'clear delete browsing data history cookies cache storage logins sign-ins autofill site settings hour day week Cannot be undone Clear browsing data Delete history, cookies, cache and other site data for a period you choose.' },
  { id: 'dashboard', section: 'privacy', label: 'Dashboard', aria: 'Privacy dashboard', description: '', keywords: 'clear cookies cache data permissions camera microphone location notifications statistics Privacy dashboard Review site permissions and blocking statistics.' },
  { id: 'efficiency', section: 'performance', label: 'Efficiency', aria: 'Efficiency mode', description: '', keywords: 'battery power eco energy saver laptop efficiency mode Efficiency While it is in force, hidden tabs use less memory and sleep sooner. Auto: on battery or with Battery saver on.' },
  { id: 'sleep', section: 'performance', label: 'Sleep tabs', aria: 'Sleep unused tabs', description: '', keywords: 'memory discard suspend hibernate inactive hidden tabs Sleep unused tabs Hidden tabs go to sleep after a while to free memory.' },
  { id: 'sleepAfter', section: 'performance', label: 'Sleep after', aria: 'Sleep tabs after', description: '', keywords: 'delay timeout minutes memory Sleep after How long a tab stays hidden before it sleeps. Only while Sleep tabs is on.' },
  ...ADDRESS_ROWS.filter((row) => row.section !== 'appearance'),
  ...PAGE_TOOLS_ROWS,
  ...FEED_ROWS,
  { id: 'kbEnabled', section: 'keyboard', label: 'Shortcuts', aria: 'Keyboard shortcuts', description: '', keywords: 'keyboard shortcuts enable disable hotkeys keybindings keys ctrl shift k Keyboard shortcuts Turn Bowser’s shortcuts on or off. Quick commands stays available.' },
  { id: 'kbSingleKey', section: 'keyboard', label: 'Single-key shortcuts', aria: 'Allow single-key shortcuts', description: '', keywords: 'single key shortcuts letter number plain keys Allow single-key shortcuts Lets a letter, number or punctuation key be a shortcut on its own. They work only from Bowser’s own bars, never while you type in a page or a text field.' },
  { id: 'kbAltMenu', section: 'keyboard', label: 'Alt menu', aria: 'Alt opens the menu', description: '', keywords: 'alt key menu windows Alt opens the menu Pressing and releasing Alt on its own opens the Bowser menu. Off, it does nothing.' },
  { id: 'kbFocusCycling', section: 'keyboard', label: 'F6 focus', aria: 'F6 moves between the address bar and the page', description: '', keywords: 'f6 focus cycle address bar page tab F6 moves between the address bar and the page F6 jumps from the address bar to the page, and back. Off, F6 always goes to the address bar.' },
  { id: 'kbSites', section: 'keyboard', label: 'Site shortcuts', aria: 'Prioritise website shortcuts on these sites', description: '', keywords: 'website shortcuts priority sites page keys web app gmail docs Prioritise website shortcuts on these sites On a site listed here, shortcuts the page uses go to the page. The browser keeps the shortcuts marked Browser priority below. The “Prioritise website shortcuts on this site” command adds or removes the site you are on.' },
  { id: 'kbEditor', section: 'keyboard', label: 'Key bindings', aria: 'Edit shortcuts', description: '', keywords: 'shortcuts assign rebind remap clear restore reset cheat sheet ctrl f1 keybindings browser priority Shortcuts Give a command several shortcuts, clear them or restore the defaults. Ctrl+F1 shows the cheat sheet. Editing shortcuts such as Ctrl+C and Ctrl+V cannot be taken.' },
  { id: 'qcTypes', section: 'commands', label: 'Result types', description: '', keywords: 'quick commands palette results types order prefix bookmark note tab history command action workspace calculator Result types Choose which kinds of result Quick commands shows and in what order. Type bkm:, tb:, cmd: and similar before a search to look in one place. Shift or the right arrow completes the top result.' },
  { id: 'qcMax', section: 'commands', label: 'Result limit', aria: 'Visible results', description: '', keywords: 'quick commands palette results limit maximum number Visible results The most results shown for a typed search.' },
  { id: 'qcKeepLast', section: 'commands', label: 'Remember search', aria: 'Keep the last typed search', description: '', keywords: 'quick commands palette remember last typed value query Keep the last typed search Quick commands opens with what you typed last time, selected, so the first key you press replaces it.' },
  { id: 'qcNewTab', section: 'commands', label: 'Open in tab', aria: 'Open links in a new tab', description: '', keywords: 'quick commands palette new tab bookmark history open link Open links in a new tab A bookmark or history entry picked in Quick commands opens in a new tab, not the one you are on.' },
  { id: 'qcNotesPanel', section: 'commands', label: 'Notes panel', aria: 'Open notes in the Notes panel', description: '', keywords: 'quick commands palette notes manager panel copy Open notes in the Notes panel Picking a note opens the Notes panel; the panel does not yet go to the picked note. Off, it copies the note’s text instead.' },
  { id: 'qcNickname', section: 'commands', label: 'Nicknames', aria: 'Open a bookmark when its nickname is typed', description: '', keywords: 'quick commands palette bookmark nickname automatic open match keyword Open a bookmark when its nickname is typed Typing a bookmark’s whole nickname in Quick commands opens it at once, without pressing Enter. Give a bookmark a nickname in its bookmark dialog.' },
  { id: 'chains', section: 'commands', label: 'Chains', aria: 'Command chains', description: '', keywords: 'command chains macro sequence delay milliseconds test clone create edit delete automation Command chains Run several commands one after another, with a delay in milliseconds before each. Run a chain from Quick commands, give it a shortcut in Keyboard, or test it here.' },
  { id: 'downloadDir', section: 'downloads', label: 'Download folder', description: '', keywords: 'download folder directory save path where change location Download folder' },
  { id: 'downloadAsk', section: 'downloads', label: 'Ask where to save', aria: 'Ask where to save each file', description: '', keywords: 'save as prompt dialog ask where download location before downloading Ask where to save each file Off, files go straight to the folder above. On, the Save dialog opens in it.' },
  { id: 'media', section: 'media', label: 'Playback', aria: 'Media support', description: '', keywords: 'video codec h264 vp9 av1 hevc widevine playready drm netflix hardware decode Media support Runs on this device only and sends nothing.' },
  { id: 'passwords', section: 'passwords', label: 'Passwords and autofill', description: '', keywords: 'password passwords saved logins sign-in credentials autofill auto fill form address addresses name email phone card cards credit debit payment checkout generator generate strong import export csv chrome edge firefox safari 1password bitwarden lastpass dashlane nordpass proton keepass passkeys weak reused' },
  { id: 'extensions', section: 'extensions', label: 'Extensions', description: '', keywords: 'extension add-on addon plugin unpacked crx file manifest permissions popup install remove uninstall disable enable installed Extensions Turn extensions on or off, or remove one.' },
  { id: 'autoUpdate', section: 'updates', label: 'Automatic updates', aria: 'Update automatically', description: '', keywords: 'auto update upgrade version Update automatically Checks in the background and installs the update the next time Bowser starts. Applies to every profile.' },
  { id: 'updateStatus', section: 'updates', label: 'Status', aria: 'Bowser updates', description: '', keywords: 'check for updates restart version upgrade Bowser updates' },
  { id: 'version', section: 'about', label: 'Version', description: '', keywords: 'about build release Version' },
  { id: 'file', section: 'about', label: 'Settings file', description: '', keywords: 'settings.txt advanced edit text shortcuts engines history workspace blocker updates Settings file Most settings are saved in this profile’s settings.txt, which you can also edit by hand. Kept in their own files: shortcuts and command chains, search engines, the history switch, workspace rules, and blocker rules and lists. Automatic updates are shared by all profiles.' },
  { id: 'backupExport', section: 'about', label: 'Export browsing data', aria: 'Export browsing data to a file', description: 'Cookies, saved passwords, history, bookmarks and settings in one file, locked with a password you choose.', keywords: 'backup back up export save file reinstall uninstall move new pc computer cookies passwords history bookmarks settings sign-ins encrypted password' },
  { id: 'backupImport', section: 'about', label: 'Import browsing data', aria: 'Import browsing data from a backup file', description: 'Adds the backup’s cookies and passwords, replaces this profile’s history, bookmarks and settings with its own, then closes and reopens Bowser. Close Bowser’s other windows first.', keywords: 'restore import backup file reinstall uninstall move new pc computer cookies passwords history bookmarks settings open' },
  { id: 'reset', section: 'about', label: 'Reset', aria: 'Reset settings', description: '', keywords: 'defaults restore Restores defaults factory start over tracking prevention Reset settings Puts layout, appearance, toolbars, tabs, start page, address bar, search engines, bookmarks bar, page tools, feeds, blocking and website appearance, performance, downloads and translation back to their defaults. Tracking prevention goes back to its default the next time Bowser starts. Not changed: how long history is kept, your history and bookmarks, the Keyboard section and Quick commands, extensions, site permissions and updates.' },
  { id: 'defaultBrowser', section: 'about', label: 'Default browser', description: '', keywords: 'default browser make bowser windows handler http https web links Default browser Open links from other apps in Bowser.' },
];

export const ROWS: readonly RowSpec[] = BUILT_ROWS.map((row) => {
  const section = placementOf(row.id, row.section).group.section;
  return section === row.section ? row : { ...row, section };
});

export function fold(text: string): string {
  return text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/\s+/g, ' ').trim();
}

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

export const SETTINGS_VIEW_KEY = 'bowser.settings.view';
export type SettingsView = 'simple' | 'all';

export function readSettingsView(raw: string | null): SettingsView {
  return raw === 'all' ? 'all' : 'simple';
}

export interface SimpleGroupSpec { readonly key: string; readonly title: string; readonly rows: readonly string[] }

export const SIMPLE_GROUPS: readonly SimpleGroupSpec[] = [
  { key: 'start', title: 'Startup and search', rows: ['startupMode', 'startupPages', 'startHomepage', 'searchDefault', 'searchSuggestions'] },
  { key: 'look', title: 'Appearance', rows: ['themeMode', 'themeAccent', 'tabPosition', 'toolbarButtons', 'fontSize', 'defaultZoom'] },
  { key: 'privacy', title: 'Privacy', rows: ['blocker', 'deleteData'] },
  { key: 'browser', title: 'Your browser', rows: ['profile', 'import', 'downloadDir', 'defaultBrowser', 'autoUpdate', 'updateStatus'] },
];

export const SIMPLE_MOVED: readonly string[] = SIMPLE_GROUPS.flatMap((spec) => spec.rows);

export const SIMPLE_TITLE = 'Settings';
export const SIMPLE_LEAD = '';
export const SIMPLE_ALL_LABEL = 'All settings';

export function simpleThemeChoice(schedule: string, look: string): 'light' | 'dark' | 'system' | null {
  if (schedule === 'system') return 'system';
  if (schedule === 'none') return look === 'dark' ? 'dark' : 'light';
  return null;
}

export function simpleTabChoice(edge: 'top' | 'left' | 'right' | 'bottom'): 'top' | 'side' | null {
  if (edge === 'left' || edge === 'right') return 'side';
  if (edge === 'top') return 'top';
  return null;
}

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

export const MIN_BACKUP_PASSWORD = 8;
export const MAX_BACKUP_PASSWORD = 1024;

export function backupPasswordProblem(password: string, repeat: string): string | null {
  const length = [...password].length;
  if (length < MIN_BACKUP_PASSWORD) return `Use a password of at least ${MIN_BACKUP_PASSWORD} characters.`;
  if (length > MAX_BACKUP_PASSWORD) return 'That password is too long.';
  if (password !== repeat) return 'The two passwords are different.';
  return null;
}
