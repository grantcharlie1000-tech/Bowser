import { icon, type IconName } from './dom';
import { isIconName } from './icons';
import {
  activateAt, childSlice, createNav, firstVisible, flyoutSide, footerVisible, hasFlyouts, hasLeadColumn, hasTrailColumn, innerPixels, menuKey, onFill,
  slideShift, validMenuPayload, validRoom,
  type FlyoutRoom, type MenuBadge, type MenuMessage, type MenuNav, type MenuOutcome, type MenuPage,
} from './menu-logic';
import { sendPopupDismiss, sendPopupPick, sendPopupSize, type PopupCards } from './protocol';

// Markup contract (styling lives in menu.css; keep these names stable):
//   #app.html-menu.is-paned[.has-flyouts][role=menu][data-kind][data-page][data-input]   the transparent surface
//   .html-menu-track                                   the parent card's box; child columns hang off its sides
//   .html-menu-pane.is-host[data-level].is-active      the parent card (the only card while no submenu is open)
//   .html-menu-flyout[.is-left][.is-sliding][.is-leaving][.is-retired]   a child card's column beside the
//       parent (--menu-child-y, --menu-child-x, --menu-child-overlap)
//   .html-menu-flyout > .html-menu-pane.is-child       the child card; it slides inside its column
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
// as tall as its own rows. The parent card never moves. A menu with submenus is laid out once with
// --menu-reserve of room on both sides of the parent, and the window is that whole page from the
// start (html_popup.rs flyout_bounds anchors the parent card, not the window), clipped to the parent.
// Opening a submenu only changes the clip, never the window. The child card slides out from behind the
// parent's edge: its column clips it, so it shows only past that edge. It opens to the right when the
// work area has room there (Rust's `room` message), to the left when only that side has room, and when
// neither has, on the roomier side over the parent's edge without a slide.
//
// The clip follows the child every frame, and a pixel inside the clip that the page leaves transparent
// is not see-through on Windows 10 (it shows white). So the clip only ever holds what is painted: while
// the card comes out it is cut from where the card was SLIDE_LAG_MS earlier (a late clip trims the
// card's leading edge for a frame, never uncovers), and while it goes back behind the parent it is cut
// from where the card will be SLIDE_LEAD_MS later. A card that leaves is cut out of the clip first and
// painted for KEEP_FRAMES more. A submenu replaced by another one is cut; only a resting one slides back.

export interface MenuView {
  /** Shows a `menu` message. False (and nothing changes) when it is not a valid menu payload. */
  show(raw: unknown): boolean;
  /** Takes Rust's `room` for the menu on screen. False when it is invalid or for another menu. */
  room(raw: unknown): boolean;
}

/** The clip trails a card coming out by this much of its slide, so a late clip only trims it. */
const SLIDE_LAG_MS = 25;

/** The clip runs this far ahead of a card going back behind the parent, so it never uncovers a pixel. */
const SLIDE_LEAD_MS = 50;

/** Frames a card stays painted after it was cut out of the clip: the clip lands a frame or two late. */
const KEEP_FRAMES = 3;

/** A rectangle in CSS pixels of the page. */
interface Box { x: number; y: number; w: number; h: number }

/** The child card on screen: its page, column and card, the side it opened on, and how far it slides. */
interface Flyout {
  page: MenuPage;
  wrap: HTMLElement;
  pane: HTMLElement;
  side: 'right' | 'left';
  /** The gap plus the card's width: the slide's length, from wholly behind the parent's edge to rest. */
  travel: number;
  /** How far the card is pulled over the parent when neither side has room for it. */
  overlap: number;
  /** In its place (not sliding): only a resting card slides back when it closes. */
  resting: boolean;
}

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
  // The parent card's box, the parent card (built once and kept while it is the parent, so its scroll
  // position survives a submenu opening beside it) and the child card on screen.
  let track: HTMLElement = document.createElement('div');
  let host: { page: MenuPage; pane: HTMLElement } | null = null;
  let flyout: Flyout | null = null;
  // Rust's `room` for this menu (null until it arrives), and the parent card and page as laid out,
  // measured with no height cap: what `size` reports every time, so Rust places the parent the same way.
  let room: FlyoutRoom | null = null;
  let natural: (Box & { width: number; height: number }) | null = null;
  // Voids the frame loop of a slide that was finished or replaced.
  let epoch = 0;

  const cards = (): Array<{ page: MenuPage; pane: HTMLElement }> => [host, flyout].filter((card) => card !== null);

  // Focus stays on the menu root; the selected row is exposed with aria-activedescendant, so no
  // row takes a focus ring and every key reaches one listener.
  const markSelected = (): void => {
    if (!nav) return;
    const current = nav.pages.length - 1;
    root.removeAttribute('aria-activedescendant');
    for (const { page, pane } of cards()) {
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

  // The parent card and the whole page at their natural size: the temporary lift of the height cap
  // matches what Rust caps, so a tall menu is capped by Rust rather than by the page.
  const measureNatural = (): void => {
    if (!host) return;
    root.style.maxHeight = 'none';
    root.style.height = 'auto';
    const origin = root.getBoundingClientRect();
    const card = host.pane.getBoundingClientRect();
    const box = innerPixels(card.left - origin.left, card.top - origin.top, card.width, card.height);
    natural = { ...box, x: Math.max(0, box.x), y: Math.max(0, box.y), width: root.offsetWidth, height: root.offsetHeight };
    root.style.removeProperty('max-height');
    root.style.removeProperty('height');
  };

  // Reports the size and, for a menu with submenus, the parent card and the part of the child card
  // that shows (`child`), which is the window's clip. The size never changes while submenus open and
  // close, so Rust only moves the clip.
  const publish = (child: Box | null): void => {
    if (!message || !natural) return;
    const width = Math.min(4000, Math.max(1, Math.ceil(natural.width)));
    const height = Math.min(4000, Math.max(1, Math.ceil(natural.height)));
    if (!root.classList.contains('has-flyouts')) {
      sendPopupSize(message.tok, width, height);
      return;
    }
    const shape: PopupCards = { px: natural.x, py: natural.y, pw: natural.w, ph: natural.h };
    if (child) Object.assign(shape, { cx: child.x, cy: child.y, cw: child.w, ch: child.h });
    sendPopupSize(message.tok, width, height, shape);
  };

  // The part of the child card that shows while it is `shift` px from its place: its column, less what
  // is still behind the parent's edge. Null when none of it shows.
  const childBox = (card: Flyout, shift: number): Box | null => {
    const origin = root.getBoundingClientRect();
    const column = card.wrap.getBoundingClientRect();
    const slice = childSlice(column.left - origin.left, column.width, shift, card.side);
    if (!slice || slice.x < 0) return null;
    const rows = innerPixels(0, column.top - origin.top, 1, column.height);
    return { x: slice.x, y: Math.max(0, rows.y), w: slice.w, h: rows.h };
  };

  // Measures the natural size and reports it (the parent card alone: a menu opens with no submenu).
  const measure = (): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok) return;
      measureNatural();
      publish(null);
      // The enter motion starts when the size is reported, which is when the window is shown.
      root.classList.add('is-entering');
    });
  };

  // Drops the child's column so its card's first row lines up with the parent row that opened it.
  const placeChild = (card: Flyout): void => {
    const row = host?.pane.querySelector<HTMLElement>('.html-menu-row.is-open');
    if (!host || !row) return;
    const style = getComputedStyle(card.pane);
    const pad = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.borderTopWidth) || 0);
    const y = Math.round(row.getBoundingClientRect().top - host.pane.getBoundingClientRect().top - pad);
    card.wrap.style.setProperty('--menu-child-y', `${Math.max(0, y)}px`);
  };

  // Picks the child's side from Rust's `room`: right when it fits, left when only that side has room,
  // else over the parent's edge on the roomier side.
  const placeSide = (card: Flyout): void => {
    if (!host) return;
    card.wrap.classList.remove('is-left');
    card.wrap.style.removeProperty('--menu-child-overlap');
    const need = Math.ceil(card.wrap.getBoundingClientRect().right - host.pane.getBoundingClientRect().right);
    const { side, overlap } = flyoutSide(room, need);
    card.side = side;
    card.travel = Math.max(0, need);
    card.overlap = overlap;
    card.wrap.classList.toggle('is-left', side === 'left');
    if (overlap > 0) card.wrap.style.setProperty('--menu-child-overlap', `${overlap}px`);
  };

  // Puts the child card in its place at once. The clip takes it a frame later, after it is painted.
  const rest = (card: Flyout): void => {
    card.wrap.classList.add('is-snap');
    card.wrap.style.setProperty('--menu-child-x', '0px');
    card.wrap.classList.remove('is-sliding', 'is-leaving');
    void card.pane.offsetWidth;
    card.wrap.classList.remove('is-snap');
    card.resting = true;
    const mine = ++epoch;
    requestAnimationFrame(() => {
      if (mine === epoch && flyout === card) publish(childBox(card, 0));
    });
  };

  // Slides the child card out from behind the parent (`entering`) or back behind it, one transform
  // transition, and moves the clip with it every frame: trailing on the way out, leading on the way
  // back, so the clip never holds a pixel the card has not painted. Reduced motion leaves no transform
  // transition, so the card jumps and the clip follows in the next frame.
  const slide = (card: Flyout, entering: boolean, done: () => void): void => {
    const hidden = `${card.side === 'left' ? card.travel : -card.travel}px`;
    card.resting = false;
    card.wrap.classList.add('is-sliding');
    card.wrap.classList.toggle('is-leaving', !entering);
    if (entering) {
      card.wrap.classList.add('is-snap');
      card.wrap.style.setProperty('--menu-child-x', hidden);
      void card.pane.offsetWidth;
      card.wrap.classList.remove('is-snap');
    }
    card.wrap.style.setProperty('--menu-child-x', entering ? '0px' : hidden);
    const style = getComputedStyle(card.pane);
    const duration = (Number.parseFloat(style.transitionDuration) || 0) * 1000;
    const timing = style.transitionTimingFunction;
    const motion = card.pane.getAnimations().find((animation) => animation.playState !== 'finished');
    const mine = ++epoch;
    const bias = entering ? -SLIDE_LAG_MS : SLIDE_LEAD_MS;
    const frame = (): void => {
      if (mine !== epoch) return;
      const live = motion !== undefined && motion.playState !== 'finished';
      const time = live ? motion.currentTime : null;
      const elapsed = typeof time === 'number' ? time + bias : Number.POSITIVE_INFINITY;
      publish(childBox(card, slideShift(card.travel, live ? duration : 0, timing, elapsed, entering)));
      if (!live) {
        done();
        return;
      }
      requestAnimationFrame(frame);
    };
    // Coming out, the card is painted before the clip takes any of it; going back, the clip goes first.
    if (entering) requestAnimationFrame(frame);
    else frame();
  };

  // A card that left: already out of the clip, painted a few frames more so the clip lands first.
  const retire = (wrap: HTMLElement): void => {
    wrap.classList.add('is-retired');
    wrap.setAttribute('aria-hidden', 'true');
    for (const node of Array.from(wrap.querySelectorAll('[id]'))) node.removeAttribute('id');
    let frames = KEEP_FRAMES;
    const tick = (): void => {
      if (--frames > 0) requestAnimationFrame(tick);
      else wrap.remove();
    };
    requestAnimationFrame(tick);
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

  // Builds the child card for `page` in its own column beside the parent, behind the parent's edge.
  const mountChild = (page: MenuPage): Flyout | null => {
    if (!nav || !host) return null;
    const pane = buildPane(page, nav.pages.indexOf(page), false);
    pane.classList.add('is-child');
    const wrap = document.createElement('div');
    wrap.className = 'html-menu-flyout';
    wrap.setAttribute('role', 'presentation');
    wrap.append(pane);
    track.append(wrap);
    const card: Flyout = { page, wrap, pane, side: 'right', travel: 0, overlap: 0, resting: false };
    flyout = card;
    markSelected();
    placeChild(card);
    placeSide(card);
    return card;
  };

  // A submenu opened: its card slides out from behind the parent. A card pulled over the parent (no
  // room on either side) is put in its place without a slide.
  const enter = (page: MenuPage): void => {
    const card = mountChild(page);
    if (!card) return;
    if (card.overlap > 0) rest(card);
    else slide(card, true, () => {
      card.resting = true;
      card.wrap.classList.remove('is-sliding');
    });
  };

  // The child card closes. A resting card slides back behind the parent; one still moving, or one
  // replaced by another submenu, is cut out of the clip at once.
  const leave = (card: Flyout, cut: boolean): void => {
    flyout = null;
    epoch += 1;
    if (!cut && card.resting && card.overlap === 0) {
      slide(card, false, () => retire(card.wrap));
      return;
    }
    publish(null);
    retire(card.wrap);
  };

  // Builds the parent card (and the child card at rest, when one is open) from the pages on screen.
  // Used when a menu is shown and when the parent itself changes (a third level pushes the parent off).
  const rebuild = (first: boolean): void => {
    if (!nav) return;
    epoch += 1;
    if (flyout) retire(flyout.wrap);
    flyout = null;
    host?.pane.remove();
    const start = firstVisible(nav);
    const top = nav.pages[start]!;
    host = { page: top, pane: buildPane(top, start, first) };
    host.pane.classList.add('is-host');
    track.prepend(host.pane);
    root.dataset.page = String(nav.pages.length - 1);
    markSelected();
    if (first) return;
    measureNatural();
    publish(null);
    const child = nav.pages.length - start > 1 ? nav.pages[nav.pages.length - 1]! : null;
    const card = child ? mountChild(child) : null;
    if (card) rest(card);
  };

  const pagesChanged = (): void => {
    if (!nav) return;
    const start = firstVisible(nav);
    const top = nav.pages[start]!;
    const child = nav.pages.length - start > 1 ? nav.pages[nav.pages.length - 1]! : null;
    root.dataset.page = String(nav.pages.length - 1);
    if (!host || host.page !== top) {
      rebuild(false);
      return;
    }
    if (flyout?.page === child) {
      markSelected();
      return;
    }
    if (flyout) leave(flyout, child !== null);
    markSelected();
    if (child) enter(child);
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
      epoch += 1;
      message = raw;
      nav = createNav(raw.entries);
      closed = false;
      host = null;
      flyout = null;
      room = null;
      natural = null;
      root.className = 'html-menu is-paned bw-menu-surface';
      root.classList.toggle('has-flyouts', hasFlyouts(raw.entries));
      root.tabIndex = -1;
      root.setAttribute('role', 'menu');
      root.setAttribute('aria-label', raw.footer || raw.kind);
      root.dataset.kind = raw.kind;
      root.dataset.input = 'pointer';
      track = document.createElement('div');
      track.className = 'html-menu-track';
      root.replaceChildren(track);
      rebuild(true);
      measure();
      return true;
    },
    room(raw: unknown): boolean {
      if (!validRoom(raw) || !message || raw.tok !== message.tok) return false;
      room = { l: raw.l, r: raw.r };
      return true;
    },
  };
}
