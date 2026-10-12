// Name prompt in the dialog host (`?view=dialog&p=rename`): rename profile or workspace, new
// workspace, save session. Rust sends `{t:"snap",p:"rename",d:{title,save,value,hint,error,max}}`;
// the page sends `{t:"ren",a:"save",text}` or `{t:"ren",a:"cancel"}` (Escape is the shared
// `panel/dismiss`). Rust validates again and answers an invalid name with `error`.
// Styles: the shared .bw-dialog / .bw-input / .bw-button components plus the rename block in
// styles/dialog.css.
import { MAX_TEXT, sendPanel, type Inbound } from './protocol';

export interface RenameSnapshot {
  readonly title: string;
  readonly save: string;
  readonly value: string;
  readonly hint: string;
  readonly error: string;
  readonly max: number;
}

/** Strict check of the rename snapshot `d`. */
export function parseRenameSnapshot(d: unknown): RenameSnapshot | null {
  if (typeof d !== 'object' || d === null || Array.isArray(d)) return null;
  const v = d as Record<string, unknown>;
  const keys = ['title', 'save', 'value', 'hint', 'error', 'max'];
  if (!Object.keys(v).every(key => keys.includes(key)) || !keys.every(key => key in v)) return null;
  const str = (x: unknown, max: number): x is string => typeof x === 'string' && x.length <= max;
  if (!str(v.title, 64) || !str(v.save, 32) || !str(v.value, MAX_TEXT) || !str(v.hint, 128) || !str(v.error, 256)) return null;
  if (typeof v.max !== 'number' || !Number.isInteger(v.max) || v.max < 1 || v.max > MAX_TEXT) return null;
  return { title: v.title, save: v.save, value: v.value, hint: v.hint, error: v.error, max: v.max };
}

/** The message shown before Rust's check: only the rules every caller shares. */
export function localRenameError(text: string, max: number): string {
  const name = text.trim();
  if (name === '') return 'Enter a name.';
  if ([...name].length > max) return `Use at most ${max} characters.`;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(name)) return 'Names cannot contain control characters.';
  return '';
}

export function startRenameDialog(root: HTMLElement): (message: Inbound) => void {
  root.classList.add('rename-root');
  const form = document.createElement('div');
  form.className = 'bw-dialog rename-dialog bw-enter is-rise';
  form.setAttribute('role', 'dialog');
  form.setAttribute('aria-modal', 'true');
  const title = document.createElement('h1');
  title.className = 'bw-dialog-title';
  title.id = 'rename-title';
  form.setAttribute('aria-labelledby', title.id);
  const input = document.createElement('input');
  input.className = 'bw-input rename-input';
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.dir = 'auto';
  input.setAttribute('aria-labelledby', title.id);
  input.setAttribute('aria-describedby', 'rename-message');
  const message = document.createElement('p');
  message.className = 'rename-message';
  message.id = 'rename-message';
  message.setAttribute('aria-live', 'polite');
  const actions = document.createElement('div');
  actions.className = 'bw-dialog-actions';
  const cancel = document.createElement('button');
  cancel.type = 'button';
  cancel.className = 'bw-button';
  cancel.textContent = 'Cancel';
  const save = document.createElement('button');
  save.type = 'button';
  save.className = 'bw-button is-primary';
  actions.append(cancel, save);
  form.append(title, input, message, actions);
  root.replaceChildren(form);

  let snapshot: RenameSnapshot | null = null;
  let touched = false;
  let sent = false;

  const showMessage = (error: string): void => {
    const text = error || snapshot?.hint || '';
    message.textContent = text;
    message.classList.toggle('is-error', error !== '');
    input.setAttribute('aria-invalid', String(error !== ''));
  };
  const refresh = (): void => {
    if (!snapshot) return;
    const error = localRenameError(input.value, snapshot.max);
    save.disabled = error !== '' || sent;
    showMessage(touched ? error : '');
  };
  const submit = (): void => {
    if (!snapshot || sent) return;
    touched = true;
    const error = localRenameError(input.value, snapshot.max);
    if (error !== '') { refresh(); return; }
    sent = true;
    save.disabled = true;
    if (!sendPanel('ren', { a: 'save', text: input.value })) { sent = false; refresh(); }
  };

  input.addEventListener('input', () => { touched = true; refresh(); });
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); submit(); }
  });
  save.addEventListener('click', submit);
  cancel.addEventListener('click', () => { sendPanel('ren', { a: 'cancel' }); });

  return (inbound: Inbound) => {
    if (inbound.t !== 'snap' || inbound.p !== 'rename') return;
    const next = parseRenameSnapshot(inbound.d);
    if (!next) return;
    const first = snapshot === null;
    snapshot = next;
    title.textContent = next.title;
    save.textContent = next.save;
    input.maxLength = next.max;
    if (first) {
      input.value = next.value;
      input.focus();
      input.select();
    }
    // A snapshot after a save is Rust's verdict: show its error and allow another try.
    sent = false;
    if (next.error !== '') {
      touched = true;
      save.disabled = false;
      showMessage(next.error);
      input.focus();
    } else {
      refresh();
    }
  };
}
