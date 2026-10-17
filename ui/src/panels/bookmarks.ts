import { sendPanel, type Inbound, type SnapshotMsg, type PanelFields } from '../protocol';
import { el, icon } from '../dom';
import { bookmarkActionCommand, bookmarkMoveCommand, validateBookmarkSnapshot, type BookmarkItem, type BookmarkSnapshot } from './bookmarks-logic';
import { dropZone, type DropZone } from './kit-logic';
import { emptyState, enter, faviconTile, iconButton, moveFocus, panelHeader, searchField, textButton } from './kit';

function emit(command: ReturnType<typeof bookmarkActionCommand> | ReturnType<typeof bookmarkMoveCommand>): void {
  if (!command) return;
  const { v: _version, t, ...fields } = command;
  sendPanel(t, fields as PanelFields);
}

function field(label: string, value: string, type: 'text' | 'url' = 'text'): HTMLInputElement {
  const input = el('input', 'bw-input');
  input.type = type;
  input.value = value;
  input.placeholder = label;
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.setAttribute('aria-label', label);
  return input;
}

export function startBookmarksPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: BookmarkSnapshot | null = null;
  const expanded = new Set<number>();
  let seeded = false;
  let selected: number | null = null;
  let composing = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let issue = '';
  let draft: { item?: BookmarkItem; folder: boolean } | null = null;
  let dragId: number | null = null;
  let dropTarget: { id: number; zone: DropZone } | null = null;

  const header = panelHeader('Bookmarks', 'Close bookmarks');
  const search = searchField('Search bookmarks');
  const bar = el('div', 'pn-bar');
  bar.append(search.root);
  const list = el('div', 'pn-scroll bw-scroll');
  list.setAttribute('role', 'tree');
  list.setAttribute('aria-label', 'Bookmarks');
  const status = el('p', 'pn-status');
  status.setAttribute('role', 'status');
  const host = el('section', 'pn');
  host.append(header.root, bar, status, list);
  enter(host);
  root.replaceChildren(host);

  const items = (): BookmarkItem[] => snapshot?.items ?? [];
  const getItem = (id: number | null): BookmarkItem | undefined => (id === null ? undefined : items().find((item) => item.id === id));
  const children = (parent: number | null): BookmarkItem[] => items().filter((item) => item.parent === parent);
  const folderPath = (item: BookmarkItem): string => {
    if (item.folderPath) return item.folderPath.join(' / ');
    const path: string[] = [];
    const seen = new Set<number>();
    let parent = item.parent;
    while (parent !== null && !seen.has(parent)) {
      seen.add(parent);
      const folder = getItem(parent);
      if (!folder) break;
      path.unshift(folder.title);
      parent = folder.parent;
    }
    return path.join(' / ');
  };
  const isDescendant = (candidate: number, ancestor: number): boolean => {
    const seen = new Set<number>();
    let item = getItem(candidate);
    while (item && item.parent !== null && !seen.has(item.id)) {
      if (item.parent === ancestor) return true;
      seen.add(item.id);
      item = getItem(item.parent);
    }
    return false;
  };
  const depthOf = (item: BookmarkItem): number => {
    let depth = 0;
    const seen = new Set<number>();
    let parent = item.parent;
    while (parent !== null && !seen.has(parent)) { seen.add(parent); depth += 1; parent = getItem(parent)?.parent ?? null; }
    return depth;
  };
  const rowEls = (): HTMLElement[] => Array.from(list.querySelectorAll<HTMLElement>('.pn-row'));
  const rowFor = (id: number | null): HTMLElement | undefined => rowEls().find((node) => node.dataset.id === String(id));
  const searching = (): boolean => search.input.value.trim().length > 0;

  /** The folder a new bookmark or folder lands in: the selected folder, or the selection's parent. */
  const targetParent = (): number | null => {
    const item = getItem(selected);
    if (!item) return null;
    return item.kind === 'folder' ? item.id : item.parent;
  };
  const toggle = (id: number, open?: boolean): void => {
    const next = open ?? !expanded.has(id);
    if (next) expanded.add(id); else expanded.delete(id);
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
  const moveWithin = (item: BookmarkItem, delta: -1 | 1): void => {
    const siblings = children(item.parent);
    const index = siblings.findIndex((other) => other.id === item.id);
    if (delta < 0) {
      const before = siblings[index - 1];
      if (before) emit(bookmarkMoveCommand(item.id, item.parent, before.id));
    } else if (siblings[index + 1]) {
      emit(bookmarkMoveCommand(item.id, item.parent, siblings[index + 2]?.id ?? 0));
    }
  };

  const openEditor = (item?: BookmarkItem, folder = false): void => {
    draft = { item, folder: item ? item.kind === 'folder' : folder };
    render();
    list.querySelector<HTMLInputElement>('.pn-form input')?.focus();
  };
  const buildForm = (): HTMLElement | null => {
    if (!draft) return null;
    const { item, folder } = draft;
    const form = el('form', 'pn-form');
    const title = field(folder ? 'Folder name' : 'Title', item?.title ?? '');
    const url = !folder ? field('Address', item?.kind === 'link' ? item.url ?? '' : '', 'url') : null;
    const actions = el('div', 'pn-form-actions');
    const save = el('button', 'bw-button is-primary');
    save.type = 'submit';
    save.textContent = item ? 'Save' : folder ? 'Add folder' : 'Add bookmark';
    actions.append(textButton('Cancel', () => { draft = null; render(); }), save);
    form.append(title);
    if (url) form.append(url);
    form.append(actions);
    form.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); draft = null; render(); (item ? rowFor(item.id) : header.tools.querySelector<HTMLElement>('button'))?.focus(); }
    });
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = title.value.trim();
      if (!text) { title.setCustomValidity(folder ? 'Enter a folder name.' : 'Enter a title.'); title.reportValidity(); title.setCustomValidity(''); return; }
      if (item?.kind === 'link') {
        const nextUrl = url?.value.trim() ?? '';
        if (!nextUrl) { url?.setCustomValidity('Enter an address.'); url?.reportValidity(); url?.setCustomValidity(''); return; }
        emit(bookmarkActionCommand('update', item.id, { text, url: nextUrl }));
      } else if (item) emit(bookmarkActionCommand('rename', item.id, { text }));
      else if (folder) emit(bookmarkActionCommand('addfolder', undefined, { parent: targetParent(), text }));
      else {
        const nextUrl = url?.value.trim() ?? '';
        if (!nextUrl) { url?.setCustomValidity('Enter an address.'); url?.reportValidity(); url?.setCustomValidity(''); return; }
        emit(bookmarkActionCommand('addlink', undefined, { parent: targetParent(), text, url: nextUrl }));
        if (targetParent() !== null) expanded.add(targetParent() as number);
      }
      if (folder && !item && targetParent() !== null) expanded.add(targetParent() as number);
      draft = null;
      render();
    });
    return form;
  };

  const buildRow = (item: BookmarkItem, flat: boolean): HTMLElement => {
    const isFolder = item.kind === 'folder';
    const name = item.title || (isFolder ? 'Untitled folder' : item.url ?? 'Untitled bookmark');
    const row = el('div', 'bw-row bw-focus-inset pn-row');
    row.setAttribute('role', 'treeitem');
    row.dataset.id = String(item.id);
    const depth = flat ? 0 : depthOf(item);
    row.setAttribute('aria-level', String(depth + 1));
    row.style.setProperty('--pn-depth', String(depth));
    if (isFolder) row.setAttribute('aria-expanded', String(!flat && expanded.has(item.id)));
    row.draggable = !flat;

    const chevron = el('span', 'pn-chevron');
    chevron.setAttribute('aria-hidden', 'true');
    if (isFolder && !flat) {
      chevron.append(icon('chevron-right', { size: 12, fine: true }));
      chevron.addEventListener('click', (event) => { event.stopPropagation(); toggle(item.id); });
    }
    const lead = isFolder ? (() => { const holder = el('span', 'pn-lead'); holder.append(icon('folder')); return holder; })() : faviconTile(item.title || item.url || '');
    const title = el('span', 'pn-row-title');
    title.textContent = name;
    title.setAttribute('data-tip', isFolder ? name : `${name}\n${item.url ?? ''}`);
    const trail = el('span', 'pn-trail');
    const meta = el('span', 'pn-meta');
    meta.textContent = flat ? folderPath(item) : isFolder ? String(children(item.id).length) : hostOf(item.url ?? '');
    if (isFolder && !flat) meta.classList.add('pn-count'); // --- window and panels (thread T9) --- `panel_folder_counts` hides it (data-folder-counts)
    const actions = el('span', 'pn-actions');
    if (!isFolder) actions.append(iconButton('external', `Open ${name} in a new tab`, (event) => { event.stopPropagation(); emit(bookmarkActionCommand('newtab', item.id)); }));
    else if (children(item.id).some((child) => child.kind === 'link')) actions.append(iconButton('external', `Open everything in ${name} in new tabs`, (event) => { event.stopPropagation(); emit(bookmarkActionCommand('openall', item.id)); }));
    actions.append(
      iconButton('pencil', `Edit ${name}`, (event) => { event.stopPropagation(); openEditor(item); }),
      iconButton('trash', `Delete ${name}`, (event) => { event.stopPropagation(); emit(bookmarkActionCommand('delete', item.id)); }),
    );
    trail.append(meta, actions);
    row.append(chevron, lead, title, trail);
    row.addEventListener('click', () => {
      select(item.id);
      // "Open panel items with a single click": off, a bookmark opens with a double click instead.
      if (isFolder) { if (!flat) toggle(item.id); } else if (snapshot?.singleClick !== false) emit(bookmarkActionCommand('open', item.id));
    });
    if (!isFolder) {
      row.addEventListener('dblclick', () => {
        if (snapshot?.singleClick === false) emit(bookmarkActionCommand('open', item.id));
      });
    }

    // Drag to reorder or move into a folder.
    row.addEventListener('dragstart', (event) => {
      dragId = item.id;
      row.classList.add('is-dragging');
      if (event.dataTransfer) { event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', name); }
    });
    row.addEventListener('dragend', () => { dragId = null; clearDrop(); row.classList.remove('is-dragging'); });
    row.addEventListener('dragover', (event) => {
      if (dragId === null || flat || dragId === item.id || isDescendant(item.id, dragId)) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
      const box = row.getBoundingClientRect();
      const zone = dropZone(event.clientY - box.top, box.height, isFolder);
      if (dropTarget?.id !== item.id || dropTarget.zone !== zone) { clearDrop(); dropTarget = { id: item.id, zone }; row.classList.add(`is-drop-${zone}`); }
    });
    row.addEventListener('dragleave', (event) => { if (!row.contains(event.relatedTarget as Node | null)) { row.classList.remove('is-drop-before', 'is-drop-after', 'is-drop-into'); if (dropTarget?.id === item.id) dropTarget = null; } });
    row.addEventListener('drop', (event) => {
      event.preventDefault();
      const moving = dragId;
      const zone = dropTarget?.id === item.id ? dropTarget.zone : 'after';
      dragId = null;
      clearDrop();
      if (moving === null || moving === item.id || isDescendant(item.id, moving)) return;
      if (zone === 'into') { expanded.add(item.id); emit(bookmarkMoveCommand(moving, item.id, 0)); }
      else if (zone === 'before') emit(bookmarkMoveCommand(moving, item.parent, item.id));
      else {
        const next = children(item.parent).filter((other) => other.id !== moving);
        const at = next.findIndex((other) => other.id === item.id);
        emit(bookmarkMoveCommand(moving, item.parent, next[at + 1]?.id ?? 0));
      }
    });
    return row;
  };
  const clearDrop = (): void => {
    dropTarget = null;
    for (const node of Array.from(list.querySelectorAll('.is-drop-before, .is-drop-after, .is-drop-into'))) node.classList.remove('is-drop-before', 'is-drop-after', 'is-drop-into');
  };

  const render = (): void => {
    const focusedRow = list.contains(document.activeElement) && document.activeElement instanceof HTMLElement ? document.activeElement.closest<HTMLElement>('.pn-row')?.dataset.id : undefined;
    const keepFocus = list.querySelector('.pn-form')?.contains(document.activeElement) === true;
    status.textContent = issue;
    status.hidden = issue === '';
    const nodes: HTMLElement[] = [];
    const form = buildForm();
    if (form && !draft?.item) nodes.push(form);
    if (!snapshot) {
      if (!issue) status.hidden = false, status.textContent = 'Loading bookmarks…';
      list.replaceChildren(...nodes);
      return;
    }
    const flat = searching();
    const add = (item: BookmarkItem): void => {
      nodes.push(buildRow(item, flat));
      if (draft?.item?.id === item.id && form) nodes.push(form);
      if (!flat && item.kind === 'folder' && expanded.has(item.id)) {
        const kids = children(item.id);
        if (kids.length === 0) {
          const none = el('div', 'pn-none');
          none.style.setProperty('--pn-depth', String(depthOf(item) + 1));
          none.textContent = 'Empty folder';
          nodes.push(none);
        }
        for (const kid of kids) add(kid);
      }
    };
    if (flat) for (const item of snapshot.items) add(item);
    else for (const item of children(null)) add(item);
    if (nodes.length === 0 || (nodes.length === 1 && form && !draft?.item)) {
      nodes.push(flat ? emptyState('search', 'No bookmarks match.') : emptyState('bookmark', 'Bookmarks you add appear here.'));
    }
    list.replaceChildren(...nodes);
    if (selected !== null && !rowFor(selected)) selected = null;
    select(selected);
    if (!keepFocus && focusedRow) rowFor(Number(focusedRow))?.focus();
  };

  const sendSearch = (): void => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => sendPanel('bm', { a: 'search', text: search.input.value }), 120);
  };
  search.input.addEventListener('input', () => { if (!composing) sendSearch(); render(); });
  search.input.addEventListener('compositionstart', () => { composing = true; });
  search.input.addEventListener('compositionend', () => { composing = false; sendSearch(); render(); });
  header.tools.append(
    iconButton('plus', 'Add bookmark', () => openEditor(undefined, false)),
    iconButton('folder', 'New folder', () => openEditor(undefined, true)),
  );
  list.addEventListener('focusin', (event) => {
    const row = event.target instanceof HTMLElement ? event.target.closest<HTMLElement>('.pn-row') : null;
    if (row && row.dataset.id !== String(selected)) select(Number(row.dataset.id));
  });

  host.addEventListener('keydown', (event) => {
    if (event.isComposing || event.keyCode === 229) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (event.key === 'Escape' && target === search.input && search.input.value !== '') {
      event.preventDefault();
      search.input.value = '';
      sendPanel('bm', { a: 'search', text: '' });
      render();
      return;
    }
    if (target === search.input && event.key === 'ArrowDown') { event.preventDefault(); (rowFor(selected) ?? rowEls()[0])?.focus(); return; }
    const row = target?.closest<HTMLElement>('.pn-row') ?? null;
    if (!row || target !== row) return;
    const item = getItem(Number(row.dataset.id));
    if (!item) return;
    const all = rowEls();
    if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown') && !searching()) { event.preventDefault(); moveWithin(item, event.key === 'ArrowUp' ? -1 : 1); return; }
    if (event.key === 'ArrowUp' && row === all[0]) { event.preventDefault(); search.input.focus(); return; }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (item.kind === 'folder') { if (!searching()) toggle(item.id); } else emit(bookmarkActionCommand(event.ctrlKey ? 'newtab' : 'open', item.id));
    } else if (event.key === 'ArrowRight' && item.kind === 'folder' && !searching()) {
      event.preventDefault();
      if (!expanded.has(item.id)) toggle(item.id, true); else rowEls()[all.indexOf(row) + 1]?.focus();
    } else if (event.key === 'ArrowLeft' && !searching()) {
      event.preventDefault();
      if (item.kind === 'folder' && expanded.has(item.id)) toggle(item.id, false); else if (item.parent !== null) rowFor(item.parent)?.focus();
    } else if (event.key === 'Delete') { event.preventDefault(); emit(bookmarkActionCommand('delete', item.id)); }
    else if (event.key === 'F2') { event.preventDefault(); openEditor(item); }
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
    issue = '';
    snapshot = incoming;
    if (search.input.value !== incoming.query) search.input.value = incoming.query;
    if (!seeded) {
      seeded = true;
      let folder = getItem(incoming.currentFolder);
      const seen = new Set<number>();
      while (folder && !seen.has(folder.id)) { seen.add(folder.id); expanded.add(folder.id); folder = getItem(folder.parent); }
    }
    render();
  };
  render();
  return (message) => { if (message.t === 'snap') receive(message); };
}

function hostOf(url: string): string {
  try { return new URL(url).host || url; } catch { return url; }
}
