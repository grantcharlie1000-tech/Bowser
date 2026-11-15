import { bindChromeDocument, parseInbound, send, type Inbound } from './protocol';

export type ChromeView = 'toolbar' | 'tabs' | 'rail';
export type DocumentToken = string & { readonly __brand: 'ChromeDocument' };
export type StateRevision = number & { readonly __brand: 'ChromeRevision' };
export function isDocumentToken(value: unknown): value is DocumentToken {
  return typeof value === 'string' && /^[0-9a-f]{32}$/.test(value);
}
export function isStateRevision(value: unknown): value is StateRevision {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 && value <= 0xffffffff;
}

type ToolbarState = Extract<Inbound, { t: 'state' | 'setText' | 'nativeText' | 'addr' | 'sfield' | 'dlbtn' | 'extbtn' | 'pair' }>;
type TabsState = Extract<Inbound, { t: 'state' | 'layout' | 'workspace' | 'tabopts' | 'tabs' | 'fav' | 'pair' }>;
type RailState = Extract<Inbound, { t: 'state' | 'rail' | 'railfav' }>;
export type InitialBundle =
  | { kind: 'toolbar'; messages: ToolbarState[] }
  | { kind: 'tabs'; messages: TabsState[] }
  | { kind: 'rail'; messages: RailState[] };

/** Validate the complete bundle before invoking any state handler. Repeated snapshots are allowed:
 * a later state within one transaction supersedes the earlier one (button slots can change it). */
export function initialBundle(view: ChromeView, messages: Inbound[]): InitialBundle | null {
  const has = (types: readonly string[]): boolean => types.every((t) => messages.some((m) => m.t === t));
  if (view === 'toolbar') {
    const types = ['state', 'setText', 'addr', 'sfield', 'dlbtn', 'extbtn', 'pair'];
    const parts = messages.filter((m): m is ToolbarState => types.includes(m.t) || m.t === 'nativeText');
    return parts.length === messages.length && has(types) ? { kind: view, messages: parts } : null;
  }
  if (view === 'tabs') {
    const types = ['state', 'layout', 'workspace', 'tabopts', 'tabs', 'pair'];
    const parts = messages.filter((m): m is TabsState => types.includes(m.t) || m.t === 'fav');
    const tabs = parts.filter((m) => m.t === 'tabs').at(-1);
    const favs = new Set(parts.filter((m) => m.t === 'fav').map((m) => m.id));
    return parts.length === messages.length && has(types) && tabs?.items.every((t) => !t.fav || favs.has(t.id))
      ? { kind: view, messages: parts } : null;
  }
  const parts = messages.filter((m): m is RailState => m.t === 'state' || m.t === 'rail' || m.t === 'railfav');
  const rail = parts.filter((m) => m.t === 'rail').at(-1);
  const favs = new Set(parts.filter((m) => m.t === 'railfav').map((m) => m.k));
  return parts.length === messages.length && has(['state', 'rail']) && rail?.items.every((r) => !r.fav || favs.has(r.k))
    ? { kind: view, messages: parts } : null;
}

type Phase =
  | { kind: 'waiting' }
  | { kind: 'collecting'; revision: StateRevision; count: number; bytes: number; messages: Inbound[] }
  | { kind: 'applying'; revision: StateRevision }
  | { kind: 'applied'; revision: StateRevision }
  | { kind: 'closed' };

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** The production receiver; tests control transport and DOM/render completion at its boundaries. */
export function chromeReceiver(options: {
  view: ChromeView;
  document: DocumentToken;
  apply: (message: Inbound) => void | Promise<void>;
  settle: () => Promise<void>;
  post: typeof send;
}): { receive: (data: unknown) => void; close: () => void } {
  let phase: Phase = { kind: 'waiting' };
  let lastRevision = 0;
  const fail = (revision: StateRevision): void => {
    phase = { kind: 'closed' };
    options.post({ v: 1, t: 'chromeFailed', view: options.view, doc: options.document, rev: revision });
  };
  const apply = async (bundle: InitialBundle, revision: StateRevision): Promise<void> => {
    const applying: Phase = { kind: 'applying', revision };
    phase = applying;
    try {
      for (const message of bundle.messages) {
        if (phase !== applying) return;
        await options.apply(message);
      }
      await options.settle();
      if (phase !== applying) return;
      phase = { kind: 'applied', revision };
      options.post({ v: 1, t: 'chromeApplied', view: options.view, doc: options.document, rev: revision });
    } catch {
      if (phase === applying) fail(revision);
    }
  };
  return {
    close: () => { phase = { kind: 'closed' }; },
    receive: (input) => {
      let data: unknown = input;
      if (typeof data === 'string') {
        if (new TextEncoder().encode(data).length > 257 * 1024) return;
        try { data = JSON.parse(data); } catch { return; }
      }
      if (phase.kind === 'closed' || !record(data)) return;
      if (data.t !== 'chromeBegin' && data.t !== 'chromePart' && data.t !== 'chromeCommit') {
        if (phase.kind === 'applied') {
          const message = parseInbound(data);
          if (message) void options.apply(message);
        }
        return;
      }
      if (data.v !== 1 || data.view !== options.view || data.doc !== options.document || !isStateRevision(data.rev)) return;
      const revision = data.rev;
      if (data.t === 'chromeBegin') {
        if (Object.keys(data).some((k) => !['v', 't', 'view', 'doc', 'rev', 'count'].includes(k))
          || revision <= lastRevision || typeof data.count !== 'number' || !Number.isInteger(data.count)
          || data.count < 1 || data.count > 1100) return;
        lastRevision = revision;
        phase = { kind: 'collecting', revision, count: data.count, bytes: 0, messages: [] };
        return;
      }
      if (phase.kind !== 'collecting' || phase.revision !== revision) return;
      if (data.t === 'chromePart') {
        if (Object.keys(data).some((k) => !['v', 't', 'view', 'doc', 'rev', 'i', 'state'].includes(k))
          || data.i !== phase.messages.length || phase.messages.length >= phase.count) { fail(revision); return; }
        const message = parseInbound(data.state);
        if (!message) { fail(revision); return; }
        phase.bytes += new TextEncoder().encode(JSON.stringify(data.state)).length;
        if (phase.bytes > 16 * 1024 * 1024) { fail(revision); return; }
        phase.messages.push(message);
        return;
      }
      if (Object.keys(data).some((k) => !['v', 't', 'view', 'doc', 'rev'].includes(k))
        || phase.messages.length !== phase.count) { fail(revision); return; }
      const bundle = initialBundle(options.view, phase.messages);
      if (!bundle) { fail(revision); return; }
      void apply(bundle, revision);
    },
  };
}

/** Flush real reconciliation, load fonts and decode the images it installed before yielding a
 * render opportunity. This acknowledges applied state, never compositor presentation. */
export async function settleChrome(root: HTMLElement, flush: () => void): Promise<void> {
  flush();
  root.getBoundingClientRect();
  await document.fonts.ready;
  await Promise.all(Array.from(root.querySelectorAll('img')).filter((img) => !!img.getAttribute('src')).map((img) => img.decode()));
  root.getBoundingClientRect();
  await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
}

export function onChromeInbound(view: ChromeView, root: HTMLElement, apply: (message: Inbound) => void, flush: () => void = () => {}): void {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  const token = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
  if (!isDocumentToken(token)) return;
  // The host queries this exact current top-level global before accepting chromeReady. A stale
  // same-URL document's queued ready cannot acquire the new navigation's identity.
  Object.defineProperty(window, '__bowserChromeDocument', { value: token, writable: false, configurable: false });
  bindChromeDocument(view, token);
  const receiver = chromeReceiver({ view, document: token, apply, settle: () => settleChrome(root, flush), post: send });
  window.addEventListener('pagehide', receiver.close, { once: true });
  window.chrome?.webview?.addEventListener('message', (event) => receiver.receive(event.data));
  send({ v: 1, t: 'chromeReady', view, doc: token });
}
