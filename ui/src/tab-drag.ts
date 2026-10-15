// The page side of the native tab ghost (docs/design/tab-drag.md). One drag at a time across the
// strip and the sidebar: the session sends `tabdrag` commands, keeps the ghost alive, and hands the
// renderer three moments: the ghost is up (hide your own copy, leave the gap), the ghost died
// mid-drag (show your copy again and drag it in place, as before), and the ghost has landed after the
// release (show your copy again).
import { send } from './protocol';
import { dropCommand, settleCommand, startCommand, validTabDragCommand, type GhostKind, type TabDragCommand, type TabGhostMsg } from './tab-drag-protocol';
import { GHOST_KEEPALIVE_MS, GHOST_REVEAL_FALLBACK_MS, settlePoint } from './tab-drag-logic';

export interface GhostHooks {
  /** Rust shows the ghost: hide the held tab's own copy (its slot stays as the gap). */
  shown(): void;
  /** The ghost went away while the tab is still held: show the copy and drag it in place again. */
  lost(): void;
  /** After the release, the ghost has landed (or never will): show the copy again. */
  reveal(): void;
}

type State = 'pending' | 'shown' | 'released' | 'done';

let current: GhostSession | null = null;

const sendDrag = (cmd: TabDragCommand): void => {
  if (validTabDragCommand(cmd)) send(cmd);
};

export class GhostSession {
  private state: State = 'pending';
  private floating = false;
  private keepalive = 0;
  private fallback = 0;
  private settling = false;

  private constructor(readonly id: number, private readonly hooks: GhostHooks) {}

  /**
   * Asks Rust for a ghost of tab `id` (a `w` x `h` CSS px box grabbed at gx, gy). Only a mouse drag
   * gets one: Rust follows the system cursor, which touch and pen do not reliably move.
   */
  static start(id: number, kind: GhostKind, w: number, h: number, gx: number, gy: number, close: boolean, pointerType: string, hooks: GhostHooks): GhostSession | null {
    current?.abandon();
    if (pointerType !== 'mouse') return null;
    const cmd = startCommand(id, kind, Math.max(1, Math.round(w)), Math.max(1, Math.round(h)), gx, gy, close);
    if (!validTabDragCommand(cmd)) return null;
    const session = new GhostSession(id, hooks);
    current = session;
    send(cmd);
    session.keepalive = window.setInterval(() => {
      if (session.state === 'pending' || session.state === 'shown') sendDrag({ v: 1, t: 'tabdrag', a: 'move', id, f: session.floating });
    }, GHOST_KEEPALIVE_MS);
    return session;
  }

  /** True while Rust draws the held tab (the page's own copy is hidden). */
  get live(): boolean {
    return this.state === 'shown';
  }

  /** Reports attached / floating; only a change is sent (Rust follows the cursor itself). */
  phase(floating: boolean): void {
    if (this.state !== 'pending' && this.state !== 'shown') return;
    if (floating === this.floating) return;
    this.floating = floating;
    sendDrag({ v: 1, t: 'tabdrag', a: 'move', id: this.id, f: floating });
  }

  /**
   * The tab was released. `settle` glides the ghost to (x, y) (the slot, client CSS px of this view),
   * `drop` fades it where it is when a split zone is showing (else glides to x, y), `none` hides it. The copy stays hidden until
   * Rust says the ghost landed, or GHOST_REVEAL_FALLBACK_MS at most.
   */
  end(mode: 'settle' | 'drop' | 'none', x = 0, y = 0): void {
    if (this.state === 'released' || this.state === 'done') return;
    const wasShown = this.state === 'shown';
    this.stopKeepalive();
    if (mode === 'none') {
      sendDrag({ v: 1, t: 'tabdrag', a: 'end', id: this.id, m: 'none' });
    } else {
      // `drop` carries the slot too: with no zone showing, Rust glides the ghost home instead.
      const p = settlePoint(x, y);
      sendDrag(mode === 'settle' ? settleCommand(this.id, p.x, p.y) : dropCommand(this.id, p.x, p.y));
    }
    if (!wasShown || mode === 'none') {
      this.finish();
      return;
    }
    this.state = 'released';
    this.settling = mode === 'settle';
    this.fallback = window.setTimeout(() => this.finish(), GHOST_REVEAL_FALLBACK_MS);
  }

  /** The slot moved after the release (the host's reorder re-laid the strip): glide there instead. */
  retarget(x: number, y: number): void {
    if (this.state !== 'released' || !this.settling) return;
    const p = settlePoint(x, y);
    sendDrag(settleCommand(this.id, p.x, p.y));
  }

  /** Whether this session still hides its tab after a release. */
  get landing(): boolean {
    return this.state === 'released';
  }

  /** Ends everything now (a new press, a re-render that dropped the tab): the copy shows at once. */
  abandon(): void {
    if (this.state === 'done') return;
    if (this.state === 'pending' || this.state === 'shown') sendDrag({ v: 1, t: 'tabdrag', a: 'end', id: this.id, m: 'none' });
    this.finish();
  }

  /** A `tabghost` message for this session. */
  receive(msg: TabGhostMsg): void {
    if (msg.id !== this.id) return;
    if (msg.a === 'shown') {
      if (this.state !== 'pending') return;
      this.state = 'shown';
      this.hooks.shown();
      return;
    }
    // `land` or `gone`.
    if (this.state === 'shown') {
      // Lost mid-drag (watchdog, failure): the page drags its own copy again.
      this.state = 'pending';
      this.hooks.lost();
      return;
    }
    if (this.state === 'released') this.finish();
  }

  private stopKeepalive(): void {
    if (this.keepalive !== 0) window.clearInterval(this.keepalive);
    this.keepalive = 0;
  }

  private finish(): void {
    if (this.state === 'done') return;
    const hidden = this.state === 'shown' || this.state === 'released';
    this.state = 'done';
    this.stopKeepalive();
    if (this.fallback !== 0) window.clearTimeout(this.fallback);
    this.fallback = 0;
    if (current === this) current = null;
    if (hidden) this.hooks.reveal();
  }
}

/** Routes a `tabghost` host message to the live session. */
export function routeGhostMessage(msg: TabGhostMsg): void {
  current?.receive(msg);
}

// ----- announcements -----

let live: HTMLElement | null = null;

/** Speaks `text` through a polite live region outside the strip (never part of its reconciled DOM). */
export function announce(text: string): void {
  if (typeof document === 'undefined') return;
  if (!live || !live.isConnected) {
    live = document.createElement('div');
    live.className = 'tab-drag-live';
    live.setAttribute('role', 'status');
    live.setAttribute('aria-live', 'polite');
    document.body.append(live);
  }
  live.textContent = text;
}
