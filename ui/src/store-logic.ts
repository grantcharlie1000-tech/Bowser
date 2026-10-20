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
}
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const text = (v: unknown, max = 4096): v is string => typeof v === 'string' && v.length <= max * 2 && Array.from(v).length <= max && !/[\u0000-\u001f\u007f]/.test(v);
export const isStoreId = (v: unknown): v is string => typeof v === 'string' && /^[a-p]{32}$/.test(v);
export const isStoreRequestId = (v: unknown): v is string => typeof v === 'string' && /^[a-zA-Z0-9-]{1,64}$/.test(v);
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
  return v as unknown as StoreSnapshot;
}
export function filterCatalogue(items: readonly StoreEntry[], query: string, category = ''): readonly StoreEntry[] {
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return items.filter((item) => (category === '' || category === item.category) && words.every((word) => `${item.name} ${item.description} ${item.publisher} ${item.category}`.toLocaleLowerCase().includes(word)));
}
export function storeInstallCommand(id: string): PanelFields | null { return isStoreId(id) ? { a: 'storeinstall', text: id } : null; }
export function storeRequestCommand(action: 'storecancel' | 'storeapprove', id: string): PanelFields | null { return isStoreRequestId(id) ? { a: action, text: id } : null; }
export const storeBusy = (phase: StorePhase): boolean => phase === 'downloading' || phase === 'review' || phase === 'installing';
export function storeStatus(s: StoreSnapshot): string {
  switch (s.phase) {
    case 'idle': return '';
    case 'downloading': return 'Downloading and verifying extension…';
    case 'review': return `Review ${s.name} · Version ${s.version}`;
    case 'installing': return 'Installing extension…';
    case 'ready': return s.restart ? 'Installed. Restart Bowser to finish loading the extension.' : 'Extension installed. Find it in Installed.';
    case 'error': return s.error || 'Installation could not be completed. Nothing was approved automatically.';
    case 'cancelled': return 'Installation cancelled.';
  }
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
