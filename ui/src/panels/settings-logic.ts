import { MAX_TEXT } from '../protocol';

const SEARCH_ENGINES = ['optimised', 'google', 'duckduckgo'] as const;
export type SearchEngine = typeof SEARCH_ENGINES[number];
const EFFICIENCY_MODES = ['auto', 'on', 'off'] as const;
export type EfficiencyMode = typeof EFFICIENCY_MODES[number];
export type TrackingPrevention = 'balanced' | 'strict';
export type SettingsSnapshot = {
  tabLayout: 'horizontal' | 'vertical';
  sidebarWidth: number;
  sleepEnabled: boolean;
  sleepAfterSecs: number;
  blockerEnabled: boolean;
  blockerAllow: string[];
  searchEngine: SearchEngine | null;
  searchFallback: 'default' | 'custom';
  automaticTabGrouping: boolean;
  efficiencyMode: EfficiencyMode;
  /** Read-only: whether efficiency mode is in force right now. Never sent back. */
  efficiencyActive: boolean;
  trackingPrevention: TrackingPrevention;
  /** Install-wide (every profile runs the same executable) and applied at once, so never in Save. */
  autoUpdate: boolean;
};
type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isIntBetween = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
const KEYS = ['tabLayout', 'sidebarWidth', 'sleepEnabled', 'sleepAfterSecs', 'blockerEnabled', 'blockerAllow', 'searchEngine', 'searchFallback', 'automaticTabGrouping', 'efficiencyMode', 'efficiencyActive', 'trackingPrevention', 'autoUpdate'];

/** Rejects partial or extended native snapshots before rendering any setting. */
export function parseSettingsSnapshot(value: unknown): SettingsSnapshot | null {
  if (!isRecord(value) || Object.keys(value).length !== KEYS.length || Object.keys(value).some((key) => !KEYS.includes(key))) return null;
  if ((value.tabLayout !== 'horizontal' && value.tabLayout !== 'vertical') || !isIntBetween(value.sidebarWidth, 180, 360)) return null;
  if (typeof value.sleepEnabled !== 'boolean' || !isIntBetween(value.sleepAfterSecs, 30, 604800)) return null;
  if (typeof value.blockerEnabled !== 'boolean' || !Array.isArray(value.blockerAllow) || value.blockerAllow.length > 500 || !value.blockerAllow.every((site) => typeof site === 'string' && !/[\s\u0000-\u001f\u007f]/u.test(site))) return null;
  if (!(value.searchEngine === null || (typeof value.searchEngine === 'string' && SEARCH_ENGINES.includes(value.searchEngine as SearchEngine)))) return null;
  if (value.searchFallback !== 'default' && value.searchFallback !== 'custom') return null;
  if (typeof value.automaticTabGrouping !== 'boolean') return null;
  if (!EFFICIENCY_MODES.includes(value.efficiencyMode as EfficiencyMode) || typeof value.efficiencyActive !== 'boolean') return null;
  if (value.trackingPrevention !== 'balanced' && value.trackingPrevention !== 'strict') return null;
  if (typeof value.autoUpdate !== 'boolean') return null;
  if (value.searchEngine !== null && value.searchFallback !== 'default') return null;
  return {
    tabLayout: value.tabLayout,
    sidebarWidth: value.sidebarWidth,
    sleepEnabled: value.sleepEnabled,
    sleepAfterSecs: value.sleepAfterSecs,
    blockerEnabled: value.blockerEnabled,
    blockerAllow: [...value.blockerAllow],
    searchEngine: value.searchEngine as SearchEngine | null,
    searchFallback: value.searchFallback,
    automaticTabGrouping: value.automaticTabGrouping,
    efficiencyMode: value.efficiencyMode as EfficiencyMode,
    efficiencyActive: value.efficiencyActive,
    trackingPrevention: value.trackingPrevention,
    autoUpdate: value.autoUpdate,
  };
}

export type SettingsCommand = { a: 'save'; text: string } | { a: 'reset' } | { a: 'openfile' } | { a: 'autoupdate'; on: boolean };
/** Uses the generic settings panel's existing flat `text` field; backend dispatch is not wired yet. */
export function settingsCommand(action: 'save' | 'reset' | 'openfile' | 'autoupdate', value?: unknown): SettingsCommand | null {
  if (action === 'autoupdate') return typeof value === 'boolean' ? { a: 'autoupdate', on: value } : null;
  if (action === 'reset') return value === undefined ? { a: 'reset' } : null;
  if (action === 'openfile') return value === undefined ? { a: 'openfile' } : null;
  if (action !== 'save') return null;
  const parsed = parseSettingsSnapshot(value);
  if (!parsed) return null;
  const lines = [
    `tab_layout=${parsed.tabLayout}`,
    `sidebar_width=${parsed.sidebarWidth}`,
    `sleep_enabled=${parsed.sleepEnabled}`,
    `sleep_after_secs=${parsed.sleepAfterSecs}`,
    `blocker_enabled=${parsed.blockerEnabled}`,
    `blocker_allow=${parsed.blockerAllow.join(',')}`,
    `automatic_tab_grouping=${parsed.automaticTabGrouping}`,
    `efficiency_mode=${parsed.efficiencyMode}`,
    `tracking_prevention=${parsed.trackingPrevention}`,
  ];
  if (parsed.searchEngine !== null) lines.push(`search_engine=${parsed.searchEngine}`);
  const text = lines.join('\n');
  return text.length <= MAX_TEXT ? { a: 'save', text } : null;
}
