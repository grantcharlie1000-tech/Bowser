// The Feeds panel (`?view=dialog&p=feeds`): the subscriptions, the items of the one selected (or of all of
// them), a preview of an address before subscribing, the feeds the page in front offers, OPML import and
// read and unread. Rust owns the files and does every fetch; the page sends `feed` commands and draws what
// the `feeds` snapshot says. Everything that came from a feed (titles, summaries) is set as text, never as
// markup, and an item is opened by naming it (feed id and hash), never by an address from the page.
import { sendPanel, type Inbound, type PanelFields } from '../protocol';
import { el, icon } from '../dom';
import {
  ageText, closePreviewCommand, feedLabel, importCommand, itemMeta, kindIcon, openCommand, parseFeedsSnapshot, playCommand, previewCommand,
  readAllCommand, readCommand, refreshCommand, refreshNote, selectCommand, subscribeCommand, unreadText, unsubscribeCommand, MAX_ADDRESS,
  type FeedRow, type FeedsCommand, type FeedsSnapshot, type ItemRow,
} from './feeds-logic';
import { emptyState, enter, iconButton, moveFocus, panelHeader, textButton } from './kit';

function emit(command: FeedsCommand | null): void {
  if (command) sendPanel('feed', command as PanelFields);
}

/** How long an Unsubscribe waits for its confirming second click. */
const CONFIRM_MS = 3000;

export function startFeedsPanel(root: HTMLElement): (message: Inbound) => void {
  let data: FeedsSnapshot | null = null;
  let confirming: number | null = null;
  let confirmTimer = 0;
  let focusItem: string | null = null;
  const now = (): number => Math.floor(Date.now() / 1000);

  const header = panelHeader('Feeds', 'Close Feeds');
  const refreshButton = iconButton('reload', 'Refresh all feeds', () => emit(refreshCommand()));
  const importButton = iconButton('folder-open', 'Import feeds from an OPML file', () => emit(importCommand()));
  const readAllButton = iconButton('check', 'Mark everything read', () => emit(readAllCommand(data?.selected)));
  header.tools.append(refreshButton, importButton, readAllButton);

  // Subscribe by address: a feed, or a page that offers one. Preview first, then Subscribe.
  const form = el('form', 'pf-add');
  const field = el('input', 'bw-input');
  field.type = 'text';
  field.placeholder = 'Feed or page address';
  field.autocomplete = 'off';
  field.spellcheck = false;
  field.maxLength = MAX_ADDRESS;
  field.setAttribute('aria-label', 'Feed or page address');
  const previewButton = textButton('Preview', () => submit(), { icon: 'eye' });
  previewButton.type = 'submit';
  form.append(field, previewButton);
  const bar = el('div', 'pn-bar');
  bar.append(form);

  const status = el('p', 'pn-status pf-status');
  status.setAttribute('role', 'status');
  const error = el('p', 'pn-status pf-error');
  error.setAttribute('role', 'alert');

  const pageBox = el('section', 'pf-page');
  pageBox.setAttribute('aria-label', 'Feeds on this page');
  const previewBox = el('section', 'pf-preview');
  previewBox.setAttribute('aria-label', 'Preview');
  const feedList = el('div', 'pf-feeds bw-scroll');
  feedList.setAttribute('role', 'listbox');
  feedList.setAttribute('aria-label', 'Subscriptions');
  const note = el('p', 'pn-status pf-note');
  const itemList = el('div', 'pn-scroll bw-scroll pf-items');
  itemList.setAttribute('role', 'list');
  itemList.setAttribute('aria-label', 'Items');

  const host = el('section', 'pn pf');
  host.append(header.root, bar, status, error, pageBox, previewBox, feedList, note, itemList);
  enter(host);
  root.replaceChildren(host);

  const submit = (): void => {
    const command = previewCommand(field.value);
    if (!command) {
      error.textContent = field.value.trim() === '' ? 'Enter a feed or page address.' : 'That is not an address Bowser can fetch.';
      return;
    }
    error.textContent = '';
    emit(command);
  };
  form.addEventListener('submit', (event) => { event.preventDefault(); submit(); });

  const clearConfirm = (): void => {
    window.clearTimeout(confirmTimer);
    confirming = null;
  };

  const countBadge = (feed: Pick<FeedRow, 'unread'>): HTMLElement => {
    const badge = el('span', 'pf-count');
    badge.textContent = feed.unread > 0 ? String(feed.unread) : '';
    badge.hidden = feed.unread === 0;
    return badge;
  };

  const allRow = (snapshot: FeedsSnapshot): HTMLElement => {
    const row = el('div', 'bw-row bw-focus-inset pn-row pf-feed pf-all');
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', String(snapshot.selected === 0));
    row.tabIndex = 0;
    row.dataset.feed = '0';
    const lead = el('span', 'pn-lead');
    lead.append(icon('feed'));
    const title = el('span', 'bw-row-title');
    title.textContent = 'All feeds';
    const trail = el('span', 'pn-trail');
    trail.append(countBadge({ unread: snapshot.unread }));
    row.append(lead, title, trail);
    row.addEventListener('click', () => emit(selectCommand(0)));
    row.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); emit(selectCommand(0)); } });
    return row;
  };

  const feedRow = (feed: FeedRow, selected: boolean): HTMLElement => {
    const row = el('div', 'bw-row bw-focus-inset pn-row pf-feed');
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', String(selected));
    row.setAttribute('aria-label', feedLabel(feed));
    row.tabIndex = 0;
    row.dataset.feed = String(feed.id);
    const lead = el('span', 'pn-lead');
    lead.append(icon(kindIcon(feed.kind)));
    const title = el('span', 'bw-row-title');
    title.textContent = feed.title;
    title.dir = 'auto';
    const trail = el('span', 'pn-trail');
    const tools = el('span', 'pn-actions');
    tools.append(iconButton('reload', 'Refresh this feed', (event) => { event.stopPropagation(); emit(refreshCommand(feed.id)); }));
    const del = iconButton('trash', 'Unsubscribe', (event) => {
      event.stopPropagation();
      if (confirming === feed.id) { clearConfirm(); emit(unsubscribeCommand(feed.id)); return; }
      clearConfirm();
      confirming = feed.id;
      confirmTimer = window.setTimeout(() => { confirming = null; paint(); }, CONFIRM_MS);
      paint();
    });
    if (confirming === feed.id) {
      del.setAttribute('aria-label', 'Confirm: unsubscribe from this feed');
      del.setAttribute('data-tip', 'Click again to unsubscribe');
      del.classList.add('is-danger');
      row.classList.add('is-confirming');
    }
    tools.append(del);
    const meta = el('span', 'pn-meta');
    if (feed.error !== '') {
      const warn = el('span', 'pf-warn');
      warn.append(icon('warning', { size: 12, fine: true }));
      warn.setAttribute('data-tip', feed.error);
      meta.append(warn);
    }
    meta.append(countBadge(feed));
    trail.append(meta, tools);
    row.append(lead, title, trail);
    row.addEventListener('click', () => emit(selectCommand(feed.id)));
    row.addEventListener('keydown', (event) => {
      if (event.target !== row) return;
      if (event.key === 'Enter') { event.preventDefault(); emit(selectCommand(feed.id)); }
      else if (event.key === 'Delete') { event.preventDefault(); del.click(); }
    });
    return row;
  };

  const itemRow = (item: ItemRow): HTMLElement => {
    const row = el('div', `bw-row bw-focus-inset is-tall pn-row pf-item${item.read ? '' : ' is-unread'}`);
    row.setAttribute('role', 'listitem');
    row.tabIndex = 0;
    row.dataset.item = `${item.feed}:${item.h}`;
    const lead = el('span', 'pn-lead pf-mark');
    lead.setAttribute('aria-hidden', 'true');
    if (!item.read) lead.append(el('span', 'pf-dot'));
    const text = el('span', 'bw-row-text pf-text');
    const title = el('span', 'bw-row-title pf-title');
    title.textContent = item.title === '' ? '(no title)' : item.title;
    title.dir = 'auto';
    const meta = el('span', 'bw-row-meta pf-meta');
    meta.textContent = itemMeta(item, now());
    text.append(title, meta);
    if (item.summary !== '') {
      const summary = el('span', 'pf-summary');
      summary.textContent = item.summary;
      summary.dir = 'auto';
      text.append(summary);
    }
    const tools = el('span', 'pf-tools');
    if (item.media !== '') {
      tools.append(iconButton('play', item.media === 'video' ? 'Play the video' : 'Play the episode', (event) => { event.stopPropagation(); focusItem = row.dataset.item ?? null; emit(playCommand(item.feed, item.h)); }));
    }
    tools.append(
      iconButton('external', item.watch ? 'Watch in a new tab (no cookies)' : 'Open in a new tab', (event) => { event.stopPropagation(); focusItem = row.dataset.item ?? null; emit(openCommand(item.feed, item.h)); }),
      iconButton(item.read ? 'eye' : 'check', item.read ? 'Mark unread' : 'Mark read', (event) => { event.stopPropagation(); focusItem = row.dataset.item ?? null; emit(readCommand(item.feed, item.h, !item.read)); }),
    );
    row.append(lead, text, tools);
    row.addEventListener('click', () => { focusItem = row.dataset.item ?? null; emit(openCommand(item.feed, item.h)); });
    row.addEventListener('keydown', (event) => {
      if (event.target !== row) return;
      if (event.key === 'Enter') { event.preventDefault(); focusItem = row.dataset.item ?? null; emit(openCommand(item.feed, item.h)); }
      else if (event.key === ' ') { event.preventDefault(); focusItem = row.dataset.item ?? null; emit(readCommand(item.feed, item.h, !item.read)); }
    });
    return row;
  };

  const pageRows = (snapshot: FeedsSnapshot): void => {
    const rows = snapshot.page.filter((entry) => !entry.subscribed);
    pageBox.hidden = rows.length === 0 || snapshot.preview !== null;
    if (pageBox.hidden) { pageBox.replaceChildren(); return; }
    const heading = el('h2', 'pn-day');
    heading.textContent = rows.length === 1 ? 'This page offers a feed' : `This page offers ${rows.length} feeds`;
    const list = rows.map((entry) => {
      const row = el('div', 'bw-row pn-row pf-offer');
      const lead = el('span', 'pn-lead');
      lead.append(icon('feed'));
      const text = el('span', 'bw-row-text');
      const title = el('span', 'bw-row-title');
      title.textContent = entry.title === '' ? entry.url : entry.title;
      title.dir = 'auto';
      const meta = el('span', 'bw-row-meta');
      meta.textContent = entry.url;
      text.append(title, meta);
      const view = textButton('Preview', () => emit(previewCommand(entry.url)), { subtle: true });
      row.append(lead, text, view);
      return row;
    });
    pageBox.replaceChildren(heading, ...list);
  };

  const previewRows = (snapshot: FeedsSnapshot): void => {
    const preview = snapshot.preview;
    previewBox.hidden = preview === null;
    if (preview === null) { previewBox.replaceChildren(); return; }
    const top = el('div', 'pf-preview-head');
    const heading = el('h2', 'pn-day pf-preview-title');
    heading.textContent = preview.candidates.length > 0 ? 'Feeds on that page' : preview.title === '' ? 'Preview' : preview.title;
    heading.dir = 'auto';
    const actions = el('span', 'pf-preview-actions');
    if (preview.candidates.length === 0) {
      const subscribe = textButton(preview.subscribed ? 'Subscribed' : 'Subscribe', () => emit(subscribeCommand(preview.url)), { primary: true, icon: 'plus' });
      subscribe.disabled = preview.subscribed || !snapshot.allowed;
      actions.append(subscribe);
    }
    actions.append(iconButton('close', 'Close the preview', () => emit(closePreviewCommand())));
    top.append(heading, actions);
    const rows: HTMLElement[] = [];
    for (const candidate of preview.candidates) {
      const row = el('div', 'bw-row pn-row pf-offer');
      const lead = el('span', 'pn-lead');
      lead.append(icon('feed'));
      const text = el('span', 'bw-row-text');
      const title = el('span', 'bw-row-title');
      title.textContent = candidate.title === '' ? candidate.url : candidate.title;
      title.dir = 'auto';
      const meta = el('span', 'bw-row-meta');
      meta.textContent = candidate.url;
      text.append(title, meta);
      row.append(lead, text, textButton('Preview', () => emit(previewCommand(candidate.url)), { subtle: true }));
      rows.push(row);
    }
    for (const item of preview.items) {
      const row = el('div', 'bw-row is-tall pn-row pf-peek-item');
      const text = el('span', 'bw-row-text pf-text');
      const title = el('span', 'bw-row-title pf-title');
      title.textContent = item.title === '' ? '(no title)' : item.title;
      title.dir = 'auto';
      const meta = el('span', 'bw-row-meta pf-meta');
      meta.textContent = ageText(item.published, now());
      text.append(title);
      if (meta.textContent !== '') text.append(meta);
      if (item.summary !== '') {
        const summary = el('span', 'pf-summary');
        summary.textContent = item.summary;
        summary.dir = 'auto';
        text.append(summary);
      }
      row.append(text);
      rows.push(row);
    }
    if (rows.length === 0) rows.push(emptyState('feed', 'That feed has no items yet.'));
    const list = el('div', 'pf-preview-list bw-scroll');
    list.append(...rows);
    previewBox.replaceChildren(top, list);
  };

  const paint = (): void => {
    const snapshot = data;
    if (!snapshot) return;
    if (confirming !== null && !snapshot.feeds.some((feed) => feed.id === confirming)) clearConfirm();
    status.textContent = snapshot.status;
    status.hidden = snapshot.status === '';
    // A fresh error from Rust replaces the page's own ("enter an address").
    if (snapshot.error !== '' || error.dataset.from === 'rust') error.textContent = snapshot.error;
    error.dataset.from = snapshot.error !== '' ? 'rust' : '';
    error.hidden = error.textContent === '';
    refreshButton.disabled = snapshot.busy || snapshot.feeds.length === 0;
    importButton.disabled = snapshot.busy || !snapshot.allowed;
    readAllButton.disabled = snapshot.unread === 0;
    readAllButton.setAttribute('aria-label', snapshot.selected === 0 ? 'Mark everything read' : 'Mark this feed read');
    readAllButton.setAttribute('data-tip', snapshot.selected === 0 ? 'Mark everything read' : 'Mark this feed read');
    previewButton.disabled = snapshot.busy || !snapshot.allowed;
    field.disabled = !snapshot.allowed;
    host.classList.toggle('is-busy', snapshot.busy);
    host.setAttribute('aria-busy', String(snapshot.busy));
    pageRows(snapshot);
    previewRows(snapshot);
    if (!snapshot.allowed) {
      feedList.replaceChildren();
      feedList.hidden = true;
      note.hidden = true;
      itemList.replaceChildren(emptyState('person', 'A guest window does not keep feeds.'));
      return;
    }
    feedList.hidden = snapshot.feeds.length === 0;
    note.hidden = snapshot.feeds.length === 0;
    note.textContent = refreshNote(snapshot.refresh);
    feedList.replaceChildren(...(snapshot.feeds.length > 0 ? [allRow(snapshot)] : []), ...snapshot.feeds.map((feed) => feedRow(feed, feed.id === snapshot.selected)));
    if (snapshot.feeds.length === 0) {
      itemList.replaceChildren(emptyState('feed', 'No feeds yet. Type a feed or page address above, or import an OPML file.'));
      return;
    }
    if (snapshot.items.length === 0) {
      itemList.replaceChildren(emptyState('feed', snapshot.busy ? 'Fetching…' : 'Nothing here yet. Press Refresh to fetch it.'));
      return;
    }
    itemList.replaceChildren(...snapshot.items.map(itemRow));
    if (focusItem !== null) {
      Array.from(itemList.querySelectorAll<HTMLElement>('.pf-item')).find((row) => row.dataset.item === focusItem)?.focus();
      focusItem = null;
    }
  };
  feedList.addEventListener('keydown', (event) => { moveFocus(feedList, '.pf-feed', event); });
  itemList.addEventListener('keydown', (event) => { moveFocus(itemList, '.pf-item', event); });
  // The summary of the unread count in the window is for assistive tech only; the header stays plain.
  header.title.setAttribute('aria-label', 'Feeds');

  return (message) => {
    if (message.t !== 'snap' || message.p !== 'feeds') return;
    const parsed = parseFeedsSnapshot(message.d);
    if (!parsed) return;
    data = parsed;
    // Once a subscription is made (or found to be made already) the address is no longer needed.
    if (/subscribed/i.test(parsed.status) && parsed.error === '') field.value = '';
    header.title.textContent = parsed.unread > 0 ? `Feeds · ${unreadText(parsed.unread)}` : 'Feeds';
    paint();
  };
}
