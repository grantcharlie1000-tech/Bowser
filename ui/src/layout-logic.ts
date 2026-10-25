// Pure logic for the settings page's Layout block (thread T6): the `layout` object of the settings
// snapshot, its save lines, the presets and the Saved configuration, the tab bar edge, and the UI
// zoom value. Mirrors src/layout_prefs.rs (keys, ranges and the `LayoutConfig` text). No DOM here;
// test/settings-page.mjs covers it.

/** Browser layouts, in the order the page lists them. `horizontal` is the saved key of "toolbar above tabs". */
export const TAB_LAYOUTS = ['horizontal', 'tabs-top', 'single-row', 'vertical', 'vertical-right', 'bottom'] as const;
export type TabLayout = typeof TAB_LAYOUTS[number];
/** The layouts whose tab bar is a row at the top: the "style at the top" sub-choice. */
export const TOP_TAB_STYLES = ['horizontal', 'tabs-top', 'single-row'] as const;
export const TAB_LAYOUT_LABELS: Readonly<Record<TabLayout, string>> = {
  horizontal: 'Address',
  'tabs-top': 'Tabs',
  'single-row': 'One row',
  vertical: 'Left',
  'vertical-right': 'Right',
  bottom: 'Bottom',
};
/** Fuller names for the top-style tiles. Never shown. */
export const TAB_LAYOUT_ARIA: Readonly<Record<TabLayout, string>> = {
  horizontal: 'Toolbar above tabs',
  'tabs-top': 'Tabs above toolbar',
  'single-row': 'Single row',
  vertical: 'Vertical',
  'vertical-right': 'Vertical, on the right',
  bottom: 'Tabs along the bottom',
};
export const LAYOUT_HINTS: Readonly<Record<TabLayout, string>> = {
  horizontal: 'The address bar on top, tabs underneath.',
  'tabs-top': 'Tabs on top, the address bar underneath.',
  'single-row': 'Everything in one row, for small screens.',
  vertical: 'Tabs in a sidebar on the left.',
  'vertical-right': 'Tabs in a sidebar on the right.',
  bottom: 'Tabs in a row along the bottom edge.',
};

export const TAB_BAR_EDGES = ['top', 'left', 'right', 'bottom'] as const;
export type TabBarEdge = typeof TAB_BAR_EDGES[number];
export const TAB_BAR_EDGE_LABELS: Readonly<Record<TabBarEdge, string>> = { top: 'Top', left: 'Left', right: 'Right', bottom: 'Bottom' };
export const BAR_POSITIONS = ['top', 'bottom'] as const;
export type BarPosition = typeof BAR_POSITIONS[number];
export const SIDE_PANEL_POSITIONS = ['left', 'right'] as const;
export type SidePanelPosition = typeof SIDE_PANEL_POSITIONS[number];
export const STATUS_BARS = ['overlay', 'hidden'] as const;
export type StatusBar = typeof STATUS_BARS[number];
export const STATUS_BAR_LABELS: Readonly<Record<StatusBar, string>> = { overlay: 'Info overlay only', hidden: 'Hidden' };
export const UI_DENSITIES = ['regular', 'compact'] as const;
export type UiDensity = typeof UI_DENSITIES[number];
export const MIN_UI_ZOOM = 50;
export const MAX_UI_ZOOM = 200;
export const DEFAULT_UI_ZOOM = 100;
export const UI_ZOOM_STEP = 5;
/** Longest `layout_saved` value Rust keeps. */
export const MAX_LAYOUT_SAVED = 160;

/**
 * The edge the side panel uses. A visible vertical tab bar takes one edge, so the panel takes the
 * other. `saved` is `side_panel_position` and is what a row layout, or a hidden tab bar, uses.
 * The result is not written back.
 */
export function effectiveSidePanelPosition(saved: SidePanelPosition, tabLayout: TabLayout, tabBarVisible: boolean): SidePanelPosition {
  if (!tabBarVisible) return saved;
  if (tabLayout === 'vertical') return 'right';
  if (tabLayout === 'vertical-right') return 'left';
  return saved;
}

/** Shown under the Panels control while a vertical tab bar is on screen, or `''` otherwise. */
export function panelEdgeNote(saved: SidePanelPosition, tabLayout: TabLayout, tabBarVisible: boolean): string {
  if (!tabBarVisible || (tabLayout !== 'vertical' && tabLayout !== 'vertical-right')) return '';
  const live = effectiveSidePanelPosition(saved, tabLayout, tabBarVisible);
  const where = live === 'left' ? 'the left' : 'the right';
  if (saved === live) return `Panels open on ${where}, opposite the vertical tabs.`;
  const kept = saved === 'left' ? 'Left' : 'Right';
  return `Panels open on ${where}, opposite the vertical tabs. ${kept} is kept for when the tab bar is a row.`;
}

/** Where the tab bar sits for a layout. */
export function tabBarEdge(layout: TabLayout): TabBarEdge {
  switch (layout) {
    case 'vertical': return 'left';
    case 'vertical-right': return 'right';
    case 'bottom': return 'bottom';
    default: return 'top';
  }
}

/** The layout for `edge`: a top edge keeps the current top style (or takes the first when the tabs were elsewhere). */
export function tabLayoutForEdge(edge: TabBarEdge, current: TabLayout): TabLayout {
  switch (edge) {
    case 'left': return 'vertical';
    case 'right': return 'vertical-right';
    case 'bottom': return 'bottom';
    default: return (TOP_TAB_STYLES as readonly string[]).includes(current) ? current : 'horizontal';
  }
}

/** The `layout` object of the settings snapshot (`layout_prefs.rs`). */
export interface LayoutSettings {
  tabBarVisible: boolean;
  addressBarPosition: BarPosition;
  addressBarVisible: boolean;
  showTitleBar: boolean;
  statusBar: StatusBar;
  uiDensity: UiDensity;
  compactMenus: boolean;
  uiZoom: number;
  /** The user's own configuration as `layoutConfigKey`, or `''`: the Saved tile. */
  saved: string;
}

const LAYOUT_KEYS = ['tabBarVisible', 'addressBarPosition', 'addressBarVisible', 'showTitleBar', 'statusBar', 'uiDensity', 'compactMenus', 'uiZoom', 'saved'] as const;
const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const oneOf = <T extends string>(value: unknown, list: readonly T[]): value is T => typeof value === 'string' && (list as readonly string[]).includes(value);
const isUiZoom = (value: unknown): value is number => typeof value === 'number' && Number.isInteger(value) && value >= MIN_UI_ZOOM && value <= MAX_UI_ZOOM;

/** Rejects a partial or extended `layout` object, or any value Rust would not have written. */
export function parseLayoutSettings(value: unknown): LayoutSettings | null {
  if (!isRecord(value) || Object.keys(value).length !== LAYOUT_KEYS.length || Object.keys(value).some((key) => !(LAYOUT_KEYS as readonly string[]).includes(key))) return null;
  const v = value;
  for (const key of ['tabBarVisible', 'addressBarVisible', 'showTitleBar', 'compactMenus'] as const) if (typeof v[key] !== 'boolean') return null;
  if (!oneOf(v.addressBarPosition, BAR_POSITIONS) || !oneOf(v.statusBar, STATUS_BARS) || !oneOf(v.uiDensity, UI_DENSITIES) || !isUiZoom(v.uiZoom)) return null;
  if (typeof v.saved !== 'string' || !isLayoutSaved(v.saved)) return null;
  return {
    tabBarVisible: v.tabBarVisible as boolean,
    addressBarPosition: v.addressBarPosition,
    addressBarVisible: v.addressBarVisible as boolean,
    showTitleBar: v.showTitleBar as boolean,
    statusBar: v.statusBar,
    uiDensity: v.uiDensity,
    compactMenus: v.compactMenus as boolean,
    uiZoom: v.uiZoom,
    saved: v.saved,
  };
}

/** The save lines for the Layout block, in the order Rust lists them, or `null` for a value Rust would refuse. */
export function layoutSaveLines(layout: LayoutSettings): string[] | null {
  if (!oneOf(layout.addressBarPosition, BAR_POSITIONS) || !oneOf(layout.statusBar, STATUS_BARS) || !oneOf(layout.uiDensity, UI_DENSITIES)) return null;
  if (!isUiZoom(layout.uiZoom) || !isLayoutSaved(layout.saved)) return null;
  return [
    `tab_bar_visible=${layout.tabBarVisible}`,
    `address_bar_position=${layout.addressBarPosition}`,
    `address_bar_visible=${layout.addressBarVisible}`,
    `show_title_bar=${layout.showTitleBar}`,
    `status_bar=${layout.statusBar}`,
    `ui_density=${layout.uiDensity}`,
    `compact_menus=${layout.compactMenus}`,
    `ui_zoom=${layout.uiZoom}`,
    `layout_saved=${layout.saved}`,
  ];
}

/** The settings a preset chooses: one value, the same text Rust's `LayoutConfig` keeps in `layout_saved`. */
export interface LayoutConfig {
  readonly tabLayout: TabLayout;
  readonly tabBarVisible: boolean;
  readonly addressBarPosition: BarPosition;
  readonly addressBarVisible: boolean;
  readonly sidePanelPosition: SidePanelPosition;
  readonly statusBar: StatusBar;
  readonly showTitleBar: boolean;
}

export const DEFAULT_LAYOUT_CONFIG: LayoutConfig = {
  tabLayout: 'horizontal', tabBarVisible: true, addressBarPosition: 'top', addressBarVisible: true, sidePanelPosition: 'left', statusBar: 'overlay', showTitleBar: false,
};

/** `tabs=vertical,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false` (Rust's `LayoutConfig::key`). */
export function layoutConfigKey(c: LayoutConfig): string {
  return `tabs=${c.tabLayout},tabbar=${c.tabBarVisible},address=${c.addressBarPosition},addressbar=${c.addressBarVisible},panel=${c.sidePanelPosition},status=${c.statusBar},title=${c.showTitleBar}`;
}

const flag = (value: string): boolean | null => (value === 'true' ? true : value === 'false' ? false : null);

/** Reads `layoutConfigKey`'s form: every field once, exact keys and values. */
export function parseLayoutConfig(text: string): LayoutConfig | null {
  const seen = new Set<string>();
  let config: LayoutConfig = DEFAULT_LAYOUT_CONFIG;
  for (const pair of text.split(',')) {
    const at = pair.indexOf('=');
    if (at < 0) return null;
    const key = pair.slice(0, at);
    const value = pair.slice(at + 1);
    if (seen.has(key)) return null;
    seen.add(key);
    switch (key) {
      case 'tabs': if (!oneOf(value, TAB_LAYOUTS)) return null; config = { ...config, tabLayout: value }; break;
      case 'tabbar': { const on = flag(value); if (on === null) return null; config = { ...config, tabBarVisible: on }; break; }
      case 'address': if (!oneOf(value, BAR_POSITIONS)) return null; config = { ...config, addressBarPosition: value }; break;
      case 'addressbar': { const on = flag(value); if (on === null) return null; config = { ...config, addressBarVisible: on }; break; }
      case 'panel': if (!oneOf(value, SIDE_PANEL_POSITIONS)) return null; config = { ...config, sidePanelPosition: value }; break;
      case 'status': if (!oneOf(value, STATUS_BARS)) return null; config = { ...config, statusBar: value }; break;
      case 'title': { const on = flag(value); if (on === null) return null; config = { ...config, showTitleBar: on }; break; }
      default: return null;
    }
  }
  return seen.size === 7 ? config : null;
}

/** Whether `value` may be stored as `layout_saved`: a config text, or empty for none. */
export function isLayoutSaved(value: string): boolean {
  return value === '' || (value.length <= MAX_LAYOUT_SAVED && parseLayoutConfig(value) !== null);
}

export function sameLayoutConfig(a: LayoutConfig, b: LayoutConfig): boolean {
  return layoutConfigKey(a) === layoutConfigKey(b);
}

/** The presets, in the order the page shows them; `saved` (the user's own configuration) follows them on the page. */
export const PRESETS = ['simple', 'classic', 'vertical-left', 'vertical-right', 'bottom'] as const;
export type Preset = typeof PRESETS[number];
export const PRESET_LABELS: Readonly<Record<Preset | 'saved', string>> = {
  simple: 'Simple', classic: 'Classic', 'vertical-left': 'Vertical left', 'vertical-right': 'Vertical right', bottom: 'Bottom', saved: 'Saved',
};
export const PRESET_HINTS: Readonly<Record<Preset | 'saved', string>> = {
  simple: 'One row, no overlay: the quietest chrome.',
  classic: 'Tabs above the address bar.',
  'vertical-left': 'Tabs on the left, panels on the right.',
  'vertical-right': 'Tabs on the right, panels on the left.',
  bottom: 'Tabs along the bottom edge.',
  saved: 'Your own arrangement, kept up to date.',
};

/** The settings a preset chooses (Rust's `Preset::config`). */
export function presetConfig(preset: Preset): LayoutConfig {
  switch (preset) {
    case 'simple': return { ...DEFAULT_LAYOUT_CONFIG, tabLayout: 'single-row', statusBar: 'hidden' };
    case 'classic': return { ...DEFAULT_LAYOUT_CONFIG, tabLayout: 'tabs-top' };
    case 'vertical-left': return { ...DEFAULT_LAYOUT_CONFIG, tabLayout: 'vertical', sidePanelPosition: 'left' };
    case 'vertical-right': return { ...DEFAULT_LAYOUT_CONFIG, tabLayout: 'vertical-right', sidePanelPosition: 'right' };
    case 'bottom': return { ...DEFAULT_LAYOUT_CONFIG, tabLayout: 'bottom' };
  }
}

/** The preset whose configuration `config` is, if any. */
export function matchingPreset(config: LayoutConfig): Preset | null {
  return PRESETS.find((preset) => sameLayoutConfig(presetConfig(preset), config)) ?? null;
}

/** The fields of a draft the Layout block's configuration lives in. */
export interface LayoutDraft {
  tabLayout: TabLayout;
  sidePanelPosition: SidePanelPosition;
  layout: LayoutSettings;
}

export function layoutConfigOf(draft: LayoutDraft): LayoutConfig {
  return {
    tabLayout: draft.tabLayout,
    tabBarVisible: draft.layout.tabBarVisible,
    addressBarPosition: draft.layout.addressBarPosition,
    addressBarVisible: draft.layout.addressBarVisible,
    sidePanelPosition: draft.sidePanelPosition,
    statusBar: draft.layout.statusBar,
    showTitleBar: draft.layout.showTitleBar,
  };
}

/** Puts `config` into the draft (a preset pick, or the Saved tile). Everything else stays. */
export function applyLayoutConfig(draft: LayoutDraft, config: LayoutConfig): void {
  draft.tabLayout = config.tabLayout;
  draft.sidePanelPosition = config.sidePanelPosition;
  draft.layout.tabBarVisible = config.tabBarVisible;
  draft.layout.addressBarPosition = config.addressBarPosition;
  draft.layout.addressBarVisible = config.addressBarVisible;
  draft.layout.statusBar = config.statusBar;
  draft.layout.showTitleBar = config.showTitleBar;
}

/**
 * After a change to one of the configuration's rows: a configuration that is no preset becomes the
 * Saved one, so the user's own arrangement is kept up to date and survives a try of a preset. A
 * configuration that is a preset leaves Saved as it was.
 */
export function rememberOwnLayout(draft: LayoutDraft): void {
  const config = layoutConfigOf(draft);
  if (matchingPreset(config) === null) draft.layout.saved = layoutConfigKey(config);
}

/** Which tile is selected: the matching preset, `saved` when the configuration is the Saved one, else none. */
export function selectedTile(draft: LayoutDraft): Preset | 'saved' | null {
  const config = layoutConfigOf(draft);
  const preset = matchingPreset(config);
  if (preset !== null) return preset;
  const saved = parseLayoutConfig(draft.layout.saved);
  return saved !== null && sameLayoutConfig(saved, config) ? 'saved' : null;
}

/** A typed UI zoom: a whole percentage (a trailing % is fine), rounded to the slider's step, in range. `null` otherwise. */
export function parseUiZoom(text: string): number | null {
  const trimmed = text.trim().replace(/%$/, '').trim();
  if (!/^\d{1,3}$/.test(trimmed)) return null;
  const typed = Number(trimmed);
  // Out of range is refused as typed (as Rust refuses it), not nudged into range by the rounding.
  if (typed < MIN_UI_ZOOM || typed > MAX_UI_ZOOM) return null;
  return Math.round(typed / UI_ZOOM_STEP) * UI_ZOOM_STEP;
}

export function uiZoomText(value: number): string {
  return `${value}%`;
}
