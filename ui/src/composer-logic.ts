// Pure logic of the anchored composers (New workspace, Rename workspace, Add web app, Edit web app): the page
// payload, its copy and the checks run before Rust's own. No DOM and no runtime imports, so
// test/composer.mjs can load it directly. Rust validates again (src/win32/composer.rs).

export type ComposerMode = 'workspace-new' | 'workspace-rename' | 'app-add' | 'app-edit';
export type ComposerField = 'name' | 'url';

/** Fixed width of the composer, inside the menu surface's 200..360 range. */
export const COMPOSER_WIDTH = 280;
/** Longest name Rust accepts, in characters (the payload's `max` repeats it). */
export const MAX_COMPOSER_NAME = 40;
export const MAX_COMPOSER_URL = 2048;
/** Longest error line Rust sends. */
export const MAX_COMPOSER_ERROR = 128;
/** The page's exit on Escape, or after Rust accepts, before it dismisses itself. */
export const COMPOSER_EXIT_MS = 100;

export interface ComposerError { readonly field: ComposerField; readonly text: string }

export interface ComposerMessage {
  readonly t: 'composer';
  readonly tok: number;
  readonly kind: string;
  readonly mode: ComposerMode;
  readonly max: number;
  readonly name: string;
  /** Present for the two web app modes. */
  readonly url?: string;
  readonly error?: ComposerError;
  /** Rust made the change: the page fades out and dismisses itself. */
  readonly done?: true;
}

/** Heading, confirm row label and glyphs of each composer (all existing menu icons). */
export interface ComposerCopy {
  readonly heading: string;
  readonly confirm: string;
  readonly confirmGlyph: 'plus' | 'edit';
  readonly nameLabel: string;
  readonly nameGlyph: 'workspace-add' | 'app-tile';
  readonly namePlaceholder: string;
}

export const COMPOSER_COPY: Readonly<Record<ComposerMode, ComposerCopy>> = {
  'workspace-new': {
    heading: 'New workspace', confirm: 'Create workspace', confirmGlyph: 'plus',
    nameLabel: 'Workspace name', nameGlyph: 'workspace-add', namePlaceholder: 'Workspace name',
  },
  'workspace-rename': {
    heading: 'Rename workspace', confirm: 'Save', confirmGlyph: 'edit',
    nameLabel: 'Workspace name', nameGlyph: 'workspace-add', namePlaceholder: 'Workspace name',
  },
  'app-add': {
    heading: 'Add web app', confirm: 'Add web app', confirmGlyph: 'plus',
    nameLabel: 'Web app name', nameGlyph: 'app-tile', namePlaceholder: 'Name',
  },
  'app-edit': {
    heading: 'Edit web app', confirm: 'Save web app', confirmGlyph: 'edit',
    nameLabel: 'Web app name', nameGlyph: 'app-tile', namePlaceholder: 'Name',
  },
};

export const ADDRESS_LABEL = 'Web app address';
export const ADDRESS_PLACEHOLDER = 'https://example.com/';

export function hasAddress(mode: ComposerMode): boolean {
  return mode !== 'workspace-new' && mode !== 'workspace-rename';
}

const MODES: readonly string[] = ['workspace-new', 'workspace-rename', 'app-add', 'app-edit'];
const KEYS = ['v', 't', 'tok', 'kind', 'mode', 'max', 'name', 'url', 'error', 'done'];

/** Strict check of a `composer` message from Rust; anything else is ignored. */
export function validComposerPayload(value: unknown): value is ComposerMessage {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const msg = value as Record<string, unknown>;
  if (Object.keys(msg).some(key => !KEYS.includes(key)) || msg.v !== 1 || msg.t !== 'composer') return false;
  if (!Number.isSafeInteger(msg.tok) || (msg.tok as number) < 0 || (msg.tok as number) > 0xffffffff) return false;
  if (typeof msg.kind !== 'string' || !msg.kind.startsWith('composer') || msg.kind.length > 32) return false;
  if (typeof msg.mode !== 'string' || !MODES.includes(msg.mode)) return false;
  if (!Number.isSafeInteger(msg.max) || (msg.max as number) < 1 || (msg.max as number) > MAX_COMPOSER_NAME) return false;
  if (typeof msg.name !== 'string' || msg.name.length > 128) return false;
  if (hasAddress(msg.mode as ComposerMode) !== (msg.url !== undefined)) return false;
  if (msg.url !== undefined && (typeof msg.url !== 'string' || msg.url.length > MAX_COMPOSER_URL)) return false;
  if (msg.done !== undefined && msg.done !== true) return false;
  if (msg.error !== undefined) {
    const error = msg.error as Record<string, unknown> | null;
    if (!error || typeof error !== 'object' || Array.isArray(error)) return false;
    if (Object.keys(error).some(key => key !== 'field' && key !== 'text')) return false;
    if (error.field !== 'name' && error.field !== 'url') return false;
    if (error.field === 'url' && msg.url === undefined) return false;
    if (typeof error.text !== 'string' || error.text.length < 1 || error.text.length > MAX_COMPOSER_ERROR) return false;
  }
  return true;
}

/** The message shown under the name field before Rust's check ('' when the name is fine). */
export function localNameError(text: string, max = MAX_COMPOSER_NAME): string {
  const name = text.trim();
  if (name === '') return 'Enter a name.';
  if ([...name].length > max) return `Use at most ${max} characters.`;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(name)) return 'Names cannot contain control characters.';
  return '';
}

/** The message shown under the address field before Rust's check ('' when it is an http(s) URL). */
export function localUrlError(text: string): string {
  const address = text.trim();
  if (address === '') return 'Enter a web address.';
  if (address.length > MAX_COMPOSER_URL) return 'That address is too long.';
  let parsed: URL;
  try { parsed = new URL(address); } catch { return 'Enter an address starting with http:// or https://.'; }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return 'Enter an address starting with http:// or https://.';
  if (parsed.username !== '' || parsed.password !== '') return 'Addresses cannot contain a user name or password.';
  return '';
}

/** The first field that fails the local checks, in reading order, or null when both pass. */
export function firstComposerError(mode: ComposerMode, name: string, url: string, max = MAX_COMPOSER_NAME): ComposerError | null {
  const nameError = localNameError(name, max);
  if (nameError !== '') return { field: 'name', text: nameError };
  if (!hasAddress(mode)) return null;
  const urlError = localUrlError(url);
  return urlError === '' ? null : { field: 'url', text: urlError };
}

/** Where Tab or Shift+Tab goes among `count` stops (fields, then the confirm row), wrapping. */
export function trapStep(from: number, count: number, back: boolean): number {
  if (count <= 0) return -1;
  if (from < 0 || from >= count) return back ? count - 1 : 0;
  return (from + (back ? count - 1 : 1)) % count;
}
