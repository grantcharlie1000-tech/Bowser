// Pure rules of the address dialogs: the snapshot contract and the field cleaning. test/address.mjs
// covers all of it.
import type { FolderView } from './address-settings-logic';
import type { SnapshotMsg } from './protocol';

/** Longest address the QR code holds (version 10, level M). */
export const MAX_QR_BYTES = 213;

export interface BookmarkDialog {
  readonly kind: 'bookmark';
  readonly url: string;
  readonly existing: boolean;
  readonly name: string;
  readonly folder: number;
  readonly nickname: string;
  readonly description: string;
  readonly folders: readonly FolderView[];
  readonly error: string;
}
export interface QrDialog { readonly kind: 'qr'; readonly text: string }
export type DialogSnapshot = BookmarkDialog | QrDialog;

const str = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max && !/[\u0000-\u001f\u007f]/.test(v);

/** A field as it is sent: line breaks and tabs become spaces (they separate the fields), trimmed. */
export function cleanField(text: string): string {
  return text.replace(/[\u0000-\u001f\u007f]+/g, ' ').trim();
}

/** Strict validation of an `addressdlg` snapshot. */
export function parseDialogSnapshot(d: SnapshotMsg['d']): DialogSnapshot | null {
  const keys = Object.keys(d);
  if (d.kind === 'qr') {
    return keys.length === 2 && str(d.text, 2048) ? { kind: 'qr', text: d.text } : null;
  }
  if (d.kind !== 'bookmark') return null;
  const wanted = ['kind', 'url', 'existing', 'name', 'folder', 'nickname', 'description', 'folders', 'error'];
  if (keys.length !== wanted.length || keys.some((key) => !wanted.includes(key))) return null;
  if (!str(d.url, 2048) || typeof d.existing !== 'boolean' || !str(d.name, 200) || typeof d.folder !== 'number' || !Number.isInteger(d.folder) || d.folder < 0) return null;
  if (!str(d.nickname, 24) || !str(d.description, 512) || !str(d.error, 400) || !Array.isArray(d.folders) || d.folders.length > 200) return null;
  const folders: FolderView[] = [];
  for (const raw of d.folders) {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null;
    const { id, title, depth } = raw as Record<string, unknown>;
    if (typeof id !== 'number' || !Number.isInteger(id) || id < 1 || !str(title, 200) || typeof depth !== 'number' || !Number.isInteger(depth) || depth < 0 || depth > 16) return null;
    folders.push({ id, title, depth });
  }
  return { kind: 'bookmark', url: d.url, existing: d.existing, name: d.name, folder: d.folder, nickname: d.nickname, description: d.description, folders, error: d.error };
}
