import { el } from './dom';
import { onFill } from './menu-logic';
import { iconButton, radioGroup, textButton } from './panels/kit';
import {
  MAX_PROFILE_NAME, PROFILE_ACCENTS, PROFILE_ACCENT_LABELS, PROFILE_AVATARS, PROFILE_AVATAR_LABELS,
  profileNameError, profileText, type ProfileAccent, type ProfileAvatar, type ProfileEntry, type ProfileList,
} from './settings-page-logic';

export interface ProfilesView {
  readonly parts: readonly HTMLElement[];
  paint(profiles: ProfileList, guest: boolean): void;
}

export interface ProfilesHost {
  send(text: string, answered: boolean): boolean;
}

export function paintDisc(node: HTMLElement, colour: string, glyph?: string): void {
  node.style.setProperty('--sp-profile-fill', colour);
  node.style.setProperty('--sp-profile-ink', colour === '' ? '' : onFill(colour));
  if (glyph !== undefined) node.textContent = glyph;
}

function disc(className: string, colour = '', glyph = ''): HTMLElement {
  const node = el('span', className);
  node.setAttribute('aria-hidden', 'true');
  paintDisc(node, colour, glyph);
  return node;
}

export function mountProfiles(host: ProfilesHost): ProfilesView {
  let profiles: ProfileList = { list: [], avatars: [], accents: [] };
  let guest = false;
  let painted = '';
  let editing: { id: string; mode: 'rename' | 'delete' } | null = null;

  const list = el('ul', 'sp-profiles');
  list.setAttribute('aria-label', 'Profiles');
  const me = (): ProfileEntry | undefined => profiles.list.find((p) => p.current);

  const look = el('div', 'sp-profile-look');
  const lookGroup = <T extends string>(label: string, keys: readonly T[], names: Readonly<Record<T, string>>, pick: (entry: ProfileEntry, value: T) => string) => {
    const swatches: HTMLElement[] = [];
    const group = radioGroup<T>('sp-profile-choices', label, keys.map((key) => [key, (button: HTMLButtonElement) => {
      button.className = 'sp-profile-choice';
      button.setAttribute('aria-label', names[key]);
      button.setAttribute('data-tip', names[key]);
      const swatch = disc('sp-profile-swatch');
      swatches.push(swatch);
      button.append(swatch);
    }] as const), (value) => {
      const entry = me();
      if (entry) host.send(pick(entry, value), true);
    });
    const caption = el('span', 'sp-profile-look-label');
    caption.textContent = label;
    caption.setAttribute('aria-hidden', 'true');
    look.append(caption, group.root);
    return { group, swatches };
  };
  const avatars = lookGroup<ProfileAvatar>('Avatar', PROFILE_AVATARS, PROFILE_AVATAR_LABELS, (entry, avatar) => profileText.look(entry.id, avatar, entry.accent));
  const accents = lookGroup<ProfileAccent>('Colour', PROFILE_ACCENTS, PROFILE_ACCENT_LABELS, (entry, accent) => profileText.look(entry.id, entry.avatar, accent));
  const paintLook = (): void => {
    const entry = me();
    look.hidden = entry === undefined;
    if (!entry) return;
    avatars.swatches.forEach((swatch, index) => paintDisc(swatch, entry.colour, profiles.avatars[index] ?? ''));
    accents.swatches.forEach((swatch, index) => paintDisc(swatch, profiles.accents[index] ?? '', entry.glyph));
    avatars.group.set(entry.avatar);
    accents.group.set(entry.accent);
  };

  const form = el('div', 'sp-allow-add sp-profile-add');
  const nameInput = el('input', 'bw-input sp-allow-input');
  nameInput.type = 'text';
  nameInput.placeholder = 'New profile name';
  nameInput.autocomplete = 'off';
  nameInput.spellcheck = false;
  nameInput.maxLength = MAX_PROFILE_NAME * 2;
  nameInput.setAttribute('aria-label', 'New profile name');
  const formError = el('p', 'sp-field-error');
  formError.setAttribute('role', 'alert');
  const add = (): void => {
    const error = profileNameError(nameInput.value, profiles.list);
    formError.textContent = error;
    if (error !== '') { nameInput.focus(); return; }
    if (host.send(profileText.create(nameInput.value), true)) nameInput.value = '';
  };
  nameInput.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); add(); } });
  nameInput.addEventListener('input', () => { formError.textContent = ''; });
  const guestButton = textButton('Guest window', () => { host.send(profileText.guest(), false); }, { icon: 'external', subtle: true });
  form.append(nameInput, textButton('Add profile', add, { icon: 'plus' }), guestButton);

  const rowOf = (id: string): HTMLElement | undefined => Array.from(list.querySelectorAll<HTMLElement>('.sp-profile')).find((item) => item.dataset.profile === id);
  const close = (id: string): void => {
    editing = null;
    repaint();
    rowOf(id)?.querySelector<HTMLElement>('button')?.focus();
  };

  const renameRow = (entry: ProfileEntry, item: HTMLElement): void => {
    const input = el('input', 'bw-input sp-allow-input sp-profile-rename');
    input.type = 'text';
    input.value = entry.name;
    input.maxLength = MAX_PROFILE_NAME * 2;
    input.setAttribute('aria-label', `New name for ${entry.name}`);
    const error = el('p', 'sp-field-error');
    error.setAttribute('role', 'alert');
    const commit = (): void => {
      const problem = profileNameError(input.value, profiles.list, entry.id);
      error.textContent = problem;
      if (problem !== '') { input.focus(); return; }
      if (input.value.trim() !== entry.name) host.send(profileText.rename(entry.id, input.value), true);
      close(entry.id);
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') { event.preventDefault(); commit(); }
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(entry.id); }
    });
    input.addEventListener('input', () => { error.textContent = ''; });
    const actions = el('div', 'sp-profile-actions');
    actions.append(textButton('Save', commit, { primary: true }), textButton('Cancel', () => close(entry.id), { subtle: true }));
    item.append(disc('sp-profile-avatar', entry.colour, entry.glyph), input, actions, error);
    queueMicrotask(() => { input.focus(); input.select?.(); });
  };

  const deleteRow = (entry: ProfileEntry, item: HTMLElement): void => {
    const text = el('div', 'sp-profile-text');
    const question = el('span', 'sp-profile-name');
    question.textContent = `Delete ${entry.name}?`;
    question.dir = 'auto';
    const note = el('span', 'sp-profile-note');
    note.textContent = 'Its sign-ins, history, bookmarks and settings are removed. This cannot be undone.';
    text.append(question, note);
    const keep = textButton('Keep', () => close(entry.id), { subtle: true });
    const remove = textButton('Delete', () => { editing = null; host.send(profileText.remove(entry.id), true); repaint(); }, { icon: 'trash' });
    remove.classList.add('is-danger');
    const actions = el('div', 'sp-profile-actions');
    actions.append(keep, remove);
    actions.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); keep.click(); } });
    item.append(disc('sp-profile-avatar', entry.colour, entry.glyph), text, actions);
    item.classList.add('is-asking');
    queueMicrotask(() => keep.focus());
  };

  const normalRow = (entry: ProfileEntry, item: HTMLElement): void => {
    const text = el('div', 'sp-profile-text');
    const name = el('span', 'sp-profile-name');
    name.textContent = entry.name;
    name.dir = 'auto';
    text.append(name);
    if (entry.current) {
      const note = el('span', 'sp-profile-note');
      note.textContent = 'This window';
      text.append(note);
      item.setAttribute('aria-current', 'true');
    }
    const actions = el('div', 'sp-profile-actions');
    if (!entry.current) actions.append(textButton('Open', () => { host.send(profileText.open(entry.id), false); }, { aria: `Open ${entry.name}` }));
    const edit = (mode: 'rename' | 'delete') => (): void => { editing = { id: entry.id, mode }; repaint(); };
    if (!guest) {
      actions.append(iconButton('edit', `Rename ${entry.name}`, edit('rename'), 'sp-profile-act'));
      actions.append(iconButton('link', `Desktop shortcut for ${entry.name}`, () => { host.send(profileText.shortcut(entry.id), false); }, 'sp-profile-act'));
      if (entry.id !== 'default' && !entry.current) actions.append(iconButton('trash', `Delete ${entry.name}`, edit('delete'), 'sp-profile-act'));
    }
    item.append(disc('sp-profile-avatar', entry.colour, entry.glyph), text, actions);
  };

  const repaint = (): void => {
    if (editing && !profiles.list.some((p) => p.id === editing?.id)) editing = null;
    const focused = document.activeElement;
    const focusLabel = focused instanceof HTMLElement && list.contains(focused) ? focused.getAttribute('aria-label') : null;
    list.replaceChildren(...profiles.list.map((entry) => {
      const item = el('li', 'sp-profile');
      item.dataset.profile = entry.id;
      if (editing?.id === entry.id && editing.mode === 'rename') renameRow(entry, item);
      else if (editing?.id === entry.id && editing.mode === 'delete') deleteRow(entry, item);
      else normalRow(entry, item);
      return item;
    }));
    if (focusLabel !== null && !editing) {
      Array.from(list.querySelectorAll<HTMLElement>('[aria-label]')).find((node) => node.getAttribute('aria-label') === focusLabel)?.focus();
    }
  };

  return {
    parts: [list, look, form, formError],
    paint(next, isGuest) {
      const key = JSON.stringify([next, isGuest]);
      if (key === painted) return;
      painted = key;
      profiles = next;
      guest = isGuest;
      repaint();
      paintLook();
    },
  };
}
