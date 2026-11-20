// The address bar's two small dialogs, drawn into the dialog host (`?view=dialog&p=address`): the QR code
// of the shown page and the bookmark dialog. Rust sends `{t:"snap",p:"addressdlg"}`; the page sends
// `{t:"addr",a:...}` commands (src/chrome_host/address_page.rs). Text from Rust reaches the DOM through
// textContent only; the QR code is drawn from the address on this device (qr.ts), never fetched.
import { el, icon } from './dom';
import { sendAddr, sendPanel, type SnapshotMsg } from './protocol';
import { qrMatrix, qrSvgPath } from './qr';
import { selectField } from './panels/select';
import { textButton } from './panels/kit';
import { MAX_QR_BYTES, cleanField, parseDialogSnapshot, type BookmarkDialog, type DialogSnapshot } from './address-dialog-logic';

const SVG_NS = 'http://www.w3.org/2000/svg';

function qrSvg(text: string): SVGSVGElement | null {
  const matrix = qrMatrix(text);
  if (!matrix) return null;
  const { size, d } = qrSvgPath(matrix);
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', `0 0 ${size} ${size}`);
  svg.setAttribute('shape-rendering', 'crispEdges');
  svg.setAttribute('role', 'img');
  svg.setAttribute('aria-label', 'QR code for this page');
  const back = document.createElementNS(SVG_NS, 'rect');
  back.setAttribute('width', String(size));
  back.setAttribute('height', String(size));
  back.setAttribute('fill', 'currentColor');
  back.setAttribute('class', 'dlg-qr-paper');
  const path = document.createElementNS(SVG_NS, 'path');
  path.setAttribute('d', d);
  path.setAttribute('class', 'dlg-qr-ink');
  svg.append(back, path);
  return svg;
}

function field(label: string, control: HTMLElement): HTMLElement {
  const wrap = el('label', 'dlg-field');
  const name = el('span', 'sp-label');
  name.textContent = label;
  wrap.append(name, control);
  return wrap;
}

function input(value: string, max: number): HTMLInputElement {
  const node = el('input', 'bw-input');
  node.type = 'text';
  node.value = value;
  node.maxLength = max;
  node.autocomplete = 'off';
  node.spellcheck = false;
  return node;
}

function drawQr(root: HTMLElement, text: string): void {
  const heading = el('h1');
  heading.textContent = 'QR code';
  const box = el('div', 'dlg-qr-box');
  const bytes = new TextEncoder().encode(text).length;
  const svg = bytes <= MAX_QR_BYTES ? qrSvg(text) : null;
  if (svg) box.append(svg);
  else {
    const note = el('p', 'sp-desc');
    note.textContent = 'This address is too long for a QR code.';
    box.append(note);
  }
  const address = el('p', 'sp-desc');
  address.dir = 'ltr';
  address.textContent = text;
  const actions = el('div', 'dlg-actions');
  actions.append(
    textButton('Copy address', () => { sendAddr({ a: 'qrcopy' }); }, { icon: 'copy' }),
    textButton('Close', () => { sendPanel('panel', { a: 'dismiss' }); }, { subtle: true }),
  );
  root.replaceChildren(heading, box, address, actions);
}

/** Called when the dialog page loses the keyboard to the rest of Windows (the bubble then closes). */
let onLeave: (() => void) | null = null;
/** A refused save (shown in the same drawing) lets Done and Escape try again. */
let onRefused: (() => void) | null = null;
let hadFocus = false;
window.addEventListener('focus', () => { hadFocus = true; });
window.addEventListener('blur', () => { if (hadFocus) onLeave?.(); });

/** A folder's glyph, indented by its depth inside the open list only (address.css). */
function folderLead(name: 'bookmark' | 'folder', depth: number): HTMLElement {
  const lead = el('span', `dlg-folder-lead is-depth-${Math.min(depth, 5)}`);
  lead.append(icon(name, { size: 14, fine: true }));
  return lead;
}

function drawBookmark(root: HTMLElement, b: BookmarkDialog): void {
  // The bubble (Chrome's "Bookmark added"): the page is saved already, so closing it in any way
  // keeps the bookmark, with whatever was typed. The dialog ("Add bookmark") saves only on Save.
  const bubble = b.mode !== 'add';
  root.classList.toggle('is-bubble', bubble);
  const head = el('div', 'dlg-head');
  if (bubble) {
    const star = el('span', 'dlg-star');
    star.setAttribute('aria-hidden', 'true');
    star.append(icon('bookmark', { size: 16 }));
    head.append(star);
  }
  const heading = el('h1');
  heading.textContent = b.mode === 'added' ? 'Bookmark added' : b.mode === 'edit' ? 'Edit bookmark' : 'Add bookmark';
  heading.id = 'dlg-bm-title';
  head.append(heading);
  root.setAttribute('aria-labelledby', heading.id);
  const name = input(b.name, 200);
  const nickname = input(b.nickname, 24);
  nickname.placeholder = 'A word to type in the address bar';
  const description = el('textarea', 'bw-input dlg-desc');
  description.value = b.description;
  description.maxLength = 512;
  description.rows = bubble ? 2 : 3;
  description.placeholder = 'Optional';
  // A folder past the list's cap stays chosen (as "Current folder"), so saving never moves it.
  const listed = b.folder === 0 || b.folders.some((f) => f.id === b.folder);
  let folder = String(b.folder);
  const select = selectField<string>({
    options: [
      { value: '0', label: 'Bookmarks bar', lead: () => folderLead('bookmark', 0) },
      ...(listed ? [] : [{ value: folder, label: 'Current folder', lead: () => folderLead('folder', 0) }]),
      ...b.folders.map((f) => ({ value: String(f.id), label: f.title, lead: () => folderLead('folder', f.depth) })),
    ],
    value: folder,
    label: 'Folder',
    placeholder: 'Folder',
    onChange: (next) => { folder = next; },
  });
  const address = el('p', 'sp-desc dlg-url');
  address.dir = 'ltr';
  address.textContent = b.url;
  const problem = el('p', 'sp-field-error');
  problem.setAttribute('role', 'alert');
  problem.textContent = b.error;
  const start = { name: b.name, nickname: b.nickname, description: b.description, folder };
  const dirty = (): boolean => cleanField(name.value) !== start.name || cleanField(nickname.value) !== start.nickname
    || cleanField(description.value) !== start.description || folder !== start.folder;
  let sent = false;
  const save = (): void => {
    const title = cleanField(name.value);
    if (title === '') { problem.textContent = 'Give the bookmark a name.'; name.focus(); return; }
    const text = `${title}\t${cleanField(nickname.value)}\t${cleanField(description.value)}`;
    sent = true;
    sendPanel('addr', { a: 'bmsave', text, parent: Number(folder) });
  };
  /** Done, Escape and clicking away: keep the bookmark, with any change made here. */
  const finish = (): void => {
    if (sent) return;
    if (dirty() && cleanField(name.value) !== '') save();
    else { sent = true; sendPanel('panel', { a: 'dismiss' }); }
  };
  const actions = el('div', 'dlg-actions');
  if (bubble) {
    actions.append(
      textButton('Remove', () => { sent = true; sendAddr({ a: 'bmremove' }); }, { icon: 'trash', subtle: true }),
      el('span', 'dlg-spacer'),
      textButton('Done', finish, { primary: true }),
    );
  } else {
    actions.append(textButton('Save', save, { primary: true }));
    if (b.existing) actions.append(textButton('Remove', () => { sendAddr({ a: 'bmremove' }); }, { icon: 'trash', subtle: true }));
    actions.append(textButton('Cancel', () => { sendPanel('panel', { a: 'dismiss' }); }, { subtle: true }));
  }
  const fields = el('div', 'dlg-fields');
  fields.append(field('Name', name), field('Folder', select.root), field('Nickname', nickname), field('Description', description));
  root.replaceChildren(head, ...(bubble ? [] : [address]), fields, problem, actions);
  // On this drawing's own fields: `root` outlives it, so a listener there would keep an older
  // dialog's save (and its stale fields) running on every later Enter.
  for (const control of [name, nickname]) {
    control.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); if (bubble) finish(); else save(); }
    });
  }
  root.onkeydown = bubble ? (event: KeyboardEvent) => {
    // Escape in the bubble is Done (dialog.ts would otherwise dismiss without the edits); an open
    // folder list takes its own Escape first.
    if (event.key === 'Escape' && !event.defaultPrevented && select.button.getAttribute('aria-expanded') !== 'true') { event.preventDefault(); finish(); }
  } : null;
  onLeave = bubble ? finish : null;
  onRefused = () => { sent = false; };
  name.focus();
  name.select();
}

/** Starts the address dialogs; the returned function takes the `addressdlg` snapshots. */
export function startAddressDialog(root: HTMLElement): (message: SnapshotMsg) => void {
  const page = el('div', 'dlg-address');
  root.replaceChildren(page);
  let shown: DialogSnapshot | null = null;
  return (message) => {
    const parsed = parseDialogSnapshot(message.d);
    if (!parsed) return;
    // A repeat of the same dialog (an error after Save) keeps what the user typed.
    if (shown?.kind === 'bookmark' && parsed.kind === 'bookmark' && shown.url === parsed.url && shown.mode === parsed.mode) {
      page.querySelector<HTMLElement>('.sp-field-error')!.textContent = parsed.error;
      if (parsed.error !== '') onRefused?.();
      shown = parsed;
      return;
    }
    shown = parsed;
    if (parsed.kind === 'qr') { onLeave = null; drawQr(page, parsed.text); }
    else drawBookmark(page, parsed);
  };
}
