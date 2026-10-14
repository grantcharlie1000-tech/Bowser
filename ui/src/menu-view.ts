import { icon, type IconName } from './dom';
import { isIconName } from './icons';
import {
  activateAt, createNav, currentPage, firstVisible, footerVisible, hasLeadColumn, hasTrailColumn, menuKey, onFill, validMenuPayload,
  type MenuBadge, type MenuMessage, type MenuNav, type MenuOutcome, type MenuPage,
} from './menu-logic';
import { sendPopupDismiss, sendPopupPick, sendPopupSize } from './protocol';

// Markup contract (styling lives in menu.css; keep these names stable):
//   #app.html-menu.is-paned[role=menu][data-kind][data-page][data-input]   the menu: one surface
//   .html-menu-track > .html-menu-slide                the clip and the sliding strip (--menu-slide)
//   .html-menu-pane[data-level].is-active              one page; a submenu's pane sits beside its parent's
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
// A submenu opens beside its parent; at most two panes are on screen. The popup window is widened to
// fit both and keeps its right edge (html_popup.rs keep_right_edge), so the parent slides left and the
// submenu lands where the parent was; closing slides the parent back and then the window shrinks.
// Opening starts the slide when the page's resize arrives (the window has grown); closing slides first
// and swaps the panes when the window has shrunk, so the parent never jumps.

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

  // Reports a size; Rust caps it to the monitor work area and flips or clamps the window, and
  // `.html-menu-list` scrolls inside whatever height it is given.
  const report = (tok: number, width: number, height: number): void => {
    sendPopupSize(tok, Math.max(1, Math.ceil(width)), Math.max(1, Math.ceil(height)));
  };

  // Measures the natural size and reports it.
  const measure = (first: boolean): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok) return;
      root.style.maxHeight = 'none';
      const rect = root.getBoundingClientRect();
      root.style.maxHeight = '100vh';
      report(tok, rect.width, rect.height);
      // The enter motion starts when the size is reported, which is when the window is shown.
      if (first) root.classList.add('is-entering');
    });
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
  const sync = (): void => {
    slide.classList.add('is-snap');
    slide.style.setProperty('--menu-slide', '0px');
    mount(false);
    void slide.offsetWidth;
    slide.classList.remove('is-snap');
    markSelected();
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
    return () => {
      done = true;
      window.removeEventListener('resize', check);
      window.clearTimeout(timer);
    };
  };

  // Distance from the left edge of a pane to the right edge of the strip: what the window's fixed
  // right edge keeps in place. Both rects move with the slide, so their difference does not.
  const reach = (pane: HTMLElement): number => slide.getBoundingClientRect().right - pane.getBoundingClientRect().left;

  // A submenu opened (or another replaced it): the pane beside it is already built. The window grows
  // on the left, which would throw the parent left; once the page has resized, the strip is set back
  // by the growth and eases to rest, so the parent slides aside from where it was.
  const openPages = (): void => {
    if (!nav) return;
    const parentPage = nav.pages[nav.pages.length - 2];
    const parent = parentPage ? panes.get(parentPage) : undefined;
    const before = parent ? reach(parent) : 0;
    mount(false);
    markSelected();
    const delta = parent ? Math.round(reach(parent) - before) : 0;
    const target = Math.ceil(root.getBoundingClientRect().width);
    measure(false);
    if (!parent || delta <= 0) return;
    epoch += 1;
    const mine = epoch;
    const cancel = awaitWidth(
      (width) => width >= target - 2,
      () => {
        if (epoch !== mine) return;
        abort = null;
        slide.classList.add('is-snap');
        slide.style.setProperty('--menu-slide', `${delta}px`);
        void slide.offsetWidth;
        slide.classList.remove('is-snap');
        slide.style.setProperty('--menu-slide', '0px');
      },
      () => { if (epoch === mine) abort = null; },
    );
    abort = () => { cancel(); sync(); };
  };

  // The submenu closed: its pane is still in the strip. The strip slides right until the parent is
  // where the shrunken window will put it, then the new size is reported and, once the window has
  // taken it, the pane is dropped and the strip reset in the same frame.
  const closePages = (): void => {
    if (!nav || !message) return;
    const parent = panes.get(currentPage(nav));
    markSelected();
    if (!parent) { sync(); return; }
    const delta = Math.round(slide.getBoundingClientRect().right - parent.getBoundingClientRect().right);
    if (delta <= 0) { sync(); return; }
    const tok = message.tok;
    const rect = root.getBoundingClientRect();
    const target = Math.ceil(rect.width) - delta;
    epoch += 1;
    const mine = epoch;
    let cancel = (): void => {};
    abort = () => { cancel(); sync(); };
    slide.style.setProperty('--menu-slide', `${delta}px`);
    void slide.offsetWidth;
    const afterSlide = (): void => {
      if (epoch !== mine) return;
      report(tok, target, rect.height);
      const swap = (): void => { if (epoch !== mine) return; abort = null; sync(); };
      cancel = awaitWidth((width) => width <= target + 2, swap, swap);
    };
    // Under reduced motion nothing transitions, so there is nothing to wait for.
    const running = slide.getAnimations();
    if (running.length === 0) afterSlide();
    else void Promise.allSettled(running.map((animation) => animation.finished)).then(afterSlide);
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
      measure(true);
      return true;
    },
  };
}
