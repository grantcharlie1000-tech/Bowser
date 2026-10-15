// Builds the toolbar DOM and applies state/theme messages to it.
import { MAX_TEXT, THEME_TOKENS, type DlButtonMsg, type ExtButtonMsg, type ReaderPill, type Security, type StateMsg, type Theme, type ThemeToken, type TranslatePill } from './protocol';
import { el, icon, type IconName } from './dom';
import { setTip } from './tooltip';
import { isZoomed, zoomChipLabel, zoomChipTip, zoomText } from './zoom-logic';
import { buttonLabel, buttonTip, ringMode, ringOffset } from './downloads-logic';
import { extButtonText } from './extensions-logic';

export interface View {
  readonly back: HTMLButtonElement;
  readonly forward: HTMLButtonElement;
  readonly reload: HTMLButtonElement;
  readonly site: HTMLButtonElement;
  readonly split: HTMLButtonElement;
  /** Zoom chip: shown only while the page is not at 100%; activating it resets the zoom. */
  readonly zoom: HTMLButtonElement;
  /** Reader view: shown only when the page looks like an article, pressed while it is on. */
  readonly reader: HTMLButtonElement;
  /** Translate page: shown only when the page is in another language, pressed while the translation is on. */
  readonly translate: HTMLButtonElement;
  readonly input: HTMLInputElement;
  /** Unfocused URL rendering with the host emphasised; visual only, aria-hidden. */
  readonly display: HTMLElement;
  readonly pill: HTMLElement;
  readonly progress: HTMLElement;
  /** Private-tab chip in the pill (glyph + "Private"); shown while state.private is true. */
  readonly privateChip: HTMLElement;
  readonly menu: HTMLButtonElement;
  /** The download button between the pill and the menu (docs/design/downloads.md). */
  readonly downloads: DownloadButton;
  /** The extensions button, left of the download button's slot (docs/extensions.md). */
  readonly extensions: ExtensionsButton;
}

/** Accessible name and tooltip of the pill's private-tab chip. */
export const PRIVATE_TIP = 'Private tab: browsing data is not saved';
const OMNI_LABEL = 'Address and search bar';

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
  const reader = iconButton('Reader view', 'reader', 'tb-reader bw-focus-inset is-round', 'Reader view (Ctrl+Alt+R)');
  reader.hidden = true;
  const translate = iconButton('Translate page', 'translate', 'tb-translate bw-focus-inset is-round', 'Translate this page');
  translate.hidden = true;

  const input = el('input', 'omni');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.dir = 'auto';
  input.maxLength = MAX_TEXT;
  input.placeholder = 'Search or enter address';
  input.setAttribute('aria-label', OMNI_LABEL);

  // Shown instead of the input text while the omnibox is unfocused (see main.ts showDisplay).
  const display = el('div', 'omni-display');
  display.setAttribute('aria-hidden', 'true');

  const progress = el('div', 'progress');
  progress.setAttribute('aria-hidden', 'true');

  const field = el('div', 'omni-field');
  field.append(input, display);

  // Private tab: a compact chip after the address (flex: none, so a long URL truncates before it).
  const privateChip = el('span', 'tb-private');
  privateChip.hidden = true;
  privateChip.setAttribute('role', 'img');
  privateChip.setAttribute('aria-label', PRIVATE_TIP);
  setTip(privateChip, PRIVATE_TIP);
  const privateText = el('span', 'tb-private-label');
  privateText.textContent = 'Private';
  privateChip.append(icon('private', { size: 14 }), privateText);

  const pill = el('div', 'pill');
  pill.append(site, field, privateChip, zoom, reader, translate, split, progress);

  // Three dots (menu-dots, drawn at 24px by toolbar.css .tb-menu); each is its own <circle>, so
  // they can ripple.
  const menu = iconButton('Menu', 'menu-dots', 'tb-nav tb-menu is-lg is-round');
  menu.setAttribute('aria-haspopup', 'menu');
  applyMenuOpen(menu, false);

  // Back and forward share one rounded container; reload is a plain icon beside it.
  const navGroup = el('div', 'nav-group');
  navGroup.append(back, forward);
  const nav = el('div', 'nav');
  nav.append(navGroup, reload);
  const downloads = buildDownloadButton(root);
  const extensions = buildExtensionsButton(root);
  const actions = el('div', 'actions');
  actions.append(extensions.button, downloads.button, menu);

  // Empty areas of the toolbar (the root and its flex wrappers) are the window drag surface.
  // Buttons, the omnibox and the pill are not marked, so they keep their own behaviour.
  root.setAttribute(DRAG_ATTR, '');
  nav.setAttribute(DRAG_ATTR, '');
  actions.setAttribute(DRAG_ATTR, '');
  root.style.setProperty('-webkit-user-select', 'none');

  root.replaceChildren(nav, pill, actions);
  return { back, forward, reload, site, split, zoom, reader, translate, input, display, pill, progress, privateChip, menu, downloads, extensions };
}

// ---- Extensions button (docs/extensions.md "Managing extensions", styles in toolbar.css) ----
//
// Shown only while Rust says an enabled extension has a popup (`extbtn.on`); Rust has already moved
// the pill to make room. The glyph is Bowser's own puzzle, never a stand-in for an extension's icon:
// the extension is named in the tooltip (one) or in the list Rust opens under the button (several).
// No badge, no action icon: WebView2 exposes neither.

export interface ExtensionsButton {
  readonly button: HTMLButtonElement;
  apply(msg: ExtButtonMsg): void;
}

function buildExtensionsButton(root: HTMLElement): ExtensionsButton {
  const button = iconButton('Extensions', 'puzzle', 'tb-nav is-lg is-round tb-ext');
  button.hidden = true;
  let outTimer = 0;
  button.addEventListener('animationend', (event) => {
    if (event.animationName === 'tb-dl-in' || event.animationName === 'bw-fade-in') button.classList.remove('is-in');
  });
  return {
    button,
    apply(msg: ExtButtonMsg): void {
      window.clearTimeout(outTimer);
      const text = extButtonText(msg);
      label(button, text.label, text.tip);
      if (text.menu) button.setAttribute('aria-haspopup', 'menu');
      else button.removeAttribute('aria-haspopup');
      if (msg.on) {
        button.classList.remove('is-out');
        if (button.hidden) {
          button.hidden = false;
          root.classList.add('has-ext');
          restart(button, 'is-in');
        }
        return;
      }
      if (button.hidden || button.classList.contains('is-out')) return;
      button.classList.remove('is-in');
      button.classList.add('is-out');
      outTimer = window.setTimeout(() => {
        button.hidden = true;
        button.classList.remove('is-out');
        root.classList.remove('has-ext');
      }, OUT_MS);
    },
  };
}

// ---- Download button (docs/design/downloads.md "Toolbar button", styles in toolbar.css) ----
//
// Markup: button.tb-dl[data-state][data-ring] >
//   svg.tb-dl-ring > circle.tb-dl-track, circle.tb-dl-arc (pathLength 100), g.tb-dl-spin > circle.tb-dl-sweep
//   span.tb-dl-glyph (the arrow), span.tb-dl-check (finish morph), span.tb-dl-drop (start
//   animation), span.tb-dl-halo (landing), span.tb-dl-dot (unseen finished / failed)
// Classes: .is-in (fading/scaling in), .is-out (fading out before `hidden`), .is-burst (a download
// started), .is-finish (arrow -> check, hold, back). Motion lives in toolbar.css.

export interface DownloadButton {
  readonly button: HTMLButtonElement;
  /** Applies a `dlbtn` message; the first one never plays the start animation. */
  apply(msg: DlButtonMsg): void;
}

const SVG_NS = 'http://www.w3.org/2000/svg';
/** The ring: 28px box, radius 11, so a 2px stroke spans 20-24px around the 28px hover disc's centre. */
const RING_R = '11';
/** Arrow -> check, hold, and back (toolbar.css .is-finish keyframes add up to this). */
const FINISH_MS = 2000;
const OUT_MS = 90;

function ringCircle(className: string): SVGCircleElement {
  const circle = document.createElementNS(SVG_NS, 'circle');
  circle.setAttribute('class', className);
  circle.setAttribute('cx', '14');
  circle.setAttribute('cy', '14');
  circle.setAttribute('r', RING_R);
  circle.setAttribute('pathLength', '100');
  return circle;
}

function restart(node: Element, className: string): void {
  node.classList.remove(className);
  void (node as HTMLElement).offsetWidth;
  node.classList.add(className);
}

function buildDownloadButton(root: HTMLElement): DownloadButton {
  const button = el('button', 'bw-icon-btn tb-nav is-lg is-round tb-dl');
  button.type = 'button';
  button.hidden = true;
  button.setAttribute('aria-haspopup', 'dialog');
  label(button, 'Downloads');
  const ring = document.createElementNS(SVG_NS, 'svg');
  ring.setAttribute('class', 'tb-dl-ring');
  ring.setAttribute('viewBox', '0 0 28 28');
  ring.setAttribute('aria-hidden', 'true');
  ring.setAttribute('focusable', 'false');
  const arc = ringCircle('tb-dl-arc');
  const spin = document.createElementNS(SVG_NS, 'g');
  spin.setAttribute('class', 'tb-dl-spin');
  spin.append(ringCircle('tb-dl-sweep'));
  ring.append(ringCircle('tb-dl-track'), arc, spin);
  const part = (className: string, glyph: IconName | null): HTMLElement => {
    const node = el('span', className);
    node.setAttribute('aria-hidden', 'true');
    if (glyph) node.append(icon(glyph));
    return node;
  };
  const glyph = part('tb-dl-glyph', 'download');
  const check = part('tb-dl-check', 'check');
  const drop = part('tb-dl-drop', 'download');
  const halo = part('tb-dl-halo', null);
  const dot = part('tb-dl-dot', null);
  button.append(ring, glyph, check, drop, halo, dot);
  button.dataset.state = 'idle';
  button.dataset.ring = 'none';
  // One-shot classes come off when their animation ends, so showing the button again never replays them.
  button.addEventListener('animationend', (event) => {
    if (event.animationName === 'tb-dl-in' || event.animationName === 'bw-fade-in') button.classList.remove('is-in');
    else if (event.animationName === 'tb-dl-drop') button.classList.remove('is-burst');
  });

  let last: DlButtonMsg | null = null;
  let finishTimer = 0;
  let outTimer = 0;

  const show = (on: boolean): void => {
    window.clearTimeout(outTimer);
    if (on) {
      button.classList.remove('is-out');
      if (button.hidden) {
        button.hidden = false;
        root.classList.add('has-dl');
        restart(button, 'is-in');
      }
      return;
    }
    if (button.hidden || button.classList.contains('is-out')) return;
    // Fade out (83ms) before the slot closes; the button takes input until it is gone.
    button.classList.remove('is-in');
    button.classList.add('is-out');
    outTimer = window.setTimeout(() => {
      button.hidden = true;
      button.classList.remove('is-out', 'is-burst', 'is-finish');
      root.classList.remove('has-dl');
    }, OUT_MS);
  };

  return {
    button,
    apply(msg: DlButtonMsg): void {
      const first = last === null;
      const prev = last;
      last = msg;
      show(msg.on);
      const mode = ringMode(msg);
      // A download that was running and is now finished plays the morph once.
      const finished = !first && msg.state === 'done' && prev !== null && (prev.state === 'active' || prev.state === 'paused');
      if (finished) {
        arc.style.setProperty('stroke-dashoffset', '0');
        button.dataset.ring = 'progress';
        restart(button, 'is-finish');
        window.clearTimeout(finishTimer);
        finishTimer = window.setTimeout(() => {
          button.classList.remove('is-finish');
          if (last) button.dataset.ring = ringMode(last);
        }, FINISH_MS);
      } else if (!button.classList.contains('is-finish') || mode !== 'none') {
        button.classList.remove('is-finish');
        window.clearTimeout(finishTimer);
        button.dataset.ring = mode;
        if (mode === 'progress' || mode === 'paused') arc.style.setProperty('stroke-dashoffset', String(ringOffset(msg.pct)));
      }
      // A zero-length arc would still draw its round cap as a dot.
      button.classList.toggle('is-zero', !finished && !button.classList.contains('is-finish') && (mode === 'progress' || mode === 'paused') && msg.pct <= 0);
      button.dataset.state = msg.state;
      if (!first && prev !== null && msg.burst !== prev.burst && msg.on) {
        restart(button, 'is-burst');
      }
      label(button, buttonLabel(msg), buttonTip(msg));
    },
  };
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

/** Shows the reader button only for an article-like page (or while reader view is on). */
export function applyReader(button: HTMLButtonElement, pill: ReaderPill): void {
  button.hidden = pill === 'none';
  button.setAttribute('aria-pressed', String(pill === 'on'));
  if (pill === 'on') label(button, 'Leave reader view', 'Leave reader view (Esc)');
  else label(button, 'Reader view', 'Reader view (Ctrl+Alt+R)');
}

/** Shows the translate button only when the page can be translated (or while the translation is on). */
export function applyTranslate(button: HTMLButtonElement, pill: TranslatePill): void {
  button.hidden = pill === 'none';
  button.setAttribute('aria-pressed', String(pill === 'on'));
  if (pill === 'on') label(button, 'Show original', 'Show the original page');
  else label(button, 'Translate page', 'Translate this page');
}

/** The menu button while the main menu is up (Rust `menuOpen`): the pressed bubble and spread dots. */
export function applyMenuOpen(button: HTMLButtonElement, open: boolean): void {
  button.setAttribute('aria-expanded', String(open));
}

/** The site button's glyph: a search glyph while there is no site (new tab, empty address), else the security glyph. */
export function syncSiteIcon(view: View): void {
  const security = (view.pill.dataset.security ?? 'none') as Security;
  const name = siteIconFor(security, view.input.value ?? '');
  if (view.site.dataset.glyph === name) return;
  view.site.dataset.glyph = name;
  view.site.replaceChildren(icon(name));
}

/** Pure choice of the site-button glyph (exported for tests). */
export function siteIconFor(security: Security, address: string): IconName {
  const text = address.trim();
  if (security === 'none' || (text === '' && security !== 'secure' && security !== 'notsecure' && security !== 'certproblem')) return 'search';
  return SECURITY_ICON[security];
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
  applyReader(view.reader, state.reader ?? 'none');
  applyTranslate(view.translate, state.translate ?? 'none');
  view.back.disabled = !state.back;
  view.forward.disabled = !state.forward;
  setReloadMode(view.reload, state.loading);

  // Transform only (scaleX): the bar never changes layout. Opacity hides it at rest and at 100%.
  const loadingNow = state.loading && state.progress < 100 && (state.showProgress ?? true);
  view.progress.style.transform = `scaleX(${Math.max(0, Math.min(1, state.progress / 100))})`;
  view.progress.style.opacity = loadingNow ? '1' : '0';
  view.pill.classList.toggle('is-loading', loadingNow);

  const site = view.site;
  view.pill.dataset.security = state.security;
  const priv = state.private === true;
  if (priv) {
    if (view.pill.dataset.private !== 'true') view.pill.dataset.private = 'true';
  } else if (view.pill.dataset.private !== undefined) {
    delete view.pill.dataset.private;
  }
  if (view.privateChip.hidden === priv) view.privateChip.hidden = !priv;
  const omniLabel = priv ? `${OMNI_LABEL}, private tab` : OMNI_LABEL;
  if (view.input.getAttribute('aria-label') !== omniLabel) view.input.setAttribute('aria-label', omniLabel);
  syncSiteIcon(view);
  const text = state.label.trim() !== '' ? state.label : SECURITY_LABEL[state.security];
  label(site, text);

  // Zoom chip: only while the page is zoomed and `show_zoom_indicator` is on. Nothing about the
  // pill's box changes at 100%.
  const zoomed = state.showZoom !== false && isZoomed(state.zoom);
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

export function applyTheme(theme: Theme, solid: boolean, dark: boolean, fontScale?: number, uiFont?: string): void {
  const root = document.documentElement;
  for (const token of THEME_TOKENS) {
    const value = theme[token];
    if (value !== undefined) root.style.setProperty(THEME_VAR[token], value);
  }
  document.body.classList.toggle('solid', solid);
  root.dataset.scheme = dark ? 'dark' : 'light';
  // `font_size`/`ui_font`: the chrome's text size and face. Absent (other HTML surfaces that do
  // not send them) leaves the page's own default alone.
  if (fontScale !== undefined) root.style.fontSize = `${fontScale}%`;
  if (uiFont !== undefined) root.style.fontFamily = uiFont;
}
