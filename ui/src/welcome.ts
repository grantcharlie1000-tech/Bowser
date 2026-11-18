// The first-run welcome (`bowser://settings/welcome`, docs/design/welcome.md, styles in settings.css
// "Welcome"). The settings page mounts it and shows it in place of its sections while Bowser's
// `settingsgo` names the `welcome` section. Every choice goes through the settings page's own
// commands (the host's `set*` callbacks and `send`), so Rust checks them exactly as it checks Settings.
// The words and the choice mapping are in welcome-logic.ts.
//
// Layout: the first and last steps are a centred hero. The steps between are two columns: the
// question on the left, and on the right a live preview of a Bowser window that follows every pick
// (tabs on top or down the side, light or dark, the accent) before the snapshot comes back.
import { el, icon } from './dom';
import { radioGroup, textButton, type RadioGroup } from './panels/kit';
import type { PanelFields } from './protocol';
import type { SettingsSnapshot } from './settings-page-logic';
import { engineFields } from './address-settings-logic';
import { BRAND_MARKS } from './brand-marks';
import {
  ACCENTS, BACK_LABEL, BLOCK_CHOICES, CONTINUE_LABEL, DEFAULT_LABEL, ENGINE_CHOICES, FINISH_LABEL, IMPORT_BROWSERS, IMPORT_FAN,
  IMPORT_FILE_LABEL, IMPORT_OPENED, searchPlaceholder, LOOK_CHOICES, NOT_NOW_LABEL, READ_ONLY_NOTE, SETTINGS_LABEL, SHORTCUTS, SKIP_SETUP_LABEL, START_LABEL, STEP_COPY,
  STEP_NAMES, WELCOME_STEPS, defaultStatus, hasBack, lookChoiceOf, offeredEngines, progressText, stepAfter, stepBefore, tabChoiceOf,
  type BlockChoice, type LookChoice, type TabChoice, type WelcomeStep,
} from './welcome-logic';

export interface WelcomeHost {
  setLook(choice: LookChoice): void;
  setAccent(hex: string): void;
  /** Ad blocking on (the `ads` level) or off (`none`). */
  setBlocking(on: boolean): void;
  /** A settings command (`import`, `importfrom`, `engine`, `setdefault`, `welcomedone`). */
  send(fields: PanelFields): boolean;
  /** Leaves the welcome for the top of Settings (the tab stays). */
  openSettings(): void;
}

export interface Welcome {
  readonly root: HTMLElement;
  readonly shown: boolean;
  show(): void;
  hide(): void;
  update(snapshot: SettingsSnapshot): void;
  /** The address section's snapshot: the default engine's id and every engine's id. */
  engines(def: string, ids: readonly string[]): void;
}

/** Sets the stagger index an entrance animation reads (settings.css `--wl-i`). */
function stagger(nodes: readonly HTMLElement[], from = 0): void {
  nodes.forEach((node, index) => node.style.setProperty('--wl-i', String(from + index)));
}

/** A browser's or engine's own full-colour logo (brand-marks.ts), or the Bowser logo for `bowser`. */
function logoImg(key: string, className: string): HTMLImageElement {
  const img = el('img', className);
  img.alt = '';
  img.setAttribute('src', key === 'bowser' ? 'bowser-logo.png' : BRAND_MARKS[key]?.src ?? '');
  img.setAttribute('draggable', 'false');
  img.setAttribute('aria-hidden', 'true');
  return img;
}

/**
 * A logo tile (Import's browsers, Search's engines): a soft glow in the brand's colour behind the
 * logo, the name under it, and a tick while it is the one picked.
 */
function logoFace(name: string, key: string, art: Element = logoImg(key, 'wl-brand')): (button: HTMLButtonElement) => void {
  return (button) => {
    button.className = 'wl-tile wl-logo-tile';
    const color = BRAND_MARKS[key]?.color;
    if (color) button.style.setProperty('--wl-brand', color);
    const glow = el('span', 'wl-logo-glow');
    glow.setAttribute('aria-hidden', 'true');
    const well = el('span', 'wl-logo-well');
    well.append(art);
    const label = el('span', 'wl-logo-name');
    label.textContent = name;
    const check = el('span', 'wl-tick');
    check.setAttribute('aria-hidden', 'true');
    check.append(icon('check', { size: 12 }));
    button.append(glow, well, label, check);
  };
}

/** A choice tile: a picture, a name, a line under it, and a check that shows while it is picked. */
function tileFace(name: string, hint: string, art: Element): (button: HTMLButtonElement) => void {
  return (button) => {
    button.className = 'sp-tile wl-tile';
    const label = el('span', 'sp-tile-name');
    label.textContent = name;
    const check = el('span', 'wl-tick');
    check.setAttribute('aria-hidden', 'true');
    check.append(icon('check', { size: 12 }));
    button.append(art, label, check);
    if (hint !== '') {
      const line = el('span', 'wl-tile-hint');
      line.textContent = hint;
      button.append(line);
    }
  };
}

/** The look tile's picture: a tiny window in light, dark, or split between the two. */
function lookArt(choice: LookChoice): HTMLElement {
  const frame = el('span', `wl-look is-${choice}`);
  frame.setAttribute('aria-hidden', 'true');
  frame.append(el('span', 'wl-look-bar'), el('span', 'wl-look-page'));
  return frame;
}

/** A shape in the mock window. */
function block(className: string): HTMLElement {
  const node = el('span', className);
  node.setAttribute('aria-hidden', 'true');
  return node;
}

/**
 * A Bowser window drawn to scale in spans (they sit inside tile buttons, so phrasing content only):
 * the slim tab strip and toolbar as Bowser draws them, or the side list with the address on top of
 * it, then the page. Every size is in container units (settings.css), so the same mock is the big
 * preview and a tile picture. Light or dark and the accent come from the welcome's root.
 */
function mockWindow(className: string, tabs?: TabChoice): HTMLElement {
  const box = el('span', className);
  box.setAttribute('aria-hidden', 'true');
  if (tabs) box.dataset.tabs = tabs;
  const win = block('wl-pv-win');
  const strip = block('wl-pv-strip');
  for (let i = 0; i < 3; i += 1) strip.append(block(`wl-pv-tab${i === 0 ? ' is-on' : ''}`));
  strip.append(block('wl-pv-caption'));
  const bar = block('wl-pv-bar');
  const pill = block('wl-pv-pill');
  pill.append(block('wl-pv-shield'), block('wl-pv-url'));
  bar.append(block('wl-pv-nav'), pill, block('wl-pv-tools'));
  const body = block('wl-pv-body');
  const side = block('wl-pv-side');
  const sidePill = block('wl-pv-pill is-side');
  sidePill.append(block('wl-pv-shield'), block('wl-pv-url'));
  side.append(block('wl-pv-nav'), sidePill);
  for (let i = 0; i < 5; i += 1) side.append(block(`wl-pv-row${i === 1 ? ' is-on' : ''}`));
  const page = block('wl-pv-page');
  page.append(block('wl-pv-hero'), block('wl-pv-line is-wide'), block('wl-pv-line'), block('wl-pv-line is-short'));
  const cards = block('wl-pv-cards');
  for (let i = 0; i < 3; i += 1) cards.append(block('wl-pv-card'));
  page.append(cards);
  body.append(side, page);
  win.append(strip, bar, body);
  box.append(win);
  return box;
}

export function mountWelcome(host: WelcomeHost): Welcome {
  const root = el('div', 'wl');
  root.hidden = true;
  root.setAttribute('role', 'region');
  root.setAttribute('aria-label', 'Welcome to Bowser');

  // ---- top: progress and Skip setup ----
  const top = el('div', 'wl-top');
  const dots = el('ol', 'wl-dots');
  const dotNodes = new Map<WelcomeStep, HTMLElement>();
  for (const step of WELCOME_STEPS) {
    const dot = el('li', 'wl-dot');
    dot.dataset.step = step;
    dot.dataset.tip = STEP_NAMES[step];
    dotNodes.set(step, dot);
    dots.append(dot);
  }
  const finish = (): void => { host.send({ a: 'welcomedone' }); };
  const skip = textButton(SKIP_SETUP_LABEL, finish, { subtle: true });
  skip.classList.add('wl-skip');
  top.append(dots, skip);

  // ---- the step: hero mark, count, title, lead, its own body, a status line, the buttons ----
  const main = el('div', 'wl-main');
  const stage = el('div', 'wl-stage');
  const mark = el('span', 'wl-mark');
  mark.setAttribute('aria-hidden', 'true');
  // The Bowser logo (ui/src/assets/bowser-logo.png, served as bowser-logo.png).
  const logo = el('img', 'wl-logo');
  logo.alt = '';
  logo.width = 96;
  logo.height = 96;
  logo.setAttribute('src', 'bowser-logo.png');
  logo.setAttribute('draggable', 'false');
  mark.append(logo);
  const count = el('p', 'wl-count');
  const title = el('h1', 'wl-title');
  title.id = 'wl-title';
  title.tabIndex = -1;
  root.setAttribute('aria-labelledby', title.id);
  const lead = el('p', 'wl-lead');
  const body = el('div', 'wl-body');
  const note = el('p', 'wl-note');
  note.setAttribute('role', 'status');
  note.setAttribute('aria-live', 'polite');
  const readOnly = el('p', 'wl-note is-warn');
  readOnly.textContent = READ_ONLY_NOTE;
  readOnly.hidden = true;

  const foot = el('div', 'wl-foot');
  const back = textButton(BACK_LABEL, () => { const to = stepBefore(current); if (to) go(to); }, { icon: 'chevron-left', subtle: true });
  back.classList.add('wl-back');
  const spacer = el('span', 'wl-spacer');
  const secondary = textButton('', () => secondaryAction(), { subtle: true });
  secondary.classList.add('wl-secondary');
  const primary = textButton('', () => primaryAction(), { primary: true });
  primary.classList.add('wl-primary');
  const primaryText = primary.lastChild as Text;
  const primaryArrow = icon('arrow-right', { size: 14 });
  primaryArrow.classList.add('wl-arrow');
  primary.append(primaryArrow);
  foot.append(back, spacer, secondary, primary);

  stage.append(mark, count, title, lead, body, note, readOnly, foot);
  const preview = mockWindow('wl-pv', 'top');
  main.append(stage, preview);
  root.append(top, main);

  // ---- the bodies, built once so a control keeps its state between visits ----
  let importAsked = false;
  let defaultAsked = false;
  let snapshot: SettingsSnapshot | null = null;

  // Import: a transfer picture (the picked browser, dots flowing across, Bowser), then one big logo
  // tile per browser (Rust reads that browser's profile and previews before importing), and a file.
  const transfer = el('div', 'wl-transfer');
  transfer.setAttribute('aria-hidden', 'true');
  const from = el('span', 'wl-transfer-slot is-from');
  const fan = el('span', 'wl-fan');
  for (const key of IMPORT_FAN) fan.append(logoImg(key, 'wl-fan-logo'));
  from.append(fan);
  const line = el('span', 'wl-transfer-line');
  for (let i = 0; i < 3; i += 1) line.append(el('span', 'wl-transfer-dot'));
  const to = el('span', 'wl-transfer-slot is-to');
  to.append(logoImg('bowser', 'wl-transfer-logo'));
  transfer.append(from, line, to);
  line.addEventListener('animationend', (event) => { if ((event.target as HTMLElement).classList.contains('wl-transfer-dot')) transfer.classList.remove('is-moving'); });
  const showFrom = (key: string): void => {
    const logo = logoImg(key, 'wl-transfer-logo');
    from.replaceChildren(logo);
    from.classList.remove('is-pop');
    void from.offsetWidth;
    from.classList.add('is-pop');
    transfer.classList.remove('is-moving');
    void transfer.offsetWidth;
    transfer.classList.add('is-moving');
  };

  const importTiles = el('div', 'wl-logos');
  importTiles.setAttribute('role', 'group');
  importTiles.setAttribute('aria-label', 'Import from');
  const importButtons: HTMLButtonElement[] = [];
  for (const [key, name] of IMPORT_BROWSERS) {
    const button = el('button');
    button.type = 'button';
    logoFace(name, key)(button);
    button.classList.add('wl-browser');
    button.dataset.browser = key;
    button.addEventListener('click', () => {
      if (!host.send({ a: 'importfrom', text: key })) return;
      importAsked = true;
      for (const other of importButtons) other.setAttribute('aria-pressed', String(other === button));
      showFrom(key);
      paintNote();
    });
    importButtons.push(button);
    importTiles.append(button);
  }
  const importFile = el('button');
  importFile.type = 'button';
  const fileArt = el('span', 'wl-file-glyph');
  fileArt.append(icon('file', { size: 22 }));
  logoFace(IMPORT_FILE_LABEL, 'file', fileArt)(importFile);
  importFile.classList.add('wl-file');
  importFile.setAttribute('aria-label', 'Import from a bookmarks file');
  importFile.addEventListener('click', () => {
    if (host.send({ a: 'import' })) { importAsked = true; paintNote(); }
  });
  importTiles.append(importFile);
  const importBody = el('div', 'wl-stack');
  importBody.append(transfer, importTiles);
  // The tiles arrive one after another (settings.css, `--wl-i` inside the grid).
  stagger([...importButtons, importFile]);

  // The preview follows a pick at once; the next snapshot confirms it (or puts it back).
  const paintPreview = (picks: { tabs?: TabChoice | null; look?: LookChoice | null; accent?: string }): void => {
    if (picks.tabs !== undefined) preview.dataset.tabs = picks.tabs ?? 'top';
    if (picks.look !== undefined) root.dataset.look = picks.look ?? 'system';
    if (picks.accent !== undefined) root.style.setProperty('--wl-pick', picks.accent === '' ? 'var(--bw-accent)' : picks.accent);
  };


  const looks: RadioGroup<LookChoice> = radioGroup<LookChoice>('sp-tiles wl-tiles is-three', 'Appearance',
    LOOK_CHOICES.map(([value, name]) => [value, tileFace(name, '', lookArt(value))] as const), (value) => { paintPreview({ look: value }); host.setLook(value); });
  const accents: RadioGroup<string> = radioGroup<string>('wl-swatches', 'Accent colour', ACCENTS.map(([hex, name]) => [hex === '' ? 'auto' : hex, (button: HTMLButtonElement) => {
    button.className = `wl-swatch${hex === '' ? ' is-auto' : ''}`;
    button.setAttribute('aria-label', name);
    button.dataset.tip = name;
    if (hex !== '') button.style.setProperty('--wl-swatch', hex);
    const dot = el('span', 'wl-swatch-dot');
    dot.setAttribute('aria-hidden', 'true');
    button.append(dot);
  }] as const), (value) => { const hex = value === 'auto' ? '' : value; paintPreview({ accent: hex }); host.setAccent(hex); });
  const accentLabel = el('p', 'wl-label');
  accentLabel.textContent = 'Accent';
  const lookBody = el('div', 'wl-stack');
  lookBody.append(looks.root, accentLabel, accents.root);

  // Ad blocking: two answers, each a radio row with its strong word first.
  const blocking: RadioGroup<BlockChoice> = radioGroup<BlockChoice>('wl-choices', 'Block ads',
    BLOCK_CHOICES.map(([value, word, rest]) => [value, (button: HTMLButtonElement) => {
      button.className = 'wl-choice';
      const dot = el('span', 'wl-radio');
      dot.setAttribute('aria-hidden', 'true');
      const text = el('span', 'wl-choice-text');
      const strong = el('strong');
      strong.textContent = word;
      text.append(strong, document.createTextNode(rest));
      button.append(dot, text);
    }] as const), (value) => host.setBlocking(value === 'yes'));
  // Search: a big preview of the search bar that follows the pick, then the engines as round icons.
  const searchBar = el('div', 'wl-searchbar');
  searchBar.setAttribute('aria-hidden', 'true');
  const barLogo = el('span', 'wl-searchbar-logo');
  barLogo.append(icon('search', { size: 18 }));
  const barText = el('span', 'wl-searchbar-text');
  barText.textContent = 'Search or type an address';
  const barKey = el('kbd', 'wl-kbd');
  barKey.textContent = 'Enter';
  searchBar.append(barLogo, barText, barKey);
  const paintSearchBar = (id: string | null): void => {
    const choice = ENGINE_CHOICES.find(([key]) => key === id);
    if (!choice) return;
    barLogo.replaceChildren(logoImg(choice[0], 'wl-brand'));
    barText.textContent = searchPlaceholder(choice[1]);
    barLogo.classList.remove('is-pop');
    void barLogo.offsetWidth;
    barLogo.classList.add('is-pop');
  };
  const engines: RadioGroup<string> = radioGroup<string>('wl-logos is-engines', 'Search engine',
    ENGINE_CHOICES.map(([id, name]) => [id, (button: HTMLButtonElement) => {
      logoFace(name, id)(button);
      button.dataset.engine = id;
    }] as const), (id) => {
      const fields = engineFields({ op: 'default', id });
      if (fields) host.send(fields);
      paintSearchBar(id);
    });
  const searchBody = el('div', 'wl-stack');
  searchBody.append(searchBar, engines.root);
  stagger(Array.from(engines.root.children) as HTMLElement[]);
  const privacyBody = blocking.root;

  // Default browser: a Windows "Default apps" card for the three things a default browser opens.
  // Each row's app flips from Edge to Bowser, one after another, once Bowser is (or was asked to be)
  // the default. The footer's primary button is the action ("Set as default"), then Continue.
  const defaultArt = el('div', 'wl-defaults');
  defaultArt.setAttribute('aria-hidden', 'true');
  const appChip = (key: string, name: string, className: string): HTMLElement => {
    const chip = el('span', `wl-def-app ${className}`);
    const words = el('span');
    words.textContent = name;
    chip.append(logoImg(key, 'wl-def-logo'), words);
    return chip;
  };
  ['.html', 'HTTP', 'HTTPS'].forEach((kind, index) => {
    const row = el('div', 'wl-def-row');
    row.style.setProperty('--wl-i', String(index));
    const label = el('span', 'wl-def-kind');
    label.textContent = kind;
    const swap = el('span', 'wl-def-swap');
    swap.append(appChip('edge', 'Edge', 'is-old'), appChip('bowser', 'Bowser', 'is-new'));
    row.append(label, swap, icon('chevron-right', { size: 14 }));
    defaultArt.append(row);
  });
  const defaultBody = defaultArt;
  /** The Default step's primary button asks Windows instead of moving on. */
  const asksDefault = (): boolean => current === 'default' && !defaultAsked && snapshot?.defaultBrowser !== 'yes' && snapshot?.guest !== true;

  const keys = el('ul', 'wl-keys');
  keys.setAttribute('aria-label', 'Keyboard shortcuts');
  for (const shortcut of SHORTCUTS) {
    const item = el('li', 'wl-key');
    const combo = el('span', 'wl-combo');
    shortcut.keys.forEach((key, index) => {
      if (index > 0) { const plus = el('span', 'wl-plus'); plus.textContent = '+'; plus.setAttribute('aria-hidden', 'true'); combo.append(plus); }
      const cap = el('kbd', 'wl-kbd');
      cap.textContent = key;
      combo.append(cap);
    });
    const text = el('span', 'wl-key-label');
    text.textContent = shortcut.label;
    item.append(text, combo);
    keys.append(item);
  }

  const bodies: Readonly<Record<WelcomeStep, HTMLElement | null>> = {
    hello: null, import: importBody, look: lookBody, search: searchBody, privacy: privacyBody, default: defaultBody, done: keys,
  };

  // ---- painting ----
  let current: WelcomeStep = 'hello';

  function paintNote(): void {
    if (current === 'import') note.textContent = importAsked ? IMPORT_OPENED : '';
    else if (current === 'default') note.textContent = defaultStatus(snapshot?.defaultBrowser ?? 'unknown', defaultAsked);
    else note.textContent = '';
    note.hidden = note.textContent === '';
  }

  function primaryAction(): void {
    if (current === 'done') { finish(); return; }
    if (asksDefault()) {
      if (host.send({ a: 'setdefault' })) {
        defaultAsked = true;
        paintButtons();
        paintNote();
      }
      return;
    }
    const to = stepAfter(current);
    if (to) go(to);
  }

  function secondaryAction(): void {
    if (current === 'hello') { finish(); return; }
    if (current === 'done') { host.openSettings(); return; }
    const to = stepAfter(current);
    if (to) go(to);
  }

  function paintButtons(): void {
    back.hidden = !hasBack(current);
    skip.hidden = current === 'done' || current === 'hello';
    primaryText.textContent = current === 'hello' ? START_LABEL : current === 'done' ? FINISH_LABEL : asksDefault() ? DEFAULT_LABEL : CONTINUE_LABEL;
    defaultArt.classList.toggle('is-set', snapshot?.defaultBrowser === 'yes' || defaultAsked);
    secondary.lastChild!.textContent = current === 'hello' ? SKIP_SETUP_LABEL : current === 'done' ? SETTINGS_LABEL : NOT_NOW_LABEL;
    // A step with nothing to do (or nothing left to do) needs only Continue.
    const optional = current === 'import' || asksDefault();
    secondary.hidden = !(current === 'hello' || current === 'done' || optional);
  }

  function go(step: WelcomeStep, focus = true): void {
    const changed = step !== current;
    const backwards = WELCOME_STEPS.indexOf(step) < WELCOME_STEPS.indexOf(current);
    current = step;
    root.dataset.step = step;
    const copy = STEP_COPY[step];
    title.textContent = copy.title;
    lead.textContent = copy.lead;
    lead.hidden = copy.lead === '';
    const at = WELCOME_STEPS.indexOf(step);
    count.textContent = `${at} of ${WELCOME_STEPS.length - 2}`;
    const content = bodies[step];
    body.replaceChildren(...(content ? [content] : []));
    body.hidden = content === null;
    WELCOME_STEPS.forEach((key, index) => {
      const dot = dotNodes.get(key);
      if (!dot) return;
      dot.classList.toggle('is-done', index < at);
      if (key === step) dot.setAttribute('aria-current', 'step'); else dot.removeAttribute('aria-current');
    });
    dots.setAttribute('aria-label', `${progressText(step)}: ${STEP_NAMES[step]}`);
    paintButtons();
    paintNote();
    // The entrance: the step's parts rise in one after another (`--wl-i`), then its tiles or keys; a
    // step reached with Back comes in from the other side. The mock window never moves.
    stagger([mark, count, title, lead]);
    if (content) stagger(Array.from(content.children) as HTMLElement[], 4);
    stagger([note, foot], 5 + (content?.children.length ?? 0));
    if (changed) {
      stage.classList.remove('is-in', 'is-back');
      void stage.offsetWidth; // restart the entrance for the new step
      stage.classList.add('is-in');
      stage.classList.toggle('is-back', backwards);
    }
    if (focus) title.focus({ preventScroll: true });
  }

  // The entrance ends with the footer, the last part to arrive.
  foot.addEventListener('animationend', () => stage.classList.remove('is-in', 'is-back'));

  root.addEventListener('keydown', (event) => {
    // Enter on the page (not on a control) moves on, as the primary button would.
    if (event.key === 'Enter' && (event.target === title || event.target === root)) {
      event.preventDefault();
      primaryAction();
    }
  });

  paintPreview({ tabs: 'top', look: 'system', accent: '' });
  go('hello', false);

  return {
    root,
    get shown() { return !root.hidden; },
    show(): void {
      if (!root.hidden) return;
      root.hidden = false;
      go('hello', true);
    },
    hide(): void { root.hidden = true; },
    update(next: SettingsSnapshot): void {
      snapshot = next;
      const tabChoice = tabChoiceOf(next.tabLayout);
      const look = lookChoiceOf(String(next.themeOptions.theme_schedule), String(next.themeOptions.theme_look));
      looks.set(look);
      accents.set(next.themeAccent === '' ? 'auto' : (ACCENTS.some(([hex]) => hex === next.themeAccent) ? next.themeAccent : null));
      paintPreview({ tabs: tabChoice, look, accent: next.themeAccent });
      blocking.set(next.blockerEnabled ? 'yes' : 'no');
      const locked = !next.readable;
      for (const group of [looks, accents, blocking, engines]) group.disable(locked);
      readOnly.hidden = !locked;
      for (const button of [...importButtons, importFile]) button.disabled = next.guest;
      paintButtons();
      paintNote();
    },
    engines(def: string, ids: readonly string[]): void {
      const offered = new Set(offeredEngines(ids).map(([id]) => id));
      for (const button of Array.from(engines.root.children) as HTMLElement[]) button.hidden = !offered.has(button.dataset.engine ?? '');
      engines.set(offered.has(def) ? def : null);
      if (offered.has(def)) paintSearchBar(def);
    },
  };
}
