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
export const PERMISSION_GROUPS = [
  ['permissions', 'Required permissions'], ['hostPermissions', 'Required site access'],
  ['contentScripts', 'Content scripts run on'], ['contentScriptDeclarations', 'Complete static content-script declarations'], ['optionalPermissions', 'Optional permissions'],
  ['optionalHostPermissions', 'Optional site access'],
] as const;
type PermissionKey = typeof PERMISSION_GROUPS[number][0];
export interface StoreSnapshot extends Readonly<Record<PermissionKey, readonly string[]>> {
  readonly shown: boolean; readonly catalogue: readonly StoreEntry[]; readonly phase: StorePhase;
  readonly requestId: string; readonly extensionId: string; readonly name: string;
  readonly version: string; readonly error: string; readonly restart: boolean;
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
  return { ...v, icon: storeIcon(v.icon) } as unknown as StoreSnapshot;
}
export function filterCatalogue(items: readonly StoreEntry[], query: string, category = ''): readonly StoreEntry[] {
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return items.filter((item) => (category === '' || category === item.category) && words.every((word) => `${item.name} ${item.description} ${item.publisher} ${item.category}`.toLocaleLowerCase().includes(word)));
}
export function storeInstallCommand(id: string): PanelFields | null { return isStoreId(id) ? { a: 'storeinstall', text: id } : null; }
export function storeRequestCommand(action: 'storecancel' | 'storeapprove', id: string): PanelFields | null { return isStoreRequestId(id) ? { a: action, text: id } : null; }
export const storeBusy = (phase: StorePhase): boolean => phase === 'downloading' || phase === 'review' || phase === 'installing';
/** The one-line state heading of the installation card (also what the live region announces). */
export function storeStatus(s: StoreSnapshot): string {
  switch (s.phase) {
    case 'idle': return '';
    case 'downloading': return s.stage === 'download' ? 'Downloading the package…' : s.stage === 'verify' ? 'Verifying the package…' : 'Downloading and verifying the package…';
    case 'review': return 'Review before you install';
    case 'installing': return 'Installing…';
    case 'ready': return `${storeDisplayName(s)} is installed`;
    case 'error': return storeFailure(s.error).title;
    case 'cancelled': return 'Installation cancelled';
  }
}

/** The name to show for the package in flight: the resolved name, the catalogue's, or a neutral label. */
export function storeDisplayName(s: StoreSnapshot): string {
  return s.name || s.catalogue.find((entry) => entry.id === s.extensionId)?.name || 'Chrome Web Store extension';
}

export const STORE_STEPS = [['download', 'Download'], ['verify', 'Verify'], ['review', 'Review'], ['install', 'Install']] as const;
export type StoreStep = typeof STORE_STEPS[number][0];
export type StoreStepState = 'done' | 'current' | 'failed' | 'todo';
export interface StoreStepView { readonly key: StoreStep; readonly label: string; readonly state: StoreStepState }

/** Where the installation is, step by step; empty when the snapshot cannot say (idle, cancelled, an
 *  error not tied to a step). Without `stage`, `downloading` is download and verification at once,
 *  so both are current: Rust does not say which half is running. */
export function storeSteps(s: StoreSnapshot): readonly StoreStepView[] {
  let at: number; let failed = false; let both = false;
  switch (s.phase) {
    case 'downloading': at = s.stage === 'verify' ? 1 : 0; both = s.stage === undefined; break;
    case 'review': at = 2; break;
    case 'installing': at = 3; break;
    case 'ready': at = 4; break;
    case 'error': { const step = storeFailure(s.error).step; if (step === null) return []; at = STORE_STEPS.findIndex(([key]) => key === step); failed = true; break; }
    default: return [];
  }
  return STORE_STEPS.map(([key, label], index) => ({
    key, label,
    state: index < at ? 'done' : index === at ? (failed ? 'failed' : 'current') : both && index === at + 1 ? 'current' : 'todo',
  }));
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
  /** The native message verbatim, for "Technical details"; empty when `body` already is that message. */
  readonly detail: string;
  /** The step that failed, when the message says. */
  readonly step: StoreStep | null;
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
  const f = (title: string, body: string, step: StoreStep | null, retry: StoreFailure['retry'], identity = true, detail = error, manage = false): StoreFailure => ({ title, body, detail, step, retry, identity, manage });
  if (/^Package download failed:/.test(error)) return f('The extension could not be downloaded', 'Bowser could not get the package from the Chrome Web Store. Check your connection, then try again. Nothing was installed.', 'download', 'suggest');
  if (/^Package verification failed:/.test(error)) {
    const cap = /larger than the per-file cap|more than the size cap|ratio/.test(error);
    return f('This package did not pass Bowser’s checks', cap
      ? 'One file in the package is larger than Bowser accepts, so nothing was installed. Trying again will most likely fail the same way. This is a package check, not a verdict on whether the extension works in Bowser.'
      : 'Bowser checks every package’s signature and contents before showing what it can access. This one failed, so nothing was installed. Trying again downloads a fresh copy. This is a package check, not a verdict on whether the extension works in Bowser.', 'verify', cap ? 'offer' : 'suggest');
  }
  if (/already installed/.test(error)) return f('Already installed', 'This extension is already installed in this profile. Bowser cannot update it from here yet.', null, 'none', true, error, true);
  if (/too large (?:for Bowser )?to (?:display|show) safely/.test(error)) return f('The permission review is too large to show', 'This extension declares more than Bowser can show you in full, and Bowser will not install what it cannot show. Nothing was installed.', 'review', 'none');
  if (/^Installation was not accepted|^No extension was installed\.|source identity changed|source could not be recorded/.test(error)) return f('The extension could not be installed', 'Bowser stopped before the extension was added to this profile. Nothing was installed.', 'install', 'offer');
  if (/WebView2 runtime cannot run supported extensions/.test(error)) return f('This WebView2 version cannot run extensions', 'Update the Microsoft Edge WebView2 Runtime, then start the installation again. Restarting Bowser alone will not fix this. Nothing was installed.', null, 'none', false, '');
  if (/^Other extension downloads are finishing/.test(error)) return f('Another download is still finishing', 'Wait a moment, then start the installation again from the Web Store. Nothing was installed.', null, 'none', false, '');
  if (/persistent normal profile/.test(error)) return f('Extensions need a normal profile', 'Guest and temporary profiles cannot keep extensions. Switch to a normal profile, then start the installation again. Nothing was installed.', null, 'none', false, '');
  if (/^That extension ID is not valid|valid HTTPS Chrome Web Store extension listing URL|^The profile is not ready|requires the HTML interface/.test(error)) return f('The installation could not start', error, null, 'none', false, '');
  return f('The extension was not installed', error || 'Installation could not be completed. Nothing was approved automatically.', null, 'offer', true, '');
}

/** "Try again" restarts the same native request flow: `storeinstall` with the snapshot's ID. Only
 *  after a cancel or an error tied to that request (see `StoreFailure.retry`). */
export function storeRetryCommand(s: StoreSnapshot): PanelFields | null {
  if (s.phase === 'cancelled') return storeInstallCommand(s.extensionId);
  if (s.phase !== 'error' || storeFailure(s.error).retry === 'none') return null;
  return storeInstallCommand(s.extensionId);
}

/** A convenience input only: the native backend independently validates the listing URL. */
export function storeUrlCommand(value: string): PanelFields | null {
  try {
    const url = new URL(value.trim());
    if (url.protocol !== 'https:' || url.href.slice(8).split('/')[0]!.includes('@') || url.port !== '') return null;
    const modern = url.hostname === 'chromewebstore.google.com' && /^\/detail\/(?:[^/]+\/)?[a-p]{32}\/?$/.test(url.pathname);
    const legacy = url.hostname === 'chrome.google.com' && /^\/webstore\/detail\/(?:[^/]+\/)?[a-p]{32}\/?$/.test(url.pathname);
    return modern || legacy ? { a: 'storeurl', text: url.href } : null;
  } catch { return null; }
}
