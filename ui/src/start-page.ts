// The start page (docs/design/start-page.md, styles/start.css): what a blank new tab shows. It is the
// split view's host (?view=split) drawing this over the tab; this bundle is loaded the first time Rust
// sends a `snap` with p "startpage", so no other view parses it. Rust owns every model (speed dials are
// bookmarks, see src/start_page.rs); this page renders and reports intent with `{t:"spage",a,...}`
// commands, and every one is checked again in Rust (chrome_host/start_page.rs).
//
// Text from Rust reaches the DOM through textContent and attributes only. The webpage widget is the one
// place that embeds anything: a sandboxed iframe (no same-origin, no top navigation, no popups, no
// downloads), http(s) only, created only for an address the user typed into that widget.
import { sendPanel, type PanelFields } from './protocol';
import { el, icon, type IconName } from './dom';
import { iconButton, textButton } from './panels/kit';
import { tileLetter } from './panels/kit-logic';
import { selectField } from './panels/select';
import { tileTone } from './history-page-logic';
import { onSpoilerConfig, activeSpoilerMode } from './spoiler-config'; // --- spoiler guard ---
import { guardTitle, hostOnly } from './spoiler-guard'; // --- spoiler guard --- titles that came from pages (top sites, bookmarks, dials)
import {
  DIM_STEPS, MAX_NOTE, MAX_TITLE, MAX_URL, NOTE_SAVED, SIZE_LABELS, WALL_ART, WIDGET_BGS, WIDGET_HINTS, WIDGET_KINDS, WIDGET_LABELS, blockedText, clockParts,
  columnsFor, dialCountText, dimStep, dropBefore, isWebAddress, normaliseAddress, textOn, tileWidth, titleState, toneOf, wallInk, widgetClasses,
  type BookmarkView, type Dial, type GroupTab, type StartMessage, type StartSnapshot, type TitleMode, type WallTone, type Wallpaper, type Widget,
  type WidgetBg, type WidgetKind,
} from './start-page-logic';

const NOTE_DEBOUNCE_MS = 450;
/** Pointer travel (px) before a press on a tile becomes a drag. */
const DRAG_START = 6;
/** The narrowest a widget gets before the grid drops a column. */
const WIDGET_MIN = 220;

function cmd(fields: PanelFields): boolean {
  return sendPanel('spage', fields);
}

/** Saves one start page setting from the Customise panel (Rust checks the key and value again). */
function setting(key: string, value: string | number | boolean): boolean {
  return cmd({ a: 'set', text: `${key}=${String(value)}` });
}

interface DialNode {
  readonly root: HTMLLIElement;
  readonly open: HTMLButtonElement;
  readonly face: HTMLElement;
  readonly title: HTMLElement;
  dial: Dial;
}

interface WidgetNode {
  readonly root: HTMLElement;
  readonly id: number;
}

const TITLE_CHOICES: ReadonlyArray<readonly [TitleMode, string]> = [['always', 'Always'], ['auto', 'If no image'], ['never', 'Never']];
const STYLE_CHOICES: ReadonlyArray<readonly [WidgetBg, string]> = [['translucent', 'Glass'], ['solid', 'Solid'], ['transparent', 'Clear']];

/**
 * Builds the page once. `show` applies a `page`, `thumbs` or `wall` message; `hidden` records that the
 * host took the page away (the next show comes back settled, with the entrance only for a tab it has not shown).
 */
export function buildStartPage(): {
  readonly root: HTMLElement;
  show(msg: StartMessage): void;
  hidden(): void;
} {
  const root = el('div', 'sp-page bw-scroll');
  root.setAttribute('role', 'region');
  root.setAttribute('aria-label', 'Start page');
  let snap: StartSnapshot | null = null;
  const thumbs = new Map<number, string>();
  let entered = false;
  let toastTimer = 0;
  let noteSaveTimer = 0;
  let clockTimer = 0;
  /** Widget frames and note editors outlive snapshots, so a refresh never reloads a page or loses a caret. */
  const frames = new Map<number, { url: string; node: HTMLIFrameElement }>();
  /** Spoiler guard: web page widgets the user chose to load (widget id to address) while protection is on. The start page is
   *  a Bowser chrome document, which never gets the page engine, so a live page in it is paused until a click. */
  const approvedFrames = new Map<number, string>();
  const editors = new Map<number, HTMLTextAreaElement>();
  const savedNotes = new Set<number>();
  /** The widget whose settings strip is open, if any. */
  let configuring: number | null = null;
  let modal: HTMLElement | null = null;
  let modalReturn: HTMLElement | null = null;
  let drag: { kind: 'dial' | 'widget'; id: number; node: HTMLElement; startX: number; startY: number; pointer: number; active: boolean; target: HTMLElement | null; after: boolean; into: boolean } | null = null;
  let suppressClick = false;
  /** The user's background picture: the parts of the version being received, and the one shown. */
  let wallParts: { stamp: string; parts: string[] } | null = null;
  let wallImage: { stamp: string; uri: string; tone: WallTone } | null = null;

  // ---- skeleton ----
  const backdrop = el('div', 'sp-backdrop');
  backdrop.setAttribute('aria-hidden', 'true');
  const column = el('div', 'sp-column');
  const heading = el('h1', 'sp-sr');
  heading.textContent = 'Start page';

  const hero = el('header', 'sp-top');
  const clock = el('div', 'sp-clock');
  const clockTime = el('p', 'sp-clock-time');
  const clockDate = el('p', 'sp-clock-date');
  clock.append(clockTime, clockDate);
  const search = el('form', 'sp-find');
  search.setAttribute('role', 'search');
  const searchGlyph = el('span', 'sp-find-glyph');
  searchGlyph.setAttribute('aria-hidden', 'true');
  searchGlyph.append(icon('search', { size: 18 }));
  const searchInput = el('input', 'sp-find-input');
  searchInput.type = 'search';
  searchInput.placeholder = 'Search the web or type an address';
  searchInput.maxLength = MAX_URL;
  searchInput.autocomplete = 'off';
  searchInput.spellcheck = false;
  searchInput.setAttribute('aria-label', 'Search the web or type an address');
  const searchGo = el('button', 'sp-find-go');
  searchGo.type = 'submit';
  searchGo.setAttribute('aria-label', 'Search');
  searchGo.append(icon('arrow-right', { size: 16 }));
  search.append(searchGlyph, searchInput, searchGo);
  search.addEventListener('submit', (event) => {
    event.preventDefault();
    const text = searchInput.value.trim();
    if (text === '') return;
    if (cmd({ a: 'search', text })) searchInput.value = '';
  });
  hero.append(clock, search);

  const groupBar = el('div', 'sp-groups');
  const tablist = el('div', 'sp-tabs');
  tablist.setAttribute('role', 'tablist');
  tablist.setAttribute('aria-label', 'Speed dial groups');
  const groupTools = el('div', 'sp-group-tools');
  groupBar.append(tablist, groupTools);
  const crumbs = el('nav', 'sp-crumbs');
  crumbs.setAttribute('aria-label', 'Folder');
  const grid = el('ul', 'sp-grid');
  grid.setAttribute('role', 'list');
  grid.setAttribute('aria-label', 'Speed dials');
  const dialsSection = el('section', 'sp-dials');
  dialsSection.setAttribute('aria-label', 'Speed dials');
  dialsSection.append(groupBar, crumbs, grid);
  const widgetsSection = el('section', 'sp-widgets');
  widgetsSection.setAttribute('aria-label', 'Widgets');
  const widgetGrid = el('div', 'sp-widget-grid');
  const widgetBar = el('div', 'sp-widget-bar');
  const addWidget = textButton('Add widget', () => openWidgetMenu(addWidget), { icon: 'plus', subtle: true });
  addWidget.classList.add('sp-add-widget');
  widgetBar.append(addWidget);
  widgetsSection.append(widgetGrid, widgetBar);
  const toast = el('p', 'sp-toast');
  toast.setAttribute('role', 'status');
  toast.hidden = true;
  column.append(heading, hero, dialsSection, widgetsSection);

  const customise = el('button', 'sp-customise');
  customise.type = 'button';
  customise.setAttribute('aria-label', 'Customise this page');
  customise.setAttribute('data-tip', 'Customise this page');
  customise.setAttribute('aria-expanded', 'false');
  customise.append(icon('edit', { size: 16 }));
  const drawer = el('aside', 'sp-drawer');
  drawer.id = 'sp-drawer';
  drawer.setAttribute('aria-label', 'Customise');
  drawer.hidden = true;
  customise.setAttribute('aria-controls', drawer.id);
  customise.addEventListener('click', () => (drawer.hidden ? openDrawer() : closeDrawer()));
  root.append(backdrop, column, customise, drawer, toast);

  // ---- helpers ----
  const say = (text: string): void => {
    toast.textContent = text;
    toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => { toast.hidden = true; }, 5000);
  };

  const favOf = (host: string): string => snap?.favs[host] ?? '';

  /** The tile face: the chosen image, the site's cached icon on a chip, or a letter on a tone. */
  function paintFace(face: HTMLElement, dial: { kind: 'link' | 'folder'; title: string; host: string; id?: number; thumb?: boolean }): void {
    face.replaceChildren();
    face.removeAttribute('data-tone');
    face.classList.remove('has-image', 'has-icon', 'is-folder', 'is-letter');
    if (dial.kind === 'folder') {
      face.classList.add('is-folder');
      const chip = el('span', 'sp-chip');
      chip.append(icon('folder', { size: 20 }));
      face.append(chip);
      return;
    }
    const chosen = dial.thumb && dial.id !== undefined ? thumbs.get(dial.id) : undefined;
    if (chosen) {
      const img = el('img', 'sp-thumb');
      img.alt = '';
      img.draggable = false;
      img.src = chosen;
      face.classList.add('has-image');
      face.append(img);
      return;
    }
    const fav = favOf(dial.host);
    if (fav) {
      const chip = el('span', 'sp-chip');
      const img = el('img', 'sp-fav');
      img.alt = '';
      img.draggable = false;
      img.src = fav;
      chip.append(img);
      face.classList.add('has-icon');
      face.append(chip);
      return;
    }
    face.classList.add('is-letter');
    face.dataset.tone = String(tileTone(dial.host || dial.title));
    const chip = el('span', 'sp-chip');
    const letter = el('span', 'sp-letter');
    letter.textContent = tileLetter(guardTitle(dial.title) || dial.host);
    chip.append(letter);
    face.append(chip);
  }

  // ---- dialogs (one at a time, inside the page) ----
  function closeModal(restore = true): void {
    if (!modal) return;
    modal.remove();
    modal = null;
    if (restore && modalReturn?.isConnected) modalReturn.focus({ preventScroll: true });
    modalReturn = null;
  }

  function openModal(title: string, build: (body: HTMLElement, close: () => void) => HTMLElement | null): void {
    closeModal(false);
    modalReturn = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const scrim = el('div', 'sp-scrim bw-enter');
    const dialog = el('form', 'sp-dialog bw-dialog bw-enter is-rise');
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('aria-modal', 'true');
    const heading2 = el('h2', 'bw-dialog-title');
    heading2.textContent = title;
    heading2.id = 'sp-dialog-title';
    dialog.setAttribute('aria-labelledby', 'sp-dialog-title');
    const body = el('div', 'bw-dialog-body sp-dialog-body');
    dialog.append(heading2, body);
    const focusFirst = build(body, () => closeModal());
    scrim.append(dialog);
    scrim.addEventListener('pointerdown', (event) => { if (event.target === scrim) closeModal(); });
    dialog.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        closeModal();
      }
    });
    modal = scrim;
    root.append(scrim);
    (focusFirst ?? dialog.querySelector<HTMLElement>('input, button'))?.focus({ preventScroll: true });
  }

  function field(label: string, value: string, placeholder = '', max = MAX_TITLE): { root: HTMLElement; input: HTMLInputElement } {
    const wrap = el('label', 'sp-field');
    const text = el('span', 'sp-field-label');
    text.textContent = label;
    const input = el('input', 'bw-input');
    input.type = 'text';
    input.value = value;
    input.placeholder = placeholder;
    input.maxLength = max;
    input.autocomplete = 'off';
    input.spellcheck = false;
    wrap.append(text, input);
    return { root: wrap, input };
  }

  function actions(close: () => void, submitLabel: string): HTMLElement {
    const row = el('div', 'bw-dialog-actions');
    const cancel = textButton('Cancel', close);
    const ok = textButton(submitLabel, () => {}, { primary: true });
    ok.type = 'submit';
    row.append(cancel, ok);
    return row;
  }

  function openDialForm(existing: Dial | null): void {
    if (!snap || snap.guest) return;
    // 0: no group yet. Rust makes one for the first site, so the person never has to.
    const folder = snap.folder;
    const isFolderEdit = existing?.kind === 'folder';
    openModal(existing ? (isFolderEdit ? 'Rename folder' : 'Edit site') : 'Add site', (body, close) => {
      const form = body.closest('form');
      const name = field('Name', existing?.title ?? '', 'Optional');
      const address = field('Address', existing?.url ?? '', 'example.com', MAX_URL);
      const error = el('p', 'sp-error');
      error.setAttribute('role', 'alert');
      error.hidden = true;
      if (isFolderEdit) body.append(name.root);
      else body.append(address.root, name.root);
      body.append(error);
      form?.append(actions(close, existing ? 'Save' : 'Add'));
      const fail = (message: string): void => {
        error.textContent = message;
        error.hidden = false;
      };
      form?.addEventListener('submit', (event) => {
        event.preventDefault();
        const title = name.input.value.trim();
        if (isFolderEdit) {
          if (title === '') return fail('Enter a name.');
          cmd({ a: 'edit', id: existing!.id, text: title });
          return close();
        }
        const url = normaliseAddress(address.input.value);
        if (url === null) return fail('Enter an http or https address.');
        if (existing) cmd({ a: 'edit', id: existing.id, text: title === '' ? hostOf(url) : title, url });
        else cmd(title === '' ? { a: 'add', parent: folder, url } : { a: 'add', parent: folder, text: title, url });
        return close();
      });
      return isFolderEdit ? name.input : address.input;
    });
  }

  function openRename(group: GroupTab): void {
    openModal('Rename group', (body, close) => {
      const form = body.closest('form');
      const name = field('Name', group.title);
      body.append(name.root);
      form?.append(actions(close, 'Save'));
      form?.addEventListener('submit', (event) => {
        event.preventDefault();
        const title = name.input.value.trim();
        if (title === '') return;
        cmd({ a: 'renamegroup', id: group.id, text: title });
        close();
      });
      return name.input;
    });
  }

  function confirm(title: string, message: string, label: string, run: () => void): void {
    openModal(title, (body, close) => {
      const form = body.closest('form');
      const text = el('p', 'sp-confirm');
      text.textContent = message;
      body.append(text);
      const row = actions(close, label);
      form?.append(row);
      form?.addEventListener('submit', (event) => {
        event.preventDefault();
        run();
        close();
      });
      return row.querySelector<HTMLElement>('button[type="submit"]');
    });
  }

  function hostOf(url: string): string {
    try {
      return new URL(url).hostname;
    } catch {
      return url;
    }
  }

  // ---- menus (group and add-widget) ----
  let menu: HTMLElement | null = null;
  function closeMenu(): void {
    menu?.remove();
    menu = null;
    document.removeEventListener('pointerdown', onMenuAway, true);
  }
  function onMenuAway(event: Event): void {
    if (menu && event.target instanceof Node && !menu.contains(event.target)) closeMenu();
  }
  function openMenu(anchor: HTMLElement, label: string, items: ReadonlyArray<{ label: string; hint?: string; icon: IconName; run: () => void; disabled?: boolean }>): void {
    closeMenu();
    const box = el('div', 'bw-overlay bw-menu sp-menu bw-enter is-down');
    box.setAttribute('role', 'menu');
    box.setAttribute('aria-label', label);
    for (const item of items) {
      const button = el('button', 'bw-menu-item');
      button.type = 'button';
      button.setAttribute('role', 'menuitem');
      button.disabled = item.disabled === true;
      const glyph = el('span', 'sp-menu-glyph');
      glyph.append(icon(item.icon));
      button.append(glyph);
      const text = el('span', 'bw-menu-label sp-menu-text');
      const name = el('span', 'sp-menu-name');
      name.textContent = item.label;
      text.append(name);
      if (item.hint) {
        const hint = el('span', 'sp-menu-hint');
        hint.textContent = item.hint;
        text.append(hint);
        button.classList.add('has-hint');
      }
      button.append(text);
      button.addEventListener('click', () => {
        closeMenu();
        item.run();
      });
      box.append(button);
    }
    box.addEventListener('keydown', (event) => {
      const buttons = Array.from(box.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
      const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        closeMenu();
        anchor.focus({ preventScroll: true });
      } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const next = event.key === 'ArrowDown' ? (at + 1) % buttons.length : (at - 1 + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }
    });
    const rect = anchor.getBoundingClientRect();
    root.append(box);
    // Below the anchor, or above it when the list would run off the bottom of the window.
    const size = box.getBoundingClientRect();
    const below = rect.bottom + 4;
    const above = rect.top - size.height - 4;
    const y = below + size.height > window.innerHeight - 8 && above > 8 ? above : below;
    box.style.setProperty('--sp-menu-x', `${Math.max(8, Math.min(Math.round(rect.left), window.innerWidth - size.width - 8))}px`);
    box.style.setProperty('--sp-menu-y', `${Math.round(y)}px`);
    menu = box;
    document.addEventListener('pointerdown', onMenuAway, true);
    box.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
  }

  function openWidgetMenu(anchor: HTMLElement): void {
    openMenu(anchor, 'Add a widget', WIDGET_KINDS.map((kind) => ({
      label: WIDGET_LABELS[kind],
      hint: WIDGET_HINTS[kind],
      icon: widgetIcon(kind),
      run: () => { cmd({ a: 'wadd', text: kind }); },
    })));
  }

  function openGroupMenu(anchor: HTMLElement, group: GroupTab, index: number, count: number): void {
    const ids = snap?.groups.map((g) => g.id) ?? [];
    openMenu(anchor, `${group.title} group`, [
      { label: 'Rename', icon: 'edit', run: () => openRename(group) },
      { label: 'Move left', icon: 'chevron-left', disabled: index === 0, run: () => { cmd({ a: 'movegroup', id: group.id, before: ids[index - 1]! }); } },
      { label: 'Move right', icon: 'chevron-right', disabled: index >= count - 1, run: () => { const after = ids[index + 2]; cmd(after === undefined ? { a: 'movegroup', id: group.id } : { a: 'movegroup', id: group.id, before: after }); } },
      { label: 'Remove from start page', icon: 'close', run: () => confirm('Remove group', `Stop showing “${group.title}” here? The bookmark folder and its sites stay in Bookmarks.`, 'Remove', () => { cmd({ a: 'removegroup', id: group.id }); }) },
    ]);
  }

  function widgetIcon(kind: WidgetKind): IconName {
    switch (kind) {
      case 'topsites': return 'clock';
      case 'bookmarks': return 'bookmark';
      case 'notes': return 'notes';
      case 'date': return 'calendar';
      case 'privacy': return 'shield';
      default: return 'globe';
    }
  }

  // ---- speed dials ----
  const dialNodes = new Map<number, DialNode>();

  function dialNode(dial: Dial): DialNode {
    const root2 = el('li', 'sp-dial');
    root2.setAttribute('role', 'listitem');
    const open = el('button', 'sp-open');
    open.type = 'button';
    const face = el('span', 'sp-face');
    face.setAttribute('aria-hidden', 'true');
    const title = el('span', 'sp-title');
    title.dir = 'auto';
    open.append(face, title);
    const tools = el('div', 'sp-tools');
    const node: DialNode = { root: root2, open, face, title, dial };
    const edit = iconButton('edit', 'Edit', () => openDialForm(node.dial), 'sp-tool');
    const image = iconButton('image', 'Choose an image', () => { cmd({ a: 'thumb', id: node.dial.id }); }, 'sp-tool sp-tool-image');
    const remove = iconButton('trash', 'Remove', () => askRemove(node.dial), 'sp-tool');
    const imageClear = iconButton('reset', 'Use the site icon instead', () => { cmd({ a: 'thumbclear', id: node.dial.id }); }, 'sp-tool sp-tool-clear');
    tools.append(edit, image, imageClear, remove);
    root2.append(open, tools);
    open.addEventListener('click', (event) => {
      if (suppressClick) {
        suppressClick = false;
        event.preventDefault();
        return;
      }
      const d = node.dial;
      if (d.kind === 'folder') cmd({ a: 'show', id: d.id });
      else cmd(event.ctrlKey || event.shiftKey ? { a: 'open', id: d.id, on: true } : { a: 'open', id: d.id });
    });
    open.addEventListener('auxclick', (event) => {
      if (event.button === 1 && node.dial.kind === 'link') {
        event.preventDefault();
        cmd({ a: 'open', id: node.dial.id, on: true });
      }
    });
    open.addEventListener('keydown', (event) => onDialKey(event, node));
    open.addEventListener('pointerdown', (event) => beginDrag(event, 'dial', node.dial.id, root2));
    return node;
  }

  function askRemove(dial: Dial): void {
    const what = dial.kind === 'folder' ? `the folder “${dial.title}” and the ${dialCountText(dial.n)} in it` : `“${guardTitle(dial.title)}”`;
    confirm('Remove from speed dial', `Remove ${what}? Speed dials are bookmarks, so ${dial.kind === 'folder' ? 'they are' : 'it is'} deleted from Bookmarks too.`, 'Remove', () => { cmd({ a: 'remove', id: dial.id }); });
  }

  function paintDial(node: DialNode, dial: Dial, settings: StartSnapshot['settings'], editable: boolean): void {
    node.dial = dial;
    node.root.dataset.id = String(dial.id);
    node.root.dataset.kind = dial.kind;
    node.root.dataset.labels = titleState(settings.titles, dial.thumb);
    const dialTitle = guardTitle(dial.title);
    const label = dial.kind === 'folder' ? `${dialTitle}, folder, ${dialCountText(dial.n)}` : dialTitle;
    node.open.setAttribute('aria-label', label);
    node.title.textContent = dial.kind === 'folder' ? dialTitle : dialTitle || dial.host;
    paintFace(node.face, { kind: dial.kind, title: dial.title, host: dial.host, id: dial.id, thumb: dial.thumb });
    node.root.classList.toggle('is-editable', editable);
    node.root.classList.toggle('has-thumb', dial.thumb);
    node.root.classList.toggle('is-link', dial.kind === 'link');
    node.root.querySelectorAll<HTMLElement>('.sp-tool').forEach((tool) => { tool.hidden = !editable; });
    const clear = node.root.querySelector<HTMLElement>('.sp-tool-clear');
    if (clear) clear.hidden = !editable || !dial.thumb;
    const image = node.root.querySelector<HTMLElement>('.sp-tool-image');
    if (image) image.hidden = !editable || dial.kind === 'folder';
  }

  function onDialKey(event: KeyboardEvent, node: DialNode): void {
    const ids = snap?.dials.map((d) => d.id) ?? [];
    const at = ids.indexOf(node.dial.id);
    if (event.altKey && (event.key === 'ArrowLeft' || event.key === 'ArrowRight') && snap?.settings.dragReorder && !snap.guest) {
      event.preventDefault();
      const over = ids[event.key === 'ArrowLeft' ? at - 1 : at + 1];
      if (over === undefined) return;
      const before = dropBefore(ids, node.dial.id, over, event.key === 'ArrowRight');
      if (before === undefined) return;
      cmd(before === null ? { a: 'move', id: node.dial.id } : { a: 'move', id: node.dial.id, before });
      window.setTimeout(() => dialNodes.get(node.dial.id)?.open.focus({ preventScroll: true }), 60);
      return;
    }
    if (event.key === 'Delete' && snap && !snap.guest) {
      event.preventDefault();
      askRemove(node.dial);
      return;
    }
    gridKey(event, node.open);
  }

  /** Arrow keys, Home and End move focus across the tiles, by row and column. */
  function gridKey(event: KeyboardEvent, from: HTMLElement): void {
    const opens = Array.from(grid.querySelectorAll<HTMLElement>('.sp-open'));
    const index = opens.indexOf(from);
    const cols = Number.parseInt(grid.style.getPropertyValue('--sp-cols'), 10) || 1;
    let next = -1;
    switch (event.key) {
      case 'ArrowRight': next = Math.min(opens.length - 1, index + 1); break;
      case 'ArrowLeft': next = Math.max(0, index - 1); break;
      case 'ArrowDown': next = Math.min(opens.length - 1, index + cols); break;
      case 'ArrowUp': next = Math.max(0, index - cols); break;
      case 'Home': next = 0; break;
      case 'End': next = opens.length - 1; break;
      default: return;
    }
    event.preventDefault();
    opens[next]?.focus();
  }

  // ---- dragging (dials and widgets) ----
  function beginDrag(event: PointerEvent, kind: 'dial' | 'widget', id: number, node: HTMLElement): void {
    if (event.button !== 0 || !snap || snap.guest) return;
    if (kind === 'dial' ? !snap.settings.dragReorder : !snap.settings.widgetDrag) return;
    drag = { kind, id, node, startX: event.clientX, startY: event.clientY, pointer: event.pointerId, active: false, target: null, after: false, into: false };
    window.addEventListener('pointermove', onDragMove, true);
    window.addEventListener('pointerup', onDragEnd, true);
    window.addEventListener('pointercancel', onDragCancel, true);
    window.addEventListener('keydown', onDragKey, true);
  }

  function clearDropMarks(): void {
    root.querySelectorAll('.is-drop-before, .is-drop-after, .is-drop-into').forEach((mark) => mark.classList.remove('is-drop-before', 'is-drop-after', 'is-drop-into'));
  }

  function onDragMove(event: PointerEvent): void {
    if (!drag || event.pointerId !== drag.pointer) return;
    const dx = event.clientX - drag.startX;
    const dy = event.clientY - drag.startY;
    if (!drag.active) {
      if (Math.hypot(dx, dy) < DRAG_START) return;
      drag.active = true;
      drag.node.classList.add('is-dragging');
      root.classList.add('is-dragging-' + drag.kind);
      closeMenu();
    }
    event.preventDefault();
    drag.node.style.setProperty('--sp-dx', `${dx}px`);
    drag.node.style.setProperty('--sp-dy', `${dy}px`);
    clearDropMarks();
    const selector = drag.kind === 'dial' ? '.sp-dial' : '.sp-widget';
    const hit = document.elementsFromPoint(event.clientX, event.clientY).find((n) => n instanceof HTMLElement && n.matches(selector) && n !== drag!.node) as HTMLElement | undefined;
    drag.target = hit ?? null;
    drag.into = false;
    if (!hit) return;
    const rect = hit.getBoundingClientRect();
    const rel = (event.clientX - rect.left) / Math.max(1, rect.width);
    if (drag.kind === 'dial' && hit.dataset.kind === 'folder' && rel > 0.25 && rel < 0.75) {
      drag.into = true;
      hit.classList.add('is-drop-into');
      return;
    }
    const vertical = drag.kind === 'widget' && (event.clientY - rect.top) / Math.max(1, rect.height) > 0.5;
    // Widgets sit in a grid: with one column the pointer's height decides, otherwise its side.
    const stacked = (Number.parseInt(widgetGrid.style.getPropertyValue('--sp-wcols'), 10) || 1) === 1;
    drag.after = drag.kind === 'widget' ? (stacked ? vertical : rel > 0.5) : rel > 0.5;
    hit.classList.add(drag.after ? 'is-drop-after' : 'is-drop-before');
  }

  function finishDrag(commit: boolean): void {
    const d = drag;
    drag = null;
    window.removeEventListener('pointermove', onDragMove, true);
    window.removeEventListener('pointerup', onDragEnd, true);
    window.removeEventListener('pointercancel', onDragCancel, true);
    window.removeEventListener('keydown', onDragKey, true);
    if (!d?.active) return;
    suppressClick = true;
    window.setTimeout(() => { suppressClick = false; }, 0);
    d.node.classList.remove('is-dragging');
    d.node.style.removeProperty('--sp-dx');
    d.node.style.removeProperty('--sp-dy');
    root.classList.remove('is-dragging-dial', 'is-dragging-widget');
    clearDropMarks();
    if (!commit || !d.target || !snap) return;
    const targetId = Number(d.target.dataset.id);
    if (d.kind === 'dial') {
      if (d.into) {
        cmd({ a: 'movein', id: d.id, parent: targetId });
        return;
      }
      const ids = snap.dials.map((x) => x.id);
      const before = dropBefore(ids, d.id, targetId, d.after);
      if (before === undefined) return;
      cmd(before === null ? { a: 'move', id: d.id } : { a: 'move', id: d.id, before });
    } else {
      const ids = snap.widgets.map((x) => x.id);
      const before = dropBefore(ids, d.id, targetId, d.after);
      if (before === undefined) return;
      cmd(before === null ? { a: 'wmove', id: d.id } : { a: 'wmove', id: d.id, before });
    }
  }
  function onDragEnd(event: PointerEvent): void {
    if (drag && event.pointerId === drag.pointer) finishDrag(true);
  }
  function onDragCancel(event: PointerEvent): void {
    if (drag && event.pointerId === drag.pointer) finishDrag(false);
  }
  function onDragKey(event: KeyboardEvent): void {
    if (event.key === 'Escape' && drag?.active) {
      event.preventDefault();
      event.stopPropagation();
      finishDrag(false);
    }
  }

  // ---- group tabs (only once there is more than one group: one group is just "your speed dials") ----
  function paintGroups(s: StartSnapshot): void {
    const tabs: HTMLElement[] = [];
    s.groups.forEach((group, index) => {
      const tab = el('button', 'sp-tab');
      tab.type = 'button';
      tab.setAttribute('role', 'tab');
      const selected = group.id === s.group;
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
      tab.dataset.id = String(group.id);
      tab.textContent = group.title;
      tab.dir = 'auto';
      tab.setAttribute('aria-label', `${group.title}, ${dialCountText(group.n)}`);
      tab.addEventListener('click', () => { if (!selected || s.folder !== s.group) cmd({ a: 'show', id: group.id }); });
      tab.addEventListener('keydown', (event) => {
        const all = Array.from(tablist.querySelectorAll<HTMLElement>('.sp-tab'));
        const at = all.indexOf(tab);
        let next = -1;
        if (event.key === 'ArrowRight') next = (at + 1) % all.length;
        else if (event.key === 'ArrowLeft') next = (at - 1 + all.length) % all.length;
        else if (event.key === 'Home') next = 0;
        else if (event.key === 'End') next = all.length - 1;
        else return;
        event.preventDefault();
        all[next]?.focus();
        cmd({ a: 'show', id: Number(all[next]?.dataset.id) });
      });
      tab.addEventListener('contextmenu', (event) => {
        if (s.guest) return;
        event.preventDefault();
        openGroupMenu(tab, group, index, s.groups.length);
      });
      tabs.push(tab);
    });
    tablist.replaceChildren(...tabs);
    groupTools.replaceChildren();
    const current = s.groups.findIndex((g) => g.id === s.group);
    if (current >= 0 && !s.guest) {
      const group = s.groups[current]!;
      const more = iconButton('more-horizontal', `Group options for ${group.title}`, () => openGroupMenu(more, group, current, s.groups.length), 'sp-group-more');
      groupTools.append(more);
    }
    groupBar.hidden = s.groups.length < 2;
  }

  function paintCrumbs(s: StartSnapshot): void {
    const inside = s.path.length > 1;
    crumbs.hidden = !inside;
    if (!inside) {
      crumbs.replaceChildren();
      return;
    }
    const parts: HTMLElement[] = [];
    const up = iconButton('chevron-left', 'Up one folder', () => { cmd({ a: 'show', id: s.path[s.path.length - 2]!.id }); }, 'sp-up');
    parts.push(up);
    s.path.forEach((crumb, index) => {
      if (index > 0) {
        const sep = el('span', 'sp-sep');
        sep.setAttribute('aria-hidden', 'true');
        sep.textContent = '/';
        parts.push(sep);
      }
      const last = index === s.path.length - 1;
      const button = el('button', 'sp-crumb');
      button.type = 'button';
      button.textContent = crumb.title;
      button.dir = 'auto';
      if (last) button.setAttribute('aria-current', 'page');
      else button.addEventListener('click', () => { cmd({ a: 'show', id: crumb.id }); });
      parts.push(button);
    });
    crumbs.replaceChildren(...parts);
  }

  function paintGrid(s: StartSnapshot): void {
    // With no group yet the add tile is still offered: the first site makes the group.
    const editable = !s.guest;
    const nodes: HTMLElement[] = [];
    const seen = new Set<number>();
    for (const dial of s.dials) {
      const node = dialNodes.get(dial.id) ?? dialNode(dial);
      dialNodes.set(dial.id, node);
      seen.add(dial.id);
      paintDial(node, dial, s.settings, editable);
      nodes.push(node.root);
    }
    for (const id of Array.from(dialNodes.keys())) if (!seen.has(id)) dialNodes.delete(id);
    if (editable && s.settings.addButton) {
      const add = el('li', 'sp-dial sp-add');
      add.setAttribute('role', 'listitem');
      const open = el('button', 'sp-open');
      open.type = 'button';
      open.setAttribute('aria-label', 'Add site');
      const face = el('span', 'sp-face is-add');
      face.setAttribute('aria-hidden', 'true');
      const chip = el('span', 'sp-chip');
      chip.append(icon('plus', { size: 20 }));
      face.append(chip);
      const title = el('span', 'sp-title');
      title.textContent = 'Add site';
      add.dataset.labels = s.settings.titles === 'never' ? 'hide' : 'show';
      open.append(face, title);
      open.addEventListener('click', () => openDialForm(null));
      open.addEventListener('keydown', (event) => gridKey(event, open));
      add.append(open);
      nodes.push(add);
    }
    grid.replaceChildren(...nodes);
    grid.hidden = nodes.length === 0;
    if (s.more) {
      const more = el('li', 'sp-more');
      more.textContent = 'Only the first 120 are shown.';
      grid.append(more);
    }
  }

  function layout(): void {
    if (!snap) return;
    const width = grid.clientWidth || column.clientWidth || 800;
    const s = snap.settings;
    // Never more columns than tiles, so a short row stays centred instead of leaving an empty column.
    const tiles = grid.querySelectorAll('.sp-dial').length;
    grid.style.setProperty('--sp-cols', String(Math.max(1, Math.min(columnsFor(width, s.columns, s.thumbSize), tiles || 1))));
    grid.style.setProperty('--sp-tile', `${tileWidth(s.thumbSize)}px`);
    const wcols = Math.max(1, Math.min(s.widgetColumns, Math.floor((widgetGrid.clientWidth || column.clientWidth || 800) / WIDGET_MIN)));
    widgetGrid.style.setProperty('--sp-wcols', String(wcols));
  }

  // ---- widgets ----
  const widgetNodes = new Map<number, WidgetNode>();

  function paintWidgets(s: StartSnapshot): void {
    widgetsSection.hidden = !s.settings.widgets;
    if (!s.settings.widgets) return;
    for (const cls of Array.from(root.classList)) if (cls.startsWith('is-bg-') || cls === 'is-regular' || cls === 'is-tall') root.classList.remove(cls);
    root.classList.add(...widgetClasses(s.settings.widgetBg, s.settings.widgetHeight));
    root.style.setProperty('--sp-widget-custom', s.settings.widgetColor);
    root.style.setProperty('--sp-widget-custom-fg', textOn(s.settings.widgetColor));
    const cards: HTMLElement[] = [];
    const seen = new Set<number>();
    for (const widget of s.widgets) {
      let node = widgetNodes.get(widget.id);
      if (!node) {
        const card = el('section', 'sp-widget');
        card.dataset.id = String(widget.id);
        node = { root: card, id: widget.id };
        widgetNodes.set(widget.id, node);
      }
      seen.add(widget.id);
      paintWidget(node, widget, s);
      cards.push(node.root);
    }
    for (const id of Array.from(widgetNodes.keys())) {
      if (seen.has(id)) continue;
      widgetNodes.delete(id);
      frames.delete(id);
      editors.delete(id);
      savedNotes.delete(id);
      if (configuring === id) configuring = null;
    }
    widgetGrid.replaceChildren(...cards);
    widgetGrid.hidden = cards.length === 0;
    addWidget.hidden = s.guest || s.widgets.length >= 12;
    widgetBar.hidden = addWidget.hidden;
  }

  /** What a card is called: its kind, or for a web page its site and for bookmarks its folder. */
  function widgetName(widget: Widget, s: StartSnapshot): string {
    if (widget.kind === 'webpage' && widget.url !== '') return hostOnly(widget.url);
    if (widget.kind === 'bookmarks' && widget.folder !== 0) return s.folders.find((f) => f.id === widget.folder)?.title ?? WIDGET_LABELS.bookmarks;
    return WIDGET_LABELS[widget.kind];
  }

  function widgetHead(widget: Widget, s: StartSnapshot): HTMLElement {
    const head = el('header', 'sp-widget-head');
    const title = el('h2', 'sp-widget-title');
    const glyph = el('span', 'sp-widget-glyph');
    glyph.append(icon(widgetIcon(widget.kind), { size: 14 }));
    const label = el('span', 'sp-widget-name');
    label.textContent = widgetName(widget, s);
    label.dir = 'auto';
    title.append(glyph, label);
    const tools = el('div', 'sp-widget-tools');
    const kindLabel = WIDGET_LABELS[widget.kind];
    if (!s.guest && (widget.kind === 'bookmarks' || widget.kind === 'webpage')) {
      const gear = iconButton('gear', `${kindLabel} settings`, () => {
        configuring = configuring === widget.id ? null : widget.id;
        paintWidgets(s);
        layout();
      }, 'sp-widget-tool');
      gear.setAttribute('aria-pressed', String(configuring === widget.id));
      tools.append(gear);
    }
    if (s.settings.widgetDrag && !s.guest) {
      const grip = el('button', 'bw-icon-btn sp-widget-tool sp-grip');
      grip.type = 'button';
      grip.setAttribute('aria-label', `Move ${kindLabel}. Press Alt and an arrow key.`);
      grip.setAttribute('data-tip', 'Drag to rearrange');
      grip.append(icon('grip'));
      grip.addEventListener('pointerdown', (event) => beginDrag(event, 'widget', widget.id, widgetNodes.get(widget.id)!.root));
      grip.addEventListener('keydown', (event) => {
        if (!event.altKey || (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight' && event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
        event.preventDefault();
        const ids = s.widgets.map((w) => w.id);
        const at = ids.indexOf(widget.id);
        const forward = event.key === 'ArrowRight' || event.key === 'ArrowDown';
        const over = ids[forward ? at + 1 : at - 1];
        if (over === undefined) return;
        const before = dropBefore(ids, widget.id, over, forward);
        if (before === undefined) return;
        cmd(before === null ? { a: 'wmove', id: widget.id } : { a: 'wmove', id: widget.id, before });
        window.setTimeout(() => widgetNodes.get(widget.id)?.root.querySelector<HTMLElement>('.sp-grip')?.focus({ preventScroll: true }), 60);
      });
      tools.append(grip);
    }
    if (!s.guest) {
      const remove = iconButton('close', `Remove ${kindLabel}`, () => { cmd({ a: 'wremove', id: widget.id }); }, 'sp-widget-tool');
      tools.append(remove);
    }
    head.append(title, tools);
    return head;
  }

  function paintWidget(node: WidgetNode, widget: Widget, s: StartSnapshot): void {
    node.root.dataset.kind = widget.kind;
    node.root.classList.toggle('is-configuring', configuring === widget.id);
    const body = el('div', 'sp-widget-body bw-scroll');
    const open = (url: string, event?: MouseEvent): void => { cmd(event?.ctrlKey || event?.shiftKey ? { a: 'openurl', url, on: true } : { a: 'openurl', url }); };
    switch (widget.kind) {
      case 'topsites': {
        if (widget.sites.length === 0) {
          body.append(emptyNote('clock', 'Sites you visit often will appear here.'));
          break;
        }
        const list = el('ul', 'sp-icons');
        for (const site of widget.sites) {
          const item = el('li');
          const button = el('button', 'sp-icon-link');
          button.type = 'button';
          const face = el('span', 'sp-face sp-icon-face');
          face.setAttribute('aria-hidden', 'true');
          paintFace(face, { kind: 'link', title: site.title, host: site.host });
          const name = el('span', 'sp-icon-name');
          const title = site.title.trim() === '' ? site.host : guardTitle(site.title);
          name.textContent = title;
          name.dir = 'auto';
          button.setAttribute('aria-label', `${title}, ${site.host}`);
          button.setAttribute('data-tip', site.host);
          button.append(face, name);
          button.addEventListener('click', (event) => open(site.url, event));
          item.append(button);
          list.append(item);
        }
        body.append(list);
        break;
      }
      case 'bookmarks': {
        if (configuring === widget.id && !s.guest) body.append(bookmarksConfig(widget, s));
        if (widget.links.length === 0) body.append(emptyNote('bookmark', 'No bookmarks in this folder.'));
        const tiles = widget.view === 'tiles';
        const list = el('ul', tiles ? 'sp-icons' : 'sp-list');
        for (const link of widget.links) {
          const item = el('li');
          const button = el('button', tiles ? 'sp-icon-link' : 'sp-row');
          button.type = 'button';
          const face = el('span', tiles ? 'sp-face sp-icon-face' : 'sp-face sp-row-face');
          face.setAttribute('aria-hidden', 'true');
          // Tiles show a chosen image when the bookmark has one; the list is titles and favicons.
          paintFace(face, tiles ? { kind: 'link', title: link.title, host: link.host, id: link.id, thumb: link.thumb } : { kind: 'link', title: link.title, host: link.host });
          const name = el('span', tiles ? 'sp-icon-name' : 'sp-row-title');
          name.textContent = guardTitle(link.title);
          name.dir = 'auto';
          if (tiles) button.append(face, name);
          else {
            const text = el('span', 'sp-row-text');
            const meta = el('span', 'sp-row-meta');
            meta.textContent = link.host;
            text.append(name, meta);
            button.append(face, text);
          }
          button.setAttribute('aria-label', `${guardTitle(link.title)}, ${link.host}`);
          button.addEventListener('click', (event) => open(link.url, event));
          item.append(button);
          if (configuring === widget.id && !s.guest && tiles) {
            // With the settings strip open, a tile can be given a picture of your own (or go back to its icon).
            const tools = el('div', 'sp-mini-tools');
            tools.append(iconButton('image', `Choose an image for ${link.title}`, () => { cmd({ a: 'thumb', id: link.id }); }, 'sp-tool'));
            if (link.thumb) tools.append(iconButton('reset', `Use the site icon for ${link.title}`, () => { cmd({ a: 'thumbclear', id: link.id }); }, 'sp-tool'));
            item.append(tools);
          }
          list.append(item);
        }
        if (widget.links.length > 0) body.append(list);
        break;
      }
      case 'notes': {
        let area = editors.get(widget.id);
        if (!area) {
          area = el('textarea', 'sp-note');
          area.maxLength = MAX_NOTE;
          area.placeholder = 'Write something…';
          area.setAttribute('aria-label', 'Note');
          area.spellcheck = true;
          editors.set(widget.id, area);
          const edited = area;
          edited.addEventListener('input', () => {
            savedNotes.delete(widget.id);
            const status = node.root.querySelector<HTMLElement>('.sp-note-status');
            if (status) status.textContent = '';
            clearTimeout(noteSaveTimer);
            noteSaveTimer = window.setTimeout(() => { cmd({ a: 'wnote', id: widget.id, text: edited.value }); }, NOTE_DEBOUNCE_MS);
          });
          edited.addEventListener('blur', () => {
            clearTimeout(noteSaveTimer);
            cmd({ a: 'wnote', id: widget.id, text: edited.value });
          });
        }
        if (document.activeElement !== area && area.value !== widget.text) area.value = widget.text;
        area.disabled = s.guest;
        const foot = el('div', 'sp-note-foot');
        const status = el('span', 'sp-note-status');
        status.setAttribute('role', 'status');
        status.textContent = savedNotes.has(widget.id) ? NOTE_SAVED : '';
        const save = textButton('Save to Notes', () => {
          const current = editors.get(widget.id);
          if (!current || current.value.trim() === '') return;
          cmd({ a: 'wnote', id: widget.id, text: current.value });
          cmd({ a: 'wsavenote', id: widget.id });
          savedNotes.add(widget.id);
          status.textContent = NOTE_SAVED;
        }, { icon: 'notes', subtle: true });
        save.classList.add('sp-note-save');
        save.disabled = s.guest;
        foot.append(status, save);
        body.append(area, foot);
        body.classList.add('is-note');
        break;
      }
      case 'date': {
        const time = el('p', 'sp-wclock-time');
        const day = el('p', 'sp-wclock-date');
        body.append(time, day);
        body.classList.add('is-clock');
        paintClock(body);
        break;
      }
      case 'privacy': {
        const lead = el('div', 'sp-stat-lead');
        const badge = el('span', 'sp-stat-badge');
        badge.setAttribute('aria-hidden', 'true');
        badge.append(icon('shield-check', { size: 20 }));
        const figure = el('div', 'sp-stat-figure');
        const big = el('p', 'sp-stat');
        big.textContent = widget.today.toLocaleString('en-GB');
        const caption = el('p', 'sp-stat-caption');
        caption.textContent = widget.today === 1 ? 'request blocked today' : 'requests blocked today';
        figure.append(big, caption);
        lead.append(badge, figure);
        const facts = el('dl', 'sp-facts');
        const fact = (name: string, value: string): void => {
          const wrap = el('div', 'sp-fact');
          const dt = el('dt');
          dt.textContent = name;
          const dd = el('dd');
          dd.textContent = value;
          wrap.append(dt, dd);
          facts.append(wrap);
        };
        fact('All time', blockedText(widget.total).replace(/ requests? blocked$/, ''));
        if (widget.topHost !== '') fact('Most blocked', `${widget.topHost} (${widget.topCount.toLocaleString('en-GB')})`);
        body.append(lead, facts);
        body.classList.add('is-stats');
        break;
      }
      case 'webpage': {
        if (configuring === widget.id || widget.url === '') {
          if (!s.guest) body.append(webpageConfig(widget));
        }
        if (widget.url === '') {
          frames.delete(widget.id);
          body.classList.add('is-setup');
          break;
        }
        if (activeSpoilerMode() !== 'off' && approvedFrames.get(widget.id) !== widget.url) {
          frames.delete(widget.id);
          const hold = el('div', 'sp-frame-paused');
          const message = el('p', 'sp-widget-note');
          message.textContent = 'Web page widget paused by spoiler protection';
          const load = textButton('Load', () => {
            approvedFrames.set(widget.id, widget.url);
            if (snap) paintWidgets(snap);
          }, { subtle: true });
          load.setAttribute('aria-label', `Load the web page widget (${widget.url === '' ? '' : hostOnly(widget.url)})`);
          hold.append(message, load);
          body.append(hold);
          break;
        }
        let entry = frames.get(widget.id);
        if (!entry || entry.url !== widget.url) {
          const frame = el('iframe', 'sp-frame');
          // The whole containment: scripts and forms run, but in a unique origin (no cookies or storage), cannot
          // navigate the start page, open windows or start downloads. Rust and the host policy back this up.
          frame.setAttribute('sandbox', 'allow-scripts allow-forms');
          frame.setAttribute('referrerpolicy', 'no-referrer');
          frame.setAttribute('allow', '');
          frame.setAttribute('loading', 'lazy');
          frame.setAttribute('aria-label', `Web page widget: ${widget.url}`);
          if (isWebAddress(widget.url)) frame.src = widget.url;
          entry = { url: widget.url, node: frame };
          frames.set(widget.id, entry);
        }
        body.classList.add('is-frame');
        body.append(entry.node);
        break;
      }
    }
    node.root.replaceChildren(widgetHead(widget, s), body);
  }

  function emptyNote(glyph: IconName, text: string): HTMLElement {
    const wrap = el('div', 'sp-widget-empty');
    const mark = el('span', 'sp-empty-glyph');
    mark.setAttribute('aria-hidden', 'true');
    mark.append(icon(glyph, { size: 18 }));
    const p = el('p', 'sp-widget-note');
    p.textContent = text;
    wrap.append(mark, p);
    return wrap;
  }

  function bookmarksConfig(widget: Extract<Widget, { kind: 'bookmarks' }>, s: StartSnapshot): HTMLElement {
    const strip = el('div', 'sp-config');
    const choices = [{ value: '0', label: 'All bookmarks' }, ...s.folders.map((f) => ({ value: String(f.id), label: f.title }))];
    const picker = selectField<string>({
      options: choices,
      value: String(widget.folder),
      label: 'Bookmarks folder',
      placeholder: 'Folder',
      onChange: (value) => { cmd({ a: 'wfolder', id: widget.id, parent: Number(value) }); },
    });
    const views: ReadonlyArray<[BookmarkView, string]> = [['list', 'List'], ['tiles', 'Tiles']];
    const seg = segmented('Show bookmarks as', views, widget.view, (value) => { if (widget.view !== value) cmd({ a: 'wview', id: widget.id, text: value }); });
    strip.append(picker.root, seg);
    return strip;
  }

  function webpageConfig(widget: Extract<Widget, { kind: 'webpage' }>): HTMLElement {
    const form = el('form', 'sp-config sp-url-form');
    if (widget.url === '') {
      const mark = el('span', 'sp-empty-glyph');
      mark.setAttribute('aria-hidden', 'true');
      mark.append(icon('globe', { size: 20 }));
      const lead = el('p', 'sp-widget-note');
      lead.textContent = 'Show any page in this card.';
      form.append(mark, lead);
    }
    const row = el('div', 'sp-url-row');
    const input = el('input', 'bw-input');
    input.type = 'text';
    input.value = widget.url;
    input.placeholder = 'example.com';
    input.maxLength = 2048;
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.setAttribute('aria-label', 'Web page address');
    const go = textButton('Show', () => {}, { primary: true });
    go.type = 'submit';
    row.append(input, go);
    const error = el('p', 'sp-error');
    error.setAttribute('role', 'alert');
    error.hidden = true;
    form.append(row, error);
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = input.value.trim();
      if (text === '') {
        cmd({ a: 'wurl', id: widget.id, url: '' });
        return;
      }
      const url = normaliseAddress(text);
      if (url === null) {
        error.textContent = 'Enter an http or https address.';
        error.hidden = false;
        return;
      }
      error.hidden = true;
      configuring = null;
      cmd({ a: 'wurl', id: widget.id, url });
    });
    return form;
  }

  /** A segmented control (`role=radiogroup`) that reports the chosen value. */
  function segmented<T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, value: T | null, onPick: (value: T) => void, disabled = false): HTMLElement {
    const seg = el('div', 'bw-segmented sp-seg');
    seg.setAttribute('role', 'radiogroup');
    seg.setAttribute('aria-label', label);
    for (const [key, text] of options) {
      const button = el('button');
      button.type = 'button';
      button.setAttribute('role', 'radio');
      button.setAttribute('aria-checked', String(value === key));
      button.dataset.value = key;
      button.textContent = text;
      button.disabled = disabled;
      button.addEventListener('click', () => onPick(key));
      seg.append(button);
    }
    return seg;
  }

  // ---- the clock (the page's own and every clock widget), redrawn on each new minute ----
  function paintClock(only?: HTMLElement): void {
    const now = clockParts(new Date());
    const targets = only ? [only] : [clock, ...Array.from(widgetGrid.querySelectorAll<HTMLElement>('.is-clock'))];
    for (const target of targets) {
      const time = target.querySelector<HTMLElement>('.sp-clock-time, .sp-wclock-time');
      const date = target.querySelector<HTMLElement>('.sp-clock-date, .sp-wclock-date');
      if (time && time.textContent !== now.time) time.textContent = now.time;
      if (date && date.textContent !== now.date) date.textContent = now.date;
    }
  }
  function tick(): void {
    clearTimeout(clockTimer);
    paintClock();
    clockTimer = window.setTimeout(tick, 60_000 - (Date.now() % 60_000) + 50);
  }

  // ---- the background ----
  const INK_KEYS = Object.keys(wallInk('dark'));
  function paintWallpaper(s: StartSnapshot): void {
    const chosen: Wallpaper = s.settings.wallpaper;
    let art = 'none';
    let tone: WallTone | null = null;
    if (chosen === 'custom') {
      if (wallImage && wallImage.stamp === s.wall && s.wall !== '') {
        art = `url("${wallImage.uri}")`;
        tone = wallImage.tone;
      }
    } else if (chosen !== 'none') {
      art = WALL_ART[chosen].art;
      tone = WALL_ART[chosen].tone;
    }
    root.dataset.wall = tone ? chosen : 'none';
    root.classList.toggle('has-wall', tone !== null);
    backdrop.style.setProperty('--sp-wall-art', art);
    if (tone) for (const [key, value] of Object.entries(wallInk(tone))) root.style.setProperty(key, value);
    else for (const key of INK_KEYS) root.style.removeProperty(key);
    root.style.setProperty('--sp-dim', String((tone ? s.settings.wallDim : 0) / 100));
  }

  function takeWallPart(part: Extract<StartMessage, { kind: 'wall' }>): void {
    if (wallImage?.stamp === part.stamp) return;
    if (!wallParts || wallParts.stamp !== part.stamp || wallParts.parts.length !== part.parts) {
      wallParts = { stamp: part.stamp, parts: new Array<string>(part.parts).fill('') };
    }
    wallParts.parts[part.part] = part.data;
    if (wallParts.parts.some((p) => p === '')) return;
    const stamp = wallParts.stamp;
    const uri = `data:image/jpeg;base64,${wallParts.parts.join('')}`;
    wallParts = null;
    const done = (tone: WallTone): void => {
      wallImage = { stamp, uri, tone };
      if (snap) {
        paintWallpaper(snap);
        if (!drawer.hidden) paintDrawer(snap);
      }
    };
    if (typeof Image !== 'function') { done('dark'); return; }
    // Whether the picture is mostly light or dark decides the colour of the text over it.
    const probe = new Image();
    probe.onload = () => {
      try {
        const canvas = document.createElement('canvas');
        canvas.width = 24;
        canvas.height = 16;
        const g = canvas.getContext('2d');
        if (!g) { done('dark'); return; }
        g.drawImage(probe, 0, 0, 24, 16);
        done(toneOf(g.getImageData(0, 0, 24, 16).data));
      } catch {
        done('dark');
      }
    };
    probe.onerror = () => done('dark');
    probe.src = uri;
  }

  // ---- the Customise panel ----
  let drawerReturn: HTMLElement | null = null;
  function openDrawer(): void {
    if (!snap) return;
    closeMenu();
    drawerReturn = document.activeElement instanceof HTMLElement ? document.activeElement : customise;
    drawer.hidden = false;
    root.classList.add('is-customising');
    customise.setAttribute('aria-expanded', 'true');
    paintDrawer(snap);
    drawer.querySelector<HTMLElement>('.sp-drawer-close')?.focus({ preventScroll: true });
  }
  function closeDrawer(restore = true): void {
    if (drawer.hidden) return;
    drawer.hidden = true;
    root.classList.remove('is-customising');
    customise.setAttribute('aria-expanded', 'false');
    if (restore && drawerReturn?.isConnected) drawerReturn.focus({ preventScroll: true });
    drawerReturn = null;
  }

  function switchRow(label: string, on: boolean, onChange: (on: boolean) => void, key: string): HTMLElement {
    const row = el('div', 'sp-drawer-row');
    const text = el('span', 'sp-drawer-label');
    text.id = `sp-dl-${key}`;
    text.textContent = label;
    const toggle = el('button', 'bw-toggle');
    toggle.type = 'button';
    toggle.setAttribute('role', 'switch');
    toggle.setAttribute('aria-checked', String(on));
    toggle.setAttribute('aria-labelledby', text.id);
    toggle.dataset.ctl = key;
    toggle.addEventListener('click', () => onChange(!on));
    row.append(text, toggle);
    return row;
  }

  function choiceRow(label: string, key: string, control: HTMLElement): HTMLElement {
    const row = el('div', 'sp-drawer-row is-stacked');
    const text = el('span', 'sp-drawer-label');
    text.textContent = label;
    control.dataset.ctl = key;
    row.append(text, control);
    return row;
  }

  function drawerSection(title: string, ...children: HTMLElement[]): HTMLElement {
    const section = el('section', 'sp-drawer-section');
    const h = el('h3', 'sp-drawer-h');
    h.textContent = title;
    section.append(h, ...children);
    return section;
  }

  function swatch(value: Wallpaper, label: string, checked: boolean, paint: (art: HTMLElement) => void, onPick: () => void): HTMLElement {
    const button = el('button', 'sp-swatch');
    button.type = 'button';
    button.setAttribute('role', 'radio');
    button.setAttribute('aria-checked', String(checked));
    button.setAttribute('aria-label', label);
    button.dataset.value = value;
    const art = el('span', 'sp-swatch-art');
    art.setAttribute('aria-hidden', 'true');
    paint(art);
    const name = el('span', 'sp-swatch-name');
    name.textContent = label;
    button.append(art, name);
    button.addEventListener('click', onPick);
    return button;
  }

  function paintDrawer(s: StartSnapshot): void {
    const focused = document.activeElement instanceof HTMLElement && drawer.contains(document.activeElement)
      ? (document.activeElement.closest<HTMLElement>('[data-ctl]')?.dataset.ctl ?? '') : '';
    const st = s.settings;
    const head = el('header', 'sp-drawer-head');
    const title = el('h2', 'sp-drawer-title');
    title.textContent = 'Customise';
    const close = iconButton('close', 'Close', () => closeDrawer(), 'sp-drawer-close');
    close.dataset.ctl = 'close';
    head.append(title, close);

    // Background
    const swatches = el('div', 'sp-swatches');
    swatches.setAttribute('role', 'radiogroup');
    swatches.setAttribute('aria-label', 'Background');
    swatches.dataset.ctl = 'wallpaper';
    swatches.append(swatch('none', 'Glass', st.wallpaper === 'none', (art) => { art.classList.add('is-glass'); }, () => { setting('start_wallpaper', 'none'); }));
    for (const [key, entry] of Object.entries(WALL_ART) as Array<[Exclude<Wallpaper, 'none' | 'custom'>, (typeof WALL_ART)[keyof typeof WALL_ART]]>) {
      swatches.append(swatch(key, entry.label, st.wallpaper === key, (art) => { art.style.setProperty('--sp-swatch-art', entry.art); }, () => { setting('start_wallpaper', key); }));
    }
    const hasPicture = s.wall !== '';
    swatches.append(swatch('custom', 'Your picture', st.wallpaper === 'custom', (art) => {
      if (wallImage && wallImage.stamp === s.wall && hasPicture) art.style.setProperty('--sp-swatch-art', `url("${wallImage.uri}")`);
      else {
        art.classList.add('is-upload');
        art.append(icon('image', { size: 18 }));
      }
    }, () => { if (hasPicture) setting('start_wallpaper', 'custom'); else cmd({ a: 'wallpaper' }); }));
    const pictureTools = el('div', 'sp-drawer-actions');
    const upload = textButton(hasPicture ? 'Change picture' : 'Upload a picture', () => { cmd({ a: 'wallpaper' }); }, { icon: 'image', subtle: true });
    upload.dataset.ctl = 'upload';
    pictureTools.append(upload);
    if (hasPicture) {
      const removePicture = textButton('Remove', () => { cmd({ a: 'wallclear' }); }, { icon: 'trash', subtle: true });
      removePicture.dataset.ctl = 'wallclear';
      pictureTools.append(removePicture);
    }
    const background = drawerSection('Background', swatches, pictureTools);
    if (st.wallpaper !== 'none') {
      background.append(choiceRow('Dim the picture', 'dim', segmented('Dim the picture', DIM_STEPS.map(([n, label]) => [String(n), label] as const), String(dimStep(st.wallDim)), (value) => { setting('start_wallpaper_dim', value); })));
    }

    // What the page shows
    const show = drawerSection('Show',
      switchRow('Clock', st.clock, (on) => { setting('start_clock', on); }, 'clock'),
      switchRow('Search box', st.search, (on) => { setting('start_search', on); }, 'search'),
      switchRow('Speed dials', st.dials, (on) => { setting('start_dials', on); }, 'dials'),
      switchRow('Widgets', st.widgets, (on) => { setting('start_widgets', on); }, 'widgets'),
    );

    const dials = drawerSection('Speed dials',
      choiceRow('Tile size', 'size', segmented('Tile size', SIZE_LABELS.map((label, i) => [String(i + 1), label] as const), String(st.thumbSize), (value) => { setting('start_thumb_size', value); }, !st.dials)),
      choiceRow('Site names', 'titles', segmented('Site names', TITLE_CHOICES, st.titles, (value) => { setting('start_titles', value); }, !st.dials)),
      switchRow('Add site tile', st.addButton, (on) => { setting('start_add_button', on); }, 'addButton'),
    );
    dials.classList.toggle('is-off', !st.dials);

    const addOne = textButton('Add widget', () => openWidgetMenu(addOne), { icon: 'plus', subtle: true });
    addOne.dataset.ctl = 'addwidget';
    addOne.disabled = s.widgets.length >= 12;
    const widgetsPart = drawerSection('Widgets',
      choiceRow('Card style', 'style', segmented('Card style', STYLE_CHOICES, (WIDGET_BGS as readonly WidgetBg[]).includes(st.widgetBg) && st.widgetBg !== 'custom' ? st.widgetBg : null, (value) => { setting('start_widget_bg', value); }, !st.widgets)),
      choiceRow('Columns', 'wcols', segmented('Columns', (['1', '2', '3', '4'] as const).map((n) => [n, n] as const), String(st.widgetColumns), (value) => { setting('start_widget_columns', value); }, !st.widgets)),
      choiceRow('Height', 'wheight', segmented('Height', [['regular', 'Regular'], ['tall', 'Tall']] as const, st.widgetHeight, (value) => { setting('start_widget_height', value); }, !st.widgets)),
    );
    if (st.widgets) {
      const actions = el('div', 'sp-drawer-actions');
      actions.append(addOne);
      widgetsPart.append(actions);
    }
    widgetsPart.classList.toggle('is-off', !st.widgets);

    const body = el('div', 'sp-drawer-body bw-scroll');
    body.append(background, show, dials, widgetsPart);
    const foot = el('footer', 'sp-drawer-foot');
    const more = textButton('All start page settings', () => { cmd({ a: 'settings' }); }, { icon: 'settings', subtle: true });
    more.dataset.ctl = 'more';
    foot.append(more);
    const scroll = drawer.querySelector('.sp-drawer-body')?.scrollTop ?? 0;
    drawer.replaceChildren(head, body, foot);
    body.scrollTop = scroll;
    if (focused) {
      const target = drawer.querySelector<HTMLElement>(`[data-ctl="${focused}"]`);
      const inner = target?.matches('button') ? target : target?.querySelector<HTMLElement>('[aria-checked="true"], button');
      inner?.focus({ preventScroll: true });
    }
  }

  // ---- applying a message ----
  function render(s: StartSnapshot): void {
    snap = s;
    root.classList.toggle('is-guest', s.guest);
    root.classList.toggle('has-widgets', s.settings.widgets);
    clock.hidden = !s.settings.clock;
    search.hidden = !s.settings.search;
    hero.hidden = !s.settings.clock && !s.settings.search;
    // Speed dials are opt-in (Settings > Start page): off, a new tab shows no tiles.
    dialsSection.hidden = !s.settings.dials || (s.guest && s.dials.length === 0);
    customise.hidden = s.guest;
    if (s.guest) closeDrawer(false);
    paintWallpaper(s);
    paintGroups(s);
    paintCrumbs(s);
    paintGrid(s);
    paintWidgets(s);
    paintClock();
    layout();
    if (!drawer.hidden) paintDrawer(s);
    if (s.ack === 'failed') say(s.error);
  }

  // Protection switched on: a frame that was loaded goes away, and every widget asks again.
  let lastMode = activeSpoilerMode();
  onSpoilerConfig(() => {
    const mode = activeSpoilerMode();
    const turnedOn = lastMode === 'off' && mode !== 'off';
    lastMode = mode;
    if (turnedOn) { approvedFrames.clear(); frames.clear(); }
    if (snap && snap.widgets.some((w) => w.kind === 'webpage')) paintWidgets(snap);
  });

  const observer = new ResizeObserver(() => layout());
  observer.observe(column);

  root.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape') return;
    if (menu) closeMenu();
    else if (!drawer.hidden && !modal) {
      event.preventDefault();
      closeDrawer();
    }
  });
  window.addEventListener('focus', () => {
    // A page left open overnight shows today.
    if (snap) paintClock();
  });

  return {
    root,
    show(msg: StartMessage): void {
      if (msg.kind === 'wall') {
        takeWallPart(msg);
        return;
      }
      if (msg.kind === 'thumbs') {
        for (const [id, uri] of Object.entries(msg.thumbs)) thumbs.set(Number(id), uri);
        if (snap) {
          for (const dial of snap.dials) {
            const node = dialNodes.get(dial.id);
            if (node && dial.thumb) paintFace(node.face, { kind: dial.kind, title: dial.title, host: dial.host, id: dial.id, thumb: dial.thumb });
          }
          // Tiles of a bookmarks widget show the same images.
          if (snap.settings.widgets && snap.widgets.some((w) => w.kind === 'bookmarks' && w.view === 'tiles')) paintWidgets(snap);
        }
        return;
      }
      // A chosen image that is gone (cleared, or its dial removed) leaves the cache with it.
      const keep = new Set(msg.snap.dials.filter((d) => d.thumb).map((d) => d.id));
      for (const widget of msg.snap.widgets) if (widget.kind === 'bookmarks') for (const link of widget.links) if (link.thumb) keep.add(link.id);
      for (const id of Array.from(thumbs.keys())) if (!keep.has(id)) thumbs.delete(id);
      if (msg.snap.wall === '') { wallImage = null; wallParts = null; }
      render(msg.snap);
      if (!entered) {
        // The page comes up with the short shared enter (a fade, reduced motion included) whenever a
        // blank tab shows it, not on every snapshot.
        entered = true;
        tick();
        column.classList.remove('bw-enter');
        void column.offsetWidth;
        column.classList.add('bw-enter');
      }
    },
    hidden(): void {
      closeMenu();
      closeModal(false);
      closeDrawer(false);
      clearTimeout(clockTimer);
      entered = false;
    },
  };
}
