// The Bookmarks panel (?view=panel, p=bookmarks), modelled on Chrome's and Edge's bookmark side
// panels: two roots ("Bookmarks bar" and "Other bookmarks"), site icons, search across names,
// addresses and folders, one menu per row (right-click, the ⋯ button or Shift+F10), inline add and
// edit that keep the form open when Bowser refuses the change (and say why), and Undo after delete.
// Rust owns the store (src/chrome_host/panels/bookmarks.rs); every change goes over the `bm` wire.
import { sendPanel, type Inbound, type SnapshotMsg, type PanelFields } from '../protocol';
import { el, icon, type IconName } from '../dom';
import {
  BAR_ROOT, OTHER_WIRE, bookmarkActionCommand, bookmarkExpandCommand, bookmarkMoveCommand, childrenOf, countOf, hostOf, isWithin,
  normalizeAddress, validateBookmarkSnapshot, wireParent, type BookmarkCommand, type BookmarkItem, type BookmarkSnapshot,
} from './bookmarks-logic';
import { dropZone, type DropZone } from './kit-logic';
import { emptyState, enter, faviconTile, iconButton, moveFocus, panelHeader, searchField, textButton } from './kit';
import { guardRow, RevealedRows, rowKey, SHOW_LABEL } from '../spoiler-guard'; // --- spoiler guard ---
import { onSpoilerConfig } from '../spoiler-config';

function emit(command: BookmarkCommand | null): void {
  if (!command) return;
  const { v: _version, t, ...fields } = command;
  sendPanel(t, fields as PanelFields);
}

function field(label: string, value: string, type: 'text' | 'url' = 'text'): HTMLInputElement {
  const input = el('input', 'bw-input');
  input.type = type === 'url' ? 'text' : type;
  input.value = value;
  input.placeholder = label;
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.setAttribute('aria-label', label);
  if (type === 'url') { input.inputMode = 'url'; input.dir = 'ltr'; }
  return input;
}

/** A row of the panel's own menu. */
type MenuEntry = { label: string; icon?: IconName; hint?: string; danger?: boolean; disabled?: boolean; run: () => void } | 'separator';

/** What the inline form edits: an item, or a new link or folder inside `parent`. */
type Draft = { item?: BookmarkItem; folder: boolean; parent: number; pending: boolean; name?: string; url?: string };

export function startBookmarksPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: BookmarkSnapshot | null = null;
  /** Roots open in this panel; "Bookmarks bar" starts open, as in Edge's favourites flyout. */
  const openRoots = new Set<number>([BAR_ROOT]);
  /** Folders opened here before Rust's snapshot says so (it answers within a frame). */
  const opening = new Set<number>();
  /** Bookmarks the user revealed in this panel (session only, never stored). */
  const revealed = new RevealedRows(); // --- spoiler guard ---
  let selected: number | null = null;
  let composing = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let issue = '';
  let draft: Draft | null = null;
  let dragId: number | null = null;
  let dropTarget: { id: number; zone: DropZone } | null = null;
  let menu: { node: HTMLElement; returnTo: HTMLElement | null } | null = null;

  const header = panelHeader('Bookmarks', 'Close bookmarks');
  const search = searchField('Search bookmarks');
  const bar = el('div', 'pn-bar');
  bar.append(search.root);
  const list = el('div', 'pn-scroll bw-scroll bm-tree');
  list.setAttribute('role', 'tree');
  list.setAttribute('aria-label', 'Bookmarks');
  const status = el('p', 'pn-status bm-status');
  status.setAttribute('role', 'alert');
  const toast = el('div', 'bm-toast');
  toast.setAttribute('role', 'status');
  toast.hidden = true;
  const toastText = el('span', 'bm-toast-text');
  const undoButton = textButton('Undo', () => emit(bookmarkActionCommand('undo')), { subtle: true, aria: 'Undo the delete' });
  toast.append(toastText, undoButton);
  const host = el('section', 'pn bm-panel');
  host.append(header.root, bar, status, list, toast);
  enter(host);
  root.replaceChildren(host);

  const items = (): BookmarkItem[] => snapshot?.items ?? [];
  const getItem = (id: number | null): BookmarkItem | undefined => (id === null ? undefined : items().find((item) => item.id === id));
  const otherId = (): number => snapshot?.other ?? OTHER_WIRE;
  const isRoot = (id: number): boolean => id === BAR_ROOT || id === otherId();
  const rootTitle = (id: number): string => (id === BAR_ROOT ? 'Bookmarks bar' : 'Other bookmarks');
  const titleOf = (id: number): string => (isRoot(id) ? rootTitle(id) : getItem(id)?.title || 'Untitled folder');
  const searching = (): boolean => (snapshot?.query.trim() ?? '') !== '';
  const isOpen = (id: number): boolean => (isRoot(id) ? openRoots.has(id) : opening.has(id) || (snapshot?.expanded.includes(id) ?? false));
  const kidsOf = (id: number): BookmarkItem[] => (snapshot ? childrenOf(snapshot, id) : []);
  const rowEls = (): HTMLElement[] => Array.from(list.querySelectorAll<HTMLElement>('.pn-row'));
  const rowFor = (id: number | null): HTMLElement | undefined => rowEls().find((node) => node.dataset.id === String(id));
  const folderLabel = (item: BookmarkItem): string => {
    if (item.folderPath) {
      if (item.folderPath.length === 0) return 'Bookmarks bar';
      return item.folderPath.join(' › ');
    }
    return item.parent === null ? 'Bookmarks bar' : titleOf(item.parent);
  };

  /** The folder a new bookmark goes to: the selected folder, or the selection's own folder. */
  const targetFolder = (): number => {
    if (selected !== null && isRoot(selected)) return selected;
    const item = getItem(selected);
    if (!item) return BAR_ROOT;
    if (item.kind === 'folder') return item.id;
    if (item.parent === null) return BAR_ROOT;
    return item.parent;
  };

  const setOpen = (id: number, open: boolean): void => {
    if (isRoot(id)) {
      if (open) openRoots.add(id); else openRoots.delete(id);
      // "Other bookmarks" is a real folder once it exists: its links come with the snapshot.
      if (id !== BAR_ROOT) emit(bookmarkExpandCommand(id, open));
    } else {
      if (open) opening.add(id); else opening.delete(id);
      emit(bookmarkExpandCommand(id, open));
    }
  };
  const toggle = (id: number, open?: boolean): void => {
    setOpen(id, open ?? !isOpen(id));
    render();
    rowFor(id)?.focus();
  };
  const select = (id: number | null): void => {
    selected = id;
    const all = rowEls();
    const stop = all.find((node) => node.dataset.id === String(id)) ?? all[0];
    for (const node of all) {
      const on = node === stop;
      node.tabIndex = on ? 0 : -1;
      node.setAttribute('aria-selected', String(node.dataset.id === String(id)));
      for (const button of Array.from(node.querySelectorAll('button'))) button.tabIndex = on ? 0 : -1;
    }
  };
  const siblingsOf = (item: BookmarkItem): BookmarkItem[] => (item.parent === null ? kidsOf(BAR_ROOT) : kidsOf(item.parent));
  const moveWithin = (item: BookmarkItem, delta: -1 | 1): void => {
    const siblings = siblingsOf(item);
    const index = siblings.findIndex((other) => other.id === item.id);
    const parent = item.parent ?? BAR_ROOT;
    if (delta < 0) {
      const before = siblings[index - 1];
      if (before) emit(bookmarkMoveCommand(item.id, parent, before.id));
    } else if (siblings[index + 1]) {
      emit(bookmarkMoveCommand(item.id, parent, siblings[index + 2]?.id ?? 0));
    }
  };

  // ---- the inline form ----

  const openEditor = (item?: BookmarkItem, folder = false, parent = targetFolder()): void => {
    closeMenu(false);
    draft = { item, folder: item ? item.kind === 'folder' : folder, parent, pending: false };
    if (!item && !isOpen(parent)) setOpen(parent, true);
    issue = '';
    render();
    const input = list.querySelector<HTMLInputElement>('.pn-form input');
    input?.focus();
    input?.select();
  };
  const closeEditor = (focus: boolean): void => {
    const was = draft;
    draft = null;
    issue = '';
    render();
    if (!focus) return;
    (was?.item ? rowFor(was.item.id) : rowFor(was?.parent ?? null) ?? search.input)?.focus();
  };
  const buildForm = (): HTMLElement | null => {
    if (!draft) return null;
    const current = draft;
    const { item, folder } = current;
    const form = el('form', 'pn-form bm-form');
    form.noValidate = true;
    const heading = el('p', 'bm-form-title');
    heading.textContent = item ? (folder ? 'Rename folder' : 'Edit bookmark') : folder ? `New folder in ${titleOf(current.parent)}` : `New bookmark in ${titleOf(current.parent)}`;
    const title = field(folder ? 'Folder name' : 'Name', current.name ?? item?.title ?? '');
    const url = !folder ? field('Address', current.url ?? (item?.kind === 'link' ? item.url ?? '' : ''), 'url') : null;
    const problem = el('p', 'bm-form-error');
    problem.setAttribute('role', 'alert');
    problem.textContent = current.pending ? '' : issue;
    problem.hidden = problem.textContent === '';
    const actions = el('div', 'pn-form-actions');
    const save = el('button', 'bw-button is-primary');
    save.type = 'submit';
    save.textContent = item ? 'Save' : folder ? 'Add folder' : 'Add bookmark';
    save.disabled = current.pending;
    actions.append(textButton('Cancel', () => closeEditor(true), { subtle: true }), save);
    form.append(heading, title);
    if (url) form.append(url);
    form.append(problem, actions);
    const keep = (): void => { current.name = title.value; if (url) current.url = url.value; };
    title.addEventListener('input', keep);
    url?.addEventListener('input', keep);
    form.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeEditor(true); }
    });
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = title.value.trim();
      const say = (message: string, target: HTMLInputElement): void => { problem.textContent = message; problem.hidden = false; target.focus(); };
      if (!text) { say(folder ? 'Give the folder a name.' : 'Give the bookmark a name.', title); return; }
      const address = url ? normalizeAddress(url.value) : '';
      if (url && !address) { say('Enter the page’s address.', url); return; }
      if (url) { url.value = address; current.url = address; }
      current.name = text;
      current.pending = true;
      if (item?.kind === 'link') emit(bookmarkActionCommand('update', item.id, { text, url: address }));
      else if (item) emit(bookmarkActionCommand('rename', item.id, { text }));
      else if (folder) emit(bookmarkActionCommand('addfolder', undefined, { parent: wireParent(current.parent), text }));
      else emit(bookmarkActionCommand('addlink', undefined, { parent: wireParent(current.parent), text, url: address }));
      save.disabled = true;
    });
    return form;
  };

  // ---- the row menu ----

  const closeMenu = (restore = true): void => {
    if (!menu) return;
    const { node, returnTo } = menu;
    menu = null;
    node.remove();
    if (restore) returnTo?.focus();
  };
  const openMenu = (entries: MenuEntry[], at: { x: number; y: number }, returnTo: HTMLElement | null): void => {
    closeMenu(false);
    const node = el('div', 'bw-overlay bw-menu bm-menu');
    node.setAttribute('role', 'menu');
    const buttons: HTMLButtonElement[] = [];
    for (const entry of entries) {
      if (entry === 'separator') {
        const line = el('div', 'bw-separator');
        line.setAttribute('role', 'separator');
        node.append(line);
        continue;
      }
      const button = el('button', `bw-menu-item${entry.danger ? ' is-danger' : ''}`);
      button.type = 'button';
      button.setAttribute('role', 'menuitem');
      button.tabIndex = -1;
      button.disabled = entry.disabled === true;
      if (entry.icon) button.append(icon(entry.icon));
      else button.append(el('span', 'bm-menu-gap'));
      const label = el('span', 'bw-menu-label');
      label.textContent = entry.label;
      button.append(label);
      if (entry.hint) {
        const hint = el('span', 'bw-menu-accel');
        hint.textContent = entry.hint;
        button.append(hint);
      }
      button.addEventListener('click', () => { closeMenu(false); entry.run(); });
      buttons.push(button);
      node.append(button);
    }
    node.addEventListener('keydown', (event) => {
      const enabled = buttons.filter((button) => !button.disabled);
      const index = enabled.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); event.stopPropagation(); closeMenu(); }
      else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const step = event.key === 'ArrowDown' ? 1 : -1;
        enabled[(index + step + enabled.length) % enabled.length]?.focus();
      } else if (event.key === 'Home' || event.key === 'End') { event.preventDefault(); (event.key === 'Home' ? enabled[0] : enabled[enabled.length - 1])?.focus(); }
    });
    host.append(node);
    // Inside the panel, below and right of the point, flipped where it would leave the panel.
    const box = node.getBoundingClientRect();
    const room = host.getBoundingClientRect();
    const left = Math.max(4, Math.min(at.x, room.width - box.width - 4));
    const top = at.y + box.height + 4 > room.height ? Math.max(4, at.y - box.height) : at.y;
    node.style.setProperty('--bm-menu-x', `${left}px`);
    node.style.setProperty('--bm-menu-y', `${top}px`);
    menu = { node, returnTo };
    buttons.find((button) => !button.disabled)?.focus();
  };
  document.addEventListener('pointerdown', (event) => {
    if (menu && !(event.target instanceof Node && menu.node.contains(event.target))) closeMenu(false);
  }, true);
  window.addEventListener('blur', () => closeMenu(false));

  const folderEntries = (id: number): MenuEntry[] => {
    const kids = kidsOf(id);
    const links = kids.filter((kid) => kid.kind === 'link').length;
    const entries: MenuEntry[] = [];
    // The bar's own root has no id to open from; every real folder (Other bookmarks too) has.
    if (getItem(id)) entries.push({ label: links > 0 ? `Open all (${links})` : 'Open all', icon: 'external', disabled: countOf(snapshot!, id) === 0, run: () => emit(bookmarkActionCommand('openall', id)) });
    if (snapshot?.canAdd) entries.push({ label: 'Add this page here', icon: 'bookmark', run: () => emit(bookmarkActionCommand('addcurrent', undefined, { parent: wireParent(id) })) });
    entries.push(
      { label: 'Add bookmark…', icon: 'plus', run: () => openEditor(undefined, false, id) },
      { label: 'New folder…', icon: 'folder', run: () => openEditor(undefined, true, id) },
      { label: 'Sort by name', icon: 'menu', disabled: countOf(snapshot!, id) < 2, run: () => emit(bookmarkActionCommand('sort', undefined, { parent: wireParent(id) })) },
    );
    const item = getItem(id);
    if (item && !isRoot(id)) {
      entries.push('separator',
        { label: 'Rename…', icon: 'edit', hint: 'F2', run: () => openEditor(item) },
        { label: 'Delete', icon: 'trash', hint: 'Del', danger: true, run: () => emit(bookmarkActionCommand('delete', id)) });
    }
    return entries;
  };
  const linkEntries = (item: BookmarkItem): MenuEntry[] => [
    { label: 'Open', icon: 'enter', run: () => emit(bookmarkActionCommand('open', item.id)) },
    { label: 'Open in new tab', icon: 'external', hint: 'Ctrl+Enter', run: () => emit(bookmarkActionCommand('newtab', item.id)) },
    { label: 'Copy link', icon: 'copy', run: () => emit(bookmarkActionCommand('copy', item.id)) },
    'separator',
    { label: 'Edit…', icon: 'edit', hint: 'F2', run: () => openEditor(item) },
    { label: 'Delete', icon: 'trash', hint: 'Del', danger: true, run: () => emit(bookmarkActionCommand('delete', item.id)) },
  ];
  const menuFor = (id: number, at: { x: number; y: number }, returnTo: HTMLElement | null): void => {
    if (!snapshot) return;
    const item = getItem(id);
    openMenu(isRoot(id) || item?.kind === 'folder' ? folderEntries(id) : item ? linkEntries(item) : [], at, returnTo);
  };
  const menuBelow = (anchor: HTMLElement, id: number): void => {
    const box = anchor.getBoundingClientRect();
    const room = host.getBoundingClientRect();
    menuFor(id, { x: box.right - room.left - 200, y: box.bottom - room.top + 2 }, anchor.closest<HTMLElement>('.pn-row') ?? anchor);
  };

  // ---- rows ----

  const leadFor = (item: BookmarkItem | null, id: number, masked: boolean): HTMLElement => {
    if (id === BAR_ROOT || id === otherId() || item?.kind === 'folder') {
      const holder = el('span', 'pn-lead');
      holder.append(icon(id === BAR_ROOT ? 'bookmark' : isOpen(id) && !searching() ? 'folder-open' : 'folder'));
      return holder;
    }
    const url = item?.url ?? '';
    const site = item?.host || hostOf(url);
    return faviconTile(masked ? site : item?.title || site, snapshot?.favs[item?.host ?? ''] ?? undefined);
  };

  const buildRow = (id: number, flat: boolean, depth: number): HTMLElement => {
    const item = isRoot(id) ? null : getItem(id) ?? null;
    const isFolder = item === null || item.kind === 'folder';
    // A bookmark whose title or address reads as a spoiler shows the mask and the host (src/spoiler-guard.ts).
    const key = item ? rowKey(item.id, item.title, item.url ?? '') : '';
    const guarded = item && !isFolder ? guardRow({ title: item.title, url: item.url ?? '' }, revealed.has(key)) : { masked: false, title: '' };
    const name = item === null ? rootTitle(id) : guarded.masked ? guarded.title : item.title || (isFolder ? 'Untitled folder' : hostOf(item.url ?? '') || 'Untitled bookmark');
    const row = el('div', `bw-row bw-focus-inset pn-row bm-row${item === null ? ' is-root' : ''}`);
    row.setAttribute('role', 'treeitem');
    row.dataset.id = String(id);
    row.setAttribute('aria-level', String(depth + 1));
    row.style.setProperty('--pn-depth', String(depth));
    if (isFolder && !flat) row.setAttribute('aria-expanded', String(isOpen(id)));
    row.draggable = !flat && item !== null;

    const chevron = el('span', 'pn-chevron');
    chevron.setAttribute('aria-hidden', 'true');
    if (isFolder && !flat) {
      chevron.append(icon('chevron-right', { size: 12, fine: true }));
      chevron.addEventListener('click', (event) => { event.stopPropagation(); toggle(id); });
    }
    const title = el('span', 'pn-row-title');
    title.textContent = name;
    title.dir = 'auto';
    const url = item?.url ?? '';
    title.setAttribute('data-tip', isFolder ? name : guarded.masked ? hostOf(url) : `${name}\n${url}`);
    if (guarded.masked) row.dataset.spoiler = 'hidden';
    const trail = el('span', 'pn-trail');
    const meta = el('span', 'pn-meta');
    if (flat) meta.textContent = item ? folderLabel(item) : '';
    else if (isFolder) { meta.textContent = String(countOf(snapshot!, id)); meta.classList.add('pn-count'); } // --- window and panels (thread T9) --- `panel_folder_counts` hides it (data-folder-counts)
    else meta.textContent = hostOf(url);
    const actions = el('span', 'pn-actions');
    if (guarded.masked) actions.append(textButton(SHOW_LABEL, (event) => { event.stopPropagation(); revealed.add(key); render(); }, { subtle: true, aria: 'Show this hidden bookmark' }));
    if (!isFolder && item) actions.append(iconButton('external', `Open ${name} in a new tab`, (event) => { event.stopPropagation(); emit(bookmarkActionCommand('newtab', item.id)); }));
    const more = iconButton('more', `More for ${name}`, (event) => { event.stopPropagation(); menuBelow(more, id); });
    more.setAttribute('aria-haspopup', 'menu');
    actions.append(more);
    trail.append(meta, actions);
    row.append(chevron, leadFor(item, id, guarded.masked), title, trail);
    row.addEventListener('click', (event) => {
      select(id);
      if (isFolder) { if (!flat) toggle(id); return; }
      if (!item) return;
      // "Open panel items with a single click": off, a bookmark opens with a double click instead.
      if (snapshot?.singleClick !== false) emit(bookmarkActionCommand(event.ctrlKey || event.metaKey ? 'newtab' : 'open', item.id));
    });
    row.addEventListener('auxclick', (event) => {
      if (event.button === 1 && item && !isFolder) { event.preventDefault(); emit(bookmarkActionCommand('newtab', item.id)); }
    });
    row.addEventListener('dblclick', () => {
      if (item && !isFolder && snapshot?.singleClick === false) emit(bookmarkActionCommand('open', item.id));
    });
    row.addEventListener('contextmenu', (event) => {
      event.preventDefault();
      select(id);
      const room = host.getBoundingClientRect();
      menuFor(id, { x: event.clientX - room.left, y: event.clientY - room.top }, row);
    });

    // Drag to reorder, or into a folder (or a root).
    row.addEventListener('dragstart', (event) => {
      if (!item) { event.preventDefault(); return; }
      dragId = item.id;
      row.classList.add('is-dragging');
      if (event.dataTransfer) { event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', item.url ?? name); }
    });
    row.addEventListener('dragend', () => { dragId = null; clearDrop(); row.classList.remove('is-dragging'); });
    row.addEventListener('dragover', (event) => {
      if (dragId === null || flat || dragId === id || (snapshot && item && isWithin(snapshot, id, dragId))) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
      const box = row.getBoundingClientRect();
      const zone: DropZone = item === null ? 'into' : dropZone(event.clientY - box.top, box.height, isFolder);
      if (dropTarget?.id !== id || dropTarget.zone !== zone) { clearDrop(); dropTarget = { id, zone }; row.classList.add(`is-drop-${zone}`); }
    });
    row.addEventListener('dragleave', (event) => { if (!row.contains(event.relatedTarget as Node | null)) { row.classList.remove('is-drop-before', 'is-drop-after', 'is-drop-into'); if (dropTarget?.id === id) dropTarget = null; } });
    row.addEventListener('drop', (event) => {
      event.preventDefault();
      const moving = dragId;
      const zone = dropTarget?.id === id ? dropTarget.zone : 'after';
      dragId = null;
      clearDrop();
      if (moving === null || moving === id || (snapshot && isWithin(snapshot, id, moving))) return;
      if (zone === 'into' || !item) { setOpen(id, true); emit(bookmarkMoveCommand(moving, id, 0)); return; }
      const parent = item.parent ?? BAR_ROOT;
      if (zone === 'before') emit(bookmarkMoveCommand(moving, parent, item.id));
      else {
        const next = siblingsOf(item).filter((other) => other.id !== moving);
        const at = next.findIndex((other) => other.id === item.id);
        emit(bookmarkMoveCommand(moving, parent, next[at + 1]?.id ?? 0));
      }
    });
    return row;
  };
  const clearDrop = (): void => {
    dropTarget = null;
    for (const node of Array.from(list.querySelectorAll('.is-drop-before, .is-drop-after, .is-drop-into'))) node.classList.remove('is-drop-before', 'is-drop-after', 'is-drop-into');
  };
  const note = (text: string, depth: number, className = ''): HTMLElement => {
    const node = el('div', `pn-none ${className}`.trim());
    node.style.setProperty('--pn-depth', String(depth));
    node.textContent = text;
    return node;
  };

  const render = (): void => {
    const focusedRow = list.contains(document.activeElement) && document.activeElement instanceof HTMLElement ? document.activeElement.closest<HTMLElement>('.pn-row')?.dataset.id : undefined;
    const keepFocus = list.querySelector('.pn-form')?.contains(document.activeElement) === true;
    const message = draft ? '' : issue;
    status.textContent = message;
    status.hidden = message === '';
    toast.hidden = !snapshot || snapshot.undo === '';
    toastText.textContent = snapshot?.undo ?? '';
    const nodes: HTMLElement[] = [];
    const form = buildForm();
    if (!snapshot) {
      status.hidden = false;
      status.textContent = issue || 'Loading bookmarks…';
      list.replaceChildren();
      return;
    }
    const flat = searching();
    if (flat) {
      const found = snapshot.items;
      if (found.length === 0) nodes.push(emptyState('search', `No bookmarks match “${snapshot.query.trim()}”.`));
      else {
        const count = el('p', 'bm-count');
        count.textContent = snapshot.total > found.length ? `Showing ${found.length} of ${snapshot.total} matches` : `${snapshot.total || found.length} ${(snapshot.total || found.length) === 1 ? 'match' : 'matches'}`;
        nodes.push(count);
      }
      for (const item of found) {
        nodes.push(buildRow(item.id, true, 0));
        if (draft?.item?.id === item.id && form) nodes.push(form);
      }
    } else {
      const add = (id: number, depth: number): void => {
        nodes.push(buildRow(id, false, depth));
        if (draft?.item?.id === id && form) nodes.push(form);
        const folder = isRoot(id) || getItem(id)?.kind === 'folder';
        if (!folder || !isOpen(id)) return;
        if (draft && !draft.item && draft.parent === id && form) nodes.push(form);
        const kids = kidsOf(id);
        for (const kid of kids) add(kid.id, depth + 1);
        const total = countOf(snapshot!, id);
        if (total === 0 && !(draft && !draft.item && draft.parent === id)) nodes.push(note(id === BAR_ROOT ? 'Bookmarks you add to the bar appear here.' : 'Empty folder', depth + 1));
        else if (kids.length < total) nodes.push(note(opening.has(id) && !snapshot!.expanded.includes(id) ? 'Loading…' : `${total - kids.length} more not shown here. Search to find them.`, depth + 1));
      };
      add(BAR_ROOT, 0);
      add(otherId(), 0);
    }
    list.replaceChildren(...nodes);
    if (selected !== null && !rowFor(selected)) selected = null;
    select(selected);
    if (!keepFocus && focusedRow) rowFor(Number(focusedRow))?.focus();
  };

  const sendSearch = (): void => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => sendPanel('bm', { a: 'search', text: search.input.value.slice(0, 200) }), 90);
  };
  search.input.addEventListener('input', () => { if (!composing) sendSearch(); });
  search.input.addEventListener('compositionstart', () => { composing = true; });
  search.input.addEventListener('compositionend', () => { composing = false; sendSearch(); });
  const addPage = iconButton('bookmark', 'Bookmark this page', () => emit(bookmarkActionCommand('addcurrent', undefined, { parent: wireParent(targetFolder()) })));
  const addFolder = iconButton('folder', 'New folder', () => openEditor(undefined, true));
  const addLink = iconButton('plus', 'Add bookmark', () => openEditor(undefined, false));
  header.tools.append(addPage, addLink, addFolder);
  list.addEventListener('focusin', (event) => {
    const row = event.target instanceof HTMLElement ? event.target.closest<HTMLElement>('.pn-row') : null;
    if (row && row.dataset.id !== String(selected)) select(Number(row.dataset.id));
  });

  host.addEventListener('keydown', (event) => {
    if (event.isComposing || event.keyCode === 229) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (menu?.node.contains(target)) return;
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === 'z' && !(target instanceof HTMLInputElement) && snapshot?.undo) {
      event.preventDefault();
      emit(bookmarkActionCommand('undo'));
      return;
    }
    if (event.key === 'Escape' && target === search.input && search.input.value !== '') {
      event.preventDefault();
      search.input.value = '';
      sendPanel('bm', { a: 'search', text: '' });
      return;
    }
    if (target === search.input && event.key === 'ArrowDown') { event.preventDefault(); (rowFor(selected) ?? rowEls()[0])?.focus(); return; }
    const row = target?.closest<HTMLElement>('.pn-row') ?? null;
    if (!row || target !== row) return;
    const id = Number(row.dataset.id);
    const item = getItem(id);
    const folder = isRoot(id) || item?.kind === 'folder';
    const all = rowEls();
    if ((event.shiftKey && event.key === 'F10') || event.key === 'ContextMenu') { event.preventDefault(); menuBelow(row, id); return; }
    if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown') && !searching() && item) { event.preventDefault(); moveWithin(item, event.key === 'ArrowUp' ? -1 : 1); return; }
    if (event.key === 'ArrowUp' && row === all[0]) { event.preventDefault(); search.input.focus(); return; }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (folder) { if (!searching()) toggle(id); } else if (item) emit(bookmarkActionCommand(event.ctrlKey ? 'newtab' : 'open', item.id));
    } else if (event.key === 'ArrowRight' && folder && !searching()) {
      event.preventDefault();
      if (!isOpen(id)) toggle(id, true); else all[all.indexOf(row) + 1]?.focus();
    } else if (event.key === 'ArrowLeft' && !searching()) {
      event.preventDefault();
      if (folder && isOpen(id)) toggle(id, false);
      else if (item) rowFor(item.parent === null ? BAR_ROOT : item.parent)?.focus();
    } else if (event.key === 'Delete' && item && !isRoot(id)) { event.preventDefault(); emit(bookmarkActionCommand('delete', item.id)); }
    else if (event.key === 'F2' && item && !isRoot(id)) { event.preventDefault(); openEditor(item); }
    else moveFocus(list, '.pn-row', event);
  });

  const receive = (message: SnapshotMsg): void => {
    if (message.p !== 'bookmarks') return;
    const incoming = validateBookmarkSnapshot(message.d);
    if (!incoming) {
      issue = 'Bookmarks could not be loaded. The response was invalid.';
      snapshot = null;
      render();
      return;
    }
    snapshot = incoming;
    for (const id of Array.from(opening)) if (incoming.expanded.includes(id)) opening.delete(id);
    // "Other bookmarks" made by this change (it is a real folder now): keep it open if it was.
    if (openRoots.has(OTHER_WIRE) && incoming.other !== OTHER_WIRE) { openRoots.delete(OTHER_WIRE); openRoots.add(incoming.other); }
    if (incoming.other !== OTHER_WIRE && incoming.expanded.includes(incoming.other)) openRoots.add(incoming.other);
    issue = incoming.error;
    // A form whose change went through closes; one Bowser refused stays open and says why.
    let refocus: number | null = null;
    if (draft?.pending) {
      if (incoming.error === '') { refocus = draft.item?.id ?? draft.parent; draft = null; }
      else draft.pending = false;
    }
    if (search.input.value.trim() === '' && incoming.query !== '' && document.activeElement !== search.input) search.input.value = incoming.query;
    addPage.disabled = !incoming.canAdd;
    render();
    if (refocus !== null) rowFor(refocus)?.focus();
  };
  render();
  onSpoilerConfig(render);
  return (message) => { if (message.t === 'snap') receive(message); };
}
