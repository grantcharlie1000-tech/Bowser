import { icon, type IconName } from './dom';
import { isIconName } from './icons';
import {
  activateAt, createNav, currentPage, firstVisible, footerVisible, hasLeadColumn, hasTrailColumn, menuKey, onFill, validMenuPayload,
  type MenuBadge, type MenuMessage, type MenuNav, type MenuOutcome, type MenuPage,
} from './menu-logic';
import { sendPopupDismiss, sendPopupPick, sendPopupSize, type PopupCards } from './protocol';

// Markup contract (styling lives in menu.css; keep these names stable):
//   #app.html-menu.is-paned[role=menu][data-kind][data-page][data-input]   the transparent surface
//   .html-menu-track > .html-menu-slide                the clip and the two cards (--menu-slide, --menu-child-y, --menu-child-in, --menu-child-x)
//   .html-menu-pane[data-level].is-active              one card; a submenu's card sits beside the row that opened it
//   .html-menu-header > .html-menu-avatar, .html-menu-name, .html-menu-caption   main menu card (level 0)
//   .html-menu-list.bw-scroll                          the scrolling part (header and footer stay pinned)
//   .html-menu-section                                 non-interactive heading
//   .html-menu-separator[role=separator]
//   .html-menu-row[role=menuitem|menuitemcheckbox][data-index][data-id][data-icon]
//       aria-disabled, aria-checked, aria-haspopup, aria-expanded; .selected marks the keyboard/hover
//       row of the open page, .is-open the row of the parent page whose submenu is open
//       children: .html-menu-lead (icon | favicon | badge | spacer), .html-menu-label,
//                 .html-menu-shortcut, .html-menu-trail (check or chevron | spacer)
//   .html-menu-footer                                  muted note under the list (level 0)
// `--menu-accent` / `--menu-on-accent` on the header and badges carry the accent and its text colour.
//
// A submenu opens beside the row that opened it; at most two cards are on screen, and each card is only
// as tall as its own rows. The popup window is widened to fit both and keeps its top and right edge
// (html_popup.rs keep_right_edge). The parent card is pre-shifted right, so the growth does not throw
// it, then it eases left to sit beside the child. The child stays next to that row and only fades and
// nudges in. Closing eases the parent back over the child, then the window shrinks and the child pane
// is dropped. The window clip is the whole window during the move and the two cards only at rest: a
// region that chases the transform arrives a frame late and cuts the cards.

export interface MenuView {
  /** Shows a `menu` message. False (and nothing changes) when it is not a valid menu payload. */
  show(raw: unknown): boolean;
}

/** How long to wait for the window to take the reported size before settling without the slide. */
const RESIZE_WAIT_MS = 250;

function disc(className: string, badge: MenuBadge): HTMLElement {
  const node = document.createElement('span');
  node.className = className;
  node.setAttribute('aria-hidden', 'true');
  node.style.setProperty('--menu-accent', badge.accent);
  node.style.setProperty('--menu-on-accent', onFill(badge.accent));
  node.textContent = badge.avatar;
  return node;
}

/** Scrolls `list` just enough to show `row`. Only the list moves: the browser's own row-into-view call would also scroll the clip around the slide. */
function reveal(list: Element | null, row: HTMLElement): void {
  if (!list) return;
  const l = list.getBoundingClientRect();
  const r = row.getBoundingClientRect();
  if (r.top < l.top) list.scrollTop -= l.top - r.top;
  else if (r.bottom > l.bottom) list.scrollTop += r.bottom - l.bottom;
}

/** One menu view per popup page. Listeners attach once; each `menu` message replaces the menu. */
export function createMenuView(root: HTMLElement, now: () => number = () => performance.now()): MenuView {
  let message: MenuMessage | null = null;
  let nav: MenuNav | null = null;
  // Set once a pick or dismissal is sent, so a double click or key repeat sends nothing more.
  let closed = true;
  // The strip the panes sit in, and the pane of each page on screen (built once, kept while visible,
  // so a parent's scroll position survives a submenu opening beside it).
  let slide: HTMLElement = document.createElement('div');
  const panes = new Map<MenuPage, HTMLElement>();
  // What a page change that arrives mid-motion runs to finish that motion at once; `epoch` voids the
  // callbacks of a motion that was finished or replaced.
  let abort: (() => void) | null = null;
  let epoch = 0;

  const settle = (): void => {
    const run = abort;
    abort = null;
    epoch += 1;
    run?.();
  };

  // Focus stays on the menu root; the selected row is exposed with aria-activedescendant, so no
  // row takes a focus ring and every key reaches one listener.
  const markSelected = (): void => {
    if (!nav) return;
    const current = nav.pages.length - 1;
    root.removeAttribute('aria-activedescendant');
    for (const [page, pane] of panes) {
      const level = nav.pages.indexOf(page);
      if (level < 0) continue;
      pane.classList.toggle('is-active', level === current);
      for (const row of Array.from(pane.querySelectorAll<HTMLElement>('.html-menu-row'))) {
        const picked = Number(row.dataset.index) === page.selected;
        const on = picked && level === current;
        const open = picked && level !== current;
        row.classList.toggle('selected', on);
        row.classList.toggle('is-open', open);
        if (row.hasAttribute('aria-haspopup')) row.setAttribute('aria-expanded', String(open));
        if (on) {
          root.setAttribute('aria-activedescendant', row.id);
          reveal(pane.querySelector('.html-menu-list'), row);
        }
      }
    }
    if (document.activeElement !== root) root.focus({ preventScroll: true });
  };

  // The two cards, in CSS pixels of the surface. Absent when only one menu is open. Sent at rest, not
  // while a card is moving: the clip would lag the transform and cut the edges.
  const flyoutCards = (): PopupCards | undefined => {
    if (!nav || nav.pages.length < 2) return undefined;
    const parent = panes.get(nav.pages[nav.pages.length - 2]);
    const child = panes.get(nav.pages[nav.pages.length - 1]);
    if (!parent || !child) return undefined;
    const origin = root.getBoundingClientRect();
    const box = (pane: HTMLElement) => {
      const rect = pane.getBoundingClientRect();
      return {
        x: Math.round(rect.left - origin.left),
        y: Math.round(rect.top - origin.top),
        w: Math.max(1, Math.round(rect.width)),
        h: Math.max(1, Math.round(rect.height)),
      };
    };
    const host = box(parent);
    const next = box(child);
    return {
      px: Math.max(0, host.x),
      py: Math.max(0, host.y),
      pw: host.w,
      ph: host.h,
      cx: Math.max(0, next.x),
      cy: Math.max(0, next.y),
      cw: next.w,
      ch: next.h,
    };
  };

  // Drops the child card so its first row lines up with the parent row that opened it. The offset is
  // a custom property, not a layout write the slide animates.
  const placeChild = (): void => {
    if (!nav || nav.pages.length < 2) {
      slide.style.removeProperty('--menu-child-y');
      return;
    }
    const parent = panes.get(nav.pages[nav.pages.length - 2]);
    const child = panes.get(nav.pages[nav.pages.length - 1]);
    const row = parent?.querySelector<HTMLElement>('.html-menu-row.is-open');
    if (!parent || !child || !row) return;
    const style = getComputedStyle(child);
    const pad = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.borderTopWidth) || 0);
    const y = Math.round(row.getBoundingClientRect().top - parent.getBoundingClientRect().top - pad);
    slide.style.setProperty('--menu-child-y', `${Math.max(0, y)}px`);
  };

  // Reports a size. Rust caps it to the work area. `clip` is the resting two-card window. During a
  // move the clip stays the whole window: a region that chases the transform arrives a frame late
  // and cuts the cards.
  const publish = (tok: number, width: number, height: number, clip: boolean): void => {
    sendPopupSize(tok, Math.max(1, Math.ceil(width)), Math.max(1, Math.ceil(height)), clip ? flyoutCards() : undefined);
  };

  // Natural layout size. A transform does not change it, and the temporary max-height lift matches
  // what measure reports, so a tall menu is capped by Rust rather than by the page.
  const layoutSize = (): { width: number; height: number } => {
    root.style.maxHeight = 'none';
    const width = root.offsetWidth;
    const height = root.offsetHeight;
    root.style.maxHeight = '100vh';
    return { width, height };
  };

  // Measures the natural size and reports it, clipped to the cards when two are open.
  const measure = (first: boolean): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok) return;
      const size = layoutSize();
      publish(tok, size.width, size.height, true);
      // The enter motion starts when the size is reported, which is when the window is shown.
      if (first) root.classList.add('is-entering');
    });
  };

  // Parent shift and child fade. `snap` applies them with no transition (the position the window
  // grows into, or a reduced-motion jump).
  const setMotion = (slidePx: number, childHidden: boolean, snap: boolean): void => {
    if (snap) slide.classList.add('is-snap');
    slide.style.setProperty('--menu-slide', `${slidePx}px`);
    slide.style.setProperty('--menu-child-in', childHidden ? '0' : '1');
    slide.style.setProperty('--menu-child-x', childHidden ? '8px' : '0px');
    if (snap) {
      void slide.offsetWidth;
      slide.classList.remove('is-snap');
    }
  };

  const tagPanes = (): void => {
    if (!nav) return;
    const wanted = nav.pages.slice(firstVisible(nav));
    for (const [page, pane] of panes) {
      const index = wanted.indexOf(page);
      pane.classList.toggle('is-host', wanted.length > 1 && index === 0);
      pane.classList.toggle('is-child', wanted.length > 1 && index === wanted.length - 1);
    }
  };

  // Runs `then` when the parent shift and the child fade have finished. Reduced motion leaves no
  // transform transition, so it runs at once.
  const afterMotion = (then: () => void): void => {
    // Flush style so the transitions exist before they are queried. A duration of zero (reduced
    // motion) leaves nothing running, and `then` runs in this turn.
    void slide.offsetWidth;
    const running = [slide, ...slide.querySelectorAll<HTMLElement>('.html-menu-pane')]
      .flatMap((node) => node.getAnimations())
      .filter((animation) => animation.playState === 'running' || animation.playState === 'pending');
    if (running.length === 0) then();
    else void Promise.allSettled(running.map((animation) => animation.finished)).then(then);
  };

  const buildPane = (page: MenuPage, level: number, first: boolean): HTMLElement => {
    const msg = message;
    const top = level === 0;
    const pane = document.createElement('div');
    pane.className = first ? 'html-menu-pane is-first' : 'html-menu-pane';
    pane.dataset.level = String(level);
    if (top) {
      pane.setAttribute('role', 'presentation');
    } else {
      pane.setAttribute('role', 'group');
      pane.setAttribute('aria-label', page.label);
    }
    const lead = hasLeadColumn(page.entries);
    const trail = hasTrailColumn(page.entries);

    if (top && msg?.header) {
      const header = document.createElement('div');
      header.className = 'html-menu-header';
      header.setAttribute('aria-hidden', 'true');
      const avatar = disc('html-menu-avatar', { avatar: msg.header.avatar, accent: msg.header.accent });
      const name = document.createElement('span');
      name.className = 'html-menu-name';
      name.textContent = msg.header.name;
      const caption = document.createElement('span');
      caption.className = 'html-menu-caption';
      caption.textContent = msg.header.caption;
      header.append(avatar, name, caption);
      pane.append(header);
    }

    const list: HTMLElement[] = [];
    page.entries.forEach((entry, index) => {
      if ('section' in entry) {
        const heading = document.createElement('div');
        heading.className = 'html-menu-section';
        heading.setAttribute('role', 'presentation');
        heading.textContent = entry.section;
        list.push(heading);
        return;
      }
      if ('separator' in entry) {
        const separator = document.createElement('div');
        separator.className = 'html-menu-separator';
        separator.setAttribute('role', 'separator');
        list.push(separator);
        return;
      }
      const row = document.createElement('button');
      row.type = 'button';
      row.tabIndex = -1;
      row.className = 'html-menu-row';
      row.setAttribute('role', entry.checked ? 'menuitemcheckbox' : 'menuitem');
      if (entry.checked) row.setAttribute('aria-checked', 'true');
      row.disabled = !entry.enabled;
      row.setAttribute('aria-disabled', String(!entry.enabled));
      if (entry.submenu) {
        row.setAttribute('aria-haspopup', 'menu');
        row.setAttribute('aria-expanded', 'false');
      }
      if (entry.shortcut) row.setAttribute('aria-keyshortcuts', entry.shortcut);
      row.id = `html-menu-row-${level}-${index}`;
      row.dataset.index = String(index);
      row.dataset.id = String(entry.id);
      if (entry.icon) row.dataset.icon = entry.icon;

      if (lead) {
        // Icon, favicon or badge; rows without one keep the column so labels line up.
        const slot = document.createElement('span');
        slot.className = 'html-menu-lead';
        slot.setAttribute('aria-hidden', 'true');
        const favUri = entry.fav !== undefined ? msg?.favs?.[entry.fav] : undefined;
        if (favUri) {
          const img = document.createElement('img');
          img.className = 'html-menu-favicon';
          img.alt = '';
          img.width = 16;
          img.height = 16;
          img.src = favUri;
          slot.append(img);
        } else if (entry.badge) {
          slot.append(disc('html-menu-badge', entry.badge));
        } else if (entry.icon && isIconName(entry.icon)) {
          slot.append(icon(entry.icon as IconName));
        }
        row.append(slot);
      }
      const label = document.createElement('span');
      label.className = 'html-menu-label';
      label.textContent = entry.label;
      row.append(label);
      if (entry.shortcut) {
        const shortcut = document.createElement('span');
        shortcut.className = 'html-menu-shortcut';
        shortcut.setAttribute('aria-hidden', 'true');
        shortcut.textContent = entry.shortcut;
        row.append(shortcut);
      }
      if (trail) {
        const slot = document.createElement('span');
        slot.className = 'html-menu-trail';
        slot.setAttribute('aria-hidden', 'true');
        if (entry.checked) slot.append(icon('check'));
        else if (entry.submenu) slot.append(icon('chevron-right'));
        row.append(slot);
      }
      row.addEventListener('mousemove', () => {
        root.dataset.input = 'pointer';
        // Only the open page follows the pointer; the parent's rows tint on hover in CSS.
        if (!nav || !entry.enabled || nav.pages[nav.pages.length - 1] !== page || page.selected === index) return;
        page.selected = index;
        markSelected();
      });
      // Mouse up, not click: a press that started outside the menu (the right click that opened it)
      // must not pick, and keyboard Space/Enter are handled on keydown.
      row.addEventListener('mouseup', (event) => {
        if (event.button !== 0 || !nav || nav.pages[level] !== page) return;
        finish(activateAt(nav, level, index));
      });
      list.push(row);
    });
    const scroller = document.createElement('div');
    scroller.className = 'html-menu-list bw-scroll';
    scroller.setAttribute('role', 'presentation');
    scroller.append(...list);
    pane.append(scroller);

    if (top && msg?.footer && footerVisible(msg.kind)) {
      const footer = document.createElement('div');
      footer.className = 'html-menu-footer';
      footer.setAttribute('aria-hidden', 'true');
      footer.textContent = msg.footer;
      pane.append(footer);
    }
    return pane;
  };

  // Makes the strip hold exactly the panes of the pages on screen (the open page and its parent),
  // building the missing ones and dropping the rest. Panes already there are left where they are.
  const mount = (first: boolean): void => {
    if (!nav) return;
    const wanted = nav.pages.slice(firstVisible(nav));
    for (const [page, pane] of panes) {
      if (wanted.includes(page)) continue;
      pane.remove();
      panes.delete(page);
    }
    let next: HTMLElement | null = null;
    for (const page of [...wanted].reverse()) {
      let pane = panes.get(page);
      if (!pane) {
        pane = buildPane(page, nav.pages.indexOf(page), first);
        panes.set(page, pane);
        slide.insertBefore(pane, next);
      }
      next = pane;
    }
    root.dataset.page = String(nav.pages.length - 1);
  };

  // Puts the strip at rest with the panes of the current pages and reports the size they need.
  // The child pane is dropped before the shift snaps back, both before the next paint.
  const sync = (): void => {
    mount(false);
    tagPanes();
    setMotion(0, false, true);
    markSelected();
    placeChild();
    measure(false);
  };

  // Calls `ready` once the page is `test`-wide (the window took the size just reported) or `late`
  // when it never does. Returns the cancel function.
  const awaitWidth = (test: (width: number) => boolean, ready: () => void, late: () => void): (() => void) => {
    let done = false;
    const finishWith = (then: () => void) => (): void => {
      if (done) return;
      done = true;
      window.removeEventListener('resize', check);
      window.clearTimeout(timer);
      then();
    };
    const check = (): void => { if (test(window.innerWidth)) finishWith(ready)(); };
    const timer = window.setTimeout(finishWith(late), RESIZE_WAIT_MS);
    window.addEventListener('resize', check);
    check();
    return () => {
      done = true;
      window.removeEventListener('resize', check);
      window.clearTimeout(timer);
    };
  };

  // Distance from the left edge of a pane to the right edge of the strip: what the window's fixed
  // right edge keeps in place. Both rects move with the slide, so their difference does not.
  const reach = (pane: HTMLElement): number => slide.getBoundingClientRect().right - pane.getBoundingClientRect().left;

  // A submenu opened (or another replaced it). The parent is snapped right by the growth before the
  // size is reported, so the wider window does not throw it; once the page has resized, the parent
  // eases left and the child fades in beside the row. The clip stays the whole window until rest.
  const openPages = (): void => {
    if (!nav || !message) return;
    const parentPage = nav.pages[nav.pages.length - 2];
    const parent = parentPage ? panes.get(parentPage) : undefined;
    const before = parent ? reach(parent) : 0;
    mount(false);
    markSelected();
    placeChild();
    tagPanes();
    const delta = parent ? Math.round(reach(parent) - before) : 0;
    const tok = message.tok;
    if (!parent || delta <= 0) {
      setMotion(0, false, true);
      const size = layoutSize();
      publish(tok, size.width, size.height, true);
      return;
    }
    epoch += 1;
    const mine = epoch;
    let cancel = (): void => {};
    abort = () => { cancel(); sync(); };
    setMotion(delta, true, true);
    const size = layoutSize();
    const target = Math.ceil(size.width);
    publish(tok, size.width, size.height, false);
    const go = (): void => {
      if (epoch !== mine) return;
      setMotion(0, false, false);
      afterMotion(() => {
        if (epoch !== mine) return;
        abort = null;
        publish(tok, size.width, size.height, true);
      });
    };
    cancel = awaitWidth((width) => width >= target - 2, go, go);
  };

  // The submenu closed: its pane is still in the strip. The clip opens to the whole window, the
  // parent eases right onto where the child is, and the child fades. Then the narrower, shorter
  // size is reported and, once the window has taken it, the pane is dropped and the shift reset.
  const closePages = (): void => {
    if (!nav || !message) return;
    const parent = panes.get(currentPage(nav));
    markSelected();
    if (!parent) { sync(); return; }
    const delta = Math.round(slide.getBoundingClientRect().right - parent.getBoundingClientRect().right);
    if (delta <= 0) { sync(); return; }
    const tok = message.tok;
    const size = layoutSize();
    const parentHeight = Math.max(1, Math.ceil(parent.getBoundingClientRect().height));
    const target = Math.max(1, Math.ceil(size.width) - delta);
    epoch += 1;
    const mine = epoch;
    let cancel = (): void => {};
    abort = () => { cancel(); sync(); };
    publish(tok, size.width, size.height, false);
    setMotion(delta, true, false);
    afterMotion(() => {
      if (epoch !== mine) return;
      publish(tok, target, parentHeight, false);
      const swap = (): void => {
        if (epoch !== mine) return;
        abort = null;
        sync();
      };
      cancel = awaitWidth((width) => width <= target + 2, swap, swap);
    });
  };

  const pagesChanged = (): void => {
    if (!nav) return;
    // A change that lands mid-slide finishes that slide first and takes the current pages as they are.
    if (abort) { settle(); return; }
    if (nav.pages.some((page) => !panes.has(page))) openPages();
    else closePages();
  };

  const finish = (outcome: MenuOutcome): void => {
    if (!message || !nav || closed) return;
    switch (outcome.kind) {
      case 'select': markSelected(); break;
      case 'page': pagesChanged(); break;
      case 'pick': closed = true; sendPopupPick(message.tok, outcome.id); break;
      case 'dismiss': closed = true; sendPopupDismiss(message.tok); break;
      case 'none': break;
    }
  };

  root.addEventListener('keydown', (event: KeyboardEvent) => {
    if (!nav || closed) return;
    root.dataset.input = 'key';
    const outcome = menuKey(nav, event.key, { ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey, shift: event.shiftKey }, now());
    // Keep every key inside the menu: no page scrolling, no focus leaving, no button activation.
    if (event.key.length === 1 || outcome.kind !== 'none' || ['Tab', 'Enter', ' ', 'Home', 'End', 'PageUp', 'PageDown', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Backspace', 'Escape'].includes(event.key)) {
      event.preventDefault();
    }
    finish(outcome);
  });
  root.addEventListener('contextmenu', (event) => event.preventDefault());
  // Clicks must not move focus off the root (a focused button would draw a focus ring).
  root.addEventListener('mousedown', (event) => event.preventDefault());

  return {
    show(raw: unknown): boolean {
      if (!validMenuPayload(raw)) return false;
      settle();
      message = raw;
      nav = createNav(raw.entries);
      closed = false;
      panes.clear();
      root.className = 'html-menu is-paned bw-menu-surface';
      root.tabIndex = -1;
      root.setAttribute('role', 'menu');
      root.setAttribute('aria-label', raw.footer || raw.kind);
      root.dataset.kind = raw.kind;
      root.dataset.input = 'pointer';
      const track = document.createElement('div');
      track.className = 'html-menu-track';
      slide = document.createElement('div');
      slide.className = 'html-menu-slide';
      track.append(slide);
      root.replaceChildren(track);
      mount(true);
      markSelected();
      placeChild();
      tagPanes();
      measure(true);
      return true;
    },
  };
}
