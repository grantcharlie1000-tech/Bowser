// --- profiles --- The profile manager on bowser://settings/profile: every profile with its avatar and
// name, Open, Rename (in place), a desktop shortcut and Delete (asked in the page), the current
// profile's avatar and colour, and a form that adds a named profile. Each change is one `profile`
// command (src/chrome_host/settings.rs `parse_profile_command`); Rust answers with a snapshot.

import { el, icon } from './dom';
import { onFill } from './menu-logic';
import { textButton } from './panels/kit';
import {
  MAX_PROFILE_NAME, PROFILE_ACCENTS, PROFILE_ACCENT_COLOURS, PROFILE_ACCENT_LABELS, PROFILE_AVATARS, PROFILE_AVATAR_LABELS,
  cleanProfileName, profileGlyph, profileNameError, profileText, type ProfileEntry,
} from './settings-page-logic';

export interface ProfilesView {
  /** The list, the current profile's look, and the add form, in page order. */
  readonly parts: readonly HTMLElement[];
  paint(profiles: readonly ProfileEntry[], guest: boolean): void;
}

export interface ProfilesHost {
  /** Sends one `profile` command; `answered` is true when Rust replies with Saved or an error. */
  send(text: string, answered: boolean): boolean;
}

function disc(glyph: string, colour: string, className = 'sp-profile-avatar'): HTMLElement {
  const node = el('span', className);
  node.setAttribute('aria-hidden', 'true');
  node.style.setProperty('--sp-profile-fill', colour);
  node.style.setProperty('--sp-profile-ink', onFill(colour));
  node.textContent = glyph;
  return node;
}

function iconAction(name: Parameters<typeof icon>[0], label: string, onClick: () => void): HTMLButtonElement {
  const button = el('button', 'bw-icon-btn sp-profile-act');
  button.type = 'button';
  button.setAttribute('aria-label', label);
  button.setAttribute('data-tip', label.split(' ')[0] ?? label);
  button.append(icon(name, { size: 14 }));
  button.addEventListener('click', onClick);
  return button;
}

export function mountProfiles(host: ProfilesHost): ProfilesView {
  let profiles: readonly ProfileEntry[] = [];
  let guest = false;
  let painted = '';
  /** The row being renamed or asked about, kept across snapshots. */
  let editing: { id: string; mode: 'rename' | 'delete' } | null = null;

  const list = el('ul', 'sp-profiles');
  list.setAttribute('aria-label', 'Profiles');

  // The current profile's look: avatar and colour as two radio rows.
  const look = el('div', 'sp-profile-look');
  const avatarGroup = el('div', 'sp-profile-choices');
  avatarGroup.setAttribute('role', 'radiogroup');
  avatarGroup.setAttribute('aria-label', 'Avatar');
  const colourGroup = el('div', 'sp-profile-choices');
  colourGroup.setAttribute('role', 'radiogroup');
  colourGroup.setAttribute('aria-label', 'Colour');
  const lookLabel = (text: string): HTMLElement => {
    const node = el('span', 'sp-profile-look-label');
    node.textContent = text;
    node.setAttribute('aria-hidden', 'true');
    return node;
  };
  look.append(lookLabel('Avatar'), avatarGroup, lookLabel('Colour'), colourGroup);
  const roving = (group: HTMLElement): void => {
    group.addEventListener('keydown', (event) => {
      const step = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : 0;
      if (step === 0) return;
      event.preventDefault();
      const buttons = Array.from(group.querySelectorAll<HTMLButtonElement>('button'));
      const index = buttons.findIndex((button) => button === document.activeElement);
      const next = buttons[(index + step + buttons.length) % buttons.length];
      next?.focus();
      next?.click();
    });
  };
  roving(avatarGroup);
  roving(colourGroup);

  // Add a profile: a name, then Add. Guest window sits beside it.
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
    const error = profileNameError(nameInput.value, profiles);
    if (error !== null) { formError.textContent = error; nameInput.focus(); return; }
    formError.textContent = '';
    if (host.send(profileText.create(nameInput.value), true)) nameInput.value = '';
  };
  nameInput.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); add(); } });
  nameInput.addEventListener('input', () => { formError.textContent = ''; });
  const guestButton = textButton('Guest window', () => { host.send(profileText.guest(), false); }, { icon: 'external', subtle: true });
  form.append(nameInput, textButton('Add profile', add, { icon: 'plus' }), guestButton);

  const renameRow = (entry: ProfileEntry, item: HTMLElement): void => {
    const input = el('input', 'bw-input sp-allow-input sp-profile-rename');
    input.type = 'text';
    input.value = entry.name;
    input.maxLength = MAX_PROFILE_NAME * 2;
    input.setAttribute('aria-label', `New name for ${entry.name}`);
    const error = el('p', 'sp-field-error');
    error.setAttribute('role', 'alert');
    const close = (): void => {
      editing = null;
      repaint();
      Array.from(list.querySelectorAll<HTMLElement>('.sp-profile')).find((item) => item.dataset.profile === entry.id)?.querySelector<HTMLElement>('.sp-profile-act')?.focus();
    };
    const commit = (): void => {
      const problem = profileNameError(input.value, profiles, entry.id);
      if (problem !== null) { error.textContent = problem; input.focus(); return; }
      if (cleanProfileName(input.value) !== entry.name) host.send(profileText.rename(entry.id, input.value), true);
      close();
    };
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') { event.preventDefault(); commit(); }
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
    });
    input.addEventListener('input', () => { error.textContent = ''; });
    const actions = el('div', 'sp-profile-actions');
    actions.append(textButton('Save', commit, { primary: true }), textButton('Cancel', close, { subtle: true }));
    item.append(disc(entry.glyph, entry.colour), input, actions, error);
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
    const keep = textButton('Keep', () => { editing = null; repaint(); }, { subtle: true });
    const remove = textButton('Delete', () => { editing = null; host.send(profileText.remove(entry.id), true); repaint(); }, { icon: 'trash' });
    remove.classList.add('is-danger');
    const actions = el('div', 'sp-profile-actions');
    actions.append(keep, remove);
    actions.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); keep.click(); } });
    item.append(disc(entry.glyph, entry.colour), text, actions);
    item.classList.add('is-asking');
    // The safe answer takes focus, so Enter right after the bin keeps the profile.
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
    if (!entry.current) {
      const open = textButton('Open', () => { host.send(profileText.open(entry.id), false); }, { aria: `Open ${entry.name}` });
      actions.append(open);
    }
    if (!guest) {
      actions.append(iconAction('edit', `Rename ${entry.name}`, () => { editing = { id: entry.id, mode: 'rename' }; repaint(); }));
      actions.append(iconAction('link', `Desktop shortcut for ${entry.name}`, () => { host.send(profileText.shortcut(entry.id), false); }));
      if (entry.id !== 'default' && !entry.current) {
        actions.append(iconAction('trash', `Delete ${entry.name}`, () => { editing = { id: entry.id, mode: 'delete' }; repaint(); }));
      }
    }
    item.append(disc(entry.glyph, entry.colour), text, actions);
  };

  const paintLook = (): void => {
    const me = profiles.find((p) => p.current);
    look.hidden = me === undefined;
    if (!me) return;
    avatarGroup.replaceChildren(...PROFILE_AVATARS.map((avatar) => {
      const button = el('button', 'sp-profile-choice');
      button.type = 'button';
      button.setAttribute('role', 'radio');
      button.setAttribute('aria-checked', String(avatar === me.avatar));
      button.setAttribute('aria-label', PROFILE_AVATAR_LABELS[avatar]);
      button.setAttribute('data-tip', PROFILE_AVATAR_LABELS[avatar]);
      button.tabIndex = avatar === me.avatar ? 0 : -1;
      button.append(disc(profileGlyph(avatar, me.name), me.colour, 'sp-profile-swatch'));
      button.addEventListener('click', () => { if (avatar !== me.avatar) host.send(profileText.look(me.id, avatar, me.accent), true); });
      return button;
    }));
    colourGroup.replaceChildren(...PROFILE_ACCENTS.map((accent) => {
      const button = el('button', 'sp-profile-choice');
      button.type = 'button';
      button.setAttribute('role', 'radio');
      button.setAttribute('aria-checked', String(accent === me.accent));
      button.setAttribute('aria-label', PROFILE_ACCENT_LABELS[accent]);
      button.setAttribute('data-tip', PROFILE_ACCENT_LABELS[accent]);
      button.tabIndex = accent === me.accent ? 0 : -1;
      button.append(disc(me.glyph, PROFILE_ACCENT_COLOURS[accent], 'sp-profile-swatch'));
      button.addEventListener('click', () => { if (accent !== me.accent) host.send(profileText.look(me.id, me.avatar, accent), true); });
      return button;
    }));
  };

  const repaint = (): void => {
    if (editing && !profiles.some((p) => p.id === editing?.id)) editing = null;
    // Focus stays on the same control across a redraw when it can (the radios keep their place).
    const focused = document.activeElement;
    const focusKey = focused instanceof HTMLElement && list.contains(focused) ? focused.getAttribute('aria-label') : null;
    list.replaceChildren(...profiles.map((entry) => {
      const item = el('li', 'sp-profile');
      item.dataset.profile = entry.id;
      if (editing?.id === entry.id && editing.mode === 'rename') renameRow(entry, item);
      else if (editing?.id === entry.id && editing.mode === 'delete') deleteRow(entry, item);
      else normalRow(entry, item);
      return item;
    }));
    if (focusKey !== null && !editing) {
      Array.from(list.querySelectorAll<HTMLElement>('[aria-label]')).find((node) => node.getAttribute('aria-label') === focusKey)?.focus();
    }
    const lookFocus = document.activeElement instanceof HTMLElement && look.contains(document.activeElement) ? document.activeElement.getAttribute('aria-label') : null;
    paintLook();
    if (lookFocus !== null) Array.from(look.querySelectorAll<HTMLElement>('[aria-label]')).find((node) => node.getAttribute('aria-label') === lookFocus)?.focus();
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
    },
  };
}
