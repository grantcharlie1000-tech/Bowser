// --- hover layer ---
// Pure tooltip timing: a 500 ms rest before the first tooltip, none when moving straight on to a
// neighbour (while one is up, or within TIP_WARM_MS of it hiding), and nothing again for a target
// that was dismissed until the pointer leaves it. The clock and timers are injected, so the rules
// are unit tested without a DOM (see test/hover.mjs).

export const TIP_DELAY_MS = 500;
/** After a tooltip hides because the pointer left its target, the next one shows at once for this long. */
export const TIP_WARM_MS = 300;

export interface TipClock {
  now(): number;
  set(fn: () => void, ms: number): number;
  clear(id: number): void;
}

export class TooltipTiming<T> {
  private current: T | null = null;
  private shown = false;
  private dismissed = false;
  private timer = 0;
  private warmUntil = -Infinity;

  constructor(
    private readonly clock: TipClock,
    private readonly show: (target: T) => void,
    private readonly hide: () => void,
  ) {}

  /** The pointer is over `target` (or still is). */
  enter(target: T): void {
    if (target === this.current) return;
    const warm = this.shown || this.clock.now() < this.warmUntil;
    this.cancelTimer();
    this.current = target;
    this.dismissed = false;
    if (warm) {
      this.reveal();
      return;
    }
    this.timer = this.clock.set(() => {
      this.timer = 0;
      this.reveal();
    }, TIP_DELAY_MS);
  }

  /** The pointer is over no tooltip target. */
  leave(): void {
    this.cancelTimer();
    const wasShown = this.shown;
    if (wasShown) this.conceal();
    if (wasShown) this.warmUntil = this.clock.now() + TIP_WARM_MS;
    this.current = null;
    this.dismissed = false;
  }

  /** Scroll, key press, click or blur: hide now, stay cold, and keep quiet over the same target. */
  dismiss(): void {
    this.cancelTimer();
    if (this.shown) this.conceal();
    this.warmUntil = -Infinity;
    this.dismissed = true;
  }

  get visible(): boolean {
    return this.shown;
  }

  get target(): T | null {
    return this.current;
  }

  private reveal(): void {
    if (this.current === null || this.dismissed) return;
    this.shown = true;
    this.show(this.current);
  }

  private conceal(): void {
    this.shown = false;
    this.hide();
  }

  private cancelTimer(): void {
    if (this.timer !== 0) this.clock.clear(this.timer);
    this.timer = 0;
  }
}

/**
 * Which tab-strip element the pointer is over, reported only when it changes. `key` is
 * `tab:<id>`, `ball:<key>` or '' for none.
 */
export class HoverReport {
  private last = '';

  /** Returns true when `key` differs from the last one reported (and records it). */
  changed(key: string): boolean {
    if (key === this.last) return false;
    this.last = key;
    return true;
  }

  reset(): void {
    this.last = '';
  }
}
// --- end hover layer ---
