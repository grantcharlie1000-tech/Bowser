// Builds the toolbar DOM and applies state/theme messages to it.
import { MAX_TEXT, THEME_TOKENS, type Security, type StateMsg, type Theme, type ThemeToken } from './protocol';
import { el, icon, type IconName } from './dom';

export interface View {
  readonly back: HTMLButtonElement;
  readonly forward: HTMLButtonElement;
  readonly reload: HTMLButtonElement;
  readonly site: HTMLButtonElement;
  readonly input: HTMLInputElement;
  readonly progress: HTMLElement;
  readonly newTab: HTMLButtonElement;
  readonly menu: HTMLButtonElement;
}

const SECURITY_ICON: Readonly<Record<Security, IconName>> = {
  secure: 'lock',
  notsecure: 'info',
  certproblem: 'warning',
  internal: 'info',
  unknown: 'info',
  none: 'info',
};

const SECURITY_LABEL: Readonly<Record<Security, string>> = {
  secure: 'Connection is secure',
  notsecure: 'Connection is not secure',
  certproblem: 'Certificate problem',
  internal: 'Browser page',
  unknown: 'Site information',
  none: 'Site information',
};

const THEME_VAR: Readonly<Record<ThemeToken, string>> = {
  bg: '--bg',
  fill: '--fill',
  border: '--border',
  focusBorder: '--focus-border',
  text: '--text',
  placeholder: '--placeholder',
  completion: '--completion',
  selection: '--selection',
  hover: '--hover',
  progress: '--progress',
};

function iconButton(label: string, name: IconName, className = 'btn'): HTMLButtonElement {
  const button = el('button', className);
  button.type = 'button';
  button.setAttribute('aria-label', label);
  button.title = label;
  button.append(icon(name));
  return button;
}

export function buildView(root: HTMLElement): View {
  const back = iconButton('Back', 'back');
  const forward = iconButton('Forward', 'forward');
  const reload = iconButton('Reload', 'reload');
  const site = iconButton('Site information', 'info', 'btn site');

  const input = el('input', 'omni');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.dir = 'auto';
  input.maxLength = MAX_TEXT;
  input.setAttribute('aria-label', 'Address and search bar');

  const progress = el('div', 'progress');
  progress.setAttribute('aria-hidden', 'true');

  const pill = el('div', 'pill');
  pill.append(site, input, progress);

  const newTab = iconButton('New tab', 'plus');
  const menu = iconButton('Menu', 'menu');

  const nav = el('div', 'nav');
  nav.append(back, forward, reload);
  const actions = el('div', 'actions');
  actions.append(newTab, menu);

  root.replaceChildren(nav, pill, actions);
  return { back, forward, reload, site, input, progress, newTab, menu };
}

function setReloadMode(button: HTMLButtonElement, loading: boolean): void {
  const mode: IconName = loading ? 'stop' : 'reload';
  if (button.dataset.mode === mode) return;
  button.dataset.mode = mode;
  const label = loading ? 'Stop' : 'Reload';
  button.setAttribute('aria-label', label);
  button.title = label;
  button.replaceChildren(icon(mode));
}

export function applyState(view: View, state: StateMsg): void {
  view.back.disabled = !state.back;
  view.forward.disabled = !state.forward;
  setReloadMode(view.reload, state.loading);

  view.progress.style.transform = `scaleX(${state.progress / 100})`;
  view.progress.style.opacity = state.loading && state.progress < 100 ? '1' : '0';

  const site = view.site;
  if (site.dataset.security !== state.security) {
    site.dataset.security = state.security;
    site.replaceChildren(icon(SECURITY_ICON[state.security]));
  }
  const text = state.label.trim() !== '' ? state.label : SECURITY_LABEL[state.security];
  site.setAttribute('aria-label', text);
  site.title = text;
}

export function applyTheme(theme: Theme, solid: boolean, dark: boolean): void {
  const root = document.documentElement;
  for (const token of THEME_TOKENS) {
    const value = theme[token];
    if (value !== undefined) root.style.setProperty(THEME_VAR[token], value);
  }
  document.body.classList.toggle('solid', solid);
  root.dataset.scheme = dark ? 'dark' : 'light';
}
