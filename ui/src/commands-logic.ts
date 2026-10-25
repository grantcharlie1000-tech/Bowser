// Pure logic for the settings page's Keyboard and Quick commands sections (docs/commands.md): the
// `commands` snapshot contract, the command and chain wire texts, capturing a chord from a key
// event, filtering and grouping the shortcut list, and the words. No DOM here; test/commands.mjs
// covers all of it, and test/fixtures/commands-snapshot.json is what Rust writes.
import type { PanelFields } from './protocol';

export const MAX_COMMANDS = 256;
export const MAX_CHAINS = 64;
export const MAX_STEPS = 32;
export const MAX_DELAY_MS = 60000;
export const MAX_CHAIN_NAME = 48;
export const MAX_CHORDS = 6;
export const MIN_RESULTS = 3;
export const MAX_RESULTS = 30;
const MAX_SITES = 200;

/** The result types, in their default order (src/commands/store.rs). */
export const RESULT_TYPES = ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history', 'calc'] as const;
export type ResultTypeKey = typeof RESULT_TYPES[number];
export const RESULT_TYPE_LABELS: Readonly<Record<ResultTypeKey, string>> = {
  tab: 'Open tabs',
  closed: 'Recently closed tabs',
  workspace: 'Workspaces',
  command: 'Commands',
  action: 'Page actions',
  bookmark: 'Bookmarks',
  note: 'Notes',
  history: 'History',
  calc: 'Calculator',
};
export const RESULT_TYPE_HINTS: Readonly<Record<ResultTypeKey, string>> = {
  tab: 'tab: or tb:',
  closed: 'with tab:',
  workspace: 'workspace:',
  command: 'command: or cmd:',
  action: 'action:',
  bookmark: 'bookmark: or bkm:',
  note: 'note:',
  history: 'history:',
  calc: 'type a sum',
};

export interface CommandInfo {
  readonly id: string;
  readonly label: string;
  readonly category: string;
  readonly chords: readonly string[];
  readonly defaults: readonly string[];
  /** The shortcuts differ from the defaults. */
  readonly changed: boolean;
  /** The browser keeps this command's shortcuts on a site that prioritises its own. */
  readonly priority: boolean;
  readonly chain: boolean;
}

export interface ChainStep { readonly delay: number; readonly command: string }
export interface ChainInfo { readonly id: number; readonly name: string; readonly steps: readonly ChainStep[] }

export type CommandsAck = 'none' | 'saved' | 'failed' | 'conflict' | 'ran';

export interface CommandsSnapshot {
  readonly ack: CommandsAck;
  readonly error: string;
  readonly conflictChord: string;
  readonly conflictId: string;
  readonly conflictLabel: string;
  readonly shortcutsEnabled: boolean;
  readonly singleKey: boolean;
  readonly altMenu: boolean;
  readonly focusCycling: boolean;
  readonly paletteKeepLast: boolean;
  readonly paletteNewTab: boolean;
  readonly paletteNotesInPanel: boolean;
  readonly paletteOpenNickname: boolean;
  readonly paletteMax: number;
  readonly paletteTypes: readonly ResultTypeKey[];
  readonly paletteDefaultOrder: boolean;
  readonly sites: readonly string[];
  readonly commands: readonly CommandInfo[];
  readonly chains: readonly ChainInfo[];
}

type RecordValue = Record<string, unknown>;
const isRecord = (v: unknown): v is RecordValue => typeof v === 'object' && v !== null && !Array.isArray(v);
const isStr = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max && !/[\u0000-\u001f\u007f]/.test(v);
const isInt = (v: unknown, min: number, max: number): v is number => typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
const ID_RE = /^[a-z0-9:-]{1,40}$/;
const SITE_RE = /^[a-z0-9.-]{1,253}$/;
const ACKS: readonly CommandsAck[] = ['none', 'saved', 'failed', 'conflict', 'ran'];
const KEYS = ['ack', 'error', 'conflictChord', 'conflictId', 'conflictLabel', 'shortcutsEnabled', 'singleKey', 'altMenu', 'focusCycling', 'paletteKeepLast',
  'paletteNewTab', 'paletteNotesInPanel', 'paletteOpenNickname', 'paletteMax', 'paletteTypes', 'paletteDefaultOrder', 'sites', 'commands', 'chains'];
const COMMAND_KEYS = ['id', 'label', 'category', 'chords', 'defaults', 'changed', 'priority', 'chain'];

function chordList(v: unknown): string[] | null {
  if (!Array.isArray(v) || v.length > MAX_CHORDS) return null;
  return v.every((chord) => isStr(chord, 40) && chord !== '') ? [...(v as string[])] : null;
}

/** `delay:command` as Rust writes a step. */
export function parseStep(text: string): ChainStep | null {
  const at = text.indexOf(':');
  if (at < 1) return null;
  const delay = Number(text.slice(0, at));
  const command = text.slice(at + 1);
  if (!isInt(delay, 0, MAX_DELAY_MS) || !ID_RE.test(command) || command.startsWith('chain:')) return null;
  return { delay, command };
}

/** Rejects a partial, extended or contradictory snapshot before anything is drawn. */
export function parseCommandsSnapshot(value: unknown): CommandsSnapshot | null {
  if (!isRecord(value) || Object.keys(value).length !== KEYS.length || Object.keys(value).some((key) => !KEYS.includes(key))) return null;
  const v = value;
  if (typeof v.ack !== 'string' || !(ACKS as readonly string[]).includes(v.ack)) return null;
  if (!isStr(v.error, 240) || !isStr(v.conflictChord, 40) || !isStr(v.conflictId, 40) || !isStr(v.conflictLabel, 64)) return null;
  if ((v.ack === 'conflict') !== (v.conflictChord !== '' && v.conflictId !== '')) return null;
  for (const key of ['shortcutsEnabled', 'singleKey', 'altMenu', 'focusCycling', 'paletteKeepLast', 'paletteNewTab', 'paletteNotesInPanel', 'paletteOpenNickname', 'paletteDefaultOrder'] as const) {
    if (typeof v[key] !== 'boolean') return null;
  }
  if (!isInt(v.paletteMax, MIN_RESULTS, MAX_RESULTS)) return null;
  if (!Array.isArray(v.paletteTypes) || v.paletteTypes.length < 1 || v.paletteTypes.length > RESULT_TYPES.length) return null;
  if (!v.paletteTypes.every((t) => typeof t === 'string' && (RESULT_TYPES as readonly string[]).includes(t)) || new Set(v.paletteTypes).size !== v.paletteTypes.length) return null;
  if (!Array.isArray(v.sites) || v.sites.length > MAX_SITES || !v.sites.every((s) => typeof s === 'string' && SITE_RE.test(s))) return null;
  if (!Array.isArray(v.commands) || v.commands.length > MAX_COMMANDS || !Array.isArray(v.chains) || v.chains.length > MAX_CHAINS) return null;
  const commands: CommandInfo[] = [];
  const ids = new Set<string>();
  for (const raw of v.commands as unknown[]) {
    if (!isRecord(raw) || Object.keys(raw).length !== COMMAND_KEYS.length || Object.keys(raw).some((key) => !COMMAND_KEYS.includes(key))) return null;
    const chords = chordList(raw.chords);
    const defaults = chordList(raw.defaults);
    if (typeof raw.id !== 'string' || !ID_RE.test(raw.id) || ids.has(raw.id) || !isStr(raw.label, 64) || !isStr(raw.category, 40) || raw.label === '' || !chords || !defaults) return null;
    if (typeof raw.changed !== 'boolean' || typeof raw.priority !== 'boolean' || typeof raw.chain !== 'boolean') return null;
    ids.add(raw.id);
    commands.push({ id: raw.id, label: raw.label, category: raw.category, chords, defaults, changed: raw.changed, priority: raw.priority, chain: raw.chain });
  }
  const chains: ChainInfo[] = [];
  const chainIds = new Set<number>();
  for (const raw of v.chains as unknown[]) {
    if (!isRecord(raw) || Object.keys(raw).length !== 3 || !isInt(raw.id, 1, 0xffffffff) || chainIds.has(raw.id) || !isStr(raw.name, MAX_CHAIN_NAME) || raw.name === '') return null;
    if (!Array.isArray(raw.steps) || raw.steps.length > MAX_STEPS) return null;
    const steps: ChainStep[] = [];
    for (const text of raw.steps as unknown[]) {
      const step = typeof text === 'string' ? parseStep(text) : null;
      if (!step) return null;
      steps.push(step);
    }
    chainIds.add(raw.id);
    chains.push({ id: raw.id, name: raw.name, steps });
  }
  if (v.ack === 'conflict' && !ids.has(v.conflictId as string)) return null;
  return {
    ack: v.ack as CommandsAck,
    error: v.error,
    conflictChord: v.conflictChord,
    conflictId: v.conflictId,
    conflictLabel: v.conflictLabel,
    shortcutsEnabled: v.shortcutsEnabled as boolean,
    singleKey: v.singleKey as boolean,
    altMenu: v.altMenu as boolean,
    focusCycling: v.focusCycling as boolean,
    paletteKeepLast: v.paletteKeepLast as boolean,
    paletteNewTab: v.paletteNewTab as boolean,
    paletteNotesInPanel: v.paletteNotesInPanel as boolean,
    paletteOpenNickname: v.paletteOpenNickname as boolean,
    paletteMax: v.paletteMax,
    paletteTypes: [...(v.paletteTypes as ResultTypeKey[])],
    paletteDefaultOrder: v.paletteDefaultOrder as boolean,
    sites: [...(v.sites as string[])],
    commands,
    chains,
  };
}

// ---- commands the page sends (each maps to one arm of chrome_host::commands_page::map_command) ----

export type FlagName = 'shortcuts_enabled' | 'single_key_shortcuts' | 'alt_opens_menu' | 'focus_cycling' | 'palette_keep_last' | 'palette_open_new_tab' | 'palette_notes_in_panel' | 'palette_open_nickname';
export const assignCommand = (id: string, chord: string, take: boolean): PanelFields => ({ a: 'kbassign', text: `${id}\t${chord}`, on: take });
export const removeChordCommand = (id: string, chord: string): PanelFields => ({ a: 'kbremove', text: `${id}\t${chord}` });
export const clearCommand = (id: string): PanelFields => ({ a: 'kbclear', text: id });
export const restoreCommand = (id: string): PanelFields => ({ a: 'kbrestore', text: id });
export const RESTORE_ALL_COMMAND: PanelFields = { a: 'kbrestoreall' };
export const CHEAT_SHEET_COMMAND: PanelFields = { a: 'kbsheet' };
/** The editor starts or stops waiting for a key press: while it waits Bowser hands every key to the page. */
export const captureModeCommand = (on: boolean): PanelFields => ({ a: 'kbcapture', on });
export const priorityCommand = (id: string, on: boolean): PanelFields => ({ a: 'kbpriority', text: id, on });
export const flagCommand = (name: FlagName, on: boolean): PanelFields => ({ a: 'kboption', text: name, on });
export const resultsMaxCommand = (n: number): PanelFields => ({ a: 'qcmax', n });
export const typesCommand = (types: readonly ResultTypeKey[]): PanelFields => ({ a: 'qctypes', text: types.join(',') });
export const TYPES_RESET_COMMAND: PanelFields = { a: 'qctypesreset' };
export const siteAddCommand = (site: string): PanelFields => ({ a: 'kbsiteadd', text: site });
export const siteRemoveCommand = (site: string): PanelFields => ({ a: 'kbsiteremove', text: site });
export const chainCloneCommand = (id: number): PanelFields => ({ a: 'chainclone', n: id });
export const chainDeleteCommand = (id: number): PanelFields => ({ a: 'chaindelete', n: id });

/** The chain as the page edits it: id 0 is a new chain. */
export interface ChainDraft { id: number; name: string; steps: ChainStep[] }

/** The text Rust reads for a chain: `id<TAB>name`, then one `delay<TAB>command` per step. */
export function chainDraftText(draft: ChainDraft): string {
  return [`${draft.id}\t${draft.name.replace(/[\t\n\r]/g, ' ').trim()}`, ...draft.steps.map((step) => `${step.delay}\t${step.command}`)].join('\n');
}
export const chainSaveCommand = (draft: ChainDraft): PanelFields => ({ a: 'chainsave', text: chainDraftText(draft) });
export const chainTestCommand = (draft: ChainDraft): PanelFields => ({ a: 'chaintest', text: chainDraftText(draft) });

/** Why a draft cannot be saved or tested yet, or `null` when it can (Rust checks again). */
export function draftError(draft: ChainDraft, others: readonly ChainInfo[]): string | null {
  const name = draft.name.trim();
  if (name === '') return 'Give the chain a name.';
  if (Array.from(name).length > MAX_CHAIN_NAME) return `A chain name holds at most ${MAX_CHAIN_NAME} characters.`;
  if (others.some((chain) => chain.id !== draft.id && chain.name.trim().toLowerCase() === name.toLowerCase())) return `There is already a chain called ${name}.`;
  if (draft.steps.length === 0) return 'Add at least one command to the chain.';
  if (draft.steps.length > MAX_STEPS) return `A chain holds at most ${MAX_STEPS} commands.`;
  if (draft.steps.some((step) => !Number.isInteger(step.delay) || step.delay < 0 || step.delay > MAX_DELAY_MS)) return `A delay is between 0 and ${MAX_DELAY_MS} ms.`;
  return null;
}

/** The delay a typed value means: whole milliseconds clamped to 0..MAX_DELAY_MS; 0 for anything else. */
export function parseDelay(text: string): number {
  const n = Math.round(Number(text));
  return Number.isFinite(n) ? Math.min(MAX_DELAY_MS, Math.max(0, n)) : 0;
}

/** `list` with the item at `from` moved to `to` (both clamped); the original is untouched. */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const copy = [...list];
  if (from < 0 || from >= copy.length) return copy;
  const [item] = copy.splice(from, 1);
  copy.splice(Math.min(Math.max(to, 0), copy.length), 0, item as T);
  return copy;
}

/** The enabled types of `order` with `type` shown or hidden; the order of the rest is kept, a shown type goes last. Never empty. */
export function toggleType(order: readonly ResultTypeKey[], type: ResultTypeKey, on: boolean): ResultTypeKey[] {
  if (on) return order.includes(type) ? [...order] : [...order, type];
  const rest = order.filter((t) => t !== type);
  return rest.length === 0 ? [...order] : rest;
}

// ---- capturing a chord from a key event ----

/** What a key event offers. `code` is the physical key; `key` the character it makes. */
export interface KeyLike {
  readonly key: string;
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
  readonly metaKey?: boolean;
}

const CODE_NAMES: Readonly<Record<string, string>> = {
  Backspace: 'Backspace', Tab: 'Tab', Enter: 'Enter', Space: 'Space', PageUp: 'PageUp', PageDown: 'PageDown', End: 'End', Home: 'Home',
  ArrowLeft: 'Left', ArrowUp: 'Up', ArrowRight: 'Right', ArrowDown: 'Down', Insert: 'Insert', Delete: 'Delete',
  Semicolon: ';', Equal: '=', Comma: ',', Minus: '-', Period: '.', Slash: '/', Backquote: '`', BracketLeft: '[', Backslash: '\\', BracketRight: ']', Quote: "'",
  NumpadMultiply: 'Numpad*', NumpadAdd: 'Numpad+', NumpadSubtract: 'Numpad-', NumpadDecimal: 'Numpad.', NumpadDivide: 'Numpad/',
  BrowserBack: 'BrowserBack', BrowserForward: 'BrowserForward', BrowserRefresh: 'BrowserRefresh', BrowserStop: 'BrowserStop',
  ContextMenu: 'Menu',
};

/** The name Rust's `Chord::parse` reads for a key event's key, or `null` for a modifier or an unknown key. */
export function keyName(event: Pick<KeyLike, 'key' | 'code'>): string | null {
  const { code } = event;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  return Object.hasOwn(CODE_NAMES, code) ? CODE_NAMES[code]! : null;
}

export type Capture =
  | { readonly kind: 'wait' }
  | { readonly kind: 'cancel' }
  | { readonly kind: 'chord'; readonly chord: string };

/**
 * What a key press means while the editor waits for a new shortcut: a modifier alone keeps waiting,
 * Escape on its own cancels, anything else with a known key is the chord (Ctrl, Alt, Shift in that order).
 */
export function captureChord(event: KeyLike): Capture {
  if (event.key === 'Escape' && !event.ctrlKey && !event.altKey && !event.shiftKey) return { kind: 'cancel' };
  const key = keyName(event);
  if (key === null) return { kind: 'wait' };
  const parts = [event.ctrlKey ? 'Ctrl' : '', event.altKey ? 'Alt' : '', event.shiftKey ? 'Shift' : '', key].filter(Boolean);
  return { kind: 'chord', chord: parts.join('+') };
}

// ---- the shortcut list ----

export interface CommandGroup { readonly category: string; readonly commands: readonly CommandInfo[] }

/** Commands whose label, category or shortcuts hold every word of `query`, grouped by category in the order they came. */
export function groupCommands(commands: readonly CommandInfo[], query: string): CommandGroup[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const groups: { category: string; commands: CommandInfo[] }[] = [];
  for (const command of commands) {
    const hay = `${command.label} ${command.category} ${command.chords.join(' ')}`.toLowerCase();
    if (!words.every((word) => hay.includes(word))) continue;
    let group = groups.find((g) => g.category === command.category);
    if (!group) {
      group = { category: command.category, commands: [] };
      groups.push(group);
    }
    group.commands.push(command);
  }
  return groups;
}

/** A typed site for the prioritised list (the same rules as the blocker's list, see settings-page-logic normaliseSite). */
export function siteError(list: readonly string[], site: string | null): string | null {
  if (site === null) return 'Enter a site.';
  if (list.includes(site)) return `${site} is already on the list.`;
  if (list.length >= MAX_SITES) return `The list holds at most ${MAX_SITES} sites.`;
  return null;
}

/** The toast for Bowser's answer to a command: nothing for `none`. */
export function ackToast(snapshot: Pick<CommandsSnapshot, 'ack' | 'error'>): { readonly text: string; readonly ok: boolean } | null {
  switch (snapshot.ack) {
    case 'saved': return { text: 'Saved', ok: true };
    case 'ran': return { text: 'Chain started', ok: true };
    case 'failed': return { text: snapshot.error !== '' ? snapshot.error : 'That could not be saved.', ok: false };
    default: return null;
  }
}

/** The line under a command when its chord is wanted by another: what the editor offers to do. */
export function conflictText(snapshot: Pick<CommandsSnapshot, 'conflictChord' | 'conflictLabel'>): string {
  return `${snapshot.conflictChord} is already used by ${snapshot.conflictLabel}.`;
}

export const SHORTCUTS_OFF_NOTE = 'Shortcuts are off. Quick commands (Ctrl+Shift+K) still works so you can turn them back on.';
export const ALT_MENU_NOTE = 'Alt on its own opens the Bowser menu.';
