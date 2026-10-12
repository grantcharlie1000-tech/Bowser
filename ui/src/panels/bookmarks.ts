import { sendPanel, type Inbound, type SnapshotMsg, type PanelFields } from '../protocol';
import { bookmarkActionCommand, bookmarkMoveCommand, validateBookmarkSnapshot, type BookmarkItem, type BookmarkSnapshot } from './bookmarks-logic';

function emit(command: ReturnType<typeof bookmarkActionCommand> | ReturnType<typeof bookmarkMoveCommand>): void {
  if (!command) return;
  const { v: _version, t, ...fields } = command;
  sendPanel(t, fields as PanelFields);
}

function button(label: string, action: () => void, ariaLabel = label): HTMLButtonElement {
  const result = document.createElement('button');
  result.type = 'button';
  result.textContent = label;
  result.setAttribute('aria-label', ariaLabel);
  result.addEventListener('click', action);
  return result;
}

function input(label: string, value: string, type: 'text' | 'url' = 'text'): HTMLInputElement {
  const result = document.createElement('input');
  result.type = type;
  result.value = value;
  result.setAttribute('aria-label', label);
  result.autocomplete = 'off';
  return result;
}

export function startBookmarksPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: BookmarkSnapshot | null = null;
  let localFolder: number | null = null;
  let localNavigation = false;
  let composing = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let issue = '';

  const host = document.createElement('section');
  host.className = 'bookmarks-panel';
  const search = input('Search bookmarks', '', 'text');
  search.placeholder = 'Search bookmarks';
  search.type = 'search';
  const toolbar = document.createElement('div');
  toolbar.className = 'bookmarks-toolbar';
  const crumb = document.createElement('div');
  crumb.className = 'bookmarks-location';
  crumb.setAttribute('aria-live', 'polite');
  const list = document.createElement('div');
  list.className = 'bookmarks-list';
  list.setAttribute('role', 'list');
  const status = document.createElement('p');
  status.className = 'bookmarks-status';
  status.setAttribute('role', 'status');
  host.append(search, toolbar, crumb, status, list);
  root.replaceChildren(host);

  const sendSearch = (): void => { sendPanel('bm', { a: 'search', text: search.value }); }
  const folders = (): BookmarkItem[] => snapshot?.items.filter((item) => item.kind === 'folder') ?? [];
  const getItem = (id: number): BookmarkItem | undefined => snapshot?.items.find((item) => item.id === id);
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

  const openDraft = (item?: BookmarkItem): void => {
    const form = document.createElement('form');
    form.className = 'bookmarks-editor';
    const title = input(item ? 'Bookmark title' : 'Bookmark title', item?.title ?? '');
    const url = !item || item.kind === 'link' ? input('Bookmark URL', item?.kind === 'link' ? item.url ?? '' : '', 'url') : null;
    const save = document.createElement('button');
    save.type = 'submit';
    save.textContent = item ? 'Save' : 'Add bookmark';
    const cancel = button('Cancel', render);
    form.append(title);
    if (url) form.append(url);
    form.append(save, cancel);
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = title.value.trim();
      if (!text) { title.setCustomValidity('Enter a title.'); title.reportValidity(); return; }
      if (item?.kind === 'link') {
        const nextUrl = url?.value.trim() ?? '';
        if (!nextUrl) { url?.setCustomValidity('Enter a URL.'); url?.reportValidity(); return; }
        emit(bookmarkActionCommand('update', item.id, { text, url: nextUrl }));
      } else if (item) emit(bookmarkActionCommand('rename', item.id, { text }));
      else emit(bookmarkActionCommand('addlink', undefined, { parent: localFolder, text, url: url?.value.trim() ?? '' }));
      render();
    });
    list.prepend(form);
    title.focus();
  };

  const openFolderDraft = (): void => {
    const form = document.createElement('form');
    form.className = 'bookmarks-editor';
    const title = input('Folder name', '');
    const save = document.createElement('button');
    save.type = 'submit';
    save.textContent = 'Add folder';
    form.append(title, save, button('Cancel', render));
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = title.value.trim();
      if (!text) { title.setCustomValidity('Enter a folder name.'); title.reportValidity(); return; }
      emit(bookmarkActionCommand('addfolder', undefined, { parent: localFolder, text }));
      render();
    });
    list.prepend(form);
    title.focus();
  };

  const render = (): void => {
    toolbar.replaceChildren(button('New folder', openFolderDraft), button('Add bookmark', () => openDraft()));
    crumb.replaceChildren();
    list.replaceChildren();
    if (!snapshot) {
      status.textContent = issue || 'Loading bookmarks…';
      return;
    }
    status.textContent = issue;
    const searching = search.value.trim().length > 0;
    const current = localFolder;
    const currentItem = current === null ? undefined : getItem(current);
    if (current !== null && !currentItem) localFolder = null;
    if (!searching && currentItem) {
      crumb.append(button('Bookmarks', () => { localFolder = null; localNavigation = true; render(); }, 'Go to bookmarks root'));
      crumb.append(document.createTextNode(` / ${currentItem.title}`));
    } else crumb.textContent = searching ? 'Search results' : 'Bookmarks';

    const visible = searching
      ? snapshot.items
      : snapshot.items.filter((item) => item.parent === localFolder);
    if (visible.length === 0) {
      status.textContent = searching
        ? 'No bookmarks match this search.'
        : snapshot.items.length === 0
          ? 'No bookmarks yet. Add a bookmark or create a folder.'
          : 'This folder is empty.';
      return;
    }

    for (const item of visible) {
      const row = document.createElement('article');
      row.className = 'bookmarks-row';
      row.setAttribute('role', 'listitem');
      row.dataset.id = String(item.id);
      const primary = document.createElement('div');
      primary.className = 'bookmarks-primary';
      const label = button(item.title || (item.kind === 'folder' ? 'Untitled folder' : item.url ?? 'Untitled bookmark'), () => {
        if (item.kind === 'folder') { localFolder = item.id; localNavigation = true; search.value = ''; render(); }
        else emit(bookmarkActionCommand('open', item.id));
      }, item.kind === 'folder' ? `Open folder ${item.title}` : `Open bookmark ${item.title}`);
      label.className = 'bookmarks-name';
      primary.append(label);
      if (searching) {
        const path = document.createElement('span');
        path.className = 'bookmarks-path';
        path.textContent = folderPath(item);
        primary.append(path);
      } else if (item.kind === 'link') {
        const address = document.createElement('span');
        address.className = 'bookmarks-path';
        address.textContent = item.url ?? '';
        primary.append(address);
      }
      row.append(primary);
      if (item.kind === 'link') row.append(button('New tab', () => emit(bookmarkActionCommand('newtab', item.id)), `Open ${item.title} in a new tab`));
      row.append(button('Edit', () => openDraft(item), `Edit ${item.title}`));
      row.append(button('Delete', () => emit(bookmarkActionCommand('delete', item.id)), `Delete ${item.title}`));

      const siblings = snapshot.items.filter((other) => other.parent === item.parent);
      const index = siblings.findIndex((other) => other.id === item.id);
      if (!searching) {
        row.append(button('Move up', () => {
          const before = siblings[index - 1];
          if (before) emit(bookmarkMoveCommand(item.id, item.parent, before.id));
        }, `Move ${item.title} up`));
        row.append(button('Move down', () => {
          const next = siblings[index + 1];
          const after = siblings[index + 2];
          if (next) emit(bookmarkMoveCommand(item.id, item.parent, after?.id ?? 0));
        }, `Move ${item.title} down`));
        const moveTo = document.createElement('select');
        moveTo.setAttribute('aria-label', `Move ${item.title} to folder`);
        const currentOption = document.createElement('option');
        currentOption.value = '';
        currentOption.textContent = `Move to…`;
        moveTo.append(currentOption);
        for (const folder of folders()) {
          if (folder.id === item.id || (item.kind === 'folder' && isDescendant(folder.id, item.id)) || folder.id === item.parent) continue;
          const option = document.createElement('option');
          option.value = String(folder.id);
          option.textContent = folderPath(folder) ? `${folderPath(folder)} / ${folder.title}` : folder.title;
          moveTo.append(option);
        }
        moveTo.addEventListener('change', () => {
          if (!moveTo.value) return;
          const parent = Number(moveTo.value);
          emit(bookmarkMoveCommand(item.id, parent, 0));
          moveTo.value = '';
        });
        row.append(moveTo);
      }
      list.append(row);
    }
  };

  search.addEventListener('input', () => {
    if (composing) return;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(sendSearch, 120);
    render();
  });
  search.addEventListener('compositionstart', () => { composing = true; });
  search.addEventListener('compositionend', () => {
    composing = false;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(sendSearch, 120);
    render();
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
    if (search.value !== incoming.query) search.value = incoming.query;
    if (!localNavigation) localFolder = incoming.currentFolder;
    render();
  };
  render();
  return (message) => { if (message.t === 'snap') receive(message); };
}
