// Pure logic for the settings page's Extensions section and the toolbar's extensions button
// (docs/extensions.md "Managing extensions"): the snapshot contract, the words that depend on state,
// and the commands. No DOM here; test/extensions.mjs covers all of it.
//
// Honesty rules for the words: Bowser installs unpacked folders and .crx files already on this
// computer, never from a store; it shows what a manifest
// declares but does not enforce or narrow it (assume an extension that is on has all of it);
// turning an extension back on reuses the copy that was installed and asks for nothing new.
import { MAX_EXTENSION_NAME, MAX_EXTENSIONS, type ExtButtonMsg, type PanelFields } from './protocol';

/** Items of each declaration list Rust sends (`extensions_page::MAX_LISTED`); the full count comes beside them. */
export const MAX_LISTED = 8;
const MAX_ITEM = 64;
const MAX_VERSION = 24;
const MAX_FAILURE = 200;
const MAX_ERROR = 240;
const MAX_DECLARED = 512;
const ID_RE = /^ext-[0-9a-f]{16}$/;
const CONTROL_CHAR_RE = /[\u0000-\u001f\u007f]/;

export const EXTENSION_ACKS = ['none', 'enabled', 'disabled', 'removed', 'installed', 'failed'] as const;
export type ExtensionsAck = typeof EXTENSION_ACKS[number];

/** The four declaration lists, in the order the page shows them. */
export const DECLARATIONS = ['sites', 'scripts', 'perms', 'optional'] as const;
export type Declaration = typeof DECLARATIONS[number];
export const DECLARATION_LABELS: Readonly<Record<Declaration, string>> = {
  sites: 'Site access',
  scripts: 'Runs scripts on',
  perms: 'Permissions',
  optional: 'Optional',
};

export interface DeclaredList { readonly items: readonly string[]; readonly total: number }

export interface ExtensionItem {
  readonly id: string;
  readonly name: string;
  readonly version: string;
  readonly enabled: boolean;
  /** The manifest names a popup Bowser can open. */
  readonly popup: boolean;
  /** Why it failed to load or change this session, or ''. */
  readonly failure: string;
  /** Bowser's copy of the manifest could be read (otherwise every list is empty). */
  readonly readable: boolean;
  /** Its required site access, permissions or content scripts cover every website. */
  readonly all: boolean;
  readonly declared: Readonly<Record<Declaration, DeclaredList>>;
}

export interface ExtensionsSnapshot {
  /** `error`: the installed list could not be read; `error` says why. */
  readonly state: 'on' | 'error';
  /** The extension being removed, or ''. */
  readonly busy: string;
  readonly ack: ExtensionsAck;
  readonly error: string;
  readonly items: readonly ExtensionItem[];
}

const SNAPSHOT_KEYS = ['state', 'busy', 'ack', 'error', 'items'];
const ITEM_KEYS = ['id', 'name', 'version', 'enabled', 'popup', 'failure', 'readable', 'all', ...DECLARATIONS.flatMap((key) => [key, `${key}N`])];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function exactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const own = Object.keys(value);
  return own.length === keys.length && own.every((key) => keys.includes(key));
}
/** Text Rust capped at `max` characters, with no control characters (it is drawn as text only). */
function isText(value: unknown, max: number, allowEmpty = true): value is string {
  return typeof value === 'string' && (allowEmpty || value !== '') && value.length <= max * 2 && Array.from(value).length <= max && !CONTROL_CHAR_RE.test(value);
}
function isCount(value: unknown, max: number): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max;
}

export function isExtensionId(value: unknown): value is string {
  return typeof value === 'string' && ID_RE.test(value);
}

function parseItem(value: unknown): ExtensionItem | null {
  if (!isRecord(value) || !exactKeys(value, ITEM_KEYS)) return null;
  const v = value;
  if (!isExtensionId(v.id) || !isText(v.name, MAX_EXTENSION_NAME, false) || !isText(v.version, MAX_VERSION) || !isText(v.failure, MAX_FAILURE)) return null;
  for (const key of ['enabled', 'popup', 'readable', 'all'] as const) if (typeof v[key] !== 'boolean') return null;
  const declared = {} as Record<Declaration, DeclaredList>;
  for (const key of DECLARATIONS) {
    const items = v[key];
    const total = v[`${key}N`];
    if (!Array.isArray(items) || items.length > MAX_LISTED || !items.every((item) => isText(item, MAX_ITEM, false))) return null;
    if (!isCount(total, MAX_DECLARED) || total < items.length || (total > items.length && items.length < MAX_LISTED)) return null;
    declared[key] = { items: items as string[], total };
  }
  // An unreadable manifest declares nothing the page could show, and reaches nothing it knows of.
  if (v.readable === false && (v.all === true || DECLARATIONS.some((key) => declared[key].total > 0))) return null;
  return { id: v.id, name: v.name, version: v.version, enabled: v.enabled as boolean, popup: v.popup as boolean, failure: v.failure, readable: v.readable as boolean, all: v.all as boolean, declared };
}

/** The `extensions` snapshot, strictly: every key, nothing else, ids unique. `null` when it is not one. */
export function parseExtensionsSnapshot(value: unknown): ExtensionsSnapshot | null {
  if (!isRecord(value) || !exactKeys(value, SNAPSHOT_KEYS)) return null;
  const v = value;
  if (v.state !== 'on' && v.state !== 'error') return null;
  if (!(v.busy === '' || isExtensionId(v.busy))) return null;
  if (!(EXTENSION_ACKS as readonly unknown[]).includes(v.ack) || !isText(v.error, MAX_ERROR)) return null;
  if (!Array.isArray(v.items) || v.items.length > MAX_EXTENSIONS) return null;
  if (v.state === 'error' && (v.items.length > 0 || v.error === '')) return null;
  const items: ExtensionItem[] = [];
  for (const raw of v.items) {
    const item = parseItem(raw);
    if (!item || items.some((other) => other.id === item.id)) return null;
    items.push(item);
  }
  return { state: v.state, busy: v.busy as string, ack: v.ack as ExtensionsAck, error: v.error, items };
}

// ---- words ----

/** Under the list: what the permissions shown are, and what Bowser can and cannot do about them. */
export const EXTENSIONS_NOTE = 'Assume an extension that is on has everything it declares. Bowser shows these but cannot narrow them.';

/** Under the name: the version and whether it is on, or that it is being removed. */
export function statusText(item: ExtensionItem, busy: boolean): string {
  const version = item.version === '' ? '' : `Version ${item.version}`;
  const state = busy ? 'Removing…' : item.enabled ? (item.failure === '' ? 'On' : 'On · not running') : 'Off';
  return version === '' ? state : `${version} · ${state}`;
}

/** The one-line summary of what it can reach, said plainly. */
export function reachText(item: ExtensionItem): string {
  if (!item.readable) return 'Bowser could not read its manifest, so what it declares is unknown.';
  if (item.all) return 'Can read and change every website you visit.';
  const sites = item.declared.sites.total;
  const scripts = item.declared.scripts.total;
  if (sites === 0 && scripts === 0) {
    const perms = item.declared.perms.total;
    return perms === 0 ? 'Declares no site access and no permissions.' : `Declares no site access. ${perms} ${perms === 1 ? 'permission' : 'permissions'}.`;
  }
  const patterns = sites + scripts;
  return `Can read and change pages that match ${patterns} ${patterns === 1 ? 'pattern' : 'patterns'}.`;
}

/** The lists the details show: only the ones that declare something, each with how many were left out. */
export function declarationGroups(item: ExtensionItem): { readonly key: Declaration; readonly label: string; readonly items: readonly string[]; readonly more: number }[] {
  return DECLARATIONS
    .filter((key) => item.declared[key].total > 0)
    .map((key) => ({ key, label: DECLARATION_LABELS[key], items: item.declared[key].items, more: item.declared[key].total - item.declared[key].items.length }));
}

/** The toggle's accessible name (the switch itself says on or off). */
export function switchLabel(item: ExtensionItem): string {
  return `${item.name} on`;
}

/** The toast for Bowser's answer to the page's last command, or `null` for none. */
export function ackToast(ack: ExtensionsAck, error: string): { readonly text: string; readonly ok: boolean } | null {
  switch (ack) {
    case 'none': return null;
    case 'enabled': return { text: 'Extension turned on', ok: true };
    case 'disabled': return { text: 'Extension turned off', ok: true };
    case 'removed': return { text: 'Extension removed', ok: true };
    case 'installed': return { text: 'Extension installed', ok: true };
    case 'failed': return { text: error !== '' ? error : 'The extension was not changed.', ok: false };
  }
}

/** What the confirmation asks before a removal. */
export function removeQuestion(item: ExtensionItem): string {
  return `Remove ${item.name}? Bowser unloads it and deletes its copied files. To use it again, install its folder again.`;
}

// ---- commands (the settings page's `set` family; Rust checks each id again) ----

export function setEnabledCommand(id: string, on: boolean): PanelFields | null {
  return isExtensionId(id) ? { a: 'extension', text: id, on } : null;
}
export function removeCommand(id: string): PanelFields | null {
  return isExtensionId(id) ? { a: 'extremove', text: id } : null;
}
export function popupCommand(id: string): PanelFields | null {
  return isExtensionId(id) ? { a: 'extpopup', text: id } : null;
}
export const INSTALL_COMMAND: PanelFields = { a: 'extinstall' };
/** Opens Bowser's own file picker for one `.crx` on this computer; the page never names the file. */
export const INSTALL_CRX_COMMAND: PanelFields = { a: 'extinstallcrx' };
export const INSTALL_CRX_LABEL = 'Install CRX file…';
/** A file on disk, verified, then the same confirmation: it is not a store install and never says so. */
export const INSTALL_CRX_TIP = 'Choose a .crx file on this computer. Bowser checks its signature, then asks before copying anything.';
export const EMPTY_DETAIL = 'Install an unpacked extension by choosing the folder that holds its manifest.json, or install a .crx file you already have on this computer. Bowser lists what it declares and asks before copying anything.';

// ---- the toolbar button ----

/** The button's accessible name and tooltip: the extension's own name when it is the only one. */
export function extButtonText(msg: Pick<ExtButtonMsg, 'n' | 'name'>): { readonly label: string; readonly tip: string; readonly menu: boolean } {
  if (msg.n === 1 && msg.name !== '') return { label: `Open ${msg.name}`, tip: msg.name, menu: false };
  return { label: 'Extensions', tip: `Extensions (${msg.n})`, menu: true };
}
