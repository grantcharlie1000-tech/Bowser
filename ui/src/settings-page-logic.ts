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
}

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isIntBetween = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
const isStrUpTo = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);
const KEYS = ['tabLayout', 'sidebarWidth', 'sleepEnabled', 'sleepAfterSecs', 'blockerEnabled', 'blockerAllow', 'searchEngine', 'searchFallback',
  'automaticTabGrouping', 'efficiencyMode', 'efficiencyActive', 'trackingPrevention', 'autoUpdate', 'historyEnabled', 'guest', 'profile', 'avatar',
  'version', 'update', 'updateVersion', 'lastCheck', 'checkResult', 'ack', 'error', 'readable'];
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
  };
}

/** The profile settings `save` carries (everything else in the snapshot is read-only). */
export type Draft = Pick<SettingsSnapshot, 'tabLayout' | 'sidebarWidth' | 'sleepEnabled' | 'sleepAfterSecs' | 'blockerEnabled' | 'blockerAllow' | 'searchEngine' | 'automaticTabGrouping' | 'efficiencyMode' | 'trackingPrevention'>;

export function draftOf(snapshot: SettingsSnapshot): Draft {
  const { tabLayout, sidebarWidth, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow, searchEngine, automaticTabGrouping, efficiencyMode, trackingPrevention } = snapshot;
  return { tabLayout, sidebarWidth, sleepEnabled, sleepAfterSecs, blockerEnabled, blockerAllow: [...blockerAllow], searchEngine, automaticTabGrouping, efficiencyMode, trackingPrevention };
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
  const text = lines.join('\n');
  return text.length <= MAX_TEXT ? text : null;
}

// ---- Sections and rows (the nav, the search index and the deep links) ----

export interface SectionSpec { readonly key: SettingsSection; readonly title: string; readonly nav: string; readonly lead: string }
export const SECTIONS: readonly SectionSpec[] = [
  { key: 'profile', title: 'You and Bowser', nav: 'You and Bowser', lead: 'The profile these settings belong to.' },
  { key: 'appearance', title: 'Appearance', nav: 'Appearance', lead: 'Where the toolbar and the tabs sit, and how tabs are grouped.' },
  { key: 'privacy', title: 'Privacy and security', nav: 'Privacy and security', lead: 'Blocking, tracking prevention and history.' },
  { key: 'performance', title: 'Performance', nav: 'Performance', lead: 'Save memory and power in the background.' },
  { key: 'search', title: 'Search engine', nav: 'Search engine', lead: 'What the address bar searches with.' },
  { key: 'media', title: 'Media', nav: 'Media', lead: 'Which video formats and protected streams this device can play.' },
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
}

/** Every row the page draws, in page order. The view builds a control per id. */
export const ROWS: readonly RowSpec[] = [
  { id: 'profile', section: 'profile', label: 'Profile', description: 'Settings on this page apply to this profile only.', keywords: 'account person name avatar guest' },
  { id: 'profiles', section: 'profile', label: 'Manage profiles', description: 'Switch, create, rename or delete profiles.', keywords: 'switch new guest delete rename' },
  { id: 'import', section: 'profile', label: 'Import from another browser', description: 'Bring bookmarks and other data over from a browser on this PC.', keywords: 'chrome edge firefox migrate transfer bookmarks' },
  { id: 'layout', section: 'appearance', label: 'Browser layout', description: 'Where the toolbar and the tabs sit.', keywords: 'tabs toolbar vertical horizontal sidebar single row top' },
  { id: 'sidebarWidth', section: 'appearance', label: 'Sidebar width', description: 'Width of the tab sidebar in the vertical layout.', keywords: 'vertical tabs size wide narrow pixels' },
  { id: 'grouping', section: 'appearance', label: 'Group tabs by site', description: 'Tabs from one site share an island. Groups you make by hand stay.', keywords: 'automatic tab groups islands domain', restart: true },
  { id: 'blocker', section: 'privacy', label: 'Block ads and trackers', description: 'Stops known ad and tracking requests before they load.', keywords: 'adblock ad blocker shield trackers', restart: true },
  { id: 'allow', section: 'privacy', label: 'Sites the blocker skips', description: 'The blocker never blocks on these sites.', keywords: 'allow list exceptions whitelist adblock', restart: true },
  { id: 'tracking', section: 'privacy', label: 'Tracking prevention', description: 'Balanced is the default. Strict blocks more trackers but can break some sites.', keywords: 'privacy cookies strict balanced fingerprinting', restart: true },
  { id: 'history', section: 'privacy', label: 'Save browsing history', description: 'Pages you visit are listed in History and suggested in the address bar.', keywords: 'history visits record' },
  { id: 'dashboard', section: 'privacy', label: 'Privacy dashboard', description: 'Clear browsing data, review site permissions and blocking statistics.', keywords: 'clear cookies cache data permissions camera microphone location notifications statistics' },
  { id: 'efficiency', section: 'performance', label: 'Efficiency mode', description: 'Auto saves power on battery or in Battery Saver: hidden tabs sleep sooner and background tabs use less memory.', keywords: 'battery power eco energy saver laptop' },
  { id: 'sleep', section: 'performance', label: 'Sleep hidden tabs', description: 'Hidden tabs are put to sleep after a delay to free memory, sooner under memory pressure.', keywords: 'memory discard suspend hibernate inactive' },
  { id: 'sleepAfter', section: 'performance', label: 'Sleep after', description: 'How long a tab stays hidden before it sleeps.', keywords: 'delay timeout minutes memory' },
  { id: 'engine', section: 'search', label: 'Search engine', description: 'Used for searches typed in the address bar. Type img before a query for images.', keywords: 'google duckduckgo optimised default address bar omnibox images' },
  { id: 'media', section: 'media', label: 'Media support', description: 'Runs on this device only and sends nothing.', keywords: 'video codec h264 vp9 av1 hevc widevine playready drm netflix hardware decode' },
  { id: 'autoUpdate', section: 'updates', label: 'Update automatically', description: 'Checks in the background and installs the update the next time Bowser starts. Applies to every profile.', keywords: 'auto update upgrade version' },
  { id: 'updateStatus', section: 'updates', label: 'Bowser updates', description: '', keywords: 'check for updates restart version upgrade' },
  { id: 'version', section: 'about', label: 'Version', description: '', keywords: 'about build release' },
  { id: 'file', section: 'about', label: 'Settings file', description: 'Every setting here is saved in this profile’s settings.txt, which you can also edit by hand.', keywords: 'settings.txt advanced edit text' },
  { id: 'reset', section: 'about', label: 'Reset settings', description: 'Puts layout, tabs, blocking, performance and search back to their defaults. History, updates and bookmarks are not changed.', keywords: 'defaults restore factory start over' },
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
