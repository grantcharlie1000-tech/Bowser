import { onChromeInbound } from './chrome-handshake';
// Tab strip view (?view=tabs): keyed reconciliation of the tab list, favicons, groups, pointer and keyboard input.
// Page-controlled text is written with textContent only; favicons are data: URIs set as img.src.
import { send, sendClosedTabs, sendGroupMove, sendTabSelection, sendTabStack, sendTool, type TabAction, type TabItem, type TabOptsMsg, type TabsMsg } from './protocol';
import { el, icon, place, type IconName } from './dom';
import { itemIcon } from './bars-logic'; // --- toolbars and feeds (thread T13) ---
import { applyTheme, DRAG_ATTR } from './view';
import { setTip } from './tooltip';
import { guardTitle } from './spoiler-guard'; // --- spoiler guard ---
// --- split view ---
import { markPair } from './split-chrome';
import { overPage, pairOrder, pairWeight, wirePoint } from './split-logic';
import { validSplitCommand, type PairMsg, type SplitCommand } from './split-protocol';
// --- end split view ---
// --- vertical sidebar (lane S) ---
import { createSidebarRenderer, type SidebarWorkspace } from './sidebar';
// --- end vertical sidebar ---
// --- tab ghost (docs/design/tab-drag.md) ---
import { announce, GhostSession, routeGhostMessage } from './tab-drag';
import { DRAG_START_PX, dragAnnouncement, dropOutcome, gapShifts, grabOffset, nextPhase, stackAnnouncement, stackCandidate, stripBand, type DragPhase } from './tab-drag-logic';
// --- end tab ghost ---
import {
  beforeSlotId,
  focusHeir,
  groupColourIndex,
  buildEntries,
  dropIndex,
  dragScrollDelta,
  flipInvert,
  groupWeight,
  hiddenForTab,
  landingFirst,
  landingPhase,
  layoutSignature,
  privateTabName,
  pruneFavs,
  rememberFav,
  revealKey,
  stepMoveBefore,
  scrollDeltaForWheel,
  type Entry,
  type GroupEntry,
} from './tabs-logic';
// --- group drag --- a whole island picked up by its handle.
import { blockDropIndex, clampBlock, groupDragAnnouncement, groupMoveBefore, groupStepBefore, groupUnits, moveChordDir } from './tabs-logic';
// --- end group drag ---
// --- tab options (Settings > Tabs) ---
import {
  clickSelection, loadMark, lockedWidth, markedName, notifyLabel, pruneSelection, reloadLabel, reloadRemaining, stepTab, stripClasses, wheelSteps,
  type LoadIndicatorChoice,
} from './tab-marks-logic';
// --- end tab options ---

/** Fixed island accents, indexed by the host's colour ids (-1 means the theme accent). */
const ISLAND: readonly string[] = [
  'var(--bw-group-0)',
  'var(--bw-group-1)',
  'var(--bw-group-2)',
  'var(--bw-group-3)',
  'var(--bw-group-4)',
  'var(--bw-group-5)',
  'var(--bw-group-6)',
  'var(--bw-group-7)',
];
const ACCENT = 'var(--bw-accent)';
/** Above this many tabs, position animations are skipped (each one costs a measured layout). */
const FLIP_MAX_TABS = 80;
/** Scale of the held tab; it rides inside the landing transform and reaches 1 as the tab arrives. */
const LIFT_SCALE = 1.02;
/** Wait for the host's reorder after a drop before the tab eases back to where it started. */
const REFUSE_MS = 400;
/** Longest a landing transition is waited for (167ms plus slack) before `.landing` is dropped anyway. */
const LAND_FALLBACK_MS = 250;

/** A dropped tab on its way to its slot. `timer` is the refuse fallback, then the transition fallback. */
interface Landing {
  readonly node: TabNode;
  readonly signature: string;
  timer: number;
  off: (() => void) | null;
}

export interface TabNode {
  readonly id: number;
  readonly root: HTMLDivElement;
  readonly img: HTMLImageElement;
  readonly title: HTMLSpanElement;
  readonly marker: HTMLSpanElement;
  readonly audio: HTMLButtonElement;
  readonly close: HTMLButtonElement;
  uri: string;
  shift: number;
  // --- tab options --- created only while a tab needs them, so a plain tab keeps its plain DOM.
  bar?: HTMLSpanElement;
  ring?: HTMLSpanElement;
  dot?: HTMLSpanElement;
  badge?: HTMLSpanElement;
  rl?: HTMLSpanElement;
  /** Seconds left in the tab's reload when the strip last heard (and when), so the label can count down locally. */
  rlFrom?: number;
  rlAt?: number;
  // --- end tab options ---
}

/** What one tab needs from the strip's options when it is drawn. */
export interface TabView {
  readonly indicator: LoadIndicatorChoice;
  /** Part of the selection made with Ctrl or Shift click. */
  readonly picked: boolean;
  readonly now: number;
}
const PLAIN_VIEW: TabView = { indicator: 'spinner', picked: false, now: 0 };
/** Before the host's first `tabopts`: Bowser's own defaults. */
const DEFAULT_OPTS: TabOptsMsg = {
  t: 'tabopts', closeLeft: false, closeAlways: false, scroll: false, minWidth: 0, indicator: 'spinner', wheel: false, separator: false,
  alignClose: false, selection: true, selectionActive: true, removeSpacing: false, maximised: false, closed: 0,
  closedButton: true, tools: [], // --- toolbars and feeds (thread T13) ---
};
const STRIP_OPTION_CLASSES = ['close-left', 'close-always', 'scrolling', 'pin-sep', 'flush'] as const;
/** How long (ms) a held tab rests over the middle of another before a release stacks the two (Rust `stacks::STACK_DROP_DELAY_MS`). */
export const STACK_DROP_DELAY_MS = 400;

/** Counts every reload label down by the time since the host last said how long was left. Exported for test/tab-options.mjs. */
export function tickReloads(nodes: Iterable<TabNode>, now: number): void {
  for (const n of nodes) {
    if (n.rl && n.rlFrom !== undefined && n.rlAt !== undefined) n.rl.textContent = reloadLabel(reloadRemaining(n.rlFrom, now - n.rlAt));
  }
}

interface GroupNode {
  readonly shell: HTMLDivElement;
  readonly ball: HTMLButtonElement;
  /** The closing mark after a folded island's shown tab: a sliver per tab folded behind it. */
  readonly cap: HTMLSpanElement;
  label: string;
  color: string;
  weight: number;
  visible: number;
  /** Slivers the closing mark shows (0 to 2). */
  more: number;
  /** Observes the handle so the island's flex-basis can reserve its width (a bar, or a pill when folded away). */
  readonly watch: ResizeObserver;
  /** Horizontal offset of the whole island (px): a group drag moves it, or opens a gap with it. */
  shift: number;
}

// --- group drag ---
/** One top-level unit of the strip while a group is held: an unpinned tab or a whole island. */
interface DragUnit {
  readonly el: HTMLElement;
  /** The tab a move in front of this unit names (an island's first member). */
  readonly first: number;
  readonly tab?: TabNode;
  readonly group?: GroupNode;
}

/** A press on an island's handle (bar, folded pill or closing mark): a click, or a drag of the whole island. */
interface GroupDrag {
  readonly pointerId: number;
  readonly key: number;
  readonly group: GroupNode;
  /** The handle pressed; it holds the pointer capture. */
  readonly handle: HTMLElement;
  readonly startX: number;
  readonly startY: number;
  readonly startScroll: number;
  /** Unpinned tabs and islands in strip order when the press began; the island at `from`. */
  readonly units: readonly DragUnit[];
  readonly from: number;
  centers: readonly number[];
  lefts: readonly number[];
  readonly widths: readonly number[];
  active: boolean;
  dest: number;
  lastX: number;
}

/** A dropped island on its way to its slot (as `Landing` for a tab). */
interface GroupLanding {
  readonly group: GroupNode;
  readonly signature: string;
  timer: number;
  off: (() => void) | null;
}
// --- end group drag ---

interface Drag {
  readonly pointerId: number;
  readonly startX: number;
  readonly node: TabNode;
  /** Visible tabs in strip order when the drag started. */
  readonly order: readonly TabNode[];
  centers: readonly number[];
  readonly from: number;
  active: boolean;
  dest: number;
  /** Pointer travel since the drag started (px). */
  dx: number;
  // --- tab ghost ---
  readonly startY: number;
  readonly pointerType: string;
  /** Home (unshifted) left edge and width of every tab in `order`, for the gap offsets. */
  lefts: readonly number[];
  readonly widths: readonly number[];
  /** Attached to the strip (reorders) or floating off it (follows the pointer over the page). */
  phase: DragPhase;
  /** The native ghost, when Rust shows one (mouse drags). */
  ghost: GhostSession | null;
  lastX: number;
  lastY: number;
  // --- end tab ghost ---
  // --- tab stacks (T10) --- the tab the held tab rests over the middle of, since when, and whether
  // the stacking delay has run (a release then stacks the two).
  stackOn: TabNode | null;
  stackArmed: boolean;
  stackTimer: number;
}

function colourOf(index: number): string {
  return ISLAND[index & 7] ?? ACCENT;
}

/** Exported for test/private.mjs (fake DOM); the strip itself is built by startTabs. */
export function createTab(id: number): TabNode {
  const root = el('div', 'tab bw-focus-inset');
  root.setAttribute('role', 'tab');
  root.dataset.id = String(id);
  root.dataset.icon = 'glyph';

  const img = el('img', 'fav');
  img.alt = '';
  img.draggable = false;
  // Placeholder for tabs without a favicon: the family's globe glyph, no tile.
  const glyph = el('span', 'glyph');
  glyph.setAttribute('aria-hidden', 'true');
  glyph.append(icon('workspace-globe'));
  const spin = el('span', 'spin bw-spinner');
  // Private tabs (tabs.css): with no favicon the private glyph replaces the globe; with a favicon (or
  // the spinner) a small glasses badge sits inside the icon's corner, out of flow, so neither ever
  // moves or sizes anything in the row.
  const privGlyph = el('span', 'priv-glyph');
  privGlyph.setAttribute('aria-hidden', 'true');
  privGlyph.append(icon('private'));
  const priv = el('span', 'priv');
  priv.setAttribute('aria-hidden', 'true');
  priv.append(icon('glasses', { size: 8, fine: true }));
  const ico = el('span', 'ico');
  ico.append(img, glyph, privGlyph, spin, priv);

  const title = el('span', 'title');
  title.dir = 'auto';
  const marker = el('span', 'marker');
  marker.setAttribute('aria-hidden', 'true');

  const audio = el('button', 'audio bw-icon-btn');
  audio.type = 'button';
  audio.tabIndex = -1;
  audio.hidden = true;
  audio.setAttribute('aria-label', 'Unmute tab');
  audio.append(icon('mute'));

  const close = el('button', 'close bw-icon-btn');
  close.type = 'button';
  close.tabIndex = -1;
  close.setAttribute('aria-label', 'Close tab: Tab');
  close.append(icon('close'));

  root.append(ico, title, marker, audio, close);
  return { id, root, img, title, marker, audio, close, uri: '', shift: 0 };
}

/** A mark node of a tab: created and put before `before` the first time, dropped when the mark goes. */
function mark(n: TabNode, key: 'bar' | 'ring' | 'dot' | 'badge' | 'rl', on: boolean, parent: HTMLElement, before: Node | null): HTMLSpanElement | undefined {
  const have = n[key];
  if (!on) {
    if (have) { have.remove(); delete n[key]; }
    return undefined;
  }
  if (have) return have;
  const node = el('span', key === 'bar' ? 'loadbar' : key === 'ring' ? 'ring' : key === 'dot' ? 'unread' : key === 'badge' ? 'notify' : 'rl');
  node.setAttribute('aria-hidden', 'true');
  parent.insertBefore(node, before);
  n[key] = node;
  return node;
}

export function updateTab(n: TabNode, item: TabItem, selected: boolean, uri: string, view: TabView = PLAIN_VIEW): void {
  // A title that reads as a GTA VI spoiler shows the engine's guard wording (src/spoiler-guard.ts).
  const shown = guardTitle(item.title);
  if (n.title.textContent !== shown) n.title.textContent = shown;
  const label = shown !== '' ? shown : 'Tab';
  const base = item.private ? privateTabName(label) : label;
  const name = markedName(base, item.notify, item.unread, item.rl);
  if (n.root.getAttribute('aria-label') !== name) {
    // No data-tip on the tab itself: the hover card shows the title, and a plain tooltip would flash before it.
    n.root.setAttribute('aria-label', name);
  }
  if (item.private) {
    if (n.root.dataset.private !== 'true') n.root.dataset.private = 'true';
  } else if (n.root.dataset.private !== undefined) {
    delete n.root.dataset.private;
  }
  n.root.setAttribute('aria-selected', String(selected));
  n.close.setAttribute('aria-label', `Close tab: ${label}`);
  setTip(n.close, 'Close tab');
  setTip(n.audio, 'Unmute tab');
  if (n.audio.hidden !== !item.muted) n.audio.hidden = !item.muted;
  n.audio.setAttribute('aria-label', `Unmute tab: ${label}`);
  n.root.classList.toggle('active', selected);
  n.root.classList.toggle('pinned', item.pinned);
  n.root.classList.toggle('muted', item.muted);

  // --- tab options --- the page load indicator, and the marks a tab can carry.
  const loading = loadMark(view.indicator, item.loading, item.pct);
  const mode = loading === 'spin' ? 'spin' : uri !== '' ? 'fav' : 'glyph';
  if (n.root.dataset.icon !== mode) n.root.dataset.icon = mode;
  const ring = mark(n, 'ring', loading === 'ring', n.img.parentElement ?? n.root, null);
  if (ring) ring.style.setProperty('--pct', String(item.pct ?? 0));
  const bar = mark(n, 'bar', loading === 'bar', n.root, null);
  if (bar) bar.style.setProperty('--pct', String((item.pct ?? 0) / 100));
  mark(n, 'dot', item.unread === true, n.root, n.audio);
  const badge = mark(n, 'badge', item.notify !== undefined, n.root, n.audio);
  if (badge) badge.textContent = notifyLabel(item.notify ?? 0);
  const rl = mark(n, 'rl', item.rl !== undefined, n.root, n.audio);
  if (rl && item.rl !== undefined) {
    n.rlFrom = item.rl;
    n.rlAt = view.now;
    rl.textContent = reloadLabel(item.rl);
  } else {
    delete n.rlFrom;
    delete n.rlAt;
  }
  n.root.classList.toggle('asleep', item.asleep === true);
  n.root.classList.toggle('idle', item.idle === true);
  n.root.classList.toggle('picked', view.picked);
  // --- end tab options ---
  if (n.uri !== uri) {
    n.uri = uri;
    if (uri !== '') n.img.src = uri;
    else n.img.removeAttribute('src');
  }

  const marked = item.marker >= 0;
  n.marker.classList.toggle('on', marked);
  if (marked) {
    const colour = ISLAND[item.marker & 7] ?? ACCENT;
    if (n.marker.style.getPropertyValue('--mc') !== colour) n.marker.style.setProperty('--mc', colour);
  }
}

function createGroup(key: number): GroupNode {
  const shell = el('div', 'group');
  shell.setAttribute('role', 'none');
  shell.dataset.key = String(key);
  // The island's handle: a coloured bar before its tabs (a wider pill once every tab is folded away).
  // A click folds or unfolds the island; resting on it opens the island menu.
  const ball = el('button', 'ball bw-focus-inset');
  ball.type = 'button';
  ball.dataset.key = String(key);
  const bar = el('span', 'bar');
  bar.setAttribute('aria-hidden', 'true');
  ball.append(bar);
  // The mark that closes a folded island after its one shown tab: a sliver for each tab folded behind
  // it (two at most), like the edges of stacked cards. A click unfolds, like the handle.
  const cap = el('span', 'cap');
  cap.dataset.key = String(key);
  cap.setAttribute('aria-hidden', 'true');
  cap.append(el('span', 'sliver'), el('span', 'sliver'));
  // The group is one flex item: `--go` (handle width) lets CSS give its tabs the same share of the
  // row as ungrouped tabs, `--gw` / `--gn` the summed weight and number of visible tabs.
  const watch = new ResizeObserver(() => {
    const w = `${ball.offsetWidth}px`;
    if (shell.style.getPropertyValue('--go') !== w) shell.style.setProperty('--go', w);
  });
  watch.observe(ball);
  return { shell, ball, cap, label: '', color: '', weight: -1, visible: -1, more: -1, watch, shift: 0 };
}

function syncGroup(g: GroupNode, e: GroupEntry, active: number, weightOf?: (id: number, fallback: number) => number): void {
  const name = e.leader.gname;
  // The strip shows no name (the island menu and the tooltip do).
  const label = name !== undefined && name !== '' ? name : 'Tab island';
  if (g.label !== label) {
    g.label = label;
    setTip(g.ball, label);
    g.ball.setAttribute('aria-label', label);
  }
  const expanded = String(!e.collapsed);
  if (g.ball.getAttribute('aria-expanded') !== expanded) g.ball.setAttribute('aria-expanded', expanded);

  const color = colourOf(groupColourIndex(e.leader.gcol, e.key, name));
  if (g.color !== color) {
    g.color = color;
    g.shell.style.setProperty('--gc', color);
  }
  g.shell.classList.toggle('collapsed', e.collapsed);

  const weight = groupWeight(e.members, active, weightOf);
  if (g.weight !== weight) {
    g.weight = weight;
    g.shell.style.setProperty('--gw', String(weight));
  }
  if (g.visible !== e.visible) {
    g.visible = e.visible;
    g.shell.style.setProperty('--gn', String(e.visible));
  }
  // How many slivers the closing mark shows: one per tab folded away, two at most.
  const more = Math.min(2, e.members.length - e.visible);
  if (g.more !== more) {
    g.more = more;
    g.shell.dataset.more = String(more);
  }
}

function setShift(n: TabNode, px: number): void {
  // A dragged tab's transform also carries the lift scale, so 0 must clear it even when shift is 0.
  if (n.shift === px && (px !== 0 || n.root.style.transform === '')) return;
  n.shift = px;
  n.root.style.transform = px === 0 ? '' : `translateX(${px}px)`;
}

/** Moves a whole island by `px` (0 clears it), as setShift does for one tab. */
function setShellShift(g: GroupNode, px: number): void {
  if (g.shift === px && (px !== 0 || g.shell.style.transform === '')) return;
  g.shift = px;
  g.shell.style.transform = px === 0 ? '' : `translateX(${px}px)`;
}

/** Builds the tab strip into `strip` (the #app element) and wires it to the host. */
export function startTabs(strip: HTMLElement): void {
  // --- vertical sidebar (lane S) ---
  const sidebar = createSidebarRenderer(strip);
  let layoutMode: 'horizontal' | 'vertical' = 'horizontal';
  // The strip runs along the bottom of the window (the page is above it).
  let stripAtBottom = false;
  let workspace: SidebarWorkspace = { name: 'Workspace', icon: 'g:briefcase' };
  // --- end vertical sidebar ---
  strip.classList.remove('sidebar');
  strip.removeAttribute('aria-orientation');
  strip.classList.add('strip');
  strip.setAttribute('role', 'tablist');
  strip.setAttribute('aria-label', 'Tabs');
  strip.setAttribute(DRAG_ATTR, '');
  strip.replaceChildren();

  const tabNodes = new Map<number, TabNode>();
  const groups = new Map<number, GroupNode>();
  const favs = new Map<number, string>();
  let activeId = 0;
  let latest: TabsMsg | null = null;
  let dirty = false;
  let frame = 0;
  let drag: Drag | null = null;
  let ignoreClick = false;
  /** True after the first horizontal render: later new tabs animate in, the first batch does not. */
  let rendered = false;
  let signature = '';
  /** Tab id to refocus after the next render (keyboard reorder moves the focused node). */
  let refocus = 0;
  /**
   * A released drag leaves its transforms in place until the host's reorder arrives (or is refused):
   * the dropped tab keeps its lift and its place above its neighbours while it lands.
   */
  let landing: Landing | null = null;
  // --- group drag --- the island held by its handle, the one landing after a drop, and the click a
  // drag swallows (the handle's click folds the island).
  let groupDrag: GroupDrag | null = null;
  let groupLanding: GroupLanding | null = null;
  let groupIgnoreClick = false;
  /** Island whose handle takes focus back after the next render (keyboard move). */
  let refocusGroup = 0;
  // --- end group drag ---
  const reduced = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;
  // --- tab options ---
  let ui: TabOptsMsg = DEFAULT_OPTS;
  /** Tabs selected with Ctrl or Shift click, and the tab the next Shift click ranges from. */
  let picked: number[] = [];
  let anchor = 0;
  let wheelTotal = 0;
  let wheelAt = 0;
  let locked = false;
  let reloadTimer = 0;
  const closedBtn = el('button', 'closed-btn bw-icon-btn');
  closedBtn.type = 'button';
  closedBtn.setAttribute('aria-label', 'Closed tabs');
  closedBtn.append(icon('tab-restore'));
  setTip(closedBtn, 'Closed tabs');
  closedBtn.addEventListener('click', () => { sendClosedTabs(); });
  // --- toolbars and feeds (thread T13) --- the end of the strip: the Closed tabs button (when `toolbar_tabs` lists it)
  // and the command buttons the user put on the tab bar, in one box so they stay together while the strip scrolls.
  const stripTools = el('div', 'strip-tools');
  let toolSig = '';
  const toolButtons: HTMLButtonElement[] = [];
  const syncTools = (): void => {
    const sig = ui.tools.map((tool) => `${tool.k}|${tool.i}|${tool.l}|${tool.g ?? ''}`).join('\n');
    if (sig === toolSig) return;
    toolSig = sig;
    toolButtons.length = 0;
    for (const tool of ui.tools) {
      const button = el('button', 'closed-btn tab-tool bw-icon-btn');
      button.type = 'button';
      button.setAttribute('aria-label', tool.l);
      button.append(icon(itemIcon(tool.k, tool.g) as IconName));
      setTip(button, tool.l);
      button.addEventListener('click', () => { sendTool('tabs', tool.k, tool.i); });
      toolButtons.push(button);
    }
  };
  // --- end toolbars and feeds ---
  const pinSep = el('span', 'pinsep');
  pinSep.setAttribute('aria-hidden', 'true');
  pinSep.setAttribute('role', 'presentation');
  const applyUi = (): void => {
    syncTools(); // --- toolbars and feeds (thread T13) ---
    const wanted = stripClasses(ui);
    for (const name of STRIP_OPTION_CLASSES) strip.classList.toggle(name, wanted.includes(name));
    strip.style.setProperty('--tab-active-min', ui.minWidth > 0 ? `${ui.minWidth}px` : '');
    if (!ui.alignClose) unlockWidths();
  };
  /** Holds every tab of the strip at `tab`'s width until the pointer leaves, so the next close button lands under it. */
  const lockWidths = (tab: HTMLElement): void => {
    unlockWidths();
    strip.style.setProperty('--tab-lock', `${lockedWidth(tab.getBoundingClientRect().width)}px`);
    strip.classList.add('locked');
    locked = true;
  };
  const unlockWidths = (): void => {
    if (!locked) return;
    locked = false;
    strip.classList.remove('locked');
    strip.style.setProperty('--tab-lock', '');
  };
  const setPicked = (ids: readonly number[]): void => {
    if (ids.length === picked.length && ids.every((id, i) => id === picked[i])) return;
    picked = [...ids];
    if (sendTabSelection(picked)) { /* Rust keeps the same list for the tab menu */ }
    dirty = true;
    schedule();
  };
  const ensureReloadTick = (): void => {
    const need = [...tabNodes.values()].some((n) => n.rl !== undefined);
    if (need && reloadTimer === 0) reloadTimer = window.setInterval(() => tickReloads(tabNodes.values(), Date.now()), 1000);
    else if (!need && reloadTimer !== 0) { window.clearInterval(reloadTimer); reloadTimer = 0; }
  };
  // --- end tab options ---
  // --- split view ---
  // The window's split pair (joined capsule) and a tab drag that left the strip for the page.
  let pair: PairMsg | null = null;
  let splitOut = false;
  let splitFrame = 0;
  let splitPoint: { x: number; y: number } | null = null;
  const sendSplit = (cmd: SplitCommand): void => {
    if (validSplitCommand(cmd)) send(cmd);
  };
  const splitWeight = (id: number, fallback: number): number => pairWeight(id, pair, fallback);
  // --- end split view ---

  const schedule = (): void => {
    if (frame === 0) frame = requestAnimationFrame(flush);
  };

  // One render per animation frame, however many host messages arrived. Deferred while a drag is live.
  const flush = (): void => {
    frame = 0;
    const msg = latest;
    if (drag?.active || groupDrag?.active || !dirty || msg === null) return;
    dirty = false;
    render({ ...msg, items: pairOrder(msg.items, pair) }); // --- split view ---
    markPair(strip, pair); // --- split view ---
  };

  const tabFor = (item: TabItem): TabNode => {
    let node = tabNodes.get(item.id);
    if (!node) {
      node = createTab(item.id);
      tabNodes.set(item.id, node);
      if (rendered) {
        // A new tab grows in (opacity + .96 scale, 167ms); the class is dropped afterwards so it
        // never fights the position animation or the drag transform.
        const root = node.root;
        root.classList.add('bw-enter', 'is-scale');
        root.addEventListener('animationend', (ev) => {
          if (ev.target === root) root.classList.remove('bw-enter', 'is-scale');
        });
      }
    }
    return node;
  };

  const syncTab = (item: TabItem): TabNode => {
    const n = tabFor(item);
    updateTab(n, item, item.id === activeId, item.fav ? (favs.get(item.id) ?? '') : '', { indicator: ui.indicator, picked: picked.includes(item.id), now: Date.now() }); // --- tab options ---
    return n;
  };

  /** The strip's shown tabs, left to right. */
  const visibleTabs = (): TabNode[] => {
    const list: TabNode[] = [];
    for (const node of Array.from(strip.querySelectorAll<HTMLElement>('.tab:not([hidden])'))) {
      const n = tabNodes.get(Number(node.dataset.id));
      if (n) list.push(n);
    }
    return list;
  };

  // Roving tabindex: one tab in the tab order, the active one unless the keyboard moved focus.
  const applyRoving = (focus?: TabNode): void => {
    const list = visibleTabs();
    const target = focus ?? list.find((n) => n.id === activeId) ?? list[0];
    for (const n of tabNodes.values()) {
      const t = n === target ? 0 : -1;
      if (n.root.tabIndex !== t) n.root.tabIndex = t;
    }
  };

  /** The active tab and place the strip last scrolled into view (`revealKey`). */
  let revealed = '';
  const render = (msg: TabsMsg): void => {
    // --- vertical sidebar (lane S) ---
    if (layoutMode === 'vertical') {
      activeId = msg.active;
      sidebar.render(msg, favs, workspace);
      return;
    }
    strip.classList.remove('sidebar');
    strip.classList.add('strip');
    strip.removeAttribute('aria-orientation');
    // --- end vertical sidebar ---
    activeId = msg.active;
    const entries: Entry[] = buildEntries(msg.items, msg.active);
    const live = new Set<number>(msg.items.map((it) => it.id));
    // A closed tab that had the keyboard (Delete) hands it to the tab beside it, not to the body (U10).
    {
      const shown = visibleTabs();
      const had = shown.find((n) => n.root === document.activeElement);
      const heir = had ? focusHeir(shown.map((n) => n.id), had.id, live) : 0;
      if (heir !== 0 && refocus === 0) refocus = heir;
    }
    // --- tab options --- a closed tab leaves the selection.
    const kept = pruneSelection(picked, live);
    if (kept.length !== picked.length) { picked = kept; sendTabSelection(picked); }
    // --- end tab options ---
    const keys = new Set<number>();
    for (const e of entries) if (e.kind === 'group') keys.add(e.key);

    // FLIP "first": where every visible tab is now (drag transforms included), measured only when
    // the arrangement is about to change.
    const nextSignature = layoutSignature(entries);
    const phase = landingPhase(landing?.signature ?? groupLanding?.signature ?? null, nextSignature);
    // A stale render (the layout the drop left) updates content only: no transform is touched.
    const changed = nextSignature !== signature && phase !== 'stale';
    const animate = rendered && !reduced() && tabNodes.size <= FLIP_MAX_TABS && changed;
    const before = new Map<TabNode, number>();
    const ghosts: HTMLElement[] = [];
    const landed = phase === 'reorder' ? landing : null;
    // --- group drag --- islands slide as one piece too: each shell is inverted, and its tabs only by
    // their own move inside it.
    const landedGroup = phase === 'reorder' ? groupLanding : null;
    const shellsBefore = new Map<GroupNode, number>();
    if (animate) {
      for (const n of tabNodes.values()) {
        if (n.root.isConnected && !n.root.hidden) before.set(n, n.root.getBoundingClientRect().left);
      }
      for (const g of groups.values()) {
        if (g.shell.isConnected) shellsBefore.set(g, g.shell.getBoundingClientRect().left);
      }
      strip.classList.add('settle');
      // The dropped tab still carries its drag offset: its "first" is home + offset, not a scaled rect.
      const held = landed !== null && landed.node.shift !== 0 ? landed.node : null;
      const prev = held ? held.shift : 0;
      for (const n of tabNodes.values()) setShift(n, 0);
      for (const g of groups.values()) setShellShift(g, 0);
      if (held && before.has(held)) before.set(held, landingFirst(held.root.getBoundingClientRect().left, prev));
    } else if (landed !== null || landedGroup !== null) {
      strip.classList.add('settle');
      for (const n of tabNodes.values()) setShift(n, 0);
      for (const g of groups.values()) setShellShift(g, 0);
      void strip.offsetWidth;
      strip.classList.remove('settle');
    }

    for (const [id, n] of tabNodes) {
      if (!live.has(id)) {
        if (animate && before.has(n)) {
          // The closed tab fades where it was while its neighbours slide into the gap.
          const r = n.root.getBoundingClientRect();
          const stripBox = strip.getBoundingClientRect();
          n.root.removeAttribute('role');
          n.root.setAttribute('aria-hidden', 'true');
          n.root.classList.add('ghost', 'bw-exit');
          n.root.style.left = `${r.left - stripBox.left + strip.scrollLeft}px`;
          n.root.style.top = `${r.top - stripBox.top}px`;
          n.root.style.width = `${r.width}px`;
          const drop = (): void => n.root.remove();
          n.root.addEventListener('animationend', (ev) => {
            if (ev.target === n.root) drop();
          });
          setTimeout(drop, 400);
          ghosts.push(n.root);
        } else {
          n.root.remove();
        }
        tabNodes.delete(id);
      }
    }
    for (const [key, g] of groups) {
      if (!keys.has(key)) {
        g.watch.disconnect();
        g.shell.remove();
        groups.delete(key);
      }
    }
    pruneFavs(favs, live);

    const top: HTMLElement[] = [];
    let lastPinned = -1; // --- tab options ---
    for (const e of entries) {
      if (e.kind === 'tab') {
        const n = syncTab(e.slot.item);
        n.root.hidden = false;
        top.push(n.root);
        if (e.slot.item.pinned) lastPinned = top.length - 1; // --- tab options ---
        continue;
      }
      let g = groups.get(e.key);
      if (!g) {
        g = createGroup(e.key);
        groups.set(e.key, g);
      }
      syncGroup(g, e, activeId, splitWeight); // --- split view ---
      const nodes: HTMLElement[] = [g.ball];
      for (const slot of e.members) {
        const n = syncTab(slot.item);
        n.root.hidden = hiddenForTab(true, e.collapsed, slot.item.id === activeId);
        nodes.push(n.root);
      }
      nodes.push(g.cap);
      place(g.shell, nodes);
      top.push(g.shell);
    }
    // --- tab options --- a thin line where the pinned tabs end, and the Closed tabs button at the end.
    if (ui.separator && lastPinned >= 0 && lastPinned < top.length - 1) top.splice(lastPinned + 1, 0, pinSep);
    // --- toolbars and feeds (thread T13) --- Closed tabs only when the bar lists it; command buttons after it.
    const tools: HTMLElement[] = [...(ui.closed > 0 && ui.closedButton ? [closedBtn] : []), ...toolButtons];
    if (tools.length > 0) {
      place(stripTools, tools);
      top.push(stripTools);
    }
    // --- end toolbars and feeds ---
    // --- end tab options ---
    place(strip, top);
    strip.append(...ghosts);
    ensureReloadTick(); // --- tab options ---
    // --- tab ghost --- the host has laid out the released tab's slot: the ghost glides there.
    if (ghostLand !== null && ghostLand.session.landing && ghostLand.node.root.isConnected && ghostLand.node.root.style.transform === '') {
      const r = ghostLand.node.root.getBoundingClientRect();
      ghostLand.session.retarget(r.left, r.top);
    }
    // --- end tab ghost ---

    if (animate) {
      // FLIP "last, invert, play": jump each moved tab back to where it was, then release.
      let moved = false;
      // Every "last" is read before any invert is written, so a tab's own move excludes its island's.
      const lastLeft = new Map<TabNode, number>();
      for (const n of before.keys()) {
        if (n.root.isConnected && !n.root.hidden && !n.root.classList.contains('ghost')) lastLeft.set(n, n.root.getBoundingClientRect().left);
      }
      const shellDx = new Map<Element, number>();
      for (const [g, left] of shellsBefore) {
        if (!g.shell.isConnected) continue;
        const dx = flipInvert(left, g.shell.getBoundingClientRect().left);
        if (Math.abs(dx) >= 0.5) shellDx.set(g.shell, dx);
      }
      for (const [shell, dx] of shellDx) {
        (shell as HTMLElement).style.transform = `translateX(${dx}px)`;
        moved = true;
      }
      for (const [n, left] of before) {
        const last = lastLeft.get(n);
        if (last === undefined) continue;
        const parent = n.root.parentElement;
        const dx = flipInvert(left, last) - (parent ? shellDx.get(parent) ?? 0 : 0);
        const lifted = n === landed?.node;
        if (Math.abs(dx) < 0.5 && !lifted) continue;
        // The dropped tab keeps its lift through the invert, so the scale eases to 1 as it arrives.
        n.root.style.transform = lifted ? `translateX(${dx}px) scale(${LIFT_SCALE})` : `translateX(${dx}px)`;
        moved = true;
      }
      if (moved) void strip.offsetWidth;
      strip.classList.remove('settle');
      for (const n of before.keys()) if (n.root.style.transform !== '') n.root.style.transform = '';
      for (const shell of shellDx.keys()) (shell as HTMLElement).style.transform = '';
    }
    if (landed !== null) {
      if (animate) landTransition(landed.node);
      else endLanding();
    }
    if (landedGroup !== null) {
      if (animate && landedGroup.group.shell.isConnected) landGroupTransition(landedGroup);
      else endGroupLanding();
    }
    if (phase !== 'stale') signature = nextSignature;
    rendered = true;
    applyRoving();
    if (refocus !== 0) {
      const node = tabNodes.get(refocus);
      if (node) {
        applyRoving(node); // the focused tab is the one in the tab order
        node.root.focus({ preventScroll: true });
      }
      refocus = 0;
    }
    // --- group drag --- the moved island's handle keeps the keyboard.
    if (refocusGroup !== 0) {
      groups.get(refocusGroup)?.ball.focus({ preventScroll: true });
      refocusGroup = 0;
    }
    // Only a newly selected or moved active tab is revealed: a background tab's update leaves the
    // strip where the user scrolled it.
    const reveal = revealKey(msg.items, msg.active);
    if (reveal !== revealed) {
      revealed = reveal;
      tabNodes.get(activeId)?.root.scrollIntoView({
        block: 'nearest', inline: 'nearest',
        behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth',
      });
    }
    updateEdges();
  };

  // ----- drag reorder -----

  const onDragKey = (ev: KeyboardEvent): void => {
    if (ev.key !== 'Escape') return;
    ev.preventDefault();
    ev.stopPropagation();
    endDrag(false);
  };

  /** Visible tab of every collapsed group -> the group's first member (the slot's edge), except the dragged tab's. */
  const collapsedLeaders = (dragged: number): Map<number, number> => {
    const map = new Map<number, number>();
    if (!latest) return map;
    for (const e of buildEntries(latest.items, latest.active)) {
      if (e.kind !== 'group' || !e.collapsed) continue;
      const first = e.members.find((m) => m.item.id !== dragged);
      if (!first) continue;
      for (const m of e.members) if (!m.hidden) map.set(m.item.id, first.item.id);
    }
    return map;
  };

  /** Drops `.landing` and the landing record; the tab is home. */
  const endLanding = (): void => {
    const l = landing;
    if (!l) return;
    landing = null;
    clearTimeout(l.timer);
    l.off?.();
    l.node.root.classList.remove('landing');
  };

  /** Keeps `.landing` (the lift's z-index) until the tab's own transform transition ends. */
  const landTransition = (node: TabNode): void => {
    const l = landing;
    if (!l || l.node !== node) return;
    clearTimeout(l.timer);
    l.off?.();
    const done = (): void => {
      if (landing === l) endLanding();
    };
    const onEnd = (ev: TransitionEvent): void => {
      if (ev.target === node.root && ev.propertyName === 'transform') done();
    };
    node.root.addEventListener('transitionend', onEnd);
    l.off = () => node.root.removeEventListener('transitionend', onEnd);
    l.timer = window.setTimeout(done, LAND_FALLBACK_MS);
  };

  /** One ease back to where the drag began (refused move, Escape, a drop on the same slot). */
  const landHome = (): void => {
    const l = landing;
    if (!l) return;
    const nodes = Array.from(tabNodes.values());
    if (!reduced() && tabNodes.size <= FLIP_MAX_TABS) {
      for (const n of nodes) setShift(n, 0);
      landTransition(l.node);
      return;
    }
    snapHome();
  };

  /** Ends a landing at once: every tab jumps to its slot with no transition and `.landing` is dropped. */
  const snapHome = (): void => {
    strip.classList.add('settle');
    for (const n of tabNodes.values()) setShift(n, 0);
    for (const g of groups.values()) setShellShift(g, 0); // --- group drag ---
    void strip.offsetWidth;
    strip.classList.remove('settle');
    endLanding();
    endGroupLanding(); // --- group drag ---
  };

  // ----- group drag: a whole island picked up by its handle -----

  /** Drops `.landing` from the island that was dropped; it is home. */
  const endGroupLanding = (): void => {
    const l = groupLanding;
    if (!l) return;
    groupLanding = null;
    clearTimeout(l.timer);
    l.off?.();
    l.group.shell.classList.remove('landing');
  };

  /** Keeps the dropped island raised until its own transform transition ends. */
  const landGroupTransition = (l: GroupLanding): void => {
    if (groupLanding !== l) return;
    clearTimeout(l.timer);
    l.off?.();
    const shell = l.group.shell;
    const done = (): void => {
      if (groupLanding === l) endGroupLanding();
    };
    const onEnd = (ev: TransitionEvent): void => {
      if (ev.target === shell && ev.propertyName === 'transform') done();
    };
    shell.addEventListener('transitionend', onEnd);
    l.off = () => shell.removeEventListener('transitionend', onEnd);
    l.timer = window.setTimeout(done, LAND_FALLBACK_MS);
  };

  /** One ease back to where the island was picked up (refused move, Escape, a drop on its own slot). */
  const landGroupHome = (): void => {
    const l = groupLanding;
    if (!l) return;
    if (!reduced() && tabNodes.size <= FLIP_MAX_TABS) {
      for (const n of tabNodes.values()) setShift(n, 0);
      for (const g of groups.values()) setShellShift(g, 0);
      landGroupTransition(l);
      return;
    }
    snapHome();
  };

  /** The strip's unpinned tabs and islands in order, as a group drag moves among them. */
  const dragUnits = (): DragUnit[] => {
    const firstOf = new Map<number, number>();
    if (latest) for (const u of groupUnits(buildEntries(latest.items, latest.active))) if (u.key !== 0) firstOf.set(u.key, u.first);
    const units: DragUnit[] = [];
    for (const child of Array.from(strip.children)) {
      if (!(child instanceof HTMLElement)) continue;
      if (child.classList.contains('group')) {
        const key = Number(child.dataset.key);
        const group = groups.get(key);
        const first = firstOf.get(key);
        if (group && first !== undefined) units.push({ el: child, first, group });
      } else if (child.classList.contains('tab') && !child.classList.contains('pinned') && !child.classList.contains('ghost') && !child.hidden) {
        const tab = tabNodes.get(Number(child.dataset.id));
        if (tab) units.push({ el: child, first: tab.id, tab });
      }
    }
    return units;
  };

  const unitShift = (u: DragUnit): number => u.tab?.shift ?? u.group?.shift ?? 0;
  const setUnitShift = (u: DragUnit, px: number): void => {
    if (u.tab) setShift(u.tab, px);
    else if (u.group) setShellShift(u.group, px);
  };

  /** Home (unshifted) centres, left edges and widths of `units`. */
  const measureUnits = (units: readonly DragUnit[]): { centers: number[]; lefts: number[]; widths: number[] } => {
    const centers: number[] = [];
    const lefts: number[] = [];
    const widths: number[] = [];
    for (const u of units) {
      const r = u.el.getBoundingClientRect();
      const width = u.el.offsetWidth || r.width;
      const left = r.left - unitShift(u);
      lefts.push(left);
      widths.push(width);
      centers.push(left + width / 2);
    }
    return { centers, lefts, widths };
  };

  const onGroupDragKey = (ev: KeyboardEvent): void => {
    if (ev.key !== 'Escape') return;
    ev.preventDefault();
    ev.stopPropagation();
    endGroupDrag(false);
  };

  /** A press on an island's handle: nothing moves until the pointer travels DRAG_START_PX (else it is a click). */
  const pressGroup = (ev: PointerEvent, handle: HTMLElement): void => {
    if (layoutMode !== 'horizontal') return;
    const key = Number(handle.dataset.key);
    const group = groups.get(key);
    if (!group) return;
    const units = dragUnits();
    const from = units.findIndex((u) => u.group === group);
    if (from < 0) return;
    ghostLand?.session.abandon(); // --- tab ghost ---
    if (landing !== null || groupLanding !== null) snapHome();
    const home = measureUnits(units);
    groupDrag = {
      pointerId: ev.pointerId, key, group, handle, startX: ev.clientX, startY: ev.clientY, startScroll: strip.scrollLeft,
      units, from, centers: home.centers, lefts: home.lefts, widths: home.widths, active: false, dest: from, lastX: ev.clientX,
    };
    try { handle.setPointerCapture(ev.pointerId); } catch { /* window listeners still end the drag */ }
  };

  /**
   * Lays the strip out for the held island at the last pointer x: the island follows the pointer
   * along the row of unpinned units (never over the pinned tabs), and the units it passes slide aside
   * by its width. Group drags stay on the strip: no ghost, no tearing off, no split drop.
   */
  const layoutGroupDrag = (d: GroupDrag): void => {
    const box = strip.getBoundingClientRect();
    const edgeDelta = dragScrollDelta(d.lastX, box.left, box.right);
    if (edgeDelta !== 0) {
      const was = strip.scrollLeft;
      strip.scrollLeft += edgeDelta;
      if (strip.scrollLeft !== was) {
        updateEdges();
        const home = measureUnits(d.units);
        d.centers = home.centers;
        d.lefts = home.lefts;
      }
    }
    const dx = clampBlock(d.lastX - d.startX + strip.scrollLeft - d.startScroll, d.lefts, d.widths, d.from);
    setShellShift(d.group, dx);
    const left = (d.lefts[d.from] ?? 0) + dx;
    d.dest = blockDropIndex(d.centers, d.from, left, left + (d.widths[d.from] ?? 0));
    const shifts = gapShifts(d.lefts, d.widths, stripGap(), d.from, d.dest, false);
    d.units.forEach((u, i) => {
      if (i !== d.from) setUnitShift(u, shifts[i] ?? 0);
    });
  };

  /** Pointer travel on a held handle; true when the event belonged to it. */
  const moveGroup = (ev: PointerEvent): boolean => {
    const d = groupDrag;
    if (!d || ev.pointerId !== d.pointerId) return false;
    if (!d.active) {
      if (Math.abs(ev.clientX - d.startX) < DRAG_START_PX && Math.abs(ev.clientY - d.startY) < DRAG_START_PX) return true;
      d.active = true;
      d.group.shell.classList.add('dragging');
      strip.classList.add('group-dragging');
      window.addEventListener('keydown', onGroupDragKey, true);
      announce(groupDragAnnouncement('lift', d.group.label));
    }
    d.lastX = ev.clientX;
    layoutGroupDrag(d);
    return true;
  };

  /**
   * The handle was released (or the drag cancelled). A drop on another slot sends one `group` move for
   * the whole island; the island stays where it was let go until the host's reorder arrives (one
   * FLIP from there to its slot) or, refused, eases home.
   */
  const endGroupDrag = (commit: boolean): void => {
    const d = groupDrag;
    if (!d) return;
    groupDrag = null;
    window.removeEventListener('keydown', onGroupDragKey, true);
    try { if (d.handle.hasPointerCapture(d.pointerId)) d.handle.releasePointerCapture(d.pointerId); } catch { /* capture may already be lost */ }
    if (!d.active) return;
    // The click that follows the release must not fold the island.
    groupIgnoreClick = true;
    window.setTimeout(() => { groupIgnoreClick = false; }, 0);
    strip.classList.remove('group-dragging');
    d.group.shell.classList.add('landing');
    d.group.shell.classList.remove('dragging');
    endGroupLanding();
    const l: GroupLanding = { group: d.group, signature, timer: 0, off: null };
    groupLanding = l;
    const before = commit ? groupMoveBefore(d.units.map((u) => u.first), d.from, d.dest) : null;
    if (before !== null && sendGroupMove(d.key, before)) {
      announce(groupDragAnnouncement('drop', d.group.label, d.dest + 1, d.units.length));
      l.timer = window.setTimeout(() => {
        if (groupLanding === l) landGroupHome();
      }, REFUSE_MS);
      schedule();
      return;
    }
    announce(groupDragAnnouncement(commit ? 'drop' : 'cancel', d.group.label));
    landGroupHome();
    schedule();
  };

  // ----- tab ghost (docs/design/tab-drag.md) -----

  /** The released ghost still gliding to its slot, and the tab it stands for. */
  let ghostLand: { readonly session: GhostSession; readonly node: TabNode } | null = null;

  /** Gap between neighbouring tabs (the strip's flex gap). */
  const stripGap = (): number => {
    const gap = parseFloat(getComputedStyle(strip).columnGap);
    return Number.isFinite(gap) ? gap : 4;
  };

  /** Home (unshifted) centres, left edges and widths of `order`: shifts and the lift scale removed. */
  const measureHome = (order: readonly TabNode[]): { centers: number[]; lefts: number[]; widths: number[] } => {
    const centers: number[] = [];
    const lefts: number[] = [];
    const widths: number[] = [];
    for (const n of order) {
      const r = n.root.getBoundingClientRect();
      const width = n.root.offsetWidth || r.width;
      const centre = r.left + r.width / 2 - n.shift;
      centers.push(centre);
      lefts.push(centre - width / 2);
      widths.push(width);
    }
    return { centers, lefts, widths };
  };

  const titleOf = (n: TabNode): string => n.root.getAttribute('aria-label') ?? n.title.textContent ?? '';

  const ghostHooks = (node: TabNode): { shown(): void; lost(): void; reveal(): void } => ({
    shown: () => {
      const d = drag;
      if (!d || d.node !== node) return;
      // The ghost has the tab now: its own copy turns into the gap, wherever the slot is.
      node.root.classList.add('lifted');
      setShift(node, 0);
      layoutDrag(d);
    },
    lost: () => {
      node.root.classList.remove('lifted');
      const d = drag;
      if (d && d.node === node) layoutDrag(d);
    },
    reveal: () => {
      node.root.classList.remove('lifted');
      if (ghostLand?.node === node) ghostLand = null;
    },
  });

  /**
   * --- tab stacks (T10) --- marks `target` as the tab the held tab rests over the middle of. Once it
   * has rested there for the stacking delay the tab shows it, and a release stacks the two.
   */
  const setStackOn = (d: Drag, target: TabNode | null): void => {
    if (d.stackOn === target) return;
    window.clearTimeout(d.stackTimer);
    d.stackOn?.root.classList.remove('stack-target');
    d.stackOn = target;
    d.stackArmed = false;
    if (target === null) return;
    const arm = (): void => {
      if (drag !== d || d.stackOn !== target) return;
      d.stackArmed = true;
      target.root.classList.add('stack-target');
      announce(stackAnnouncement(titleOf(target)));
    };
    d.stackTimer = window.setTimeout(arm, STACK_DROP_DELAY_MS);
  };

  /**
   * Lays the strip out for the held tab at the last pointer position: attached, the neighbours open
   * the gap under the pointer (the ghost, or the tab itself without one, rides above); floating, the
   * gap closes and, over the page, Rust shows the split drop zone.
   */
  const layoutDrag = (d: Drag): void => {
    const x = d.lastX;
    const y = d.lastY;
    const ghosted = d.ghost?.live === true;
    const box = strip.getBoundingClientRect();
    const phase = nextPhase(d.phase, stripBand(box), x, y);
    if (phase !== d.phase) {
      d.phase = phase;
      d.ghost?.phase(phase === 'floating');
      announce(dragAnnouncement(phase === 'floating' ? 'float' : 'attach', titleOf(d.node)));
    }
    d.dx = x - d.startX;
    if (!ghosted) {
      // No ghost (touch, pen, or no native layer): the tab itself slides along the strip.
      // shift tracks the transform, so a later setShift(n, 0) clears the dragged tab like any other.
      d.node.shift = d.dx;
      d.node.root.style.transform = reduced() ? `translateX(${d.dx}px)` : `translateX(${d.dx}px) scale(${LIFT_SCALE})`;
    }
    // --- split view ---
    // Off the strip and below it the tab is over the page: the strip keeps its order and Rust shows
    // where the tab would open (one report per frame); back on the strip the reorder resumes.
    const out = phase === 'floating' && overPage(y, box, stripAtBottom);
    if (out !== splitOut) {
      splitOut = out;
      strip.classList.toggle('split-out', out);
      if (!out) {
        cancelAnimationFrame(splitFrame);
        splitFrame = 0;
        sendSplit({ v: 1, t: 'split', a: 'end', id: d.node.id });
      }
    }
    if (out) {
      splitPoint = wirePoint(x, y);
      if (splitFrame === 0) {
        splitFrame = requestAnimationFrame(() => {
          splitFrame = 0;
          if (splitOut && splitPoint && drag) sendSplit({ v: 1, t: 'split', a: 'over', id: drag.node.id, x: splitPoint.x, y: splitPoint.y });
        });
      }
    }
    // --- end split view ---
    if (phase === 'floating') {
      setStackOn(d, null); // --- tab stacks (T10) ---
      // Off the strip: no slot. With a ghost the gap closes up behind it; without one the tab stays
      // under the pointer and its neighbours go home.
      d.dest = d.from;
      const closed = ghosted ? gapShifts(d.lefts, d.widths, stripGap(), d.from, d.from, true) : null;
      d.order.forEach((n, i) => {
        if (i !== d.from) setShift(n, closed?.[i] ?? 0);
      });
      return;
    }
    const edgeDelta = dragScrollDelta(x, box.left, box.right);
    if (edgeDelta !== 0) {
      strip.scrollLeft += edgeDelta;
      updateEdges();
      const home = measureHome(d.order);
      d.centers = home.centers;
      d.lefts = home.lefts;
    }
    // --- tab stacks (T10) --- over the middle of another tab the strip holds still, so that tab stays
    // under the pointer; resting there for the stacking delay marks it, and a release stacks the two.
    // Where each tab is drawn now (home plus its gap shift), so the tab marked is the one under the pointer.
    const shown = d.lefts.map((left, i) => left + (d.order[i]?.shift ?? 0));
    const over = !d.node.root.classList.contains('pinned') ? stackCandidate(shown, d.widths, d.from, x) : -1;
    const target = over >= 0 ? d.order[over] ?? null : null;
    setStackOn(d, target !== null && !target.root.classList.contains('pinned') ? target : null);
    if (d.stackOn !== null) return;
    // The slot comes from the pointer, not from the dragged tab's centre: grabbing a wide tab by its edge
    // must not shift the drop by the grab offset.
    d.dest = dropIndex(d.centers, d.from, x);
    const shifts = gapShifts(d.lefts, d.widths, stripGap(), d.from, d.dest, false);
    d.order.forEach((n, i) => {
      if (i !== d.from) setShift(n, shifts[i] ?? 0);
    });
    // The hidden copy sits in the gap, so a release can glide the ghost straight into it.
    if (ghosted) setShift(d.node, shifts[d.from] ?? 0);
  };

  const endDrag = (commit: boolean): void => {
    const d = drag;
    if (!d) return;
    drag = null;
    window.removeEventListener('keydown', onDragKey, true);
    try { if (d.node.root.hasPointerCapture(d.pointerId)) d.node.root.releasePointerCapture(d.pointerId); } catch { /* capture may already be lost */ }
    // --- tab stacks (T10) --- a release over a marked tab stacks the two; the held tab goes home.
    window.clearTimeout(d.stackTimer);
    const stackOnto = commit && d.stackArmed && !splitOut ? d.stackOn : null;
    d.stackOn?.root.classList.remove('stack-target');
    d.stackOn = null;
    if (!d.active) return;
    if (stackOnto !== null) {
      sendTabStack(d.node.id, stackOnto.id);
      d.dest = d.from;
    }
    ignoreClick = true;
    const ghost = d.ghost;
    const outcome = dropOutcome(commit, d.phase, splitOut, d.from, d.dest);
    const title = titleOf(d.node);
    announce(outcome === 'reorder'
      ? dragAnnouncement('drop', title, d.dest + 1, d.order.length)
      : outcome === 'settle' ? dragAnnouncement('drop', title) : dragAnnouncement(outcome, title));
    const homeX = d.lefts[d.from] ?? 0;
    const homeY = d.node.root.getBoundingClientRect().top;
    // The ghost glides to the slot (or home), fades on a split drop, or just goes with reduced motion.
    const releaseGhost = (slotX: number): void => {
      if (!ghost) return;
      if (ghost.live) ghostLand = { session: ghost, node: d.node };
      if (reduced()) ghost.end('none');
      else if (outcome === 'split') ghost.end('drop', homeX, homeY);
      else ghost.end('settle', slotX, homeY);
    };
    // --- split view ---
    // Released over the page: Rust opens the split if it showed a drop target there.
    if (splitOut) {
      splitOut = false;
      cancelAnimationFrame(splitFrame);
      splitFrame = 0;
      strip.classList.remove('split-out');
      d.node.root.classList.remove('dragging');
      sendSplit({ v: 1, t: 'split', a: commit ? 'drop' : 'end', id: d.node.id });
      d.node.root.style.transform = '';
      d.node.shift = 0;
      for (const n of d.order) setShift(n, 0);
      releaseGhost(homeX);
      schedule();
      return;
    }
    // --- end split view ---
    // `.landing` takes over from `.dragging` in the same frame (same stacking, the tab's own shadow),
    // and no transform changes yet: the tab stays where it was released.
    d.node.root.classList.add('landing');
    d.node.root.classList.remove('dragging');
    endLanding();
    landing = { node: d.node, signature, timer: 0, off: null };
    releaseGhost(outcome === 'reorder' ? homeX + d.node.shift : homeX);
    if (commit && d.dest !== d.from) {
      // The host's reorder then arrives and render() makes one FLIP from the release point to the
      // slot; if it never comes, the tab eases back to its origin.
      const others = d.order.filter((n) => n !== d.node).map((n) => n.id);
      send({ v: 1, t: 'tabmove', id: d.node.id, before: beforeSlotId(others, d.dest, collapsedLeaders(d.node.id)) });
      const l = landing;
      l.timer = window.setTimeout(() => {
        if (landing === l) landHome();
      }, REFUSE_MS);
      schedule();
      return;
    }
    landHome();
    schedule();
  };

  const updateEdges = (): void => {
    const left = strip.scrollLeft > 1;
    const right = strip.scrollLeft + strip.clientWidth < strip.scrollWidth - 1;
    strip.style.setProperty('--fade-left', left ? '12px' : '0px');
    strip.style.setProperty('--fade-right', right ? '12px' : '0px');
  };

  const onPointerDown = (ev: PointerEvent): void => {
    ignoreClick = false;
    groupIgnoreClick = false; // --- group drag ---
    if (ev.button !== 0 || drag !== null || groupDrag !== null || !(ev.target instanceof Element)) return;
    if (!ev.target.closest('.close')) unlockWidths(); // --- tab options ---
    // --- group drag --- the island's handle (its bar, folded pill or closing mark) can pick it up whole.
    const handle = ev.target.closest<HTMLElement>('.ball, .cap');
    if (handle) {
      pressGroup(ev, handle);
      return;
    }
    // --- end group drag ---
    if (ev.target.closest('.close, .audio, .closed-btn')) return;
    const tab = ev.target.closest<HTMLElement>('.tab');
    if (!tab) {
      if (ev.target instanceof HTMLElement && ev.target.hasAttribute(DRAG_ATTR)) send({ v: 1, t: 'drag' });
      return;
    }
    const node = tabNodes.get(Number(tab.dataset.id));
    if (!node) return;
    // A press while the last drop is still landing (its ease, or up to REFUSE_MS waiting for the host)
    // takes over: the landing ends where it is going and this press can lift a tab straight away.
    ghostLand?.session.abandon(); // --- tab ghost ---
    if (landing !== null) snapHome();
    const order = visibleTabs();
    const from = order.indexOf(node);
    if (from < 0) return;
    const home = measureHome(order);
    drag = {
      pointerId: ev.pointerId, startX: ev.clientX, node, order, centers: home.centers, from, active: false, dest: from, dx: 0,
      startY: ev.clientY, pointerType: ev.pointerType, lefts: home.lefts, widths: home.widths, phase: 'attached', ghost: null,
      lastX: ev.clientX, lastY: ev.clientY,
      stackOn: null, stackArmed: false, stackTimer: 0, // --- tab stacks (T10) ---
    };
    try { node.root.setPointerCapture(ev.pointerId); } catch { /* window listeners still end the drag */ }
  };

  const onPointerMove = (ev: PointerEvent): void => {
    if (moveGroup(ev)) return; // --- group drag ---
    const d = drag;
    if (!d || ev.pointerId !== d.pointerId) return;
    if (!d.active) {
      // Either axis lifts the tab: along the strip to reorder, down onto the page to split.
      if (Math.abs(ev.clientX - d.startX) < DRAG_START_PX && Math.abs(ev.clientY - d.startY) < DRAG_START_PX) return;
      d.active = true;
      d.node.root.classList.add('dragging');
      window.addEventListener('keydown', onDragKey, true);
      // --- tab ghost --- Rust lifts a native copy off the strip; the point grabbed stays under the cursor.
      const r = d.node.root.getBoundingClientRect();
      const { gx, gy } = grabOffset(d.startX, d.startY, r);
      const pinned = d.node.root.classList.contains('pinned');
      // The close button shows on the hovered tab unless the tab is too narrow for it (tabs.css).
      const close = !pinned && (d.node.root.classList.contains('active') || d.node.root.offsetWidth >= 123);
      d.ghost = GhostSession.start(d.node.id, 'tab', r.width, r.height, gx, gy, close, d.pointerType, ghostHooks(d.node));
      announce(dragAnnouncement('lift', titleOf(d.node)));
      // --- end tab ghost ---
    }
    d.lastX = ev.clientX;
    d.lastY = ev.clientY;
    layoutDrag(d);
  };

  const onPointerEnd = (ev: PointerEvent, commit: boolean): void => {
    // --- group drag ---
    if (groupDrag !== null && ev.pointerId === groupDrag.pointerId) {
      endGroupDrag(commit);
      return;
    }
    const d = drag;
    if (d && ev.pointerId === d.pointerId) endDrag(commit);
  };

  // ----- events -----

  strip.addEventListener('pointerdown', onPointerDown);
  strip.addEventListener('pointermove', onPointerMove);
  strip.addEventListener('pointerup', (ev) => onPointerEnd(ev, true));
  strip.addEventListener('pointercancel', (ev) => onPointerEnd(ev, false));
  strip.addEventListener('lostpointercapture', (ev) => onPointerEnd(ev, false));
  window.addEventListener('pointerup', (ev) => onPointerEnd(ev, true));
  window.addEventListener('pointercancel', (ev) => onPointerEnd(ev, false));
  window.addEventListener('blur', () => { endDrag(false); endGroupDrag(false); });
  strip.addEventListener('scroll', updateEdges, { passive: true });
  // The wheel steps among the strip's shown tabs.
  const wheelSwitch = (ev: WheelEvent, order: readonly TabNode[]): void => {
    ev.preventDefault();
    const now = Date.now();
    if (now - wheelAt > 400) wheelTotal = 0;
    wheelAt = now;
    const moved = wheelSteps(wheelTotal, ev.deltaY !== 0 ? ev.deltaY : ev.deltaX);
    wheelTotal = moved.rest;
    const target = moved.steps === 0 ? null : stepTab(order.map((n) => n.id), activeId, moved.steps);
    if (target !== null) send({ v: 1, t: 'tab', a: 'select', id: target });
  };
  strip.addEventListener('wheel', (ev) => {
    // --- tab options --- "Switch tabs by scrolling": a wheel over the strip steps through the tabs.
    if (ui.wheel && layoutMode === 'horizontal') {
      wheelSwitch(ev, visibleTabs());
      return;
    }
    // --- end tab options ---
    const delta = scrollDeltaForWheel(ev.deltaX, ev.deltaY, ev.shiftKey);
    if (delta !== 0) { ev.preventDefault(); strip.scrollLeft += delta; }
  }, { passive: false });

  strip.addEventListener('click', (ev) => {
    const t = ev.target;
    if (!(t instanceof Element)) return;
    const ball = t.closest<HTMLElement>('.ball, .cap');
    if (ball) {
      // --- group drag --- the release of a drag is not a click on the handle.
      if (groupIgnoreClick) {
        groupIgnoreClick = false;
        return;
      }
      send({ v: 1, t: 'group', a: 'toggle', key: Number(ball.dataset.key) });
      return;
    }
    const tab = t.closest<HTMLElement>('.tab');
    if (!tab) return;
    if (ignoreClick) {
      ignoreClick = false;
      return;
    }
    const id = Number(tab.dataset.id);
    const a: TabAction = t.closest('.close') ? 'close' : t.closest('.audio') ? 'mute' : 'select';
    // --- tab options ---
    if (a === 'select') {
      // Ctrl and Shift click select tabs without leaving the one being read; a plain click clears the selection.
      const outcome = ui.selection
        ? clickSelection(picked, visibleTabs().map((n) => n.id), anchor, activeId, id, { ctrl: ev.ctrlKey || ev.metaKey, shift: ev.shiftKey }, ui.selectionActive)
        : { selected: [] as number[], anchor: id, activate: true };
      anchor = outcome.anchor;
      setPicked(outcome.selected);
      if (!outcome.activate) return;
    } else if (a === 'close' && ui.alignClose && ev.detail > 0) {
      lockWidths(tab);
    }
    // --- end tab options ---
    send({ v: 1, t: 'tab', a, id });
  });

  strip.addEventListener('auxclick', (ev) => {
    if (ev.button !== 1 || !(ev.target instanceof Element)) return;
    const tab = ev.target.closest<HTMLElement>('.tab');
    if (!tab) return;
    ev.preventDefault();
    send({ v: 1, t: 'tab', a: 'close', id: Number(tab.dataset.id) });
  });

  // Middle button on a tab closes it; stop the browser's autoscroll cursor from starting.
  strip.addEventListener('mousedown', (ev) => {
    if (ev.button === 1 && ev.target instanceof Element && ev.target.closest('.tab')) ev.preventDefault();
  });

  strip.addEventListener('contextmenu', (ev) => {
    const t = ev.target;
    if (!(t instanceof Element)) return;
    const ball = t.closest<HTMLElement>('.ball, .cap');
    if (ball) {
      ev.preventDefault();
      send({ v: 1, t: 'group', a: 'menu', key: Number(ball.dataset.key) });
      return;
    }
    const tab = t.closest<HTMLElement>('.tab');
    if (tab) {
      ev.preventDefault();
      send({ v: 1, t: 'tab', a: 'menu', id: Number(tab.dataset.id) });
      return;
    }
    if (t instanceof HTMLElement && t.hasAttribute(DRAG_ATTR)) {
      ev.preventDefault();
      send({ v: 1, t: 'caption', a: 'systemMenu' });
    }
  });

  strip.addEventListener('dblclick', (ev) => {
    if (ev.target instanceof HTMLElement && ev.target.hasAttribute(DRAG_ATTR)) {
      send({ v: 1, t: 'caption', a: 'toggleMaximize' });
      return;
    }
    // --- tab options --- "Double click on a tab": Rust decides (rename, close or nothing).
    if (ev.target instanceof Element && !ev.target.closest('.close, .audio')) {
      const tab = ev.target.closest<HTMLElement>('.tab');
      if (tab) send({ v: 1, t: 'tab', a: 'dblclick', id: Number(tab.dataset.id) });
    }
    // --- end tab options ---
  });
  // --- tab options --- the pointer leaving the row ends a width lock.
  strip.addEventListener('pointerleave', unlockWidths);
  // --- end tab options ---

  strip.addEventListener('keydown', (ev) => {
    const t = ev.target;
    if (!(t instanceof Element)) return;
    // --- tab options --- Escape drops the selection.
    if (ev.key === 'Escape' && picked.length > 0) { ev.preventDefault(); setPicked([]); return; }
    // --- end tab options ---
    // --- group drag --- the tab move chord on an island's handle moves the whole island one slot.
    const handle = t.closest<HTMLElement>('.ball');
    if (handle && layoutMode === 'horizontal') {
      const dir = moveChordDir(ev.key, ev.ctrlKey, ev.shiftKey, ev.altKey);
      if (dir === 0) return;
      ev.preventDefault();
      const key = Number(handle.dataset.key);
      const units = dragUnits();
      const before = groupStepBefore(units.map((u) => u.first), units.findIndex((u) => u.group?.shell.dataset.key === String(key)), dir);
      if (before !== null && sendGroupMove(key, before)) refocusGroup = key;
      return;
    }
    // --- end group drag ---
    const tab = t.closest<HTMLElement>('.tab');
    if (!tab) return;
    // Enter/Space on the close or mute button activates that button (click), not the tab.
    if ((ev.key === 'Enter' || ev.key === ' ') && t.closest('.close, .audio')) return;
    const id = Number(tab.dataset.id);
    const list = visibleTabs();
    const at = list.findIndex((n) => n.id === id);
    if (at < 0) return;
    let next: number;
    if (ev.ctrlKey && ev.shiftKey && !ev.altKey && (ev.key === 'ArrowLeft' || ev.key === 'ArrowRight' || ev.key === 'PageUp' || ev.key === 'PageDown')) {
      // Keyboard reorder: one slot per press; focus follows the moved tab once the host reorders.
      // A collapsed group is one slot here too: before it means before its first member (U2).
      // Ctrl+Shift+Page Up/Down is the chord that reaches the strip: the host keeps Ctrl+Shift+Left/
      // Right for the split panes (docs/shortcuts.md).
      const before = stepMoveBefore(list.map((n) => n.id), at, ev.key === 'ArrowLeft' || ev.key === 'PageUp' ? -1 : 1, collapsedLeaders(id));
      ev.preventDefault();
      if (before !== null) {
        refocus = id;
        send({ v: 1, t: 'tabmove', id, before });
      }
      return;
    }
    switch (ev.key) {
      case 'ArrowLeft':
        next = Math.max(0, at - 1);
        break;
      case 'ArrowRight':
        next = Math.min(list.length - 1, at + 1);
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = list.length - 1;
        break;
      case 'Enter':
      case ' ':
        ev.preventDefault();
        send({ v: 1, t: 'tab', a: 'select', id });
        return;
      case 'Delete':
        ev.preventDefault();
        send({ v: 1, t: 'tab', a: 'close', id });
        return;
      default:
        return;
    }
    ev.preventDefault();
    const target = list[next];
    if (!target) return;
    applyRoving(target);
    target.root.focus();
    target.root.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  });

  onChromeInbound('tabs', strip.parentElement ?? strip, (msg) => {
    switch (msg.t) {
      // --- vertical sidebar (lane S) ---
      case 'layout':
        // A selection made in the other layout cannot be seen here: drop it in both, and in Rust.
        if (msg.mode !== layoutMode) {
          setPicked([]);
          sidebar.clearSelection();
        }
        if (msg.mode !== layoutMode) revealed = ''; // the other layout's view starts on the active tab
        layoutMode = msg.mode;
        stripAtBottom = msg.mode === 'horizontal' && msg.side === 'bottom';
        strip.classList.toggle('at-bottom', stripAtBottom);
        sidebar.side(msg.mode === 'vertical' && msg.side === 'right');
        sidebar.rail(msg.mode === 'vertical' && msg.rail === true);
        if (latest) {
          dirty = true;
          schedule();
        } else if (layoutMode === 'vertical') {
          strip.classList.remove('strip');
          strip.classList.add('sidebar');
          strip.setAttribute('aria-orientation', 'vertical');
        } else {
          strip.classList.remove('sidebar');
          strip.classList.add('strip');
          strip.removeAttribute('aria-orientation');
        }
        break;
      case 'workspace':
        // --- tab stacks and workspaces (T10) --- the emoji and the off flag ride along.
        workspace = { name: msg.name, icon: msg.icon, ...(msg.emoji !== undefined ? { emoji: msg.emoji } : {}), ...(msg.off ? { off: true } : {}) };
        if (layoutMode === 'vertical') sidebar.workspace(workspace);
        break;
      // --- end vertical sidebar ---
      case 'state':
        // Only the theme and mode reach the tab strip; scale and page state are toolbar concerns.
        applyTheme(msg.theme, msg.solid, msg.dark, msg.fontScale, msg.uiFont, msg.density, msg.compactMenus, msg.look);
        break;
      case 'tabs':
        latest = msg;
        dirty = true;
        schedule();
        break;
      // --- tab options ---
      case 'tabopts':
        ui = msg;
        applyUi();
        sidebar.options(msg, layoutMode === 'vertical');
        if (!msg.selection) setPicked([]); // the option turned off: the strip drops its selection too
        dirty = true;
        schedule();
        break;
      // --- end tab options ---
      case 'fav':
        rememberFav(favs, msg.id, msg.uri, new Set(latest?.items.map((item) => item.id) ?? []));
        dirty = true;
        schedule();
        break;
      // --- split view ---
      case 'pair':
        pair = msg;
        dirty = true;
        schedule();
        break;
      // --- end split view ---
      case 'tabghost': // --- tab ghost ---
        routeGhostMessage(msg);
        break;
    }
  }, () => { if (frame !== 0) cancelAnimationFrame(frame); flush(); });
}
