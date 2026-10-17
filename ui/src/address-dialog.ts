// The address bar's two small dialogs, drawn into the dialog host (`?view=dialog&p=address`): the QR code
// of the shown page and the bookmark dialog. Rust sends `{t:"snap",p:"addressdlg"}`; the page sends
// `{t:"addr",a:...}` commands (src/chrome_host/address_page.rs). Text from Rust reaches the DOM through
// textContent only; the QR code is drawn from the address on this device (qr.ts), never fetched.
import { el } from './dom';
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

function drawBookmark(root: HTMLElement, b: BookmarkDialog): void {
  const heading = el('h1');
  heading.textContent = b.existing ? 'Edit bookmark' : 'Add bookmark';
  const name = input(b.name, 200);
  const nickname = input(b.nickname, 24);
  nickname.placeholder = 'A word to type in the address bar';
  const description = el('textarea', 'bw-input dlg-desc');
  description.value = b.description;
  description.maxLength = 512;
  description.rows = 3;
  let folder = String(b.folders.some((f) => f.id === b.folder) ? b.folder : 0);
  const select = selectField<string>({
    options: [{ value: '0', label: 'Bookmarks' }, ...b.folders.map((f) => ({ value: String(f.id), label: `${' '.repeat(f.depth)}${f.title}` }))],
    value: folder,
    label: 'Folder',
    placeholder: 'Folder',
    onChange: (next) => { folder = next; },
  });
  const address = el('p', 'sp-desc');
  address.dir = 'ltr';
  address.textContent = b.url;
  const problem = el('p', 'sp-field-error');
  problem.setAttribute('role', 'alert');
  problem.textContent = b.error;
  const save = (): void => {
    const title = cleanField(name.value);
    if (title === '') { problem.textContent = 'Give the bookmark a name.'; name.focus(); return; }
    const text = `${title}\t${cleanField(nickname.value)}\t${cleanField(description.value)}`;
    sendPanel('addr', { a: 'bmsave', text, parent: Number(folder) });
  };
  const actions = el('div', 'dlg-actions');
  actions.append(textButton('Save', save, { primary: true }));
  if (b.existing) actions.append(textButton('Remove', () => { sendAddr({ a: 'bmremove' }); }, { icon: 'trash', subtle: true }));
  actions.append(textButton('Cancel', () => { sendPanel('panel', { a: 'dismiss' }); }, { subtle: true }));
  root.replaceChildren(heading, address, field('Name', name), field('Folder', select.root), field('Nickname', nickname), field('Description', description), problem, actions);
  root.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && event.target instanceof HTMLInputElement) { event.preventDefault(); save(); }
  });
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
    if (shown?.kind === 'bookmark' && parsed.kind === 'bookmark' && shown.url === parsed.url) {
      page.querySelector<HTMLElement>('.sp-field-error')!.textContent = parsed.error;
      shown = parsed;
      return;
    }
    shown = parsed;
    if (parsed.kind === 'qr') drawQr(page, parsed.text);
    else drawBookmark(page, parsed);
  };
}
