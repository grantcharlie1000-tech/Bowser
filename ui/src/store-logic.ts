// Built-in store contract and pure filtering. Resolved manifest lists are never shortened.
import type { PanelFields } from './protocol';
export interface StoreEntry {
  readonly id: string; readonly name: string; readonly description: string;
  readonly publisher: string; readonly category: string; readonly source: string;
  readonly homepage: string; readonly provenance: string; readonly checked: string;
  readonly compatibility: string; readonly compatibilityNote: string;
}
export const STORE_PHASES = ['idle', 'downloading', 'review', 'installing', 'ready', 'error', 'cancelled'] as const;
export type StorePhase = typeof STORE_PHASES[number];
/** Every list Rust sends, with the label the review's Details use. None is ever shortened. */
export const PERMISSION_GROUPS = [
  ['permissions', 'Permissions'], ['hostPermissions', 'Site access'],
  ['contentScripts', 'Scripts run on'], ['contentScriptDeclarations', 'Script declarations'], ['optionalPermissions', 'Optional permissions'],
  ['optionalHostPermissions', 'Optional site access'],
] as const;
export type StoreSource = 'chrome' | 'edge';
export const STORE_SOURCES: Readonly<Record<StoreSource, string>> = { chrome: 'Chrome Web Store', edge: 'Edge Add-ons' };
type PermissionKey = typeof PERMISSION_GROUPS[number][0];
export interface StoreSnapshot extends Readonly<Record<PermissionKey, readonly string[]>> {
  readonly shown: boolean; readonly catalogue: readonly StoreEntry[]; readonly phase: StorePhase;
  readonly requestId: string; readonly extensionId: string; readonly name: string;
  readonly version: string; readonly error: string; readonly restart: boolean;
  /** Which store the package comes from; older senders omit it (Chrome Web Store). */
  readonly source: StoreSource;
  /** The package's own icon as a base64 raster data URL, or `''` (Rust sets it only in review,
   *  installing and ready). Always present after parsing: see `storeIcon`. */
  readonly icon: string;
  /** Optional, not sent by Rust yet: which half of `downloading` is running, and the byte counts of
   *  the download. Absent means "download and verification, progress unknown" (indeterminate). */
  readonly stage?: 'download' | 'verify'; readonly received?: number; readonly total?: number;
}
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const text = (v: unknown, max = 4096): v is string => typeof v === 'string' && v.length <= max * 2 && Array.from(v).length <= max && !/[\u0000-\u001f\u007f]/.test(v);
export const isStoreId = (v: unknown): v is string => typeof v === 'string' && /^[a-p]{32}$/.test(v);
export const isStoreRequestId = (v: unknown): v is string => typeof v === 'string' && /^[a-zA-Z0-9-]{1,64}$/.test(v);
const STORE_ICON = /^data:image\/(?:png|jpeg|gif|webp);base64,[A-Za-z0-9+/]+={0,2}$/;
/** The snapshot's icon if it is a bounded base64 PNG/JPEG/GIF/WebP data URL, else `''`. Never SVG
 *  (scriptable markup) or a fetchable URL; a bad icon costs the picture, never the snapshot. */
export const storeIcon = (v: unknown): string => typeof v === 'string' && v.length <= 131072 && STORE_ICON.test(v) ? v : '';
export function parseStoreSnapshot(v: unknown): StoreSnapshot | null {
  if (!record(v) || typeof v.shown !== 'boolean' || typeof v.restart !== 'boolean' || !STORE_PHASES.includes(v.phase as StorePhase)) return null;
  for (const key of ['requestId', 'extensionId', 'name', 'version', 'error']) if (!text(v[key])) return null;
  if (v.requestId !== '' && !isStoreRequestId(v.requestId)) return null;
  if (!Array.isArray(v.catalogue) || v.catalogue.length > 128) return null;
  const ids = new Set<string>();
  for (const item of v.catalogue) {
    if (!record(item) || !isStoreId(item.id) || ids.has(item.id)) return null;
    ids.add(item.id);
    if (!['untested', 'tested', 'partial', 'unsupported'].includes(item.compatibility as string)) return null;
    for (const key of ['name', 'description', 'publisher', 'category', 'source', 'homepage', 'provenance', 'checked', 'compatibility', 'compatibilityNote']) if (!text(item[key])) return null;
    if (item.name === '' || item.publisher === '' || item.category === '') return null;
  }
  for (const [key] of PERMISSION_GROUPS) {
    const limit = key === 'contentScriptDeclarations' ? 180 * 1024 : 256;
    if (!Array.isArray(v[key]) || v[key].length > 256 || !v[key].every((item) => text(item, limit))) return null;
    if (key === 'contentScriptDeclarations' && v[key].some((item) => /[^\x20-\x7e]/.test(item as string))) return null;
  }
  if (v.phase === 'review' && (!isStoreRequestId(v.requestId) || v.version === '' || v.name === '' || !isStoreId(v.extensionId))) return null;
  if ('stage' in v && v.stage !== 'download' && v.stage !== 'verify') return null;
  const bytes = (n: unknown): boolean => Number.isSafeInteger(n) && (n as number) >= 0;
  if (('received' in v || 'total' in v) && (!bytes(v.received) || !bytes(v.total) || (v.total as number) === 0 || (v.received as number) > (v.total as number))) return null;
  if ('source' in v && v.source !== 'chrome' && v.source !== 'edge') return null;
  return { ...v, source: v.source ?? 'chrome', icon: storeIcon(v.icon) } as unknown as StoreSnapshot;
}
/** The store command text for one package: a bare id for the Chrome Web Store, `edge:` and the id for Edge Add-ons. */
const storeText = (id: string, source: StoreSource): string => source === 'edge' ? `edge:${id}` : id;
export function storeInstallCommand(id: string, source: StoreSource = 'chrome'): PanelFields | null { return isStoreId(id) ? { a: 'storeinstall', text: storeText(id, source) } : null; }
export function storeListingCommand(id: string, source: StoreSource = 'chrome'): PanelFields | null { return isStoreId(id) ? { a: 'storelisting', text: storeText(id, source) } : null; }
export function storeRequestCommand(action: 'storecancel' | 'storeapprove', id: string): PanelFields | null { return isStoreRequestId(id) ? { a: action, text: id } : null; }
export const storeBusy = (phase: StorePhase): boolean => phase === 'downloading' || phase === 'review' || phase === 'installing';
/** The one-line state heading of the installation card (also what the live region announces). */
export function storeStatus(s: StoreSnapshot): string {
  switch (s.phase) {
    case 'idle': return '';
    case 'downloading': return s.stage === 'verify' ? 'Checking…' : 'Downloading…';
    case 'review': return `Add ${storeDisplayName(s)}?`;
    case 'installing': return 'Adding…';
    case 'ready': return `${storeDisplayName(s)} added`;
    case 'error': return storeFailure(s.error).title;
    case 'cancelled': return 'Cancelled';
  }
}

/** The name to show for the package in flight: the resolved name, the catalogue's, or a neutral label. */
export function storeDisplayName(s: StoreSnapshot): string {
  return s.name || s.catalogue.find((entry) => entry.id === s.extensionId)?.name || 'this extension';
}

// ---- What the review says it can do: short, plain lines, the most far-reaching first ----

/** Patterns that reach every site. */
const ALL_SITES = /^(?:<all_urls>|\*:\/\/\*\/\*?|https?:\/\/\*\/\*?|\*:\/\/\*)$/;

/** Chromium's install warnings, in its words where it has them. Permissions with no warning in
 *  Chromium (storage, alarms, contextMenus, scripting, activeTab…) say nothing here either. */
const PERMISSION_LINES: Readonly<Record<string, string>> = {
  debugger: 'Read and change all your data on all websites',
  proxy: 'Read and change all your data on all websites',
  history: 'Read and change your browsing history',
  tabs: 'Read your browsing history',
  webNavigation: 'Read your browsing history',
  sessions: 'Read your browsing history',
  topSites: 'Read a list of your most visited websites',
  bookmarks: 'Read and change your bookmarks',
  readingList: 'Read and change your reading list',
  downloads: 'Manage your downloads',
  'downloads.open': 'Open downloaded files',
  declarativeNetRequest: 'Block content on any page',
  declarativeNetRequestWithHostAccess: 'Block content on the sites it can access',
  clipboardRead: 'Read data you copy and paste',
  clipboardWrite: 'Change data you copy and paste',
  geolocation: 'Know your location',
  notifications: 'Show notifications',
  nativeMessaging: 'Talk to apps on this computer',
  management: 'Manage your other extensions',
  privacy: 'Change your privacy settings',
  contentSettings: 'Change site settings, such as cookies and JavaScript',
  desktopCapture: 'Capture what is on your screen',
  tabCapture: 'Capture tab audio and video',
  pageCapture: 'Save pages you visit',
  'identity.email': 'Know your email address',
  ttsEngine: 'Read all text spoken by Bowser',
  'system.storage': 'See and eject storage devices',
};

/** The host part of a match pattern, for "on example.com"; `null` when it has none to show. */
function patternHost(pattern: string): string | null {
  const host = /^[a-z*-]+:\/\/([^/]+)/i.exec(pattern)?.[1];
  if (!host || host === '*') return null;
  return host.replace(/^\*\./, '');
}

/** One line per thing the extension can do, most far-reaching first, and whether it may ask for more later. */
export function storeAccess(s: Pick<StoreSnapshot, 'permissions' | 'hostPermissions' | 'contentScripts' | 'optionalPermissions' | 'optionalHostPermissions'>): { readonly lines: readonly string[]; readonly later: boolean } {
  const lines: string[] = [];
  const add = (line: string): void => { if (!lines.includes(line)) lines.push(line); };
  const sites = [...s.hostPermissions, ...s.contentScripts];
  if (sites.some((pattern) => ALL_SITES.test(pattern)) || s.permissions.some((p) => p === 'debugger' || p === 'proxy')) {
    add('Read and change all your data on all websites');
  } else {
    const hosts = [...new Set(sites.map(patternHost).filter((host): host is string => host !== null))];
    if (hosts.length === 1) add(`Read and change your data on ${hosts[0]}`);
    else if (hosts.length > 1 && hosts.length <= 3) add(`Read and change your data on ${hosts.slice(0, -1).join(', ')} and ${hosts.at(-1)}`);
    else if (hosts.length > 3) add(`Read and change your data on ${hosts.length} sites`);
  }
  for (const permission of s.permissions) {
    const line = PERMISSION_LINES[permission];
    if (line) add(line);
  }
  return { lines, later: s.optionalPermissions.length + s.optionalHostPermissions.length > 0 };
}

export type StoreProgress = { readonly kind: 'indeterminate'; readonly label: string } | { readonly kind: 'determinate'; readonly label: string; readonly value: number; readonly text: string };

const size = (n: number): string => n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

/** A bar only while something runs: determinate only when Rust reports both byte counts. */
export function storeProgress(s: StoreSnapshot): StoreProgress | null {
  if (s.phase === 'installing') return { kind: 'indeterminate', label: 'Installation progress' };
  if (s.phase !== 'downloading') return null;
  if (s.stage !== 'verify' && s.received !== undefined && s.total !== undefined) {
    const value = s.received / s.total;
    return { kind: 'determinate', label: 'Download progress', value, text: `${size(s.received)} of ${size(s.total)}` };
  }
  return { kind: 'indeterminate', label: s.stage === 'verify' ? 'Verification progress' : 'Download progress' };
}

export interface StoreFailure {
  readonly title: string; readonly body: string;
  /** The native message verbatim, for "Details"; empty when `body` already is that message. */
  readonly detail: string;
  /** `suggest`: trying again may well work; `offer`: it might; `none`: it cannot help, or the snapshot's
   *  extension ID may belong to an earlier request (Rust keeps it on errors raised before a new start). */
  readonly retry: 'suggest' | 'offer' | 'none';
  /** False when the error happens before a request starts, so the ID/name may be an earlier one's. */
  readonly identity: boolean;
  /** Offer the installed-extensions list (the package is already there). */
  readonly manage: boolean;
}

/** Plain-language reading of the native error text. Rust sends free text, so this matches its known
 *  messages (src/store.rs, src/win32/store.rs) and falls back to showing the text as it is. */
export function storeFailure(error: string): StoreFailure {
  const f = (title: string, body: string, retry: StoreFailure['retry'], identity = true, detail = error, manage = false): StoreFailure => ({ title, body, detail, retry, identity, manage });
  if (/^Package download failed:/.test(error)) return f('Couldn’t download the extension', 'Check your connection and try again.', 'suggest');
  if (/^Package verification failed:|^The package verified, but it could not be unpacked/.test(error)) {
    const cap = /larger than the per-file cap|more than the size cap|ratio/.test(error);
    return f('The package didn’t pass Bowser’s checks', cap ? 'A file in it is larger than Bowser allows.' : 'Try again to download a fresh copy.', cap ? 'offer' : 'suggest');
  }
  if (/already installed/.test(error)) return f('Already added', '', 'none', true, '', true);
  if (/too large (?:for Bowser )?to (?:display|show) safely/.test(error)) return f('Too much to review', 'It declares more access than Bowser can show, so it wasn’t added.', 'none');
  if (/^Installation was not accepted|^No extension was installed\.|source identity changed|source could not be recorded/.test(error)) return f('Couldn’t add the extension', '', 'offer');
  if (/WebView2 runtime cannot run supported extensions/.test(error)) return f('Update WebView2 to use extensions', 'Install the latest Microsoft Edge WebView2 Runtime, then try again.', 'none', false, '');
  if (/^Other extension downloads are finishing/.test(error)) return f('Another download is finishing', 'Try again in a moment.', 'none', false, '');
  if (/persistent normal profile/.test(error)) return f('Extensions need a normal profile', 'Guest and temporary profiles can’t keep extensions.', 'none', false, '');
  if (/^That extension ID is not valid|Chrome Web Store or Edge Add-ons extension link|^The profile is not ready|requires the HTML interface/.test(error)) return f('Couldn’t start', error, 'none', false, '');
  return f('The extension wasn’t added', error, 'offer', true, '');
}

/** "Try again" restarts the same native request flow: `storeinstall` with the snapshot's ID. Only
 *  after a cancel or an error tied to that request (see `StoreFailure.retry`). */
export function storeRetryCommand(s: StoreSnapshot): PanelFields | null {
  if (s.phase === 'cancelled') return storeInstallCommand(s.extensionId, s.source);
  if (s.phase !== 'error' || storeFailure(s.error).retry === 'none') return null;
  return storeInstallCommand(s.extensionId, s.source);
}

/** A convenience input only: the native backend independently validates the listing URL. */
export function storeUrlCommand(value: string): PanelFields | null {
  try {
    const url = new URL(value.trim());
    if (url.protocol !== 'https:' || url.href.slice(8).split('/')[0]!.includes('@') || url.port !== '') return null;
    const modern = url.hostname === 'chromewebstore.google.com' && /^\/detail\/(?:[^/]+\/)?[a-p]{32}\/?$/.test(url.pathname);
    const legacy = url.hostname === 'chrome.google.com' && /^\/webstore\/detail\/(?:[^/]+\/)?[a-p]{32}\/?$/.test(url.pathname);
    const edge = url.hostname === 'microsoftedge.microsoft.com' && /^\/addons\/detail\/(?:[^/]+\/)?[a-p]{32}\/?$/.test(url.pathname);
    return modern || legacy || edge ? { a: 'storeurl', text: url.href } : null;
  } catch { return null; }
}
