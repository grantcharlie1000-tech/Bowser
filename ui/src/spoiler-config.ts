// --- spoiler guard ---
// The live Spoiler Firewall setting as the chrome views see it. Rust posts
// `{"v":1,"t":"spoiler","on":bool,"mode":"lockdown"|"strict"|"balanced"}` to every view when it says
// `ready` (before any content) and whenever the setting changes. Until the first message arrives the
// default is the product default (on, Strict), so a list can never render unmasked while the message
// is in flight. No engine code here: protocol-level tests and views that only need the setting import
// this file; the classifier lives in spoiler-guard.ts.

export type SpoilerMode = 'lockdown' | 'strict' | 'balanced';
export const SPOILER_MODES: readonly SpoilerMode[] = ['lockdown', 'strict', 'balanced'];

// A view can load a second bundle after the host's first message (the start page is fetched when a blank
// tab first shows it): every copy of this module in the document shares the live setting through this
// window property, so a late bundle starts from it instead of from the product default.
const SHARED = '__bowserSpoilerConfig';
interface Shared { on: boolean; mode: SpoilerMode }
const shared = (): Shared | null => {
  const found = typeof window === 'undefined' ? undefined : (window as unknown as Record<string, unknown>)[SHARED];
  if (typeof found !== 'object' || found === null) return null;
  const { on, mode: m } = found as Partial<Shared>;
  return typeof on === 'boolean' && typeof m === 'string' && (SPOILER_MODES as readonly string[]).includes(m) ? { on, mode: m as SpoilerMode } : null;
};
const inherited = shared();
let enabled = inherited?.on ?? true;
let mode: SpoilerMode = inherited?.mode ?? 'strict';
const listeners = new Set<() => void>();

/** The mode masking uses now, or 'off' when protection is off (no masking at all). */
export function activeSpoilerMode(): SpoilerMode | 'off' {
  return enabled ? mode : 'off';
}

/** Calls `listener` after the setting changes; returns the unsubscribe function. */
export function onSpoilerConfig(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** Sets the live setting directly (tests, and `applySpoilerMessage`). */
export function setSpoilerConfig(on: boolean, next: SpoilerMode): void {
  if (on === enabled && next === mode) return;
  enabled = on;
  mode = next;
  if (typeof window !== 'undefined') (window as unknown as Record<string, unknown>)[SHARED] = { on, mode: next };
  for (const listener of Array.from(listeners)) listener();
}

/** Applies a host message if it is a well-formed `spoiler` message; anything else is ignored. */
export function applySpoilerMessage(raw: unknown): boolean {
  let value = raw;
  if (typeof value === 'string') {
    try { value = JSON.parse(value) as unknown; } catch { return false; }
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const msg = value as Record<string, unknown>;
  if (msg.t !== 'spoiler' || msg.v !== 1 || typeof msg.on !== 'boolean') return false;
  if (!Object.keys(msg).every((key) => key === 'v' || key === 't' || key === 'on' || key === 'mode')) return false;
  const next = msg.mode;
  if (typeof next !== 'string' || !(SPOILER_MODES as readonly string[]).includes(next)) return false;
  setSpoilerConfig(msg.on, next as SpoilerMode);
  return true;
}

// Every view imports this file, so one listener here covers views that read host messages through
// `onInbound` and the ones (popup, settings page) that add their own listener. `parseInbound` ignores
// the unknown type, so nothing else sees it.
if (typeof window !== 'undefined') window.chrome?.webview?.addEventListener('message', (event) => { applySpoilerMessage(event.data); });
// --- end spoiler guard ---
