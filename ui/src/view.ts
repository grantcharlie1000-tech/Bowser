// Builds the toolbar DOM and applies state/theme messages to it.
import { MAX_TEXT, THEME_TOKENS, type Security, type StateMsg, type Theme, type ThemeToken } from './protocol';
import { el, icon, type IconName } from './dom';
import { setTip } from './tooltip';
import { isZoomed, zoomChipLabel, zoomChipTip, zoomText } from './zoom-logic';

export interface View {
  readonly back: HTMLButtonElement;
  readonly forward: HTMLButtonElement;
  readonly reload: HTMLButtonElement;
  readonly site: HTMLButtonElement;
  readonly split: HTMLButtonElement;
  /** Zoom chip: shown only while the page is not at 100%; activating it resets the zoom. */
  readonly zoom: HTMLButtonElement;
  readonly input: HTMLInputElement;
  /** Unfocused URL rendering with the host emphasised; visual only, aria-hidden. */
  readonly display: HTMLElement;
  readonly pill: HTMLElement;
  readonly progress: HTMLElement;
  readonly menu: HTMLButtonElement;
}

/** Attribute marking a non-interactive toolbar area as the drag surface (see main.ts). */
export const DRAG_ATTR = 'data-drag';

const SECURITY_ICON: Readonly<Record<Security, IconName>> = {
  secure: 'lock',
  notsecure: 'warning',
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

/** Accessible name plus the HTML tooltip text (the tooltip layer reads data-tip; no title=). */
function label(button: HTMLElement, name: string, tip: string = name): void {
  button.setAttribute('aria-label', name);
  setTip(button, tip);
}

/** Toolbar icon button: shared .bw-icon-btn with the one toolbar icon weight (toolbar.css). */
function iconButton(text: string, name: IconName, className: string, tip?: string): HTMLButtonElement {
  const button = el('button', `bw-icon-btn ${className}`);
  button.type = 'button';
  label(button, text, tip);
  button.append(icon(name));
  return button;
}

/** A URL split for display. */
export interface UrlParts {
  /** "http://" for plain http, empty for https (the lock says it). */
  readonly scheme: string;
  readonly host: string;
  /** Path, query and fragment; empty for a bare origin. */
  readonly rest: string;
}

/** Null when `text` is not a plain http(s) URL (search text, other schemes, user-info, partial input). */
export function splitUrl(text: string): UrlParts | null {
  if (text.length > MAX_TEXT || /\s/.test(text)) return null;
  const m = /^(https?):\/\/([^/?#@]+)(.*)$/i.exec(text);
  if (!m) return null;
  const scheme = (m[1] ?? '').toLowerCase();
  const rest = m[3] ?? '';
  return {
    scheme: scheme === 'https' ? '' : `${scheme}://`,
    host: m[2] ?? '',
    rest: rest === '/' ? '' : rest,
  };
}

export function buildView(root: HTMLElement): View {
  const back = iconButton('Back', 'back', 'tb-nav is-lg is-round', 'Back (Alt+Left)');
  const forward = iconButton('Forward', 'forward', 'tb-nav is-lg is-round', 'Forward (Alt+Right)');
  const reload = iconButton('Reload', 'reload', 'tb-nav is-lg is-round', 'Reload (Ctrl+R)');
  const site = iconButton('Site information', 'info', 'tb-site bw-focus-inset is-round');
  const split = iconButton('Split view', 'split', 'tb-split bw-focus-inset is-round');
  const zoom = el('button', 'tb-zoom bw-focus-inset');
  zoom.type = 'button';
  zoom.hidden = true;

  const input = el('input', 'omni');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.dir = 'auto';
  input.maxLength = MAX_TEXT;
  input.placeholder = 'Search or enter address';
  input.setAttribute('aria-label', 'Address and search bar');

  // Shown instead of the input text while the omnibox is unfocused (see main.ts showDisplay).
  const display = el('div', 'omni-display');
  display.setAttribute('aria-hidden', 'true');

  const progress = el('div', 'progress');
  progress.setAttribute('aria-hidden', 'true');

  const field = el('div', 'omni-field');
  field.append(input, display);

  const pill = el('div', 'pill');
  pill.append(site, field, zoom, split, progress);

  const menu = iconButton('Menu', 'menu', 'tb-nav is-lg is-round');

  // Back and forward share one rounded container; reload is a plain icon beside it.
  const navGroup = el('div', 'nav-group');
  navGroup.append(back, forward);
  const nav = el('div', 'nav');
  nav.append(navGroup, reload);
  const actions = el('div', 'actions');
  actions.append(menu);

  // Empty areas of the toolbar (the root and its flex wrappers) are the window drag surface.
  // Buttons, the omnibox and the pill are not marked, so they keep their own behaviour.
  root.setAttribute(DRAG_ATTR, '');
  nav.setAttribute(DRAG_ATTR, '');
  actions.setAttribute(DRAG_ATTR, '');
  root.style.setProperty('-webkit-user-select', 'none');

  root.replaceChildren(nav, pill, actions);
  return { back, forward, reload, site, split, zoom, input, display, pill, progress, menu };
}

/**
 * Renders `text` into the unfocused display and reports whether it is a URL worth emphasising.
 * Called on setText and blur only, never per keystroke.
 */
export function renderDisplay(view: View, text: string): boolean {
  const parts = splitUrl(text);
  if (!parts) {
    view.display.replaceChildren();
    return false;
  }
  const host = el('span', 'u-host');
  host.textContent = parts.host;
  const nodes: Node[] = [];
  if (parts.scheme !== '') {
    const scheme = el('span', 'u-rest');
    scheme.textContent = parts.scheme;
    nodes.push(scheme);
  }
  nodes.push(host);
  if (parts.rest !== '') {
    const rest = el('span', 'u-rest');
    rest.textContent = parts.rest;
    nodes.push(rest);
  }
  view.display.replaceChildren(...nodes);
  return true;
}

function setReloadMode(button: HTMLButtonElement, loading: boolean): void {
  const mode: IconName = loading ? 'stop' : 'reload';
  if (button.dataset.mode === mode) return;
  button.dataset.mode = mode;
  if (loading) label(button, 'Stop');
  else label(button, 'Reload', 'Reload (Ctrl+R)');
  button.replaceChildren(icon(mode));
}

export function applyState(view: View, state: StateMsg): void {
  const root = document.getElementById('app');
  if (root) {
    if (state.pillx !== undefined && state.pillw !== undefined && state.menux !== undefined) {
      root.classList.add('placed');
      root.style.setProperty('--pill-x', `${state.pillx}px`);
      root.style.setProperty('--pill-w', `${state.pillw}px`);
      root.style.setProperty('--menu-x', `${state.menux}px`);
    } else {
      root.classList.remove('placed');
      root.style.removeProperty('--pill-x');
      root.style.removeProperty('--pill-w');
      root.style.removeProperty('--menu-x');
    }
  }
  if (state.split !== undefined) view.split.setAttribute('aria-pressed', String(state.split)); // --- omnibox suggest ---
  view.back.disabled = !state.back;
  view.forward.disabled = !state.forward;
  setReloadMode(view.reload, state.loading);

  // Transform only (scaleX): the bar never changes layout. Opacity hides it at rest and at 100%.
  const loadingNow = state.loading && state.progress < 100;
  view.progress.style.transform = `scaleX(${Math.max(0, Math.min(1, state.progress / 100))})`;
  view.progress.style.opacity = loadingNow ? '1' : '0';
  view.pill.classList.toggle('is-loading', loadingNow);

  const site = view.site;
  if (site.dataset.security !== state.security) {
    site.dataset.security = state.security;
    site.replaceChildren(icon(SECURITY_ICON[state.security]));
  }
  view.pill.dataset.security = state.security;
  const text = state.label.trim() !== '' ? state.label : SECURITY_LABEL[state.security];
  label(site, text);

  // Zoom chip: only while the page is zoomed. Nothing about the pill's box changes at 100%.
  const zoomed = isZoomed(state.zoom);
  view.zoom.hidden = !zoomed;
  if (zoomed) {
    const zoom = state.zoom as number;
    if (view.zoom.dataset.zoom !== String(zoom)) {
      view.zoom.dataset.zoom = String(zoom);
      view.zoom.replaceChildren(icon(zoom > 100 ? 'zoom-in' : 'zoom-out', { size: 12, fine: true }), document.createTextNode(zoomText(zoom)));
      label(view.zoom, zoomChipLabel(zoom), zoomChipTip(zoom));
    }
  } else {
    delete view.zoom.dataset.zoom;
  }
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
